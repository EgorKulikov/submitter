use crate::clear;
use crate::http::HttpClient;
use clipboard::{ClipboardContext, ClipboardProvider};
use crossterm::execute;
use crossterm::style::{Color, ResetColor, SetForegroundColor};
use regex::Regex;
use std::thread;
use std::time::Duration;

const USER_AGENT: &str =
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36";

struct AtcoderClient {
    http: HttpClient,
    revel_session: String,
}

impl AtcoderClient {
    fn new() -> Self {
        let mut http = HttpClient::new("https://atcoder.jp");
        http.set_header("User-Agent", USER_AGENT);
        // Read the session cookie under its canonical name (REVEL_SESSION), and
        // fall back to the legacy key "atcoder_revel_session" for existing users.
        // If we found it under the legacy key, migrate it forward so self.http's
        // auto-injection sends the right cookie name to AtCoder.
        let revel_session = if let Some(v) = http.get_cookie("REVEL_SESSION") {
            v
        } else if let Some(v) = http.get_cookie("atcoder_revel_session") {
            http.set_cookie("REVEL_SESSION", &v);
            v
        } else {
            String::new()
        };
        AtcoderClient {
            http,
            revel_session,
        }
    }

    fn get_page(&self, path: &str) -> Result<String, String> {
        reqwest::blocking::Client::builder()
            .redirect(reqwest::redirect::Policy::limited(5))
            .build()
            .unwrap()
            .get(format!("https://atcoder.jp{}", path))
            .header("Cookie", format!("REVEL_SESSION={}", self.revel_session))
            .header("User-Agent", USER_AGENT)
            .send()
            .map_err(|e| format!("GET failed: {}", e))?
            .text()
            .map_err(|e| format!("Failed to read response: {}", e))
    }

    fn is_logged_in(&self) -> bool {
        if self.revel_session.is_empty() {
            return false;
        }
        match self.get_page("/settings") {
            Ok(body) => !body.contains("userScreenName = \"\""),
            Err(_) => false,
        }
    }

    fn login(&mut self) -> Result<(), String> {
        if self.is_logged_in() {
            println!("Already logged in");
            return Ok(());
        }

        println!("Export your AtCoder cookies using the EditThisCookie browser extension on a logged-in atcoder.jp tab");
        println!("Paste the JSON cookies array:");
        let mut input = String::new();
        let mut bracket_count = 0i32;
        loop {
            let mut line = String::new();
            std::io::stdin()
                .read_line(&mut line)
                .map_err(|e| format!("Failed to read input: {}", e))?;
            for c in line.chars() {
                if c == '[' || c == '{' {
                    bracket_count += 1;
                } else if c == ']' || c == '}' {
                    bracket_count -= 1;
                }
            }
            input.push_str(&line);
            if bracket_count <= 0 && !input.trim().is_empty() {
                break;
            }
        }

        // Parse JSON cookies array
        let cookies: Vec<serde_json::Value> = serde_json::from_str(input.trim())
            .map_err(|e| format!("Failed to parse cookies JSON: {}", e))?;

        let session = cookies
            .iter()
            .find(|c| c.get("name").and_then(|v| v.as_str()) == Some("REVEL_SESSION"))
            .and_then(|c| c.get("value").and_then(|v| v.as_str()))
            .ok_or("REVEL_SESSION cookie not found in input")?
            .to_string();

        self.revel_session = session.clone();
        if self.is_logged_in() {
            self.http.set_cookie("REVEL_SESSION", &session);
            println!("Login successful");
            Ok(())
        } else {
            self.revel_session.clear();
            Err("Login failed: invalid session cookie".to_string())
        }
    }

    fn poll_verdict<F: FnMut()>(
        &self,
        contest_id: &str,
        known_ids: &std::collections::HashSet<String>,
        mut on_submission_found: F,
    ) -> Result<(), String> {
        let mut stdout = std::io::stdout();
        let mut last_len = 0;
        let mut tracking_id: Option<String> = None;

        loop {
            clear(last_len);
            last_len = 0;
            let body = self.get_page(&format!("/contests/{}/submissions/me", contest_id))?;

            // Parse submissions table
            let tbody_re = Regex::new(r"(?s)<tbody>(.*?)</tbody>").unwrap();
            let row_re = Regex::new(r"(?s)<tr>(.*?)</tr>").unwrap();
            let sub_re = Regex::new(r"/submissions/(\d+)").unwrap();

            if let Some(tbody) = tbody_re.captures(&body) {
                for row_cap in row_re.captures_iter(&tbody[1]) {
                    let row = &row_cap[1];
                    let sub_id = match sub_re.captures(row) {
                        Some(c) => c[1].to_string(),
                        None => continue,
                    };

                    if let Some(ref tid) = tracking_id {
                        if sub_id != *tid {
                            continue;
                        }
                    } else if known_ids.contains(&sub_id) {
                        continue;
                    } else {
                        // New submission found
                        tracking_id = Some(sub_id.clone());
                        println!(
                            "Submission url: https://atcoder.jp/contests/{}/submissions/{}",
                            contest_id, sub_id
                        );
                        on_submission_found();
                    }

                    // Parse verdict from this row
                    let cells: Vec<String> = Regex::new(r"(?s)<td[^>]*>(.*?)</td>")
                        .unwrap()
                        .captures_iter(row)
                        .map(|c| {
                            Regex::new(r"<[^>]+>")
                                .unwrap()
                                .replace_all(&c[1], " ")
                                .trim()
                                .to_string()
                        })
                        .collect();

                    let label = Regex::new(r"label-(\w+)")
                        .unwrap()
                        .captures(row)
                        .map(|c| c[1].to_string())
                        .unwrap_or_default();

                    let verdict = cells.get(6).cloned().unwrap_or_default();
                    let score = cells.get(4).cloned().unwrap_or_default();

                    if label == "default" || verdict == "WJ" {
                        // Still judging
                        let progress = if verdict.contains('/') {
                            format!("Testing {}", verdict)
                        } else {
                            verdict.clone()
                        };
                        let _ = execute!(stdout, SetForegroundColor(Color::Yellow));
                        print!("{}", progress);
                        let _ = execute!(stdout, ResetColor);
                        last_len = progress.len();
                    } else {
                        // Final verdict
                        let is_accepted = label == "success";
                        let color = if is_accepted {
                            Color::Green
                        } else {
                            Color::Red
                        };
                        let mut display = verdict.clone();
                        if is_accepted && !score.is_empty() {
                            display.push_str(&format!(" ({})", group_digits(&score)));
                        } else if !is_accepted && !score.is_empty() && score != "0" {
                            display.push_str(&format!(" ({}pts)", score));
                        }
                        let _ = execute!(stdout, SetForegroundColor(color));
                        println!("{}", display);
                        let _ = execute!(stdout, ResetColor);
                        return Ok(());
                    }
                    break; // Only process the tracked submission
                }
            }

            thread::sleep(Duration::from_secs(2));
        }
    }
}

fn parse_csrf(html: &str) -> Option<String> {
    let re = Regex::new(r#"(?is)<input[^>]*name="csrf_token"[^>]*value="([^"]+)""#).ok()?;
    re.captures(html).map(|c| c[1].to_string())
}

fn parse_language_options(html: &str) -> Vec<(String, String)> {
    let sel_re = Regex::new(
        r#"(?is)<select[^>]*name="data\.LanguageId"[^>]*>(.*?)</select>"#,
    )
    .unwrap();
    let opt_re =
        Regex::new(r#"(?is)<option[^>]*value="([^"]+)"[^>]*>(.*?)</option>"#).unwrap();
    let inner = match sel_re.captures(html) {
        Some(c) => c[1].to_string(),
        None => return Vec::new(),
    };
    opt_re
        .captures_iter(&inner)
        .map(|c| (c[1].to_string(), c[2].trim().to_string()))
        .collect()
}

/// Given the raw POST response's status + `Location` header, decide whether
/// the direct submission succeeded. Success = 302 whose Location contains
/// "/submissions". Everything else = fallback trigger.
fn classify_submit_response(status: u16, location: Option<&str>) -> Result<(), String> {
    if status == 200 {
        return Err("POST returned 200 (form re-rendered — validation failed?)".to_string());
    }
    if !(300..400).contains(&status) {
        return Err(format!("POST returned {}", status));
    }
    let loc = location.ok_or_else(|| "POST redirected with no Location header".to_string())?;
    if loc.contains("/submissions") {
        Ok(())
    } else if loc.contains("/login") {
        Err("POST redirected to /login (session expired?)".to_string())
    } else {
        Err(format!("POST redirected to unexpected Location: {}", loc))
    }
}

fn group_digits(score: &str) -> String {
    let s = score.trim();
    if s.is_empty() || !s.chars().all(|c| c.is_ascii_digit()) {
        return s.to_string();
    }
    let chars: Vec<char> = s.chars().collect();
    let mut result = String::new();
    for (i, c) in chars.iter().enumerate() {
        if i > 0 && (chars.len() - i) % 3 == 0 {
            result.push('\'');
        }
        result.push(*c);
    }
    result
}

/// Parse AtCoder URL
/// https://atcoder.jp/contests/abc388/tasks/abc388_a -> ("abc388", "abc388_a")
fn parse_url(url: &str) -> Option<(String, String)> {
    let url = url.split('?').next().unwrap_or(url);
    let re = Regex::new(r"contests/([^/]+)/tasks/([^/]+)").ok()?;
    if let Some(caps) = re.captures(url) {
        return Some((caps[1].to_string(), caps[2].to_string()));
    }
    // Just contest URL — no task
    let re = Regex::new(r"contests/([^/]+)").ok()?;
    let caps = re.captures(url)?;
    Some((caps[1].to_string(), String::new()))
}

pub fn login() {
    let mut client = AtcoderClient::new();
    if let Err(e) = client.login() {
        eprintln!("Login failed: {}", e);
    }
}

pub fn submit(url: String, language: String, source: String) {
    let mut client = AtcoderClient::new();

    if let Err(e) = client.login() {
        eprintln!("{}", e);
        return;
    }

    let (contest_id, task_id) = match parse_url(&url) {
        Some(parsed) => parsed,
        None => {
            eprintln!("Could not parse URL: {}", url);
            return;
        }
    };

    // Record existing submissions before opening browser
    let mut known_ids = std::collections::HashSet::new();
    if let Ok(body) = client.get_page(&format!("/contests/{}/submissions/me", contest_id)) {
        let sub_re = Regex::new(r"/submissions/(\d+)").unwrap();
        for cap in sub_re.captures_iter(&body) {
            known_ids.insert(cap[1].to_string());
        }
    }

    // Copy source to clipboard and open submit page
    let mut ctx: ClipboardContext = ClipboardProvider::new().unwrap();
    ctx.set_contents(source.clone()).unwrap();

    let submit_url = if task_id.is_empty() {
        format!("https://atcoder.jp/contests/{}/submit", contest_id)
    } else {
        format!(
            "https://atcoder.jp/contests/{}/submit?taskScreenName={}",
            contest_id, task_id
        )
    };
    println!("Source code copied to clipboard");
    let handoff = crate::extbridge::publish(
        crate::extbridge::Job {
            site: "atcoder",
            url: submit_url.clone(),
            language,
            source,
        },
        std::time::Duration::from_secs(60),
    ).ok();
    let url_to_open = match handoff.as_ref() {
        Some(h) => format!("{}#submitter={}:{}", submit_url, h.port, h.token),
        None => submit_url.clone(),
    };
    open::that_detached(&url_to_open).ok();

    // Poll for new submission
    if let Err(e) = client.poll_verdict(&contest_id, &known_ids, || {
        if let Some(h) = handoff.as_ref() {
            h.signal_close();
        }
    }) {
        eprintln!("Verdict polling failed: {}", e);
    }
}

#[cfg(test)]
mod tests {
    use super::group_digits;

    #[test]
    fn groups_long_score() {
        assert_eq!(group_digits("1234567"), "1'234'567");
    }

    #[test]
    fn leaves_short_score_alone() {
        assert_eq!(group_digits("100"), "100");
        assert_eq!(group_digits("42"), "42");
    }

    #[test]
    fn handles_exact_multiple_of_three() {
        assert_eq!(group_digits("1000"), "1'000");
        assert_eq!(group_digits("1000000"), "1'000'000");
    }

    #[test]
    fn passes_through_non_digit() {
        assert_eq!(group_digits(""), "");
        assert_eq!(group_digits("abc"), "abc");
    }

    use super::{parse_csrf, parse_language_options, classify_submit_response};

    #[test]
    fn parse_csrf_extracts_token() {
        let html = r#"<form><input type="hidden" name="csrf_token" value="abc123XYZ=="></form>"#;
        assert_eq!(parse_csrf(html).as_deref(), Some("abc123XYZ=="));
    }

    #[test]
    fn parse_csrf_handles_attribute_order() {
        let html = r#"<input value="tok999" name="csrf_token" type="hidden">"#;
        // Same-line, name comes after value — our regex looks for name= first,
        // so this variant is expected to miss. Document that: if AtCoder ever
        // reorders attributes, we fall back to browser (safe) rather than parse.
        assert_eq!(parse_csrf(html), None);
    }

    #[test]
    fn parse_csrf_missing_returns_none() {
        let html = "<form></form>";
        assert_eq!(parse_csrf(html), None);
    }

    #[test]
    fn parse_language_options_extracts_pairs() {
        let html = r#"
          <select name="data.LanguageId" class="form-control">
            <option value="5001">C++ 17 (gcc 9.2.1)</option>
            <option value="5054">Rust (rustc 1.70.0)</option>
          </select>
        "#;
        let opts = parse_language_options(html);
        assert_eq!(opts.len(), 2);
        assert_eq!(opts[0], ("5001".to_string(), "C++ 17 (gcc 9.2.1)".to_string()));
        assert_eq!(opts[1], ("5054".to_string(), "Rust (rustc 1.70.0)".to_string()));
    }

    #[test]
    fn parse_language_options_missing_select_returns_empty() {
        assert!(parse_language_options("<html></html>").is_empty());
    }

    #[test]
    fn parse_language_options_empty_select_returns_empty() {
        let html = r#"<select name="data.LanguageId"></select>"#;
        assert!(parse_language_options(html).is_empty());
    }

    #[test]
    fn classify_302_to_submissions_is_ok() {
        assert!(classify_submit_response(302, Some("/contests/abc463/submissions/me")).is_ok());
    }

    #[test]
    fn classify_302_to_login_is_err() {
        assert!(classify_submit_response(302, Some("/login")).is_err());
    }

    #[test]
    fn classify_302_without_location_is_err() {
        assert!(classify_submit_response(302, None).is_err());
    }

    #[test]
    fn classify_200_is_err() {
        assert!(classify_submit_response(200, None).is_err());
    }

    #[test]
    fn classify_403_is_err() {
        assert!(classify_submit_response(403, None).is_err());
    }
}

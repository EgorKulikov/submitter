use crate::clear;
use crate::http::HttpClient;
use crossterm::execute;
use crossterm::style::{Color, ResetColor, SetForegroundColor};
use std::thread;
use std::time::Duration;

const BASE_URL: &str = "https://kep.uz";

/// Django session + CSRF cookies. We store every cookie the user pastes
/// for kep.uz, but these are the names whose presence we use to gauge
/// whether the paste was successful.
const REQUIRED_COOKIES: &[&str] = &["sessionid"];

pub struct KepClient {
    http: HttpClient,
}

impl KepClient {
    pub fn new() -> Self {
        KepClient {
            http: HttpClient::new(BASE_URL),
        }
    }

    pub fn login(&mut self) -> Result<(), String> {
        if self.is_logged_in() {
            println!("Already logged in");
            return Ok(());
        }
        self.prompt_for_cookies()?;
        if self.is_logged_in() {
            println!("Login successful");
            Ok(())
        } else {
            Err("Login failed: pasted cookies did not authenticate".to_string())
        }
    }

    fn is_logged_in(&mut self) -> bool {
        if self.http.get_cookie("sessionid").is_none() {
            return false;
        }
        // `/api/me/` returns the logged-in user (with username) or non-200.
        match self.http.get("/api/me/") {
            Ok(resp) => resp.status().is_success(),
            Err(_) => false,
        }
    }

    fn prompt_for_cookies(&mut self) -> Result<(), String> {
        println!("Export your KEP.uz cookies using the EditThisCookie browser extension on a logged-in kep.uz tab");
        println!("Paste the JSON cookies array:");
        let mut input = String::new();
        let mut bracket_count: i32 = 0;
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
        let cookies: Vec<serde_json::Value> = serde_json::from_str(input.trim())
            .map_err(|e| format!("Failed to parse cookies JSON: {}", e))?;

        let mut saved_required = 0;
        for c in &cookies {
            let name = c.get("name").and_then(|v| v.as_str()).unwrap_or("");
            let value = c.get("value").and_then(|v| v.as_str()).unwrap_or("");
            if name.is_empty() {
                continue;
            }
            self.http.set_cookie(name, value);
            if REQUIRED_COOKIES.contains(&name) {
                saved_required += 1;
            }
        }
        if saved_required == 0 {
            return Err(format!(
                "Required cookies ({}) not found in pasted JSON.",
                REQUIRED_COOKIES.join(", ")
            ));
        }
        Ok(())
    }

    /// Resolve a user-supplied language label (e.g. "Rust", "rs",
    /// "Rust 1.71") to the kep.uz `lang` slug by looking at the target's
    /// `availableLanguages` list.
    fn find_lang(&mut self, target: &Target, language: &str) -> Result<String, String> {
        let detail = self.http.get_json(&target.detail_path())?;
        let langs = target
            .available_languages(&detail)
            .ok_or("Could not find availableLanguages for problem")?;
        let needle = language.trim().to_lowercase();
        // Exact match on slug or langFull.
        for l in langs {
            let slug = l.get("lang").and_then(|v| v.as_str()).unwrap_or("");
            let full = l.get("langFull").and_then(|v| v.as_str()).unwrap_or("");
            if slug.eq_ignore_ascii_case(language) || full.eq_ignore_ascii_case(language) {
                return Ok(slug.to_string());
            }
        }
        // Starts-with on langFull (so "Rust" matches "Rust 1.71").
        for l in langs {
            let slug = l.get("lang").and_then(|v| v.as_str()).unwrap_or("");
            let full = l.get("langFull").and_then(|v| v.as_str()).unwrap_or("");
            if full.to_lowercase().starts_with(&needle) {
                return Ok(slug.to_string());
            }
        }
        let available: Vec<String> = langs
            .iter()
            .filter_map(|l| l.get("langFull").and_then(|v| v.as_str()).map(String::from))
            .collect();
        Err(format!(
            "Language '{}' not found. Available: {}",
            language,
            available.join(", ")
        ))
    }

    pub fn submit(&mut self, target: &Target, language: &str, source: &str) -> Result<i64, String> {
        let lang_slug = self.find_lang(target, language)?;
        println!("Language: {}", lang_slug);

        let body = target.submit_body(&lang_slug, source);
        let resp = self.http.post_json(&target.submit_path(), &body.to_string())?;
        let status = resp.status();
        let text = resp
            .text()
            .map_err(|e| format!("Failed to read submit response: {}", e))?;
        if !status.is_success() {
            return Err(format!(
                "Submit failed ({}): {}",
                status,
                &text[..text.len().min(300)]
            ));
        }
        let json: serde_json::Value = serde_json::from_str(&text)
            .map_err(|e| format!("Failed to parse submit response: {} body: {}", e, text))?;
        let id = target
            .extract_id(&json)
            .ok_or_else(|| format!("No submission id in response: {}", text))?;
        println!("Submission url: {}{}", BASE_URL, target.web_path());
        Ok(id)
    }

    pub fn poll_verdict(&mut self, target: &Target, submission_id: i64) -> Result<String, String> {
        let path = target.poll_path();
        let mut stdout = std::io::stdout();
        let mut last_len = 0;
        loop {
            clear(last_len);
            let json = self.http.get_json(&path)?;
            let attempts = json
                .get("data")
                .and_then(|v| v.as_array())
                .ok_or("Attempt list has no `data` array")?;
            let attempt = attempts
                .iter()
                .find(|a| a.get("id").and_then(|v| v.as_i64()) == Some(submission_id));
            let attempt = match attempt {
                Some(a) => a,
                None => {
                    // Submission not yet in the list — wait and retry.
                    let progress = "Queued";
                    let _ = execute!(stdout, SetForegroundColor(Color::Yellow));
                    print!("{}", progress);
                    let _ = execute!(stdout, ResetColor);
                    let _ = std::io::Write::flush(&mut std::io::stdout());
                    last_len = progress.len();
                    thread::sleep(Duration::from_secs(2));
                    continue;
                }
            };
            let verdict = attempt.get("verdict").and_then(|v| v.as_i64()).unwrap_or(-1);
            let verdict_title = attempt
                .get("verdictTitle")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let test_case = attempt
                .get("testCaseNumber")
                .and_then(|v| v.as_i64())
                .unwrap_or(0);

            if is_terminal_verdict(verdict) {
                let mut display = verdict_title.to_string();
                if verdict != ACCEPTED_VERDICT && test_case > 0 {
                    display.push_str(&format!(" test #{}", test_case));
                }
                let color = if verdict == ACCEPTED_VERDICT {
                    Color::Green
                } else {
                    Color::Red
                };
                let _ = execute!(stdout, SetForegroundColor(color));
                println!("{}", display);
                let _ = execute!(stdout, ResetColor);
                return Ok(display);
            }

            let progress = if verdict_title.is_empty() {
                "Pending".to_string()
            } else {
                verdict_title.to_string()
            };
            let _ = execute!(stdout, SetForegroundColor(Color::Yellow));
            print!("{}", progress);
            let _ = execute!(stdout, ResetColor);
            let _ = std::io::Write::flush(&mut std::io::stdout());
            last_len = progress.len();
            thread::sleep(Duration::from_secs(2));
        }
    }
}

/// Verdict code reserved for `Accepted` in the kep.uz API.
const ACCEPTED_VERDICT: i64 = 1;

fn is_terminal_verdict(verdict: i64) -> bool {
    // -1 = Running. Anything else is a final verdict (Accepted, Wrong Answer,
    // Compilation Error, etc.).
    verdict >= 0
}

/// A kep.uz submission target — either a problem inside a contest or a
/// standalone archive problem. The two have different endpoints, body
/// shapes, and response keys.
#[derive(Debug, PartialEq, Eq)]
pub enum Target {
    Contest { contest_id: String, problem_letter: String },
    Archive { problem_id: String },
}

impl Target {
    fn detail_path(&self) -> String {
        match self {
            Target::Contest { contest_id, .. } => {
                format!("/api/contests/{}/problems/?django_language=en", contest_id)
            }
            Target::Archive { problem_id } => {
                format!("/api/problems/{}/?django_language=en", problem_id)
            }
        }
    }

    fn available_languages<'a>(
        &self,
        detail: &'a serde_json::Value,
    ) -> Option<&'a Vec<serde_json::Value>> {
        match self {
            // Contest detail returns an array; each entry has `.problem.availableLanguages`.
            Target::Contest { .. } => detail
                .as_array()
                .and_then(|arr| arr.first())
                .and_then(|p| p.pointer("/problem/availableLanguages"))
                .and_then(|v| v.as_array()),
            // Archive detail returns the problem object with `availableLanguages` at the root.
            Target::Archive { .. } => detail
                .get("availableLanguages")
                .and_then(|v| v.as_array()),
        }
    }

    fn submit_path(&self) -> String {
        match self {
            Target::Contest { contest_id, .. } => {
                format!("/api/contests/{}/submit/?django_language=en", contest_id)
            }
            Target::Archive { problem_id } => {
                format!("/api/problems/{}/submit/?django_language=en", problem_id)
            }
        }
    }

    fn submit_body(&self, lang_slug: &str, source: &str) -> serde_json::Value {
        match self {
            Target::Contest { problem_letter, .. } => serde_json::json!({
                "contest_problem": problem_letter,
                "source_code": source,
                "lang": lang_slug,
            }),
            Target::Archive { .. } => serde_json::json!({
                "sourceCode": source,
                "lang": lang_slug,
            }),
        }
    }

    fn extract_id(&self, response: &serde_json::Value) -> Option<i64> {
        match self {
            Target::Contest { .. } => response.get("id").and_then(|v| v.as_i64()),
            Target::Archive { .. } => response.get("attemptId").and_then(|v| v.as_i64()),
        }
    }

    fn poll_path(&self) -> String {
        match self {
            Target::Contest { contest_id, problem_letter } => format!(
                "/api/attempts/?ordering=-id&contest_id={}&contest_problem={}&page=1&pageSize=10&django_language=en",
                contest_id, problem_letter
            ),
            Target::Archive { problem_id } => format!(
                "/api/attempts/?ordering=-id&problem_id={}&page=1&pageSize=10&django_language=en",
                problem_id
            ),
        }
    }

    fn web_path(&self) -> String {
        match self {
            Target::Contest { contest_id, problem_letter } => {
                format!("/contests/{}/problem/{}", contest_id, problem_letter)
            }
            Target::Archive { problem_id } => format!("/problems/{}", problem_id),
        }
    }
}

/// Parse a kep.uz URL into a `Target`.
/// Forms:
///   `https://kep.uz/contests/<cid>/problem/<letter>`  →  `Contest`
///   `https://kep.uz/problems/<id>`                    →  `Archive`
pub fn parse_url(url: &str) -> Option<Target> {
    let url = url.split('?').next().unwrap_or(url).trim_end_matches('/');
    if let Some(caps) = regex::Regex::new(r"kep\.uz/contests/(\d+)/problem/([A-Za-z0-9_-]+)")
        .ok()
        .and_then(|re| re.captures(url))
    {
        return Some(Target::Contest {
            contest_id: caps[1].to_string(),
            problem_letter: caps[2].to_string(),
        });
    }
    if let Some(caps) = regex::Regex::new(r"kep\.uz/problems/(\d+)")
        .ok()
        .and_then(|re| re.captures(url))
    {
        return Some(Target::Archive {
            problem_id: caps[1].to_string(),
        });
    }
    None
}

pub fn login() {
    let mut client = KepClient::new();
    if let Err(e) = client.login() {
        eprintln!("Login failed: {}", e);
    }
}

pub fn submit(url: String, language: String, source: String) {
    let target = match parse_url(&url) {
        Some(t) => t,
        None => {
            eprintln!("Could not parse URL: {}", url);
            return;
        }
    };

    let mut client = KepClient::new();

    println!("Logging in");
    if let Err(e) = client.login() {
        eprintln!("Login failed: {}", e);
        return;
    }

    println!("Submitting");
    let id = match client.submit(&target, &language, &source) {
        Ok(id) => id,
        Err(e) => {
            eprintln!("{}", e);
            return;
        }
    };

    if let Err(e) = client.poll_verdict(&target, id) {
        eprintln!("Verdict polling failed: {}", e);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_contest_problem_url() {
        assert_eq!(
            parse_url("https://kep.uz/contests/487/problem/A"),
            Some(Target::Contest {
                contest_id: "487".to_string(),
                problem_letter: "A".to_string(),
            })
        );
    }

    #[test]
    fn parse_contest_problem_with_trailing_slash_and_query() {
        assert_eq!(
            parse_url("https://kep.uz/contests/487/problem/A/?lang=en"),
            Some(Target::Contest {
                contest_id: "487".to_string(),
                problem_letter: "A".to_string(),
            })
        );
    }

    #[test]
    fn parse_archive_problem_url() {
        assert_eq!(
            parse_url("https://kep.uz/problems/2254"),
            Some(Target::Archive {
                problem_id: "2254".to_string()
            })
        );
    }

    #[test]
    fn rejects_unknown_url() {
        assert_eq!(parse_url("https://kep.uz/contests/487"), None);
    }

    #[test]
    fn contest_target_paths() {
        let t = Target::Contest {
            contest_id: "487".into(),
            problem_letter: "A".into(),
        };
        assert_eq!(t.submit_path(), "/api/contests/487/submit/?django_language=en");
        assert!(t.poll_path().contains("contest_id=487"));
        assert!(t.poll_path().contains("contest_problem=A"));
        assert_eq!(t.web_path(), "/contests/487/problem/A");
    }

    #[test]
    fn archive_target_paths() {
        let t = Target::Archive {
            problem_id: "2254".into(),
        };
        assert_eq!(t.submit_path(), "/api/problems/2254/submit/?django_language=en");
        assert!(t.poll_path().contains("problem_id=2254"));
        assert_eq!(t.web_path(), "/problems/2254");
    }

    #[test]
    fn archive_submit_body_uses_camelcase() {
        let body = Target::Archive {
            problem_id: "1".into(),
        }
        .submit_body("rs", "code");
        assert_eq!(body["sourceCode"], "code");
        assert_eq!(body["lang"], "rs");
        assert!(body.get("source_code").is_none());
    }

    #[test]
    fn contest_submit_body_uses_snake_case() {
        let body = Target::Contest {
            contest_id: "487".into(),
            problem_letter: "A".into(),
        }
        .submit_body("rs", "code");
        assert_eq!(body["source_code"], "code");
        assert_eq!(body["contest_problem"], "A");
        assert_eq!(body["lang"], "rs");
    }

    #[test]
    fn extract_id_handles_both_response_shapes() {
        let archive = Target::Archive {
            problem_id: "1".into(),
        };
        let contest = Target::Contest {
            contest_id: "487".into(),
            problem_letter: "A".into(),
        };
        assert_eq!(
            archive.extract_id(&serde_json::json!({"attemptId": 42})),
            Some(42)
        );
        assert_eq!(contest.extract_id(&serde_json::json!({"id": 42})), Some(42));
    }
}

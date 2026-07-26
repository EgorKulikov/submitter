use crate::clear;
use crate::http::HttpClient;
use crossterm::execute;
use crossterm::style::{Color, ResetColor, SetForegroundColor};
use reqwest::blocking::multipart;
use std::thread;
use std::time::Duration;

const BASE_URL: &str = "https://new.contest.yandex.ru";

/// Same Yandex Passport cookies coderun honors. `Session_id` is the only
/// strictly-required one; the others help when the session is rotated.
const SESSION_COOKIES: &[&str] = &["Session_id", "sessionid2", "yandex_login", "yandexuid"];

pub struct NewYandexClient {
    http: HttpClient,
}

impl NewYandexClient {
    pub fn new() -> Self {
        NewYandexClient {
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
            Err("Login failed: pasted cookies didn't authenticate".to_string())
        }
    }

    fn is_logged_in(&self) -> bool {
        self.http.get_cookie("Session_id").is_some()
    }

    fn prompt_for_cookies(&mut self) -> Result<(), String> {
        println!("Export your Yandex cookies using the EditThisCookie browser extension on a logged-in yandex.ru tab");
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
        let mut saved = 0;
        for needed in SESSION_COOKIES {
            if let Some(value) = cookies
                .iter()
                .find(|c| c.get("name").and_then(|v| v.as_str()) == Some(*needed))
                .and_then(|c| c.get("value").and_then(|v| v.as_str()))
            {
                self.http.set_cookie(needed, value);
                saved += 1;
            }
        }
        if saved == 0 {
            return Err(format!(
                "None of the expected Yandex cookies ({}) were found in the pasted JSON.",
                SESSION_COOKIES.join(", ")
            ));
        }
        Ok(())
    }

    /// Fetch the contest's compiler list by GETting the problem page HTML and
    /// extracting `__NEXT_DATA__`. Same pattern coderun uses. The compiler list
    /// lives at props.pageProps.store.<random-hash>, so we find it by shape
    /// (any list whose items have `compilerId` and `compilerName`).
    fn fetch_compilers(
        &mut self,
        contest_id: &str,
        problem_id: &str,
    ) -> Result<Vec<(String, String)>, String> {
        let encoded_id = problem_id.replace('/', "%2F");
        let path = format!("/contests/{}/problems?id={}", contest_id, encoded_id);
        let html = self.http.get_text(&path)?;
        let nd = extract_next_data(&html)?;
        let store = nd
            .pointer("/props/pageProps/store")
            .and_then(|v| v.as_object())
            .ok_or("no props.pageProps.store in __NEXT_DATA__")?;
        for v in store.values() {
            let Some(arr) = v.as_array() else { continue };
            let looks_like_compiler_list = arr
                .first()
                .map(|e| e.get("compilerId").is_some() && e.get("compilerName").is_some())
                .unwrap_or(false);
            if !looks_like_compiler_list {
                continue;
            }
            let out: Vec<(String, String)> = arr
                .iter()
                .filter_map(|c| {
                    Some((
                        c.get("compilerId").and_then(|v| v.as_str())?.to_string(),
                        c.get("compilerName").and_then(|v| v.as_str())?.to_string(),
                    ))
                })
                .collect();
            if !out.is_empty() {
                return Ok(out);
            }
        }
        Err("Compiler list not found in __NEXT_DATA__ store".to_string())
    }

    pub fn submit(
        &mut self,
        contest_id: &str,
        problem_id: &str,
        language: &str,
        source: &str,
    ) -> Result<String, String> {
        let compilers = self.fetch_compilers(contest_id, problem_id)?;
        let (compiler_id, compiler_name) = pick_compiler(&compilers, language)?;
        println!("Language: {}", compiler_name);

        // The browser always sends Origin + a problem-page Referer on POSTs.
        // Yandex 403s the submit without them.
        let encoded_problem = problem_id.replace('/', "%2F");
        self.http.set_header("Origin", BASE_URL);
        self.http.set_header(
            "Referer",
            &format!(
                "{}/contests/{}/problems?id={}",
                BASE_URL, contest_id, encoded_problem
            ),
        );

        let form = multipart::Form::new()
            .text("contestId", contest_id.to_string())
            .text("problemId", problem_id.to_string())
            .text("compilerId", compiler_id.clone())
            .part(
                "file",
                multipart::Part::text(source.to_string())
                    .file_name("source.txt")
                    .mime_str("text/plain")
                    .unwrap(),
            );

        let resp = self
            .http
            .post_multipart("/api/action/solution/submit/file", form, "")?;
        let status = resp.status();
        let body = resp
            .text()
            .map_err(|e| format!("Failed to read submit response: {}", e))?;

        if !status.is_success() {
            return Err(format!(
                "Submit failed ({}): {}",
                status,
                &body[..body.len().min(300)]
            ));
        }

        let json: serde_json::Value = serde_json::from_str(&body)
            .map_err(|e| format!("Failed to parse submit response: {} body: {}", e, body))?;
        let uuid = json
            .get("uuid")
            .and_then(|v| v.as_str())
            .ok_or_else(|| format!("No uuid in submit response: {}", body))?
            .to_string();
        println!(
            "Submission url: {}/contests/{}/submissions/{}",
            BASE_URL, contest_id, uuid
        );
        Ok(uuid)
    }

    pub fn poll_verdict(&mut self, uuid: &str) -> Result<String, String> {
        let mut stdout = std::io::stdout();
        let mut last_len = 0;
        loop {
            clear(last_len);
            let body = self
                .http
                .get_text(&format!("/api/action/submission/report/brief/{}", uuid))?;
            let json: serde_json::Value = serde_json::from_str(&body)
                .map_err(|e| format!("Failed to parse poll JSON: {} body: {}", e, body))?;
            let report = json.pointer("/report").unwrap_or(&json);
            let sub_status = report
                .get("status")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let verdict = report
                .get("verdict")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let (test_number, test_count) = report
                .get("progress")
                .and_then(|p| {
                    Some((
                        p.get("testNumber").and_then(|v| v.as_i64())?,
                        p.get("testCount").and_then(|v| v.as_i64())?,
                    ))
                })
                .unwrap_or((0, 0));

            if is_terminal(sub_status) {
                let mut pretty = pretty_verdict(verdict, sub_status);
                if !verdict.eq_ignore_ascii_case("OK") && test_number > 0 {
                    pretty.push_str(&format!(" test #{}", test_number));
                }
                if let Some(score) = report.get("score").and_then(|v| v.as_i64()) {
                    if !verdict.eq_ignore_ascii_case("OK") && score > 0 {
                        pretty.push_str(&format!(" ({}pts)", score));
                    }
                }
                let color = if verdict.eq_ignore_ascii_case("OK") {
                    Color::Green
                } else {
                    Color::Red
                };
                let _ = execute!(stdout, SetForegroundColor(color));
                println!("{}", pretty);
                let _ = execute!(stdout, ResetColor);
                return Ok(pretty);
            }

            let progress_str = if test_count > 0 {
                format!("Testing {}/{}", test_number, test_count)
            } else if sub_status.is_empty() {
                "Pending".to_string()
            } else {
                humanize(sub_status)
            };
            let _ = execute!(stdout, SetForegroundColor(Color::Yellow));
            print!("{}", progress_str);
            let _ = execute!(stdout, ResetColor);
            let _ = std::io::Write::flush(&mut std::io::stdout());
            last_len = progress_str.len();
            thread::sleep(Duration::from_secs(2));
        }
    }
}

/// Parse a new.contest.yandex.ru URL into (contest_id, problem_id_alias).
/// Accepts: https://new.contest.yandex.ru/contests/{contest_id}/problems?id=<url-encoded-problem-id>
pub fn parse_url(url: &str) -> Option<(String, String)> {
    let (path, query) = match url.split_once('?') {
        Some((p, q)) => (p, Some(q)),
        None => (url, None),
    };
    let path = path.trim_end_matches('/');
    let re = regex::Regex::new(r"contests/(\d+)/problems").ok()?;
    let caps = re.captures(path)?;
    let contest_id = caps[1].to_string();
    let problem_id = query
        .and_then(|q| q.split('&').find_map(|kv| kv.strip_prefix("id=").map(percent_decode)))?;
    Some((contest_id, problem_id))
}

/// Extract the JSON body of Next.js's `__NEXT_DATA__` script tag from the
/// problem page HTML. Same shape coderun scrapes.
fn extract_next_data(html: &str) -> Result<serde_json::Value, String> {
    let needle = "id=\"__NEXT_DATA__\"";
    let start = html
        .find(needle)
        .ok_or("__NEXT_DATA__ script tag not found in page")?;
    let after = &html[start..];
    let open = after
        .find('>')
        .ok_or("Malformed __NEXT_DATA__ tag (no `>`)")? + 1;
    let rest = &after[open..];
    let end = rest
        .find("</script>")
        .ok_or("Unterminated __NEXT_DATA__ payload")?;
    serde_json::from_str(&rest[..end])
        .map_err(|e| format!("Failed to parse __NEXT_DATA__ JSON: {}", e))
}

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let (Some(h), Some(l)) = (
                (bytes[i + 1] as char).to_digit(16),
                (bytes[i + 2] as char).to_digit(16),
            ) {
                out.push((h * 16 + l) as u8);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Given the compiler list `(id, name)` and the user's language string, pick
/// the best compiler id. Case-insensitive: exact match first, then starts-with
/// on name or id, then substring. Returns (id, name) on success.
fn pick_compiler(
    compilers: &[(String, String)],
    language: &str,
) -> Result<(String, String), String> {
    let needle = language.to_lowercase();

    // Exact match on id or name.
    for (id, name) in compilers {
        if id.eq_ignore_ascii_case(language) || name.eq_ignore_ascii_case(language) {
            return Ok((id.clone(), name.clone()));
        }
    }
    // Starts-with on name or id.
    for (id, name) in compilers {
        if name.to_lowercase().starts_with(&needle) || id.to_lowercase().starts_with(&needle) {
            return Ok((id.clone(), name.clone()));
        }
    }
    // Substring on name or id.
    for (id, name) in compilers {
        if name.to_lowercase().contains(&needle) || id.to_lowercase().contains(&needle) {
            return Ok((id.clone(), name.clone()));
        }
    }
    let listed: Vec<String> = compilers
        .iter()
        .map(|(id, name)| format!("{}={}", id, name))
        .collect();
    Err(format!(
        "Compiler '{}' not found. Available: {}",
        language,
        listed.join(", ")
    ))
}

// Verdict helpers — same shape/behaviour as coderun's, kept local to this
// module rather than shared so each site's verdict logic can evolve independently.

fn is_terminal(status: &str) -> bool {
    match status {
        "" => false,
        s if s.eq_ignore_ascii_case("pending")
            || s.eq_ignore_ascii_case("testing")
            || s.eq_ignore_ascii_case("compiling")
            || s.eq_ignore_ascii_case("running")
            || s.eq_ignore_ascii_case("waiting")
            || s.eq_ignore_ascii_case("created") =>
        {
            false
        }
        _ => true,
    }
}

fn pretty_verdict(verdict: &str, status: &str) -> String {
    if verdict.is_empty() {
        return humanize(status);
    }
    if verdict.eq_ignore_ascii_case("OK") {
        return "Accepted".to_string();
    }
    humanize(verdict)
}

fn humanize(s: &str) -> String {
    let mut tokens: Vec<String> = Vec::new();
    let mut current = String::new();
    let mut prev_upper = false;
    for c in s.chars() {
        if c == '_' || c == '-' || c == ' ' {
            if !current.is_empty() {
                tokens.push(std::mem::take(&mut current));
            }
            prev_upper = false;
            continue;
        }
        if c.is_ascii_uppercase() && !prev_upper && !current.is_empty() {
            tokens.push(std::mem::take(&mut current));
        }
        current.push(c);
        prev_upper = c.is_ascii_uppercase();
    }
    if !current.is_empty() {
        tokens.push(current);
    }
    tokens
        .into_iter()
        .map(|t| {
            let mut chars = t.chars();
            match chars.next() {
                Some(first) => {
                    let mut word = String::new();
                    word.push(first.to_ascii_uppercase());
                    for c in chars {
                        word.push(c.to_ascii_lowercase());
                    }
                    word
                }
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

pub fn login() {
    let mut client = NewYandexClient::new();
    if let Err(e) = client.login() {
        eprintln!("Login failed: {}", e);
    }
}

pub fn submit(url: String, language: String, source: String) {
    let mut client = NewYandexClient::new();

    let (contest_id, problem_id) = match parse_url(&url) {
        Some(p) => p,
        None => {
            eprintln!("Could not parse URL: {}", url);
            return;
        }
    };

    println!("Logging in");
    if let Err(e) = client.login() {
        eprintln!("Login failed: {}", e);
        return;
    }

    println!("Submitting");
    let uuid = match client.submit(&contest_id, &problem_id, &language, &source) {
        Ok(u) => u,
        Err(e) => {
            eprintln!("{}", e);
            return;
        }
    };

    if let Err(e) = client.poll_verdict(&uuid) {
        eprintln!("Verdict polling failed: {}", e);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn opts(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
        pairs
            .iter()
            .map(|(a, b)| (a.to_string(), b.to_string()))
            .collect()
    }

    #[test]
    fn parse_url_with_encoded_id() {
        assert_eq!(
            parse_url("https://new.contest.yandex.ru/contests/90041/problems?id=7847119%2F2026_02_09%2FJRsW2izyhj"),
            Some((
                "90041".to_string(),
                "7847119/2026_02_09/JRsW2izyhj".to_string(),
            )),
        );
    }

    #[test]
    fn parse_url_with_simple_id() {
        assert_eq!(
            parse_url("https://new.contest.yandex.ru/contests/42/problems?id=B"),
            Some(("42".to_string(), "B".to_string())),
        );
    }

    #[test]
    fn parse_url_without_id_returns_none() {
        assert_eq!(
            parse_url("https://new.contest.yandex.ru/contests/90041/problems"),
            None,
        );
    }

    #[test]
    fn pick_compiler_exact_id() {
        let cs = opts(&[("rust154", "Rust 1.80.1"), ("cpp23", "GNU C++23")]);
        assert_eq!(
            pick_compiler(&cs, "rust154").unwrap().0,
            "rust154".to_string(),
        );
    }

    #[test]
    fn pick_compiler_starts_with_name() {
        let cs = opts(&[("rust154", "Rust 1.80.1"), ("cpp23", "GNU C++23")]);
        assert_eq!(pick_compiler(&cs, "rust").unwrap().0, "rust154");
    }

    #[test]
    fn pick_compiler_substring_fallback() {
        let cs = opts(&[("rust154", "Rust 1.80.1"), ("cpp23", "GNU C++23")]);
        // "c++" isn't a starts-with of "GNU C++23" but is a substring.
        assert_eq!(pick_compiler(&cs, "c++").unwrap().0, "cpp23");
    }

    #[test]
    fn pick_compiler_no_match_errors() {
        let cs = opts(&[("rust154", "Rust 1.80.1")]);
        assert!(pick_compiler(&cs, "haskell").is_err());
    }

    #[test]
    fn extract_next_data_parses_embedded_json() {
        let html = r#"<html><head></head><body>
            <script id="__NEXT_DATA__" type="application/json">{"props":{"pageProps":{"store":{"abc":[{"compilerId":"rust154","compilerName":"Rust 1.80.1"}]}}}}</script>
            </body></html>"#;
        let nd = extract_next_data(html).unwrap();
        let arr = nd
            .pointer("/props/pageProps/store/abc")
            .and_then(|v| v.as_array())
            .unwrap();
        assert_eq!(arr[0]["compilerId"], "rust154");
    }

    #[test]
    fn extract_next_data_missing_tag_errors() {
        assert!(extract_next_data("<html></html>").is_err());
    }

    #[test]
    fn humanize_screaming_snake() {
        assert_eq!(humanize("WRONG_ANSWER"), "Wrong Answer");
        assert_eq!(humanize("RUNNING"), "Running");
    }

    #[test]
    fn pretty_verdict_ok_is_accepted() {
        assert_eq!(pretty_verdict("OK", "FINISHED"), "Accepted");
    }

    #[test]
    fn is_terminal_recognizes_lifecycle_states() {
        assert!(!is_terminal("WAITING"));
        assert!(!is_terminal("RUNNING"));
        assert!(is_terminal("FINISHED"));
        assert!(is_terminal("WRONG_ANSWER"));
    }
}

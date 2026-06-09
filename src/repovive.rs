use crate::clear;
use crate::http::HttpClient;
use crossterm::execute;
use crossterm::style::{Color, ResetColor, SetForegroundColor};
use std::thread;
use std::time::Duration;

const BASE_URL: &str = "https://repovive.com";

/// User-Agent string matching a recent Chrome on Windows. Repovive's WAF
/// returns `403 {"error":"Access denied","message":"Automated access ..."}`
/// when it sees reqwest's default `reqwest/x.y.z` UA.
const USER_AGENT: &str =
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/148.0.0.0 Safari/537.36";

/// Repovive's language catalogue (label → numeric Judge0-style ID).
/// Listed newest-first per family so a bare "Rust" / "Java" / "C++" / etc.
/// resolves to the most recent compiler version. New languages can be
/// rediscovered with `scrape_repovive_languages.js`.
const LANGUAGES: &[(&str, i64)] = &[
    ("C++ 23", 200),
    ("C++ 20", 201),
    ("C++ 17", 54),
    ("Java 21", 210),
    ("Java 13", 62),
    ("PyPy 3.10", 220),
    ("Python 3.8", 71),
    ("Node.js 20", 230),
    ("JavaScript", 63),
    ("TypeScript 5", 270),
    ("Go 1.23", 260),
    ("Go 1.13", 60),
    ("Rust 1.92", 250),
    ("Rust 1.40", 73),
    ("C (GCC 14)", 240),
    ("C (GCC 9)", 50),
    ("C#", 51),
    ("Kotlin", 78),
];

pub struct RepoviveClient {
    http: HttpClient,
}

impl RepoviveClient {
    pub fn new() -> Self {
        let mut http = HttpClient::new(BASE_URL);
        // Browser-mimicking headers — the bare reqwest defaults trip Repovive's
        // WAF. Referer is set per-request before submit/poll.
        http.set_header("user-agent", USER_AGENT);
        http.set_header("origin", BASE_URL);
        http.set_header("accept", "application/json, text/plain, */*");
        http.set_header("accept-language", "en-US");
        RepoviveClient { http }
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
        // /api/auth/me returns 200 with the user object when authenticated,
        // 401 {"error":"No token provided"} when anonymous.
        match self.http.get("/api/auth/me") {
            Ok(resp) => resp.status().is_success(),
            Err(_) => false,
        }
    }

    fn prompt_for_cookies(&mut self) -> Result<(), String> {
        println!("Export your Repovive cookies using the EditThisCookie browser extension on a logged-in repovive.com tab");
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
        for c in &cookies {
            let name = c.get("name").and_then(|v| v.as_str()).unwrap_or("");
            let value = c.get("value").and_then(|v| v.as_str()).unwrap_or("");
            if name.is_empty() {
                continue;
            }
            self.http.set_cookie(name, value);
            saved += 1;
        }
        if saved == 0 {
            return Err("No cookies found in pasted JSON.".to_string());
        }
        Ok(())
    }

    /// Fetch the problem detail (anonymous-readable) so we can build the
    /// submit payload. Returns (contestId, problemSlug, testCases).
    fn fetch_problem(
        &mut self,
        contest_num: &str,
        problem_letter: &str,
    ) -> Result<(String, String, Vec<serde_json::Value>), String> {
        let json = self
            .http
            .get_json(&format!("/api/contests/{}/problems/{}", contest_num, problem_letter))?;
        let problem = json
            .get("problem")
            .ok_or("Problem detail response missing `problem`")?;
        let contest_id = problem
            .get("contestId")
            .and_then(|v| v.as_str())
            .ok_or("Problem detail missing contestId")?
            .to_string();
        let slug = problem
            .get("slug")
            .and_then(|v| v.as_str())
            .ok_or("Problem detail missing slug")?
            .to_string();
        // The submit endpoint expects only {input, expectedOutput} per test.
        let test_cases: Vec<serde_json::Value> = problem
            .get("testCases")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .map(|t| {
                        serde_json::json!({
                            "input": t.get("input").cloned().unwrap_or(serde_json::Value::String(String::new())),
                            "expectedOutput": t.get("expectedOutput").cloned().unwrap_or(serde_json::Value::String(String::new())),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();
        Ok((contest_id, slug, test_cases))
    }

    pub fn submit(
        &mut self,
        contest_num: &str,
        problem_letter: &str,
        language: &str,
        source: &str,
    ) -> Result<(String, String), String> {
        let (language_id, language_label) =
            find_language(language).ok_or_else(|| format!("Language '{}' not found", language))?;
        println!("Language: {}", language_label);

        let (contest_id, slug, test_cases) =
            self.fetch_problem(contest_num, problem_letter)?;
        let problem_slug = format!("contest:{}:{}", contest_id, slug);

        // Repovive's WAF blocks requests that aren't tagged with a same-origin
        // referer to the problem page.
        let referer = format!("{}/contests/{}/problems/{}", BASE_URL, contest_num, problem_letter);
        self.http.set_header("referer", &referer);

        let body = serde_json::json!({
            "sourceCode": source,
            "languageId": language_id,
            "problemSlug": problem_slug,
            "testCases": test_cases,
            "contestId": contest_id,
        });
        let resp = self.http.post_json("/api/code/submit", &body.to_string())?;
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
        // Repovive returns the id under either `submissionId` (current shape)
        // or `data._id` (the shape captured in the original HAR).
        let id = json
            .get("submissionId")
            .or_else(|| json.pointer("/data/_id"))
            .and_then(|v| v.as_str())
            .ok_or_else(|| format!("No submission id in response: {}", text))?
            .to_string();
        println!(
            "Submission url: {}/contests/{}/problems/{}/results",
            BASE_URL, contest_num, problem_letter
        );
        Ok((id, slug))
    }

    pub fn poll_verdict(&mut self, submission_id: &str) -> Result<String, String> {
        let mut stdout = std::io::stdout();
        let mut last_len = 0;
        loop {
            clear(last_len);
            let json = self
                .http
                .get_json(&format!("/api/code/submissions/{}", submission_id))?;
            let data = json.get("data").ok_or("Poll response missing `data`")?;
            let judging_status = data
                .get("judgingStatus")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let result_status = data
                .pointer("/result/status")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let passed = data.pointer("/result/passed").and_then(|v| v.as_i64()).unwrap_or(0);
            let total = data.pointer("/result/total").and_then(|v| v.as_i64()).unwrap_or(0);

            if judging_status == "completed" {
                let is_ok = result_status.eq_ignore_ascii_case("Accepted");
                let mut display = result_status.to_string();
                if !is_ok && passed < total {
                    // First failed test is one past the last passed test.
                    display.push_str(&format!(" test #{}", passed + 1));
                }
                let color = if is_ok { Color::Green } else { Color::Red };
                let _ = execute!(stdout, SetForegroundColor(color));
                println!("{}", display);
                let _ = execute!(stdout, ResetColor);
                return Ok(display);
            }

            let progress = match (judging_status, result_status) {
                ("", "") => "Pending".to_string(),
                (js, "") => humanize(js),
                (_, rs) => humanize(rs),
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

/// Map a user-supplied language label to (languageId, canonical label).
/// Accepts:
///   - the full label as shown in the dropdown ("C++ 23", "Rust 1.92"),
///   - a family name ("rust", "java", "c++", "go", "python", ...),
///   - or a raw numeric ID like "250".
fn find_language(input: &str) -> Option<(i64, String)> {
    let trimmed = input.trim();
    if let Ok(n) = trimmed.parse::<i64>() {
        let label = LANGUAGES
            .iter()
            .find(|(_, id)| *id == n)
            .map(|(l, _)| l.to_string())
            .unwrap_or_else(|| format!("id={}", n));
        return Some((n, label));
    }
    // Exact (case-insensitive) match on the full label.
    for (label, id) in LANGUAGES {
        if label.eq_ignore_ascii_case(trimmed) {
            return Some((*id, label.to_string()));
        }
    }
    // Family aliases — pick the newest version, which is the first entry
    // for that family in LANGUAGES.
    let alias_label: &str = match trimmed.to_lowercase().as_str() {
        "cpp" | "c++" => "C++ 23",
        "java" => "Java 21",
        "python" | "py" | "python3" | "py3" => "Python 3.8",
        "pypy" | "pypy3" => "PyPy 3.10",
        "node" | "nodejs" => "Node.js 20",
        "js" | "javascript" => "JavaScript",
        "ts" | "typescript" => "TypeScript 5",
        "go" | "golang" => "Go 1.23",
        "rust" | "rs" => "Rust 1.92",
        "c" => "C (GCC 14)",
        "csharp" | "cs" | "c#" => "C#",
        "kotlin" | "kt" => "Kotlin",
        _ => return None,
    };
    LANGUAGES
        .iter()
        .find(|(l, _)| *l == alias_label)
        .map(|(l, id)| (*id, l.to_string()))
}

/// Title-case + word-split a SCREAMING_SNAKE or CamelCase status string.
/// (Repovive returns its results.status already as "Accepted" / "Wrong
/// Answer" — but in-flight `judgingStatus` is lowercase like "queued".)
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

/// Parse a Repovive URL.
/// Form: `https://repovive.com/contests/<num>/problems/<letter>`
pub fn parse_url(url: &str) -> Option<(String, String)> {
    let url = url.split('?').next().unwrap_or(url).trim_end_matches('/');
    let re = regex::Regex::new(r"repovive\.com/contests/(\d+)/problems/([A-Za-z0-9_-]+)").ok()?;
    let caps = re.captures(url)?;
    Some((caps[1].to_string(), caps[2].to_string()))
}

pub fn login() {
    let mut client = RepoviveClient::new();
    if let Err(e) = client.login() {
        eprintln!("Login failed: {}", e);
    }
}

pub fn submit(url: String, language: String, source: String) {
    let (contest_num, problem) = match parse_url(&url) {
        Some(p) => p,
        None => {
            eprintln!("Could not parse URL: {}", url);
            return;
        }
    };

    let mut client = RepoviveClient::new();

    println!("Logging in");
    if let Err(e) = client.login() {
        eprintln!("Login failed: {}", e);
        return;
    }

    println!("Submitting");
    let id = match client.submit(&contest_num, &problem, &language, &source) {
        Ok((id, _)) => id,
        Err(e) => {
            eprintln!("{}", e);
            return;
        }
    };

    if let Err(e) = client.poll_verdict(&id) {
        eprintln!("Verdict polling failed: {}", e);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_contest_problem_url() {
        assert_eq!(
            parse_url("https://repovive.com/contests/10/problems/A"),
            Some(("10".to_string(), "A".to_string()))
        );
    }

    #[test]
    fn parse_url_with_trailing_slash_and_query() {
        assert_eq!(
            parse_url("https://repovive.com/contests/10/problems/A/?tab=solutions"),
            Some(("10".to_string(), "A".to_string()))
        );
    }

    #[test]
    fn rejects_non_problem_url() {
        assert_eq!(parse_url("https://repovive.com/contests/10"), None);
    }

    #[test]
    fn language_family_aliases() {
        assert_eq!(find_language("Rust").map(|(id, _)| id), Some(250));
        assert_eq!(find_language("rust").map(|(id, _)| id), Some(250));
        assert_eq!(find_language("rs").map(|(id, _)| id), Some(250));
        assert_eq!(find_language("Java").map(|(id, _)| id), Some(210));
        assert_eq!(find_language("C++").map(|(id, _)| id), Some(200));
        assert_eq!(find_language("Go").map(|(id, _)| id), Some(260));
        assert_eq!(find_language("Python").map(|(id, _)| id), Some(71));
        assert_eq!(find_language("Kotlin").map(|(id, _)| id), Some(78));
    }

    #[test]
    fn language_exact_label() {
        assert_eq!(find_language("Rust 1.40").map(|(id, _)| id), Some(73));
        assert_eq!(find_language("Go 1.13").map(|(id, _)| id), Some(60));
        assert_eq!(find_language("C (GCC 9)").map(|(id, _)| id), Some(50));
    }

    #[test]
    fn language_numeric_passthrough() {
        let (id, label) = find_language("250").unwrap();
        assert_eq!(id, 250);
        assert_eq!(label, "Rust 1.92");
        // Unknown numeric id still passes through.
        let (id, label) = find_language("999").unwrap();
        assert_eq!(id, 999);
        assert_eq!(label, "id=999");
    }

    #[test]
    fn humanize_lowercase_status() {
        assert_eq!(humanize("queued"), "Queued");
        assert_eq!(humanize("running"), "Running");
        assert_eq!(humanize("in_queue"), "In Queue");
    }
}

use crate::clear;
use crate::http::HttpClient;
use crossterm::execute;
use crossterm::style::{Color, ResetColor, SetForegroundColor};
use reqwest::blocking::multipart;
use std::thread;
use std::time::Duration;

const BASE_URL: &str = "https://coderun.yandex.ru";

/// Yandex Passport cookies CodeRun honors. `Session_id` is the only one
/// that's strictly required; the others help when the session is rotated.
const SESSION_COOKIES: &[&str] = &["Session_id", "sessionid2", "yandex_login", "yandexuid"];

pub struct CodeRunClient {
    http: HttpClient,
}

impl CodeRunClient {
    pub fn new() -> Self {
        let http = HttpClient::new(BASE_URL);
        CodeRunClient { http }
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

    fn is_logged_in(&mut self) -> bool {
        if self.http.get_cookie("Session_id").is_none() {
            return false;
        }
        // The CSRF endpoint requires a logged-in session — failure here
        // means our cookies are invalid or expired.
        self.fetch_csrf_token().is_ok()
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

    fn fetch_csrf_token(&mut self) -> Result<String, String> {
        let json = self.http.get_json("/api/csrf")?;
        json.pointer("/result/token")
            .or_else(|| json.pointer("/token"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .ok_or_else(|| format!("No CSRF token in response: {}", json))
    }

    /// Fetch the problem page and extract `__NEXT_DATA__` JSON.
    fn fetch_page_data(&mut self, problem_path: &str) -> Result<serde_json::Value, String> {
        let html = self.http.get_text(problem_path)?;
        let needle = "id=\"__NEXT_DATA__\" type=\"application/json\"";
        let start = html
            .find(needle)
            .ok_or("Could not locate __NEXT_DATA__ on page")?;
        let after = &html[start..];
        let open = after.find('>').ok_or("Malformed __NEXT_DATA__ tag")? + 1;
        let rest = &after[open..];
        let end = rest
            .find("</script>")
            .ok_or("Unterminated __NEXT_DATA__ payload")?;
        let json_str = &rest[..end];
        serde_json::from_str(json_str)
            .map_err(|e| format!("Failed to parse __NEXT_DATA__ JSON: {}", e))
    }

    /// Walk `pageProps.values.*` looking for the problem-context object
    /// (the one that has `problemContextId`, `slug`, `compilers`).
    fn extract_problem(data: &serde_json::Value) -> Result<&serde_json::Value, String> {
        let values = data
            .pointer("/props/pageProps/values")
            .and_then(|v| v.as_object())
            .ok_or("No pageProps.values in __NEXT_DATA__")?;
        for v in values.values() {
            if v.get("problemContextId").is_some() && v.get("compilers").is_some() {
                return Ok(v);
            }
        }
        Err("Problem block not found in __NEXT_DATA__".into())
    }

    fn pick_compiler<'a>(
        problem: &'a serde_json::Value,
        language: &str,
    ) -> Result<&'a serde_json::Value, String> {
        let compilers = problem
            .get("compilers")
            .and_then(|v| v.as_array())
            .ok_or("Problem has no compiler list")?;
        let lang_lower = language.to_lowercase();
        // First try exact slug or title match (case-insensitive).
        for c in compilers {
            let title = c.get("title").and_then(|v| v.as_str()).unwrap_or("");
            let slug = c.get("slug").and_then(|v| v.as_str()).unwrap_or("");
            if title.eq_ignore_ascii_case(language) || slug.eq_ignore_ascii_case(language) {
                if c.get("available").and_then(|v| v.as_bool()).unwrap_or(false) {
                    return Ok(c);
                }
            }
        }
        // Then a starts-with match on the title.
        for c in compilers {
            let title = c.get("title").and_then(|v| v.as_str()).unwrap_or("");
            if title.to_lowercase().starts_with(&lang_lower) {
                if c.get("available").and_then(|v| v.as_bool()).unwrap_or(false) {
                    return Ok(c);
                }
            }
        }
        let available: Vec<String> = compilers
            .iter()
            .filter_map(|c| c.get("title").and_then(|v| v.as_str()).map(String::from))
            .collect();
        Err(format!(
            "Compiler '{}' not found. Available: {}",
            language,
            available.join(", ")
        ))
    }

    pub fn submit(
        &mut self,
        problem_path: &str,
        language: &str,
        source: &str,
    ) -> Result<String, String> {
        let data = self.fetch_page_data(problem_path)?;
        let problem = Self::extract_problem(&data)?;
        let problem_context_id = problem
            .get("problemContextId")
            .and_then(|v| v.as_i64())
            .ok_or("Missing problemContextId")?;
        let problem_slug = problem
            .get("slug")
            .and_then(|v| v.as_str())
            .ok_or("Missing problem slug")?
            .to_string();

        let compiler = Self::pick_compiler(problem, language)?;
        let compiler_slug = compiler
            .get("slug")
            .and_then(|v| v.as_str())
            .ok_or("Compiler has no slug")?
            .to_string();
        let compiler_title = compiler
            .get("title")
            .and_then(|v| v.as_str())
            .unwrap_or(compiler_slug.as_str());
        println!("Language: {}", compiler_title);

        let csrf = self.fetch_csrf_token()?;
        self.http.set_header("x-csrf-token", &csrf);

        let form = multipart::Form::new()
            .text("problemContextId", problem_context_id.to_string())
            .text("compilerSlug", compiler_slug.clone())
            .text("problemSlug", problem_slug.clone())
            .text("isMultifileProblem", "false")
            .part(
                "file",
                multipart::Part::text(source.to_string())
                    .file_name(format!("source.{}", file_ext_for(&compiler_slug)))
                    .mime_str("text/plain")
                    .unwrap(),
            );

        let resp = self
            .http
            .post_multipart("/api/submission/submit", form, "")?;
        let status = resp.status();
        let body = resp
            .text()
            .map_err(|e| format!("Failed to read submit response: {}", e))?;

        if status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN {
            return Err(format!(
                "CodeRun rejected the request ({}). Body: {}",
                status,
                &body[..body.len().min(300)]
            ));
        }
        if !status.is_success() {
            return Err(format!(
                "Submit failed ({}): {}",
                status,
                &body[..body.len().min(300)]
            ));
        }

        let json: serde_json::Value = serde_json::from_str(&body)
            .map_err(|e| format!("Failed to parse submit response: {} body: {}", e, body))?;
        if let Some(err) = json.get("error").filter(|v| !v.is_null()) {
            return Err(format!("Submit returned error: {}", err));
        }
        let global_id = json
            .pointer("/result/globalId")
            .and_then(|v| match v {
                serde_json::Value::String(s) => Some(s.clone()),
                serde_json::Value::Number(n) => Some(n.to_string()),
                _ => None,
            })
            .ok_or_else(|| format!("No globalId in response: {}", body))?;
        println!(
            "Submission url: {}/problem/{}/solutions/{}",
            BASE_URL, problem_slug, global_id
        );
        Ok(global_id)
    }

    pub fn poll_verdict(&mut self, global_id: &str) -> Result<String, String> {
        let mut stdout = std::io::stdout();
        let mut last_len = 0;
        loop {
            clear(last_len);
            let resp = self.http.get(&format!("/api/submission/{}", global_id))?;
            let status = resp.status();
            let body = resp
                .text()
                .map_err(|e| format!("Failed to read poll response: {}", e))?;
            if !status.is_success() {
                return Err(format!(
                    "Poll failed ({}): {}",
                    status,
                    &body[..body.len().min(300)]
                ));
            }
            let json: serde_json::Value = serde_json::from_str(&body)
                .map_err(|e| format!("Failed to parse poll JSON: {} body: {}", e, body))?;
            let submission = json
                .pointer("/result")
                .or_else(|| json.pointer("/result/submission"))
                .or_else(|| Some(&json))
                .unwrap();
            let sub_status = submission
                .get("status")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let verdict = submission
                .get("verdict")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let failed_test = submission
                .get("firstFailedTestNumber")
                .and_then(|v| v.as_i64())
                .unwrap_or(0);

            if is_terminal(sub_status) {
                let mut pretty = pretty_verdict(verdict, sub_status);
                if !verdict.eq_ignore_ascii_case("Ok") && failed_test > 0 {
                    pretty.push_str(&format!(" test #{}", failed_test));
                }
                let color = if verdict.eq_ignore_ascii_case("Ok") {
                    Color::Green
                } else {
                    Color::Red
                };
                let _ = execute!(stdout, SetForegroundColor(color));
                println!("{}", pretty);
                let _ = execute!(stdout, ResetColor);
                return Ok(pretty);
            }

            let progress = if sub_status.is_empty() {
                "Pending".to_string()
            } else {
                humanize(sub_status)
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

fn file_ext_for(slug: &str) -> &'static str {
    match slug {
        "rust" => "rs",
        "cpp" => "cpp",
        "c" => "c",
        "csharp" => "cs",
        "go" => "go",
        "java" => "java",
        "kotlin" => "kt",
        "pypy" | "python" | "python3" => "py",
        "swift" => "swift",
        "dart" => "dart",
        "nodejs_20" | "javascript" => "js",
        _ => "txt",
    }
}

fn is_terminal(status: &str) -> bool {
    match status {
        "" => false,
        // Yandex submission lifecycle: PENDING, TESTING, COMPILING, RUNNING (non-terminal).
        s if s.eq_ignore_ascii_case("pending")
            || s.eq_ignore_ascii_case("testing")
            || s.eq_ignore_ascii_case("compiling")
            || s.eq_ignore_ascii_case("running")
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

/// Convert verdict/status names like "WRONG_ANSWER", "RUNNING", or
/// "WrongAnswer" into space-separated Title Case ("Wrong Answer",
/// "Running").
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
        // CamelCase boundary: lower → Upper.
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

/// Parse a CodeRun URL into a path that can be GET-ed on coderun.yandex.ru.
/// Accepted forms:
///   https://coderun.yandex.ru/problem/<slug>
///   https://coderun.yandex.ru/seasons/<season>/tracks/<track>/problem/<slug>
pub fn parse_url(url: &str) -> Option<String> {
    let url = url.split('?').next().unwrap_or(url).trim_end_matches('/');
    let pos = url.find("coderun.yandex.ru")?;
    let after = &url[pos + "coderun.yandex.ru".len()..];
    if after.contains("/problem/") {
        Some(after.to_string())
    } else {
        None
    }
}

pub fn login() {
    let mut client = CodeRunClient::new();
    if let Err(e) = client.login() {
        eprintln!("Login failed: {}", e);
    }
}

pub fn submit(url: String, language: String, source: String) {
    let mut client = CodeRunClient::new();

    let problem_path = match parse_url(&url) {
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
    let global_id = match client.submit(&problem_path, &language, &source) {
        Ok(id) => id,
        Err(e) => {
            eprintln!("{}", e);
            return;
        }
    };

    if let Err(e) = client.poll_verdict(&global_id) {
        eprintln!("Verdict polling failed: {}", e);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_short_url() {
        assert_eq!(
            parse_url("https://coderun.yandex.ru/problem/thermal-panels"),
            Some("/problem/thermal-panels".to_string())
        );
    }

    #[test]
    fn parse_long_url() {
        assert_eq!(
            parse_url(
                "https://coderun.yandex.ru/seasons/2025-winter/tracks/common/problem/thermal-panels"
            ),
            Some("/seasons/2025-winter/tracks/common/problem/thermal-panels".to_string())
        );
    }

    #[test]
    fn parse_url_with_query() {
        assert_eq!(
            parse_url("https://coderun.yandex.ru/problem/thermal-panels?tab=Solutions"),
            Some("/problem/thermal-panels".to_string())
        );
    }

    #[test]
    fn rejects_non_problem_url() {
        assert_eq!(parse_url("https://coderun.yandex.ru/seasons"), None);
    }

    #[test]
    fn humanize_screaming_snake() {
        assert_eq!(humanize("WRONG_ANSWER"), "Wrong Answer");
        assert_eq!(humanize("TIME_LIMIT_EXCEEDED"), "Time Limit Exceeded");
        assert_eq!(humanize("RUNNING"), "Running");
        assert_eq!(humanize("PENDING"), "Pending");
        assert_eq!(humanize("FINISHED"), "Finished");
    }

    #[test]
    fn humanize_camel_case() {
        assert_eq!(humanize("WrongAnswer"), "Wrong Answer");
        assert_eq!(humanize("TimeLimitExceeded"), "Time Limit Exceeded");
    }

    #[test]
    fn pretty_verdict_ok_maps_to_accepted() {
        assert_eq!(pretty_verdict("OK", "FINISHED"), "Accepted");
        assert_eq!(pretty_verdict("Ok", "FINISHED"), "Accepted");
    }

    #[test]
    fn pretty_verdict_failure_humanized() {
        assert_eq!(pretty_verdict("WRONG_ANSWER", "FINISHED"), "Wrong Answer");
    }
}

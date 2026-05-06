use crate::clear;
use crate::http::HttpClient;
use crossterm::execute;
use crossterm::style::{Color, ResetColor, SetForegroundColor};
use dialoguer::console::Term;
use dialoguer::{Input, Password};
use std::thread;
use std::time::Duration;

pub struct CodechefClient {
    http: HttpClient,
    csrf_token: Option<String>,
}

impl CodechefClient {
    pub fn http_mut(&mut self) -> &mut HttpClient {
        &mut self.http
    }

    pub fn new() -> Self {
        let mut http = HttpClient::new("https://www.codechef.com");
        http.set_header("X-Requested-With", "XMLHttpRequest");
        CodechefClient {
            http,
            csrf_token: None,
        }
    }

    fn update_csrf_header(&mut self) {
        if let Some(token) = &self.csrf_token {
            self.http.set_header("x-csrf-token", token);
        }
    }

    fn fetch_csrf_token(&mut self) -> Result<(), String> {
        let body = self.http.get_text("/")?;
        self.csrf_token = extract_csrf_token(&body);
        self.update_csrf_header();
        if self.csrf_token.is_none() {
            return Err("Could not find CSRF token".to_string());
        }
        Ok(())
    }

    fn is_logged_in(&mut self) -> Result<bool, String> {
        let body = self.http.get_text("/")?;
        self.csrf_token = extract_csrf_token(&body);
        self.update_csrf_header();
        Ok(!body.contains("Sign Up"))
    }

    fn login(&mut self) -> Result<(), String> {
        if self.is_logged_in()? {
            println!("Already logged in");
            return Ok(());
        }

        let login: String = Input::with_theme(&dialoguer::theme::ColorfulTheme::default())
            .with_prompt("Enter your CodeChef login")
            .interact_on(&Term::stdout())
            .unwrap();
        let password: String = Password::with_theme(&dialoguer::theme::ColorfulTheme::default())
            .with_prompt("Enter your CodeChef password")
            .interact_on(&Term::stdout())
            .unwrap();

        self.login_with_credentials(&login, &password)
    }

    pub fn login_with_credentials(&mut self, username: &str, password: &str) -> Result<(), String> {
        // Clear stale cookies to ensure clean login
        self.http.clear_cookies();

        self.fetch_csrf_token()?;

        // Step 1: GET the login form to obtain form_build_id and form's csrfToken
        let form_json = self.http.post_form_json("/api/codechef/login", &[])?;
        let form_html = form_json
            .as_str()
            .ok_or("Login form response is not a string")?;

        let form_build_id = extract_form_field(form_html, "form_build_id")
            .ok_or("Could not find form_build_id in login form")?;
        let form_csrf = extract_form_field(form_html, "csrfToken")
            .or_else(|| self.csrf_token.clone())
            .ok_or("No CSRF token")?;

        // Step 2: POST the login form
        let result = self.http.post_form_json(
            "/api/codechef/login",
            &[
                ("name", username),
                ("pass", password),
                ("csrfToken", &form_csrf),
                ("form_build_id", &form_build_id),
                ("form_id", "ajax_login_form"),
            ],
        )?;

        if result.get("status").and_then(|v| v.as_str()) == Some("success") {
            println!("Login successful");
            self.fetch_csrf_token()?;
            Ok(())
        } else {
            let errors = result
                .get("errors")
                .or_else(|| result.get("message"))
                .map(|v| format!("{}", v))
                .unwrap_or_else(|| format!("{}", result));
            Err(format!("Login failed: {}", errors))
        }
    }

    pub fn find_language_id(&mut self, language: &str) -> Result<(String, String), String> {
        let result = self.http.get_json("/api/ide/all/languages/all")?;
        let languages = result
            .get("languages")
            .ok_or("No languages in response")?;

        let lang_lower = language.to_lowercase();

        if let Some(arr) = languages.as_array() {
            for info in arr {
                let id = info.get("id").and_then(|v| v.as_str()).unwrap_or("");
                let full_name = info.get("full_name").and_then(|v| v.as_str()).unwrap_or("");
                let short_name = info.get("short_name").and_then(|v| v.as_str()).unwrap_or("");
                if full_name.to_lowercase().starts_with(&lang_lower)
                    || short_name.to_lowercase().starts_with(&lang_lower)
                {
                    return Ok((id.to_string(), full_name.to_string()));
                }
            }
        }
        Err(format!("Language '{}' not found", language))
    }

    pub fn submit_solution(
        &mut self,
        problem_code: &str,
        contest_code: &str,
        language_id: &str,
        source: &str,
    ) -> Result<String, String> {
        let result = self.http.post_form_json(
            "/api/ide/submit",
            &[
                ("sourceCode", source),
                ("language", language_id),
                ("problemCode", problem_code),
                ("contestCode", contest_code),
            ],
        )?;

        if result.get("status").and_then(|v| v.as_str()) == Some("error") {
            let errors = result
                .get("errors")
                .map(|v| format!("{}", v))
                .unwrap_or_else(|| format!("{}", result));
            return Err(format!("Submit failed: {}", errors));
        }

        result
            .get("upid")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .or_else(|| result.get("upid").and_then(|v| v.as_i64()).map(|n| n.to_string()))
            .ok_or_else(|| format!("No submission ID in response: {}", result))
    }

    pub fn poll_verdict(&mut self, solution_id: &str) -> Result<String, String> {
        let mut stdout = std::io::stdout();
        let _ = execute!(stdout, SetForegroundColor(Color::Yellow));
        print!("Judging");
        let _ = execute!(stdout, ResetColor);
        let last_len = 7; // "Judging"

        loop {
            thread::sleep(Duration::from_secs(3));

            clear(last_len);
            let result =
                self.http
                    .get_json(&format!("/api/ide/submit?solution_id={}", solution_id))?;

            let result_code = result
                .get("result_code")
                .and_then(|v| v.as_str())
                .unwrap_or("wait");

            if result_code == "wait" {
                continue;
            }

            let (color, verdict_text) = match result_code {
                "accepted" => (Color::Green, "Accepted"),
                "wrong" => (Color::Red, "Wrong Answer"),
                "compile" => (Color::Red, "Compilation Error"),
                "runtime" => (Color::Red, "Runtime Error"),
                "time" => (Color::Red, "Time Limit Exceeded"),
                "partial_accepted" => (Color::DarkYellow, "Partially Correct"),
                _ => (Color::Red, result_code),
            };

            let _ = execute!(stdout, SetForegroundColor(color));
            println!("{}", verdict_text);
            let _ = execute!(stdout, ResetColor);

            if result_code != "compile" {
                self.fetch_subtask_details(solution_id, result_code == "accepted")?;
            }

            return Ok(result_code.to_string());
        }
    }

    fn fetch_subtask_details(
        &mut self,
        solution_id: &str,
        accepted: bool,
    ) -> Result<(), String> {
        let result = self
            .http
            .get_json(&format!("/api/submission-details/{}", solution_id));
        if let Ok(details) = result {
            if let Some(test_info) = details
                .pointer("/data/other_details/testInfo")
                .and_then(|v| v.as_str())
            {
                let mut stdout = std::io::stdout();

                // Parse subtask results: Subtask Score: N% ... Result - VERDICT
                // These span multiple lines, so we parse scores and results separately
                let score_re = regex::Regex::new(r"Subtask Score: (\d+)%").unwrap();
                let result_re =
                    regex::Regex::new(r"Result -\s*</strong>\s*([^<]+)").unwrap();
                let scores: Vec<String> = score_re
                    .captures_iter(test_info)
                    .map(|c| c[1].to_string())
                    .collect();
                let results: Vec<String> = result_re
                    .captures_iter(test_info)
                    .map(|c| c[1].trim().to_string())
                    .collect();
                let subtasks: Vec<(String, String)> = scores
                    .into_iter()
                    .zip(results.into_iter())
                    .collect();

                // Parse total score
                let total_score = regex::Regex::new(r"Total Score = (\d+)%")
                    .unwrap()
                    .captures(test_info)
                    .map(|c| c[1].to_string());

                // Parse per-test results
                let test_re = regex::Regex::new(
                    r"<tr class='(correct|wrong)'><td>([^<]*)</td><td>([^<]*)</td><td>([^<]*)",
                )
                .unwrap();

                if subtasks.len() > 1 {
                    // Multiple subtasks — show per-subtask with colors
                    if let Some(score) = &total_score {
                        if !accepted {
                            let _ = execute!(stdout, SetForegroundColor(Color::Red));
                            println!("  Score: {}%", score);
                            let _ = execute!(stdout, ResetColor);
                        }
                    }

                    // Group tests by subtask number
                    let mut subtask_tests: std::collections::BTreeMap<
                        String,
                        Vec<(String, String, String)>,
                    > = std::collections::BTreeMap::new();
                    for cap in test_re.captures_iter(test_info) {
                        let status = cap[1].to_string();
                        let subtask = cap[2].to_string();
                        let task = cap[3].to_string();
                        let verdict_raw = &cap[4];
                        let verdict = verdict_raw
                            .split("<br")
                            .next()
                            .unwrap_or(verdict_raw)
                            .trim()
                            .to_string();
                        subtask_tests
                            .entry(subtask)
                            .or_default()
                            .push((status, task, verdict));
                    }

                    for (i, (score, result)) in subtasks.iter().enumerate() {
                        let sub_num = (i + 1).to_string();
                        let is_passed = score != "0" || result.contains("Correct") || result.contains("Accepted");
                        let color = if is_passed {
                            Color::Green
                        } else {
                            Color::Red
                        };

                        let mut line = format!("  Subtask {} ({}%): {}", i + 1, score, result);

                        // Find first failing test in this subtask
                        if !is_passed {
                            if let Some(tests) = subtask_tests.get(&sub_num) {
                                let total = tests.len();
                                let passed = tests.iter().filter(|(s, _, _)| s == "correct").count();
                                if let Some((_, task, verdict)) =
                                    tests.iter().find(|(s, _, _)| s == "wrong")
                                {
                                    line = format!(
                                        "  Subtask {} ({}%): {} on task {} ({}/{} passed)",
                                        i + 1, score, verdict, task, passed, total
                                    );
                                }
                            }
                        }

                        let _ = execute!(stdout, SetForegroundColor(color));
                        println!("{}", line);
                        let _ = execute!(stdout, ResetColor);
                    }
                } else if !accepted {
                    // Single subtask — simple output
                    let mut passed = 0;
                    let mut total = 0;
                    let mut first_fail_info = None;

                    for cap in test_re.captures_iter(test_info) {
                        total += 1;
                        if &cap[1] == "correct" {
                            passed += 1;
                        } else if first_fail_info.is_none() {
                            let verdict = cap[4]
                                .split("<br")
                                .next()
                                .unwrap_or(&cap[4])
                                .trim()
                                .to_string();
                            first_fail_info =
                                Some((cap[2].to_string(), cap[3].to_string(), verdict));
                        }
                    }

                    if total > 0 {
                        let _ = execute!(stdout, SetForegroundColor(Color::Red));
                        let mut info = format!("  {}/{} tests passed", passed, total);
                        if let Some((sub, task, verdict)) = &first_fail_info {
                            info.push_str(&format!(
                                ", first failure: subtask {} task {} ({})",
                                sub, task, verdict
                            ));
                        }
                        println!("{}", info);
                        let _ = execute!(stdout, ResetColor);
                    }
                }
            }
        }
        Ok(())
    }
}

fn extract_form_field(html: &str, field_name: &str) -> Option<String> {
    let escaped_pattern = format!("name=\\\\\"{}\\\\\"", field_name);
    let plain_pattern = format!("name=\"{}\"", field_name);

    let search = if html.contains(&escaped_pattern) {
        &escaped_pattern
    } else if html.contains(&plain_pattern) {
        &plain_pattern
    } else {
        return None;
    };

    let pos = html.find(search)?;
    let after = &html[pos..];

    for (val_start, quote_end) in [("value=\\\\\"", "\\\\\""), ("value=\"", "\"")] {
        if let Some(vpos) = after.find(val_start) {
            let start = vpos + val_start.len();
            if let Some(end) = after[start..].find(quote_end) {
                return Some(after[start..start + end].to_string());
            }
        }
    }
    None
}

fn extract_csrf_token(html: &str) -> Option<String> {
    if let Some(pos) = html.find("window.csrfToken") {
        let after = &html[pos..];
        for quote in ['"', '\''] {
            let pattern = format!("= {}", quote);
            if let Some(eq_pos) = after.find(&pattern) {
                let start = eq_pos + pattern.len();
                if let Some(end) = after[start..].find(quote) {
                    return Some(after[start..start + end].to_string());
                }
            }
        }
    }
    None
}

fn parse_url(url: &str) -> Option<(String, String)> {
    let url = url.trim_end_matches('/');

    if let Some(pos) = url.find("/submit/") {
        let problem = &url[pos + 8..];
        let problem = problem.split('/').next().unwrap_or(problem);
        let problem = problem.split('?').next().unwrap_or(problem);
        let before = &url[..pos];
        let contest = before
            .rsplit('/')
            .next()
            .filter(|s| {
                s.chars().all(|c| c.is_alphanumeric())
                    && !s.is_empty()
                    && *s != "com"
                    && !s.starts_with("http")
            })
            .map(|s| s.to_string())
            .unwrap_or_else(|| "PRACTICE".to_string());
        return Some((contest, problem.to_string()));
    }

    if let Some(pos) = url.find("/problems/") {
        let problem = &url[pos + 10..];
        let problem = problem.split('/').next().unwrap_or(problem);
        let problem = problem.split('?').next().unwrap_or(problem);
        let before = &url[..pos];
        let contest = before
            .rsplit('/')
            .next()
            .filter(|s| {
                s.chars().all(|c| c.is_alphanumeric() || c == '_')
                    && !s.is_empty()
                    && *s != "com"
                    && !s.starts_with("http")
            })
            .map(|s| s.to_string())
            .unwrap_or_else(|| "PRACTICE".to_string());
        return Some((contest, problem.to_string()));
    }

    None
}

pub fn login() {
    let mut client = CodechefClient::new();
    if let Err(e) = client.login() {
        eprintln!("Login failed: {}", e);
    }
}

pub fn submit(url: String, language: String, source: String) {
    let mut client = CodechefClient::new();

    println!("Logging in");
    if let Err(e) = client.login() {
        eprintln!("Login failed: {}", e);
        return;
    }

    let (contest_code, problem_code) = match parse_url(&url) {
        Some(parsed) => parsed,
        None => {
            eprintln!("Could not parse URL: {}", url);
            return;
        }
    };

    let (lang_id, lang_name) = match client.find_language_id(&language) {
        Ok(found) => found,
        Err(e) => {
            eprintln!("{}", e);
            return;
        }
    };
    println!("Language: {} (id={})", lang_name, lang_id);

    println!("Submitting");
    let solution_id =
        match client.submit_solution(&problem_code, &contest_code, &lang_id, &source) {
            Ok(id) => id,
            Err(e) => {
                eprintln!("{}", e);
                return;
            }
        };
    println!(
        "Submission url: https://www.codechef.com/viewsolution/{}",
        solution_id
    );

    if let Err(e) = client.poll_verdict(&solution_id) {
        eprintln!("Verdict polling failed: {}", e);
    }
}

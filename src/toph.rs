use crate::clear;
use crate::http::HttpClient;
use crossterm::execute;
use crossterm::style::{Color, ResetColor, SetForegroundColor};
use dialoguer::console::Term;
use dialoguer::{Input, Password};
use reqwest::blocking::multipart;
use std::thread;
use std::time::Duration;

pub struct TophClient {
    http: HttpClient,
    token_id: Option<String>,
    session_cookies: String,
    last_pretty_id: Option<String>,
}

impl TophClient {
    pub fn http_mut(&mut self) -> &mut HttpClient {
        &mut self.http
    }

    pub fn token_id(&self) -> Option<&str> {
        self.token_id.as_deref()
    }

    pub fn new() -> Self {
        TophClient {
            http: HttpClient::new("https://toph.co"),
            token_id: None,
            session_cookies: String::new(),
            last_pretty_id: None,
        }
    }

    fn login(&mut self) -> Result<(), String> {
        // Try saved credentials first
        if let (Some(user), Some(pass)) = (
            self.http.get_cookie("toph_user"),
            self.http.get_cookie("toph_pass"),
        ) {
            if self.login_with_credentials(&user, &pass).is_ok() {
                return Ok(());
            }
        }

        let login: String = Input::with_theme(&dialoguer::theme::ColorfulTheme::default())
            .with_prompt("Enter your Toph login")
            .interact_on(&Term::stdout())
            .unwrap();
        let password: String = Password::with_theme(&dialoguer::theme::ColorfulTheme::default())
            .with_prompt("Enter your Toph password")
            .interact_on(&Term::stdout())
            .unwrap();

        let result = self.login_with_credentials(&login, &password);
        if result.is_ok() {
            self.http.set_cookie("toph_user", &login);
            self.http.set_cookie("toph_pass", &password);
        }
        result
    }

    pub fn login_with_credentials(&mut self, username: &str, password: &str) -> Result<(), String> {
        let client = reqwest::blocking::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .unwrap();
        let resp = client
            .post("https://toph.co/login")
            .form(&[("handle", username), ("password", password)])
            .send()
            .map_err(|e| format!("Login request failed: {}", e))?;

        // Capture session cookies
        let mut cookies = Vec::new();
        for val in resp.headers().get_all("set-cookie") {
            if let Ok(s) = val.to_str() {
                if let Some(nv) = s.split(';').next() {
                    cookies.push(nv.to_string());
                }
            }
        }
        self.session_cookies = cookies.join("; ");

        // Check if login succeeded by fetching home page with cookies
        let home = reqwest::blocking::Client::new()
            .get("https://toph.co/")
            .header("Cookie", &self.session_cookies)
            .send()
            .map_err(|e| format!("Failed to check login: {}", e))?
            .text()
            .unwrap_or_default();

        if !home.contains("tokenId") {
            return Err("Login failed: wrong username or password".to_string());
        }

        // Extract tokenId
        if let Some(pos) = home.find("tokenId") {
            let after = &home[pos + 7..];
            let after =
                after.trim_start_matches(|c: char| c.is_whitespace() || c == ':' || c == '=');
            let quote = after.chars().next().unwrap_or(' ');
            if quote == '"' || quote == '\'' {
                if let Some(end) = after[1..].find(quote) {
                    self.token_id = Some(after[1..1 + end].to_string());
                }
            }
        }

        if self.token_id.is_some() {
            println!("Login successful");
            Ok(())
        } else {
            Err("Login failed: could not extract token".to_string())
        }
    }

    pub fn get_problem_id(&mut self, problem_path: &str) -> Result<String, String> {
        let body = reqwest::blocking::Client::new()
            .get(format!("https://toph.co{}", problem_path))
            .header("Cookie", &self.session_cookies)
            .send()
            .map_err(|e| format!("Failed to load problem page: {}", e))?
            .text()
            .unwrap_or_default();
        // Parse data-codepanel-problemid=VALUE or data-codepanel-problemid="VALUE"
        let marker = "data-codepanel-problemid=";
        if let Some(pos) = body.find(marker) {
            let start = pos + marker.len();
            let rest = &body[start..];
            let rest = rest.trim_start_matches('"');
            let end = rest
                .find(|c: char| c == '"' || c == ' ' || c == '>' || c == '\n')
                .unwrap_or(rest.len());
            if end > 0 {
                return Ok(rest[..end].to_string());
            }
        }
        Err("Could not find problem ID on page".to_string())
    }

    pub fn find_language_id(
        &mut self,
        problem_id: &str,
        language: &str,
    ) -> Result<(String, String), String> {
        let token = self.token_id.as_ref().ok_or("Not logged in")?;
        let resp = reqwest::blocking::Client::new()
            .get(format!(
                "https://toph.co/api/problems/{}/languages",
                problem_id
            ))
            .header("Authorization", format!("Token {}", token))
            .header("Cookie", &self.session_cookies)
            .send()
            .map_err(|e| format!("Failed to get languages: {}", e))?;
        let body = resp
            .text()
            .map_err(|e| format!("Failed to read languages: {}", e))?;
        let json: serde_json::Value =
            serde_json::from_str(&body).map_err(|e| format!("Failed to parse languages: {}", e))?;

        let lang_lower = language.to_lowercase();
        if let Some(arr) = json.as_array() {
            for lang in arr {
                let label = lang.get("label").and_then(|v| v.as_str()).unwrap_or("");
                let family = lang.get("family").and_then(|v| v.as_str()).unwrap_or("");
                let id = lang.get("id").and_then(|v| v.as_str()).unwrap_or("");
                if label.to_lowercase().starts_with(&lang_lower)
                    || family.to_lowercase().starts_with(&lang_lower)
                {
                    return Ok((id.to_string(), label.to_string()));
                }
            }
        }
        Err(format!("Language '{}' not found", language))
    }

    pub fn submit_solution(
        &mut self,
        problem_id: &str,
        language_id: &str,
        source: &str,
    ) -> Result<String, String> {
        let token = self.token_id.as_ref().ok_or("Not logged in")?.clone();

        let form = multipart::Form::new()
            .text("languageId", language_id.to_string())
            .part(
                "source",
                multipart::Part::text(source.to_string())
                    .file_name("source.cpp")
                    .mime_str("text/plain")
                    .unwrap(),
            );

        self.do_submit(&format!("/api/problems/{}/submissions", problem_id), &token, form)
    }

    pub fn submit_challenge(
        &mut self,
        challenge_id: &str,
        practice_id: &str,
        language_id: &str,
        source: &str,
    ) -> Result<String, String> {
        let token = self.token_id.as_ref().ok_or("Not logged in")?.clone();

        let form = multipart::Form::new()
            .text("languageId", language_id.to_string())
            .part(
                "source",
                multipart::Part::text(source.to_string())
                    .file_name("source.cpp")
                    .mime_str("text/plain")
                    .unwrap(),
            );

        self.do_submit(
            &format!("/api/challenges/{}/submissions?practice={}", challenge_id, practice_id),
            &token,
            form,
        )
    }

    fn do_submit(
        &mut self,
        path: &str,
        token: &str,
        form: multipart::Form,
    ) -> Result<String, String> {
        let resp = reqwest::blocking::Client::new()
            .post(format!("https://toph.co{}", path))
            .header("Authorization", format!("Token {}", token))
            .header("Cookie", &self.session_cookies)
            .multipart(form)
            .send()
            .map_err(|e| format!("Submit failed: {}", e))?;

        let body = resp
            .text()
            .map_err(|e| format!("Failed to read submit response: {}", e))?;
        let json: serde_json::Value = serde_json::from_str(&body).map_err(|e| {
            format!(
                "Failed to parse response: {} body: {}",
                e,
                &body[..body.len().min(200)]
            )
        })?;

        let pretty_id = json
            .get("prettyId")
            .and_then(|v| v.as_i64())
            .map(|n| n.to_string())
            .or_else(|| {
                json.get("prettyId")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string())
            });
        let id = json
            .get("id")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .ok_or_else(|| format!("No submission ID in response: {}", body))?;

        if let Some(pid) = &pretty_id {
            println!("Submission url: https://toph.co/s/{}", pid);
            self.last_pretty_id = Some(pid.clone());
        }

        Ok(id)
    }

    pub fn poll_verdict(&mut self, submission_id: &str) -> Result<String, String> {
        // Poll via API first, fall back to HTML page
        // API endpoint: GET /api/submissions/{object_id}
        // Also works: GET /api/submissions/~{pretty_id}
        let poll_id = if self.last_pretty_id.is_some() {
            format!("~{}", self.last_pretty_id.as_ref().unwrap())
        } else {
            submission_id.to_string()
        };
        let mut stdout = std::io::stdout();
        let mut last_len = 0;

        let token = self.token_id.as_ref().ok_or("Not logged in")?.clone();
        loop {
            let resp = reqwest::blocking::Client::new()
                .get(format!("https://toph.co/api/submissions/{}", poll_id))
                .header("Authorization", format!("Token {}", token))
                .header("Cookie", &self.session_cookies)
                .send()
                .map_err(|e| format!("Poll failed: {}", e))?;
            let body = resp.text().unwrap_or_default();
            let json: serde_json::Value = match serde_json::from_str(&body) {
                Ok(v) => v,
                Err(_) => {
                    thread::sleep(Duration::from_secs(3));
                    continue;
                }
            };

            let status = json.get("status").and_then(|v| v.as_i64()).unwrap_or(0);
            let verdict = json
                .get("verdict")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let verdict_check_no = json
                .get("verdictCheckNo")
                .and_then(|v| v.as_i64())
                .unwrap_or(0);
            let checks = json.get("checks").and_then(|v| v.as_array());
            let total_checks = checks.map(|c| c.len()).unwrap_or(0);

            if !verdict.is_empty() && status > 2 {
                // Done
                clear(last_len);
                let is_accepted = verdict == "Accepted";
                let color = if is_accepted {
                    Color::Green
                } else {
                    Color::Red
                };
                let mut display = verdict.to_string();
                if !is_accepted && verdict_check_no > 0 {
                    if total_checks > 0 {
                        display.push_str(&format!(" on test {}/{}", verdict_check_no, total_checks));
                    } else {
                        display.push_str(&format!(" on test {}", verdict_check_no));
                    }
                }
                let _ = execute!(stdout, SetForegroundColor(color));
                println!("{}", display);
                let _ = execute!(stdout, ResetColor);
                return Ok(verdict.to_string());
            }

            // Still running
            let progress = if let Some(checks) = checks {
                let tested = checks
                    .iter()
                    .filter(|c| {
                        let v = c.get("verdict").and_then(|v| v.as_str()).unwrap_or("");
                        !v.is_empty()
                    })
                    .count();
                if total_checks > 0 {
                    format!("Running ({}/{})", tested, total_checks)
                } else {
                    "Running".to_string()
                }
            } else if status <= 1 {
                "Pending".to_string()
            } else {
                "Running".to_string()
            };

            clear(last_len);
            let _ = execute!(stdout, SetForegroundColor(Color::Yellow));
            print!("{}", progress);
            let _ = execute!(stdout, ResetColor);
            last_len = progress.len();
            thread::sleep(Duration::from_secs(2));
        }
    }
}

enum TophUrl {
    Problem(String), // /p/slug
    Arena {
        practice_id: String,
        challenge_id: String,
    },
}

/// Parse a toph URL
/// https://toph.co/p/problem-slug -> Problem("/p/problem-slug")
/// https://toph.co/arena?practice=XXX#!/p/YYY -> Arena { arena_url, problem_hash }
fn parse_url(url: &str) -> Option<TophUrl> {
    // Arena URL: /arena?practice=PRACTICE_ID#!/p/CHALLENGE_ID
    if url.contains("/arena") {
        let practice_re = regex::Regex::new(r"practice=([0-9a-f]+)").unwrap();
        let challenge_re = regex::Regex::new(r"#!/p/([0-9a-f]+)").unwrap();
        if let (Some(p), Some(c)) = (practice_re.captures(url), challenge_re.captures(url)) {
            return Some(TophUrl::Arena {
                practice_id: p[1].to_string(),
                challenge_id: c[1].to_string(),
            });
        }
        return None;
    }
    // Regular URL: /p/problem-slug
    if let Some(pos) = url.find("toph.co/p/") {
        let path = &url[pos + 7..]; // "/p/..."
        let path = path.split('?').next().unwrap_or(path);
        return Some(TophUrl::Problem(path.to_string()));
    }
    None
}

/// Resolve arena problem hash to archive /p/slug by fetching the arena page
fn resolve_arena_problem(session_cookies: &str, arena_url: &str, problem_hash: &str) -> Result<String, String> {
    // Manually follow redirects to preserve cookies
    let client = reqwest::blocking::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap();
    let mut url = arena_url.to_string();
    let mut body = String::new();
    for _ in 0..5 {
        let resp = client
            .get(&url)
            .header("Cookie", session_cookies)
            .send()
            .map_err(|e| format!("Failed to load arena page: {}", e))?;
        if resp.status().is_redirection() {
            if let Some(loc) = resp.headers().get("location") {
                let loc = loc.to_str().unwrap_or("");
                url = if loc.starts_with('/') {
                    format!("https://toph.co{}", loc)
                } else {
                    loc.to_string()
                };
                continue;
            }
        }
        body = resp.text().unwrap_or_default();
        break;
    }

    // Two possible formats:
    // 1. Older: hash:"#!/p/HASH",redirect:"problem",location:"/p/SLUG"
    // 2. Newer: {id:"HASH",contestId:"...",problemId:"PROBLEM_ID",...}

    // Try format 2: extract problemId from challenges array
    let pattern2 = format!(
        r#"id:"{}"[^}}]*?problemId:"([0-9a-f]+)""#,
        regex::escape(problem_hash)
    );
    if let Ok(re) = regex::Regex::new(&pattern2) {
        if let Some(cap) = re.captures(&body) {
            // Return as __id: so submit uses problemId directly
            return Ok(format!("__id:{}", &cap[1]));
        }
    }

    // Try format 1: extract slug from route table
    let pattern1 = format!(
        "hash:\"#!/p/{}\",redirect:\"problem\",location:\"/p/([^\"]+)\"",
        regex::escape(problem_hash)
    );
    if let Ok(re) = regex::Regex::new(&pattern1) {
        if let Some(cap) = re.captures(&body) {
            return Ok(format!("/p/{}", &cap[1]));
        }
    }

    Err(format!("Problem {} not found in arena", problem_hash))
}

pub fn login() {
    let mut client = TophClient::new();
    if let Err(e) = client.login() {
        eprintln!("Login failed: {}", e);
    }
}

pub fn submit(url: String, language: String, source: String) {
    let mut client = TophClient::new();

    println!("Logging in");
    if let Err(e) = client.login() {
        eprintln!("Login failed: {}", e);
        return;
    }

    let parsed = match parse_url(&url) {
        Some(p) => p,
        None => {
            eprintln!("Could not parse URL: {}", url);
            return;
        }
    };

    let submission_id = match parsed {
        TophUrl::Problem(path) => {
            let problem_id = match client.get_problem_id(&path) {
                Ok(id) => id,
                Err(e) => {
                    eprintln!("{}", e);
                    return;
                }
            };
            let (lang_id, lang_name) = match client.find_language_id(&problem_id, &language) {
                Ok(found) => found,
                Err(e) => {
                    eprintln!("{}", e);
                    return;
                }
            };
            println!("Language: {}", lang_name);
            println!("Submitting");
            match client.submit_solution(&problem_id, &lang_id, &source) {
                Ok(id) => id,
                Err(e) => {
                    eprintln!("{}", e);
                    return;
                }
            }
        }
        TophUrl::Arena {
            practice_id,
            challenge_id,
        } => {
            // For arena, resolve challenge to get the problem ID for language lookup
            let arena_url = format!(
                "https://toph.co/arena?practice={}",
                practice_id
            );
            let problem_path = match resolve_arena_problem(
                &client.session_cookies,
                &arena_url,
                &challenge_id,
            ) {
                Ok(path) => path,
                Err(e) => {
                    eprintln!("{}", e);
                    return;
                }
            };
            let problem_id = if problem_path.starts_with("__id:") {
                problem_path[5..].to_string()
            } else {
                match client.get_problem_id(&problem_path) {
                    Ok(id) => id,
                    Err(e) => {
                        eprintln!("{}", e);
                        return;
                    }
                }
            };
            let (lang_id, lang_name) = match client.find_language_id(&problem_id, &language) {
                Ok(found) => found,
                Err(e) => {
                    eprintln!("{}", e);
                    return;
                }
            };
            println!("Language: {}", lang_name);
            println!("Submitting");
            match client.submit_challenge(&challenge_id, &practice_id, &lang_id, &source) {
                Ok(id) => id,
                Err(e) => {
                    eprintln!("{}", e);
                    return;
                }
            }
        }
    };

    if let Err(e) = client.poll_verdict(&submission_id) {
        eprintln!("Verdict polling failed: {}", e);
    }
}

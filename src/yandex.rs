use crate::clear;
use crate::http::HttpClient;
use crossterm::execute;
use crossterm::style::{Color, ResetColor, SetForegroundColor};
use reqwest::blocking::multipart;
use std::thread;
use std::time::Duration;

const CLIENT_ID: &str = "b22a4126b26241e19b0fe79adead12e5";
const CLIENT_SECRET: &str = "6b8e9b4762ef412e9813e9284fdf2484";

pub struct YandexClient {
    http: HttpClient,
    access_token: Option<String>,
    refresh_token: Option<String>,
}

impl YandexClient {
    pub fn http_mut(&mut self) -> &mut HttpClient {
        &mut self.http
    }

    pub fn new() -> Self {
        let mut http = HttpClient::new("https://api.contest.yandex.net/api/public/v2");
        http.disable_cookie_sending();
        let saved_access = http.get_cookie("yandex_access_token");
        let saved_refresh = http.get_cookie("yandex_refresh_token");
        YandexClient {
            http,
            access_token: saved_access,
            refresh_token: saved_refresh,
        }
    }

    pub fn set_refresh_token(&mut self, token: &str) {
        self.refresh_token = Some(token.to_string());
    }

    fn save_tokens(&mut self) {
        if let Some(ref token) = self.access_token {
            self.http.set_cookie("yandex_access_token", token);
        }
        if let Some(ref token) = self.refresh_token {
            self.http.set_cookie("yandex_refresh_token", token);
        }
    }

    fn set_access_token(&mut self, token: &str) {
        self.access_token = Some(token.to_string());
        self.http
            .set_header("Authorization", &format!("OAuth {}", token));
    }

    fn is_logged_in(&mut self) -> bool {
        if self.access_token.is_none() {
            return false;
        }
        self.http
            .set_header(
                "Authorization",
                &format!("OAuth {}", self.access_token.as_ref().unwrap()),
            );
        let result = self.http.get_json("/service/introspect");
        match result {
            Ok(json) => !json.get("error").is_some(),
            Err(_) => {
                self.try_refresh_token()
            }
        }
    }

    pub fn try_refresh_token(&mut self) -> bool {
        let refresh = match &self.refresh_token {
            Some(t) => t.clone(),
            None => return false,
        };

        let client = reqwest::blocking::Client::new();
        let resp = client
            .post("https://oauth.yandex.ru/token")
            .form(&[
                ("grant_type", "refresh_token"),
                ("refresh_token", refresh.as_str()),
                ("client_id", CLIENT_ID),
                ("client_secret", CLIENT_SECRET),
            ])
            .send();

        if let Ok(resp) = resp {
            if let Ok(json) = resp.json::<serde_json::Value>() {
                if let Some(access) = json.get("access_token").and_then(|v| v.as_str()) {
                    self.set_access_token(access);
                    if let Some(new_refresh) =
                        json.get("refresh_token").and_then(|v| v.as_str())
                    {
                        self.refresh_token = Some(new_refresh.to_string());
                    }
                    self.save_tokens();
                    return true;
                }
            }
        }
        false
    }

    pub fn login(&mut self) -> Result<(), String> {
        if self.is_logged_in() {
            println!("Already logged in");
            return Ok(());
        }
        self.login_oauth()
    }

    fn login_oauth(&mut self) -> Result<(), String> {
        let client = reqwest::blocking::Client::new();

        // Request a device code
        let resp = client
            .post("https://oauth.yandex.ru/device/code")
            .form(&[
                ("client_id", CLIENT_ID),
                ("scope", "contest:submit"),
            ])
            .send()
            .map_err(|e| format!("Device code request failed: {}", e))?;

        let device: serde_json::Value = resp
            .json()
            .map_err(|e| format!("Failed to parse device code response: {}", e))?;

        let device_code = device
            .get("device_code")
            .and_then(|v| v.as_str())
            .ok_or("No device_code in response")?;
        let user_code = device
            .get("user_code")
            .and_then(|v| v.as_str())
            .ok_or("No user_code in response")?;
        let verification_url = device
            .get("verification_url")
            .and_then(|v| v.as_str())
            .unwrap_or("https://ya.ru/device");
        let interval = device
            .get("interval")
            .and_then(|v| v.as_u64())
            .unwrap_or(5);

        println!("Visit {} and enter code: {}", verification_url, user_code);
        let _ = open::that(verification_url);

        // Poll for token
        let json: serde_json::Value = loop {
            thread::sleep(Duration::from_secs(interval));
            let resp = client
                .post("https://oauth.yandex.ru/token")
                .form(&[
                    ("grant_type", "device_code"),
                    ("code", device_code),
                    ("client_id", CLIENT_ID),
                    ("client_secret", CLIENT_SECRET),
                ])
                .send()
                .map_err(|e| format!("Token poll failed: {}", e))?;

            let json: serde_json::Value = resp
                .json()
                .map_err(|e| format!("Failed to parse token response: {}", e))?;

            if json.get("access_token").is_some() {
                break json;
            }

            let error = json
                .get("error")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            match error {
                "authorization_pending" => continue,
                "slow_down" => {
                    thread::sleep(Duration::from_secs(5));
                    continue;
                }
                _ => {
                    let desc = json
                        .get("error_description")
                        .and_then(|v| v.as_str())
                        .unwrap_or(error);
                    return Err(format!("Authorization failed: {}", desc));
                }
            }
        };

        if let Some(access) = json.get("access_token").and_then(|v| v.as_str()) {
            self.set_access_token(access);
            self.refresh_token = json
                .get("refresh_token")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            self.save_tokens();
            println!("Login successful");
            Ok(())
        } else {
            Err(format!(
                "Token exchange failed: {}",
                json.get("error_description")
                    .or(json.get("error"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown error")
            ))
        }
    }

    pub fn find_compiler(&mut self, language: &str) -> Result<(String, String), String> {
        let json = self.http.get_json("/compilers")?;
        let lang_lower = language.to_lowercase();

        let compilers = json
            .get("compilers")
            .and_then(|v| v.as_array())
            .or_else(|| json.as_array());

        if let Some(compilers) = compilers {
            for c in compilers {
                let name = c.get("name").and_then(|v| v.as_str()).unwrap_or("");
                let id = c.get("id").and_then(|v| v.as_str()).unwrap_or("");
                let deprecated = c.get("deprecated").and_then(|v| v.as_bool()).unwrap_or(false);
                if !deprecated && name.to_lowercase().starts_with(&lang_lower) {
                    return Ok((id.to_string(), name.to_string()));
                }
            }
        }
        Err(format!("Compiler '{}' not found", language))
    }

    pub fn submit(
        &mut self,
        contest_id: &str,
        problem: &str,
        compiler_id: &str,
        source: &str,
    ) -> Result<i64, String> {
        let token = self.access_token.as_ref().ok_or("Not logged in")?.clone();

        let form = multipart::Form::new()
            .text("compiler", compiler_id.to_string())
            .text("problem", problem.to_string())
            .part(
                "file",
                multipart::Part::text(source.to_string())
                    .file_name("source.cpp")
                    .mime_str("text/plain")
                    .unwrap(),
            );

        let resp = self.http.post_multipart(
            &format!("/contests/{}/submissions", contest_id),
            form,
            &format!("OAuth {}", token),
        )?;

        let status = resp.status();
        let body = resp
            .text()
            .map_err(|e| format!("Failed to read submit response: {}", e))?;

        if !status.is_success() {
            return Err(format!("Submit failed ({}): {}", status, body));
        }

        let json: serde_json::Value = serde_json::from_str(&body)
            .map_err(|e| format!("Failed to parse response: {}", e))?;

        json.get("runId")
            .and_then(|v| v.as_i64())
            .or_else(|| json.get("id").and_then(|v| v.as_i64()))
            .ok_or_else(|| format!("No submission ID in response: {}", body))
    }

    pub fn poll_verdict(&mut self, contest_id: &str, run_id: i64) -> Result<String, String> {
        let mut stdout = std::io::stdout();
        let mut last_len = 0;

        loop {
            let result = self.http.get_json(&format!(
                "/contests/{}/submissions/{}",
                contest_id, run_id
            ))?;

            let verdict = result
                .get("verdict")
                .and_then(|v| v.as_str())
                .unwrap_or("");

            let test_number = result
                .get("testNumber")
                .and_then(|v| v.as_i64())
                .unwrap_or(0);

            let is_pending = verdict.is_empty()
                || verdict == "null"
                || verdict == "No report"
                || verdict == "Waiting"
                || verdict == "Running"
                || verdict == "Compiling";

            if is_pending {
                let progress = if test_number > 0 {
                    format!("Running test {}", test_number)
                } else if verdict == "Compiling" {
                    "Compiling".to_string()
                } else if verdict == "Running" {
                    "Running".to_string()
                } else {
                    "Pending".to_string()
                };
                clear(last_len);
                let _ = execute!(stdout, SetForegroundColor(Color::Yellow));
                print!("{}", progress);
                let _ = execute!(stdout, ResetColor);
                last_len = progress.len();
                thread::sleep(Duration::from_secs(2));
                continue;
            }

            clear(last_len);

            let is_accepted = verdict == "OK" || verdict == "ok" || verdict == "Accepted";
            let is_ce = verdict == "CE"
                || verdict == "CompilationError"
                || verdict == "COMPILATION_ERROR";

            let color = if is_accepted {
                Color::Green
            } else {
                Color::Red
            };

            // Convert CamelCase verdict to readable form
            let text = camel_to_words(verdict);

            let mut display = text;
            if !is_accepted && !is_ce && test_number > 0 {
                display.push_str(&format!(" on test {}", test_number));
            }

            if let Some(score) = result.get("score").and_then(|v| v.as_f64()) {
                if score > 0.0 {
                    display.push_str(&format!(" ({:.0} pts)", score));
                }
            }

            let _ = execute!(stdout, SetForegroundColor(color));
            println!("{}", display);
            let _ = execute!(stdout, ResetColor);
            return Ok(verdict.to_string());
        }
    }
}

/// Parse yandex contest URL
/// https://contest.yandex.com/contest/3/problems/B/ -> ("3", "B")
/// https://contest.yandex.ru/contest/3/problems/B/ -> ("3", "B")
fn parse_url(url: &str) -> Option<(String, String)> {
    // Strip query parameters first
    let url = url.split('?').next().unwrap_or(url);
    let url = url.trim_end_matches('/');
    // Try with specific problem: contest/ID/problems/LETTER
    let re = regex::Regex::new(r"contest/(\d+)/problems/([A-Za-z0-9_]+)").ok()?;
    if let Some(caps) = re.captures(url) {
        return Some((caps[1].to_string(), caps[2].to_string()));
    }
    // Contest URL without problem — default to A
    let re = regex::Regex::new(r"contest/(\d+)").ok()?;
    let caps = re.captures(url)?;
    Some((caps[1].to_string(), "A".to_string()))
}

/// Convert "WrongAnswer" -> "Wrong Answer", "OK" -> "Accepted"
fn camel_to_words(s: &str) -> String {
    match s {
        "OK" | "ok" | "Accepted" => return "Accepted".to_string(),
        _ => {}
    }
    let mut result = String::new();
    for c in s.chars() {
        if c.is_uppercase() && !result.is_empty() {
            result.push(' ');
        }
        result.push(c);
    }
    result
}

pub fn login() {
    let mut client = YandexClient::new();
    if let Err(e) = client.login() {
        eprintln!("Login failed: {}", e);
    }
}

pub fn submit(url: String, language: String, source: String) {
    let mut client = YandexClient::new();

    println!("Logging in");
    if let Err(e) = client.login() {
        eprintln!("Login failed: {}", e);
        return;
    }

    let (contest_id, problem) = match parse_url(&url) {
        Some(parsed) => parsed,
        None => {
            eprintln!("Could not parse URL: {}", url);
            return;
        }
    };

    let (compiler_id, compiler_name) = match client.find_compiler(&language) {
        Ok(found) => found,
        Err(e) => {
            eprintln!("{}", e);
            return;
        }
    };
    println!("Compiler: {}", compiler_name);

    println!("Submitting");
    let run_id = match client.submit(&contest_id, &problem, &compiler_id, &source) {
        Ok(id) => {
            println!(
                "Submission url: https://contest.yandex.com/contest/{}/run-report/{}/",
                contest_id, id
            );
            id
        }
        Err(e) => {
            eprintln!("{}", e);
            return;
        }
    };

    if let Err(e) = client.poll_verdict(&contest_id, run_id) {
        eprintln!("Verdict polling failed: {}", e);
    }
}

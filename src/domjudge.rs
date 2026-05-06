use crate::clear;
use crate::http::HttpClient;
use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use crossterm::execute;
use crossterm::style::{Color, ResetColor, SetForegroundColor};
use dialoguer::console::Term;
use dialoguer::{Input, Password};
use regex::Regex;
use reqwest::blocking::multipart;
use std::collections::HashMap;
use std::thread;
use std::time::Duration;

pub struct DomjudgeClient {
    http: HttpClient,
    base_url: String,
    auth_header: Option<String>,
}

impl DomjudgeClient {
    pub fn new(base_url: &str) -> Self {
        let base = base_url.trim_end_matches('/').to_string();
        DomjudgeClient {
            http: HttpClient::new(&base),
            base_url: base,
            auth_header: None,
        }
    }

    pub fn http_mut(&mut self) -> &mut HttpClient {
        &mut self.http
    }

    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    pub fn is_domjudge(&mut self) -> bool {
        let body = match self.http.get_text("/api/v4/info") {
            Ok(b) => b,
            Err(_) => return false,
        };
        let json: serde_json::Value = match serde_json::from_str(&body) {
            Ok(j) => j,
            Err(_) => return false,
        };
        json.get("domjudge").is_some()
    }

    pub fn login(&mut self) -> Result<(), String> {
        if let (Some(user), Some(pass)) = (
            self.http.get_cookie("domjudge_user"),
            self.http.get_cookie("domjudge_pass"),
        ) {
            if self.try_credentials(&user, &pass).is_ok() {
                return Ok(());
            }
        }

        let user: String = Input::with_theme(&dialoguer::theme::ColorfulTheme::default())
            .with_prompt(format!("Enter username for {}", self.base_url))
            .interact_on(&Term::stdout())
            .unwrap();
        let pass: String = Password::with_theme(&dialoguer::theme::ColorfulTheme::default())
            .with_prompt(format!("Enter password for {}", self.base_url))
            .interact_on(&Term::stdout())
            .unwrap();

        self.login_with_credentials(&user, &pass)
    }

    pub fn login_with_credentials(&mut self, user: &str, pass: &str) -> Result<(), String> {
        self.try_credentials(user, pass)?;
        self.http.set_cookie("domjudge_user", user);
        self.http.set_cookie("domjudge_pass", pass);
        println!("Login successful");
        Ok(())
    }

    fn try_credentials(&mut self, user: &str, pass: &str) -> Result<(), String> {
        let header = format!("Basic {}", STANDARD.encode(format!("{}:{}", user, pass)));
        self.auth_header = Some(header.clone());
        self.http.set_header("Authorization", &header);

        let resp = self.http.get("/api/v4/user")?;
        let status = resp.status();
        let body = resp
            .text()
            .map_err(|e| format!("Failed to read login response: {}", e))?;
        if status.as_u16() == 401 || status.as_u16() == 403 {
            self.auth_header = None;
            return Err("invalid username or password".to_string());
        }
        if !status.is_success() {
            self.auth_header = None;
            return Err(format!("login check failed ({}): {}", status, body.trim()));
        }
        let json: serde_json::Value = serde_json::from_str(&body)
            .map_err(|e| format!("invalid /api/v4/user response: {}", e))?;
        if json.get("id").is_none() && json.get("username").is_none() {
            self.auth_header = None;
            return Err(format!("login failed: {}", body.trim()));
        }
        Ok(())
    }

    fn list_active_contests(&mut self) -> Result<Vec<serde_json::Value>, String> {
        let json = self.http.get_json("/api/v4/contests?onlyActive=true")?;
        json.as_array()
            .cloned()
            .ok_or_else(|| "contests response is not an array".to_string())
    }

    fn list_problems(&mut self, cid: &str) -> Result<Vec<serde_json::Value>, String> {
        let json = self
            .http
            .get_json(&format!("/api/v4/contests/{}/problems", cid))?;
        json.as_array()
            .cloned()
            .ok_or_else(|| "problems response is not an array".to_string())
    }

    fn list_languages(&mut self, cid: &str) -> Result<Vec<serde_json::Value>, String> {
        let json = self
            .http
            .get_json(&format!("/api/v4/contests/{}/languages", cid))?;
        json.as_array()
            .cloned()
            .ok_or_else(|| "languages response is not an array".to_string())
    }

    /// Find a problem matching the given URL fragment across active contests.
    /// Matches against the problem's `id`, `label`, or `short_name`.
    pub fn find_contest_problem(
        &mut self,
        problem_ref: &str,
    ) -> Result<(String, String, String), String> {
        let contests = self.list_active_contests()?;
        if contests.is_empty() {
            return Err("no active contests".to_string());
        }
        for contest in &contests {
            let cid = contest.get("id").and_then(|v| v.as_str()).unwrap_or("");
            if cid.is_empty() {
                continue;
            }
            let problems = match self.list_problems(cid) {
                Ok(p) => p,
                Err(_) => continue,
            };
            for p in &problems {
                let pid = p.get("id").and_then(|v| v.as_str()).unwrap_or("");
                let label = p.get("label").and_then(|v| v.as_str()).unwrap_or("");
                let short = p.get("short_name").and_then(|v| v.as_str()).unwrap_or("");
                if pid == problem_ref
                    || label.eq_ignore_ascii_case(problem_ref)
                    || short == problem_ref
                {
                    return Ok((cid.to_string(), pid.to_string(), label.to_string()));
                }
            }
        }
        Err(format!(
            "problem '{}' not found in any active contest",
            problem_ref
        ))
    }

    pub fn find_language(
        &mut self,
        cid: &str,
        language: &str,
    ) -> Result<(String, String), String> {
        let langs = self.list_languages(cid)?;
        let lower = language.to_lowercase();
        // Exact id match wins.
        for l in &langs {
            let id = l.get("id").and_then(|v| v.as_str()).unwrap_or("");
            if id.eq_ignore_ascii_case(language) {
                let name = l
                    .get("name")
                    .and_then(|v| v.as_str())
                    .unwrap_or(id)
                    .to_string();
                return Ok((id.to_string(), name));
            }
        }
        // Then prefix on name.
        for l in &langs {
            let name = l.get("name").and_then(|v| v.as_str()).unwrap_or("");
            if name.to_lowercase().starts_with(&lower) {
                let id = l
                    .get("id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                return Ok((id, name.to_string()));
            }
        }
        // Then substring.
        for l in &langs {
            let name = l.get("name").and_then(|v| v.as_str()).unwrap_or("");
            if name.to_lowercase().contains(&lower) {
                let id = l
                    .get("id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                return Ok((id, name.to_string()));
            }
        }
        Err(format!("language '{}' not found", language))
    }

    pub fn submit(
        &mut self,
        cid: &str,
        problem_id: &str,
        language_id: &str,
        source: &str,
        filename: &str,
    ) -> Result<String, String> {
        let auth = self
            .auth_header
            .clone()
            .ok_or_else(|| "not logged in".to_string())?;
        let part = multipart::Part::text(source.to_string())
            .file_name(filename.to_string())
            .mime_str("application/octet-stream")
            .map_err(|e| format!("invalid mime: {}", e))?;
        let form = multipart::Form::new()
            .text("problem_id", problem_id.to_string())
            .text("language_id", language_id.to_string())
            .part("code[]", part);

        let url = format!("{}/api/v4/contests/{}/submissions", self.base_url, cid);
        let client = reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(60))
            .build()
            .map_err(|e| format!("client build failed: {}", e))?;
        let resp = client
            .post(&url)
            .header("Authorization", &auth)
            .multipart(form)
            .send()
            .map_err(|e| format!("submit request failed: {}", e))?;
        let status = resp.status();
        let body = resp
            .text()
            .map_err(|e| format!("failed to read submit response: {}", e))?;
        if !status.is_success() {
            return Err(format!("submit failed ({}): {}", status, body.trim()));
        }
        let json: serde_json::Value = serde_json::from_str(&body)
            .map_err(|e| format!("invalid submit response: {} body: {}", e, body))?;
        json.get("id")
            .and_then(|v| v.as_str())
            .map(String::from)
            .ok_or_else(|| format!("submit response missing id: {}", body))
    }

    fn judgement_types_map(&mut self, cid: &str) -> HashMap<String, (String, bool)> {
        let mut map = HashMap::new();
        if let Ok(arr) = self
            .http
            .get_json(&format!("/api/v4/contests/{}/judgement-types", cid))
        {
            if let Some(items) = arr.as_array() {
                for it in items {
                    let id = it
                        .get("id")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    let name = it
                        .get("name")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    let solved = it
                        .get("solved")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false);
                    map.insert(id, (name, solved));
                }
            }
        }
        map
    }

    pub fn poll_verdict(
        &mut self,
        cid: &str,
        submission_id: &str,
    ) -> Result<String, String> {
        let mut stdout = std::io::stdout();
        let mut last_len = 0;
        let types = self.judgement_types_map(cid);
        loop {
            clear(last_len);
            last_len = 0;
            let path = format!(
                "/api/v4/contests/{}/judgements?submission_id={}",
                cid, submission_id
            );
            let body = self.http.get_text(&path)?;
            let json: serde_json::Value = match serde_json::from_str(&body) {
                Ok(j) => j,
                Err(_) => {
                    thread::sleep(Duration::from_secs(2));
                    continue;
                }
            };
            let judgements: Vec<&serde_json::Value> = json
                .as_array()
                .map(|arr| {
                    arr.iter()
                        .filter(|j| {
                            j.get("valid").and_then(|v| v.as_bool()).unwrap_or(true)
                        })
                        .collect()
                })
                .unwrap_or_default();
            let latest = judgements.last().copied();
            let judgement_type_id = latest
                .and_then(|j| j.get("judgement_type_id"))
                .and_then(|v| v.as_str())
                .map(String::from);

            if let Some(jt_id) = judgement_type_id {
                let (name, solved) = types
                    .get(&jt_id)
                    .cloned()
                    .unwrap_or_else(|| (jt_id.clone(), jt_id == "AC"));
                let display = if name.is_empty() {
                    jt_id.clone()
                } else {
                    format!("{} ({})", name, jt_id)
                };
                let color = if solved { Color::Green } else { Color::Red };
                let _ = execute!(stdout, SetForegroundColor(color));
                println!("{}", display);
                let _ = execute!(stdout, ResetColor);
                println!(
                    "Submission url: {}/team/submissions/{}",
                    self.base_url, submission_id
                );
                return Ok(if name.is_empty() { jt_id } else { name });
            }

            let progress = if latest.is_some() {
                "Judging".to_string()
            } else {
                "Queued".to_string()
            };
            let _ = execute!(stdout, SetForegroundColor(Color::Yellow));
            print!("{}", progress);
            let _ = std::io::Write::flush(&mut stdout);
            let _ = execute!(stdout, ResetColor);
            last_len = progress.len();
            thread::sleep(Duration::from_secs(2));
        }
    }
}

/// Parse a DOMjudge-shaped URL.
///
/// Recognized forms (the host may include a sub-path for the DOMjudge install):
///   `https://host[/sub/path]/(team|public|jury|domjudge)/(problems|submit|submissions)/<id>[/...]`
///
/// Returns `(base_url, kind, id)` where `kind` is the resource segment and `id`
/// is the URL identifier (problem id for problems/submit, submission id for
/// submissions).
pub fn parse_url(url: &str) -> Option<(String, String, String)> {
    let url = url.split('?').next().unwrap_or(url);
    let url = url.split('#').next().unwrap_or(url);
    let url = url.trim_end_matches('/');
    let re = Regex::new(
        r"^(https?://[^/]+(?:/[^/]+)*?)/(team|public|jury|domjudge)/(problems|submit|submissions)/([^/]+)",
    )
    .ok()?;
    let caps = re.captures(url)?;
    Some((caps[1].to_string(), caps[3].to_string(), caps[4].to_string()))
}

/// Try to perform a DOMjudge login for `url`. The URL may be either a base
/// installation URL (`https://judge.example.com[/path]`) or a deeper page
/// matching `parse_url`. Returns true if the URL pointed at a DOMjudge server.
pub fn try_login(url: &str) -> bool {
    let base = match parse_url(url) {
        Some((base, _, _)) => base,
        None => url.trim_end_matches('/').to_string(),
    };
    let mut client = DomjudgeClient::new(&base);
    if !client.is_domjudge() {
        return false;
    }
    println!("Detected DOMjudge at {}", base);
    if let Err(e) = client.login() {
        eprintln!("Login failed: {}", e);
    }
    true
}

/// Try to submit to a DOMjudge instance. Returns true if the URL matched the
/// DOMjudge URL scheme and the server responded as a DOMjudge instance. A
/// returned `true` does not mean the submission succeeded — failures during
/// login/submit are printed and then the function returns `true` as the URL
/// was consumed.
pub fn try_submit(url: &str, language: &str, source: &str, filename: &str) -> bool {
    let (base, kind, id) = match parse_url(url) {
        Some(x) => x,
        None => return false,
    };
    if kind == "submissions" {
        eprintln!(
            "URL points at a submission view; pass a problem URL like /team/problems/<id>"
        );
        return true;
    }
    let mut client = DomjudgeClient::new(&base);
    if !client.is_domjudge() {
        return false;
    }
    println!("Detected DOMjudge at {}", base);

    println!("Logging in");
    if let Err(e) = client.login() {
        eprintln!("Login failed: {}", e);
        return true;
    }

    let (cid, pid, label) = match client.find_contest_problem(&id) {
        Ok(x) => x,
        Err(e) => {
            eprintln!("{}", e);
            return true;
        }
    };
    println!(
        "Problem: {} (id={}, contest={})",
        if label.is_empty() { &pid } else { &label },
        pid,
        cid
    );

    let (lid, lname) = match client.find_language(&cid, language) {
        Ok(x) => x,
        Err(e) => {
            eprintln!("{}", e);
            return true;
        }
    };
    println!("Language: {} (id={})", lname, lid);

    let basename = std::path::Path::new(filename)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("solution");

    println!("Submitting");
    let sid = match client.submit(&cid, &pid, &lid, source, basename) {
        Ok(x) => x,
        Err(e) => {
            eprintln!("{}", e);
            return true;
        }
    };

    if let Err(e) = client.poll_verdict(&cid, &sid) {
        eprintln!("Verdict polling failed: {}", e);
    }
    true
}

#[cfg(test)]
mod tests {
    use super::parse_url;

    #[test]
    fn parse_team_problem_no_subpath() {
        let (base, kind, id) =
            parse_url("https://demo.domjudge.org/team/problems/3").unwrap();
        assert_eq!(base, "https://demo.domjudge.org");
        assert_eq!(kind, "problems");
        assert_eq!(id, "3");
    }

    #[test]
    fn parse_team_problem_with_subpath() {
        let (base, kind, id) =
            parse_url("https://domjudge.iti.kit.edu/main/team/problems/3").unwrap();
        assert_eq!(base, "https://domjudge.iti.kit.edu/main");
        assert_eq!(kind, "problems");
        assert_eq!(id, "3");
    }

    #[test]
    fn parse_public_problem_with_query() {
        let (base, kind, id) =
            parse_url("https://j.example.com/public/problems/abc?lang=en#x").unwrap();
        assert_eq!(base, "https://j.example.com");
        assert_eq!(kind, "problems");
        assert_eq!(id, "abc");
    }

    #[test]
    fn parse_team_submit() {
        let (_, kind, id) =
            parse_url("https://j.example.com/team/submit/12").unwrap();
        assert_eq!(kind, "submit");
        assert_eq!(id, "12");
    }

    #[test]
    fn parse_jury_with_deeper_path() {
        let (base, kind, id) = parse_url(
            "https://j.example.com/installs/dj/jury/problems/4/text",
        )
        .unwrap();
        assert_eq!(base, "https://j.example.com/installs/dj");
        assert_eq!(kind, "problems");
        assert_eq!(id, "4");
    }

    #[test]
    fn parse_unrelated() {
        assert!(parse_url("https://example.com/foo/bar").is_none());
        assert!(parse_url("https://codeforces.com/contest/1/problem/A").is_none());
    }
}

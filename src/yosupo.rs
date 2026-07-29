use crate::clear;
use crate::http::HttpClient;
use crossterm::execute;
use crossterm::style::{Color, ResetColor, SetForegroundColor};
use dialoguer::console::Term;
use dialoguer::theme::ColorfulTheme;
use dialoguer::{Input, Password, Select};
use regex::Regex;
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const API_BASE: &str = "https://v3.api.judge.yosupo.jp";
const SITE_ORIGIN: &str = "https://judge.yosupo.jp";
const USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36";

/// Firebase Web API key for the judge.yosupo.jp project. Not a secret —
/// these keys identify the Firebase project; access control is per-user.
const FIREBASE_API_KEY: &str = "AIzaSyCmpkoMVbKRDm2H0MJHB0iZ43uQtSqiLV0";
/// The site turns a Yosupo username into a Firebase email using this suffix.
const EMAIL_SUFFIX: &str = "@dummy.judge.yosupo.jp";
/// If the user opts to submit anonymously we don't ask again for this long.
const ANON_TTL_SECS: u64 = 7 * 24 * 3600;

// Cookie-store keys we use as ad-hoc key/value state. We disable cookie
// sending on the HttpClient so these never leak out as request cookies.
const K_ID_TOKEN: &str = "id_token";
const K_REFRESH_TOKEN: &str = "refresh_token";
const K_TOKEN_EXP: &str = "token_exp";
const K_ANONYMOUS_UNTIL: &str = "anonymous_until";
const K_USERNAME: &str = "username";

fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[derive(Debug, Clone)]
enum AuthState {
    LoggedIn { id_token: String, name: String },
    Anonymous,
}

pub struct YosupoClient {
    http: HttpClient,
}

impl YosupoClient {
    pub fn new() -> Self {
        let mut http = HttpClient::new(API_BASE);
        http.set_header("User-Agent", USER_AGENT);
        // Auth is Bearer-based, not cookie-based; disable auto-cookie
        // injection so our ad-hoc key/value entries stay client-side only.
        http.disable_cookie_sending();
        // The API is on a subdomain; the browser POSTs it from judge.yosupo.jp
        // and needs the corresponding CORS headers, so Origin/Referer match.
        http.set_header("Origin", SITE_ORIGIN);
        http.set_header("Referer", &format!("{}/", SITE_ORIGIN));
        YosupoClient { http }
    }

    /// Decide what auth state to use for the next request. Returns:
    ///   - LoggedIn if a fresh idToken is on disk or we can refresh one;
    ///   - Anonymous if the user opted out of login within the last 7 days;
    ///   - None if state is unknown (caller must prompt).
    fn stored_auth_state(&mut self) -> Option<AuthState> {
        let now = now_unix();
        if let Some(t) = self.valid_id_token(now) {
            let name = self
                .http
                .get_cookie(K_USERNAME)
                .unwrap_or_else(|| "?".to_string());
            return Some(AuthState::LoggedIn { id_token: t, name });
        }
        // idToken expired but a refresh token might still work.
        if !self.http.get_cookie(K_REFRESH_TOKEN).unwrap_or_default().is_empty() {
            if let Ok(t) = self.refresh_id_token() {
                let name = self
                    .http
                    .get_cookie(K_USERNAME)
                    .unwrap_or_else(|| "?".to_string());
                return Some(AuthState::LoggedIn { id_token: t, name });
            }
        }
        if let Some(until) = self
            .http
            .get_cookie(K_ANONYMOUS_UNTIL)
            .and_then(|s| s.parse::<u64>().ok())
        {
            if until > now {
                return Some(AuthState::Anonymous);
            }
        }
        None
    }

    fn valid_id_token(&mut self, now: u64) -> Option<String> {
        let token = self.http.get_cookie(K_ID_TOKEN)?;
        if token.is_empty() {
            return None;
        }
        let exp = self
            .http
            .get_cookie(K_TOKEN_EXP)
            .and_then(|s| s.parse::<u64>().ok())?;
        if exp > now {
            Some(token)
        } else {
            None
        }
    }

    /// State check with interactive prompt if there's no fresh state on disk.
    fn ensure_auth_state(&mut self) -> Result<AuthState, String> {
        if let Some(s) = self.stored_auth_state() {
            return Ok(s);
        }
        self.prompt_login_or_anonymous()
    }

    fn prompt_login_or_anonymous(&mut self) -> Result<AuthState, String> {
        let choices = &[
            "Log in with judge.yosupo.jp username + password",
            "Submit anonymously (remembered for 7 days)",
        ];
        let sel = Select::with_theme(&ColorfulTheme::default())
            .with_prompt("judge.yosupo.jp — no fresh auth state")
            .items(choices)
            .default(0)
            .interact_on(&Term::stdout())
            .map_err(|e| format!("prompt cancelled: {}", e))?;
        match sel {
            0 => {
                let username: String = Input::with_theme(&ColorfulTheme::default())
                    .with_prompt("Username")
                    .interact_on(&Term::stdout())
                    .map_err(|e| e.to_string())?;
                let password: String = Password::with_theme(&ColorfulTheme::default())
                    .with_prompt("Password")
                    .interact_on(&Term::stdout())
                    .map_err(|e| e.to_string())?;
                let id_token = self.sign_in_with_password(&username, &password)?;
                // Clear any prior anonymous choice — the user just chose login.
                self.http.set_cookie(K_ANONYMOUS_UNTIL, "0");
                self.http.set_cookie(K_USERNAME, &username);
                println!("Logged in as {}", username);
                Ok(AuthState::LoggedIn {
                    id_token,
                    name: username,
                })
            }
            _ => {
                let until = now_unix() + ANON_TTL_SECS;
                self.http.set_cookie(K_ANONYMOUS_UNTIL, &until.to_string());
                println!("Continuing anonymously (remembered for 7 days).");
                Ok(AuthState::Anonymous)
            }
        }
    }

    fn sign_in_with_password(
        &mut self,
        username: &str,
        password: &str,
    ) -> Result<String, String> {
        let email = format!("{}{}", username, EMAIL_SUFFIX);
        let body = serde_json::json!({
            "email": email,
            "password": password,
            "returnSecureToken": true,
            "clientType": "CLIENT_TYPE_WEB",
        });
        let url = format!(
            "https://identitytoolkit.googleapis.com/v1/accounts:signInWithPassword?key={}",
            FIREBASE_API_KEY
        );
        let client = reqwest::blocking::Client::new();
        let resp = client
            .post(&url)
            .json(&body)
            .send()
            .map_err(|e| format!("Firebase signIn request failed: {}", e))?;
        let status = resp.status();
        let text = resp
            .text()
            .map_err(|e| format!("Failed to read signIn response: {}", e))?;
        if !status.is_success() {
            return Err(format!(
                "Firebase signIn failed ({}): {}",
                status,
                &text[..text.len().min(400)]
            ));
        }
        let j: serde_json::Value = serde_json::from_str(&text)
            .map_err(|e| format!("Failed to parse signIn response: {}", e))?;
        let id_token = j
            .get("idToken")
            .and_then(|v| v.as_str())
            .ok_or("no idToken in signIn response")?
            .to_string();
        let refresh = j
            .get("refreshToken")
            .and_then(|v| v.as_str())
            .ok_or("no refreshToken in signIn response")?
            .to_string();
        // expiresIn comes back as a string number of seconds; give ourselves
        // a 60s safety margin so we refresh before hitting the wall.
        let expires_in: u64 = j
            .get("expiresIn")
            .and_then(|v| v.as_str())
            .and_then(|s| s.parse().ok())
            .unwrap_or(3600);
        let exp = now_unix() + expires_in.saturating_sub(60);
        self.http.set_cookie(K_ID_TOKEN, &id_token);
        self.http.set_cookie(K_REFRESH_TOKEN, &refresh);
        self.http.set_cookie(K_TOKEN_EXP, &exp.to_string());
        Ok(id_token)
    }

    fn refresh_id_token(&mut self) -> Result<String, String> {
        let refresh = self
            .http
            .get_cookie(K_REFRESH_TOKEN)
            .ok_or("no stored refresh token")?;
        let url = format!(
            "https://securetoken.googleapis.com/v1/token?key={}",
            FIREBASE_API_KEY
        );
        let client = reqwest::blocking::Client::new();
        let resp = client
            .post(&url)
            .form(&[
                ("grant_type", "refresh_token"),
                ("refresh_token", refresh.as_str()),
            ])
            .send()
            .map_err(|e| format!("Firebase refresh request failed: {}", e))?;
        let status = resp.status();
        let text = resp
            .text()
            .map_err(|e| format!("Failed to read refresh response: {}", e))?;
        if !status.is_success() {
            // Refresh token is dead; wipe state so next call re-prompts.
            self.http.set_cookie(K_REFRESH_TOKEN, "");
            self.http.set_cookie(K_ID_TOKEN, "");
            self.http.set_cookie(K_TOKEN_EXP, "0");
            return Err(format!("Firebase refresh failed ({}): {}", status, text));
        }
        let j: serde_json::Value = serde_json::from_str(&text)
            .map_err(|e| format!("Failed to parse refresh response: {}", e))?;
        let id_token = j
            .get("id_token")
            .and_then(|v| v.as_str())
            .ok_or("no id_token in refresh response")?
            .to_string();
        let new_refresh = j
            .get("refresh_token")
            .and_then(|v| v.as_str())
            .unwrap_or(&refresh)
            .to_string();
        let expires_in: u64 = j
            .get("expires_in")
            .and_then(|v| v.as_str())
            .and_then(|s| s.parse().ok())
            .unwrap_or(3600);
        let exp = now_unix() + expires_in.saturating_sub(60);
        self.http.set_cookie(K_ID_TOKEN, &id_token);
        self.http.set_cookie(K_REFRESH_TOKEN, &new_refresh);
        self.http.set_cookie(K_TOKEN_EXP, &exp.to_string());
        Ok(id_token)
    }

    fn fetch_langs(&mut self) -> Result<Vec<(String, String)>, String> {
        let body = self.http.get_text("/langs")?;
        let json: serde_json::Value = serde_json::from_str(&body)
            .map_err(|e| format!("Failed to parse /langs: {} body: {}", e, body))?;
        let arr = json
            .get("langs")
            .and_then(|v| v.as_array())
            .ok_or_else(|| format!("No langs array in response: {}", body))?;
        Ok(arr
            .iter()
            .filter_map(|c| {
                let id = c.get("id").and_then(|v| v.as_str())?;
                let name = c.get("name").and_then(|v| v.as_str()).unwrap_or(id);
                Some((id.to_string(), name.to_string()))
            })
            .collect())
    }

    pub fn submit(
        &mut self,
        problem: &str,
        language: &str,
        source: &str,
    ) -> Result<i64, String> {
        let auth = self.ensure_auth_state()?;

        let langs = self.fetch_langs()?;
        let (lang_id, lang_name) = pick_lang(&langs, language)?;
        println!("Language: {}", lang_name);

        let body = serde_json::json!({
            "problem": problem,
            "source": source,
            "lang": lang_id,
            "tle_knockout": true,
        })
        .to_string();

        match &auth {
            AuthState::LoggedIn { id_token, name } => {
                println!("Submitting as {}", name);
                self.http
                    .set_header("Authorization", &format!("Bearer {}", id_token));
            }
            AuthState::Anonymous => {
                println!("Submitting anonymously");
            }
        }

        let resp = self.http.post_json("/submit", &body)?;
        let status = resp.status();
        let resp_body = resp
            .text()
            .map_err(|e| format!("Failed to read submit response: {}", e))?;
        if !status.is_success() {
            return Err(format!(
                "Submit failed ({}): {}",
                status,
                &resp_body[..resp_body.len().min(300)]
            ));
        }
        let json: serde_json::Value = serde_json::from_str(&resp_body)
            .map_err(|e| format!("Failed to parse submit response: {} body: {}", e, resp_body))?;
        let id = json
            .get("id")
            .and_then(|v| v.as_i64())
            .ok_or_else(|| format!("No id in submit response: {}", resp_body))?;
        println!("Submission url: {}/submission/{}", SITE_ORIGIN, id);
        Ok(id)
    }

    pub fn poll_verdict(&mut self, id: i64) -> Result<String, String> {
        let mut stdout = std::io::stdout();
        let mut last_len = 0;
        loop {
            clear(last_len);
            let body = self.http.get_text(&format!("/submissions/{}", id))?;
            let json: serde_json::Value = serde_json::from_str(&body)
                .map_err(|e| format!("Failed to parse poll JSON: {}", e))?;
            let overview = json.get("overview").unwrap_or(&json);
            let status = overview
                .get("status")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let cases_ref = json.get("case_results").and_then(|v| v.as_array());
            let case_count = cases_ref.map(|a| a.len()).unwrap_or(0);
            let judged_count = cases_ref
                .map(|a| {
                    a.iter()
                        .filter(|c| {
                            let s = c.get("status").and_then(|v| v.as_str()).unwrap_or("");
                            !s.is_empty() && s != "-"
                        })
                        .count()
                })
                .unwrap_or(0);

            if is_terminal(status) {
                let color = if status == "AC" { Color::Green } else { Color::Red };
                let mut display = status_name(status).to_string();
                if status != "AC" && case_count > 0 {
                    // Point at the first actually-failing case; skip "-"
                    // (not-judged-yet, appears when the judge cuts off
                    // remaining cases after the failing one).
                    if let Some(cases) = json.get("case_results").and_then(|v| v.as_array()) {
                        if let Some(bad) = cases.iter().find(|c| {
                            let s = c.get("status").and_then(|v| v.as_str()).unwrap_or("");
                            s != "AC" && s != "-" && !s.is_empty()
                        }) {
                            if let Some(name) = bad.get("case").and_then(|v| v.as_str()) {
                                display.push_str(&format!(" on {}", name));
                            }
                        }
                    }
                }
                if let Some(t) = overview.get("time").and_then(|v| v.as_f64()) {
                    if t >= 0.0 {
                        display.push_str(&format!(" ({:.3}s)", t));
                    }
                }
                let _ = execute!(stdout, SetForegroundColor(color));
                println!("{}", display);
                let _ = execute!(stdout, ResetColor);
                return Ok(display);
            }

            let progress = if case_count > 0 {
                format!("Judging {}/{}", judged_count, case_count)
            } else {
                status_name(status).to_string()
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

/// Parse a judge.yosupo.jp URL into a problem slug.
/// https://judge.yosupo.jp/problem/persistent_range_affine_range_sum -> "persistent_range_affine_range_sum"
pub fn parse_url(url: &str) -> Option<String> {
    let re = Regex::new(r"/problem/([A-Za-z0-9_]+)").ok()?;
    re.captures(url).map(|c| c[1].to_string())
}

/// Pick a compiler by matching the user's language string against the
/// (id, name) pairs from /langs. Exact match on id or name first, then
/// starts-with on either, then substring on either. Returns (id, name).
fn pick_lang(langs: &[(String, String)], language: &str) -> Result<(String, String), String> {
    let needle = language.to_lowercase();
    for (id, name) in langs {
        if id.eq_ignore_ascii_case(language) || name.eq_ignore_ascii_case(language) {
            return Ok((id.clone(), name.clone()));
        }
    }
    for (id, name) in langs {
        if id.to_lowercase().starts_with(&needle) || name.to_lowercase().starts_with(&needle) {
            return Ok((id.clone(), name.clone()));
        }
    }
    for (id, name) in langs {
        if id.to_lowercase().contains(&needle) || name.to_lowercase().contains(&needle) {
            return Ok((id.clone(), name.clone()));
        }
    }
    let listed: Vec<String> = langs
        .iter()
        .map(|(id, name)| format!("{}={}", id, name))
        .collect();
    Err(format!(
        "Compiler '{}' not found. Available: {}",
        language,
        listed.join(", ")
    ))
}

/// Yosupo's non-terminal statuses. Everything else is terminal.
/// `-` shows up per-case for tests that haven't been judged yet, and
/// as the overview status while the submission is still queued.
/// While cases are running, the overview status is a live progress
/// string like `"4/26"` (judged/total) — treat any `X/Y` as non-terminal.
fn is_terminal(status: &str) -> bool {
    match status {
        "" | "-" | "WJ" | "Judging" | "Compiling" | "Waiting" => return false,
        _ => {}
    }
    if let Some((a, b)) = status.split_once('/') {
        if a.chars().all(|c| c.is_ascii_digit()) && b.chars().all(|c| c.is_ascii_digit()) {
            return false;
        }
    }
    true
}

fn status_name(s: &str) -> &str {
    match s {
        "AC" => "Accepted",
        "WA" => "Wrong Answer",
        "TLE" => "Time Limit Exceeded",
        "MLE" => "Memory Limit Exceeded",
        "RE" => "Runtime Error",
        "CE" => "Compilation Error",
        "IE" => "Internal Error",
        "OLE" => "Output Limit Exceeded",
        "Fail" => "Fail",
        "WJ" => "Waiting for Judge",
        "Judging" => "Judging",
        "Compiling" => "Compiling",
        "" => "Pending",
        // Unknown status codes pass through verbatim so we don't hide info.
        other => other,
    }
}

pub fn login() {
    let mut client = YosupoClient::new();
    match client.ensure_auth_state() {
        Ok(AuthState::LoggedIn { name, .. }) => {
            println!("judge.yosupo.jp: logged in as {}", name);
        }
        Ok(AuthState::Anonymous) => {
            println!("judge.yosupo.jp: anonymous submissions selected");
        }
        Err(e) => eprintln!("Login failed: {}", e),
    }
}

pub fn submit(url: String, language: String, source: String) {
    let problem = match parse_url(&url) {
        Some(p) => p,
        None => {
            eprintln!("Could not parse URL: {}", url);
            return;
        }
    };

    let mut client = YosupoClient::new();
    println!("Submitting");
    let id = match client.submit(&problem, &language, &source) {
        Ok(id) => id,
        Err(e) => {
            eprintln!("{}", e);
            return;
        }
    };
    if let Err(e) = client.poll_verdict(id) {
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
    fn parse_url_extracts_slug() {
        assert_eq!(
            parse_url("https://judge.yosupo.jp/problem/persistent_range_affine_range_sum"),
            Some("persistent_range_affine_range_sum".to_string()),
        );
    }

    #[test]
    fn parse_url_rejects_non_problem_urls() {
        assert_eq!(parse_url("https://judge.yosupo.jp/"), None);
    }

    #[test]
    fn pick_lang_exact_id_wins() {
        let langs = opts(&[
            ("cpp", "C++23"),
            ("cpp20", "C++20"),
            ("cpp17", "C++17"),
            ("rust", "Rust"),
        ]);
        assert_eq!(pick_lang(&langs, "cpp").unwrap().0, "cpp");
        assert_eq!(pick_lang(&langs, "rust").unwrap().0, "rust");
    }

    #[test]
    fn pick_lang_starts_with_falls_back() {
        let langs = opts(&[
            ("cpp", "C++23"),
            ("python3", "Python3"),
            ("pypy3", "PyPy3"),
        ]);
        assert_eq!(pick_lang(&langs, "python").unwrap().0, "python3");
        assert_eq!(pick_lang(&langs, "pypy").unwrap().0, "pypy3");
    }

    #[test]
    fn pick_lang_substring_is_last_resort() {
        let langs = opts(&[
            ("cpp", "C++23"),
            ("haskell-llvm", "GHC (LLVM)"),
        ]);
        assert_eq!(pick_lang(&langs, "llvm").unwrap().0, "haskell-llvm");
    }

    #[test]
    fn pick_lang_no_match_errors() {
        let langs = opts(&[("rust", "Rust")]);
        assert!(pick_lang(&langs, "cobol").is_err());
    }

    #[test]
    fn is_terminal_recognizes_lifecycle() {
        assert!(!is_terminal("WJ"));
        assert!(!is_terminal(""));
        assert!(!is_terminal("-"));
        assert!(!is_terminal("Judging"));
        assert!(!is_terminal("0/26"), "progress N/N is not terminal");
        assert!(!is_terminal("24/26"), "progress N/N is not terminal");
        assert!(is_terminal("AC"));
        assert!(is_terminal("WA"));
        assert!(is_terminal("TLE"));
    }
}

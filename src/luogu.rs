use crate::clear;
use crate::http::HttpClient;
use clipboard::{ClipboardContext, ClipboardProvider};
use crossterm::execute;
use crossterm::style::{Color, ResetColor, SetForegroundColor};
use regex::Regex;
use std::thread;
use std::time::Duration;

const USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36";
const BASE_URL: &str = "https://www.luogu.com.cn";

struct LuoguClient {
    http: HttpClient,
    cookies: String,
    uid: String,
}

impl LuoguClient {
    fn new() -> Self {
        let mut http = HttpClient::new(BASE_URL);
        http.set_header("User-Agent", USER_AGENT);
        // Migrate legacy cookie storage keys forward to Luogu's actual cookie
        // names so self.http can inject them automatically for real requests.
        let client_id = http
            .get_cookie("__client_id")
            .or_else(|| {
                let v = http.get_cookie("luogu_client_id")?;
                http.set_cookie("__client_id", &v);
                Some(v)
            })
            .unwrap_or_default();
        let uid = http
            .get_cookie("_uid")
            .or_else(|| {
                let v = http.get_cookie("luogu_uid")?;
                http.set_cookie("_uid", &v);
                Some(v)
            })
            .unwrap_or_default();
        let cookies = if !client_id.is_empty() && !uid.is_empty() {
            format!("__client_id={}; _uid={}", client_id, uid)
        } else {
            String::new()
        };
        LuoguClient { http, cookies, uid }
    }

    fn get_page(&self, url: &str) -> Result<String, String> {
        reqwest::blocking::Client::builder()
            .redirect(reqwest::redirect::Policy::limited(5))
            .build()
            .unwrap()
            .get(url)
            .header("Cookie", &self.cookies)
            .header("User-Agent", USER_AGENT)
            .send()
            .map_err(|e| format!("GET failed: {}", e))?
            .text()
            .map_err(|e| format!("Failed to read response: {}", e))
    }

    fn is_logged_in(&self) -> bool {
        if self.cookies.is_empty() {
            return false;
        }
        match self.get_page("https://www.luogu.com.cn") {
            // Luogu embeds page data as a URL-encoded JSON blob in the HTML.
            // A logged-in homepage contains "uid":<our-uid>; a logged-out page
            // has "currentUser":null. Match the URL-encoded form ("=%22, :=%3A).
            Ok(body) => body.contains(&format!("%22uid%22%3A{}", self.uid)),
            Err(_) => false,
        }
    }

    fn login(&mut self) -> Result<(), String> {
        if self.is_logged_in() {
            println!("Already logged in");
            return Ok(());
        }

        println!("Export your Luogu cookies using the EditThisCookie browser extension on a logged-in luogu.com.cn tab");
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

        let cookies: Vec<serde_json::Value> = serde_json::from_str(input.trim())
            .map_err(|e| format!("Failed to parse cookies JSON: {}", e))?;

        let client_id = cookies
            .iter()
            .find(|c| c.get("name").and_then(|v| v.as_str()) == Some("__client_id"))
            .and_then(|c| c.get("value").and_then(|v| v.as_str()))
            .ok_or("__client_id cookie not found")?
            .to_string();

        let uid = cookies
            .iter()
            .find(|c| c.get("name").and_then(|v| v.as_str()) == Some("_uid"))
            .and_then(|c| c.get("value").and_then(|v| v.as_str()))
            .ok_or("_uid cookie not found")?
            .to_string();

        self.cookies = format!("__client_id={}; _uid={}", client_id, uid);
        self.uid = uid.clone();

        if self.is_logged_in() {
            self.http.set_cookie("__client_id", &client_id);
            self.http.set_cookie("_uid", &uid);
            println!("Login successful");
            Ok(())
        } else {
            self.cookies.clear();
            self.uid.clear();
            Err("Login failed: invalid cookies".to_string())
        }
    }

    /// Attempt to submit directly via Luogu's `POST /fe/api/problem/submit/{pid}`
    /// endpoint. Any failure (missing CSRF, unknown language, 403 CF challenge,
    /// non-2xx response) returns `Err` and the caller falls back to the browser
    /// flow. Success returns the new submission's record id (`rid`).
    fn try_direct_submit(
        &mut self,
        pid: &str,
        contest_id: Option<&str>,
        language: &str,
        source: &str,
    ) -> Result<i64, String> {
        let problem_path = match contest_id {
            Some(cid) => format!("/problem/{}?contestId={}", pid, cid),
            None => format!("/problem/{}", pid),
        };
        let page = self
            .http
            .get_text(&problem_path)
            .map_err(|e| format!("GET problem page failed: {}", e))?;

        let csrf = parse_csrf(&page).ok_or_else(|| {
            let dump = dump_page_for_debug(&page, "editor-no-csrf");
            format!("CSRF token not found on problem page ({})", dump)
        })?;

        let lang_id = pick_luogu_language(&page, language).ok_or_else(|| {
            let dump = dump_page_for_debug(&page, "editor-no-lang");
            format!("no matching language for {:?} ({})", language, dump)
        })?;

        let submit_path = match contest_id {
            Some(cid) => format!("/fe/api/problem/submit/{}?contestId={}", pid, cid),
            None => format!("/fe/api/problem/submit/{}", pid),
        };
        let body = serde_json::json!({
            "lang": lang_id,
            "code": source,
            "enableO2": 1,
        })
        .to_string();

        self.http.set_header("Origin", BASE_URL);
        self.http
            .set_header("Referer", &format!("{}{}", BASE_URL, problem_path));
        self.http.set_header("x-csrf-token", &csrf);
        self.http.set_header("x-requested-with", "XMLHttpRequest");

        let resp = self.http.post_json(&submit_path, &body)?;
        let status = resp.status();
        let resp_body = resp
            .text()
            .map_err(|e| format!("Failed to read submit response: {}", e))?;

        if !status.is_success() {
            let dump = dump_page_for_debug(&resp_body, "submit-fail");
            return Err(format!("submit POST {} ({})", status, dump));
        }

        let json: serde_json::Value = serde_json::from_str(&resp_body).map_err(|e| {
            let dump = dump_page_for_debug(&resp_body, "submit-nonjson");
            format!("Failed to parse submit response: {} ({})", e, dump)
        })?;
        json.get("rid")
            .and_then(|v| v.as_i64())
            .ok_or_else(|| format!("No rid in submit response: {}", resp_body))
    }

    fn get_records(&self, pid: &str) -> Result<Vec<serde_json::Value>, String> {
        let url = format!(
            "https://www.luogu.com.cn/record/list?user={}&pid={}&page=1&_contentOnly=1",
            self.uid, pid
        );
        let body = self.get_page(&url)?;
        let data: serde_json::Value = serde_json::from_str(&body)
            .map_err(|e| format!("Failed to parse JSON: {}", e))?;
        Ok(data
            .pointer("/currentData/records/result")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default())
    }

    fn get_record_detail(&self, record_id: &str) -> Result<serde_json::Value, String> {
        let url = format!(
            "https://www.luogu.com.cn/record/{}?_contentOnly=1",
            record_id
        );
        let body = self.get_page(&url)?;
        let data: serde_json::Value = serde_json::from_str(&body)
            .map_err(|e| format!("Failed to parse JSON: {}", e))?;
        data.pointer("/currentData/record")
            .cloned()
            .ok_or("No record data found".to_string())
    }

    fn poll_verdict<F: FnMut()>(
        &self,
        pid: &str,
        known_ids: &std::collections::HashSet<i64>,
        mut on_submission_found: F,
    ) -> Result<(), String> {
        let mut stdout = std::io::stdout();
        let mut last_len = 0;
        let mut tracking_id: Option<i64> = None;

        loop {
            clear(last_len);
            last_len = 0;
            let records = self.get_records(pid)?;

            for record in &records {
                let id = record.get("id").and_then(|v| v.as_i64()).unwrap_or(0);
                let status = record.get("status").and_then(|v| v.as_i64()).unwrap_or(-1);
                let score = record.get("score").and_then(|v| v.as_i64()).unwrap_or(0);

                if let Some(tid) = tracking_id {
                    if id != tid {
                        continue;
                    }
                } else if known_ids.contains(&id) {
                    continue;
                } else {
                    tracking_id = Some(id);
                    println!("Submission url: https://www.luogu.com.cn/record/{}", id);
                    on_submission_found();
                }

                if status == 0 || status == 1 {
                    // Waiting/Judging
                    let progress = if status == 1 {
                        "Judging".to_string()
                    } else {
                        "Waiting".to_string()
                    };
                    let _ = execute!(stdout, SetForegroundColor(Color::Yellow));
                    print!("{}", progress);
                    let _ = execute!(stdout, ResetColor);
                    last_len = progress.len();
                } else {
                    // Final verdict
                    let verdict_name = status_name(status);
                    let is_accepted = status == 12;
                    let color = if is_accepted {
                        Color::Green
                    } else {
                        Color::Red
                    };

                    // Fetch detailed results
                    let detail = self.get_record_detail(&id.to_string());
                    let mut display = verdict_name.to_string();

                    if let Ok(record) = &detail {
                        let subtasks = record
                            .pointer("/detail/judgeResult/subtasks")
                            .and_then(|v| v.as_array());

                        if let Some(subtasks) = subtasks {
                            if subtasks.len() > 1 {
                                // Multi-subtask: show score and per-subtask
                                if !is_accepted {
                                    display.push_str(&format!(" ({}pts)", score));
                                }
                                let _ = execute!(stdout, SetForegroundColor(color));
                                println!("{}", display);
                                let _ = execute!(stdout, ResetColor);

                                for s in subtasks {
                                    let sid = s.get("id").and_then(|v| v.as_i64()).unwrap_or(0);
                                    let s_score =
                                        s.get("score").and_then(|v| v.as_i64()).unwrap_or(0);
                                    let s_status =
                                        s.get("status").and_then(|v| v.as_i64()).unwrap_or(0);
                                    let cases = s.get("testCases").and_then(|v| v.as_object());
                                    let total = cases.map(|c| c.len()).unwrap_or(0);

                                    let s_accepted = s_status == 12;
                                    let line_color =
                                        if s_accepted { Color::Green } else { Color::Red };

                                    let mut line = format!("  Subtask {}: {}pts", sid + 1, s_score);
                                    if !s_accepted {
                                        if let Some(cases) = cases {
                                            let passed = cases
                                                .values()
                                                .filter(|c| {
                                                    c.get("status").and_then(|v| v.as_i64())
                                                        == Some(12)
                                                })
                                                .count();
                                            if let Some((fail_id, fail_st)) = first_fail_status(cases) {
                                                line.push_str(&format!(
                                                    " ({} on test {}, {}/{} passed)",
                                                    status_name(fail_st),
                                                    fail_id + 1,
                                                    passed,
                                                    total
                                                ));
                                            } else {
                                                line.push_str(&format!(
                                                    " ({}/{} passed)",
                                                    passed, total
                                                ));
                                            }
                                        }
                                    }

                                    let _ = execute!(stdout, SetForegroundColor(line_color));
                                    println!("{}", line);
                                    let _ = execute!(stdout, ResetColor);
                                }
                                return Ok(());
                            } else if subtasks.len() == 1 && !is_accepted {
                                // Single subtask: use first failing test verdict
                                let s = &subtasks[0];
                                if let Some(cases) = s.get("testCases").and_then(|v| v.as_object())
                                {
                                    let total = cases.len();
                                    if let Some((fail_id, fail_st)) = first_fail_status(cases) {
                                        display = format!(
                                            "{} on test {}/{}",
                                            status_name(fail_st),
                                            fail_id + 1,
                                            total
                                        );
                                    }
                                    if score > 0 {
                                        display.push_str(&format!(" ({}pts)", score));
                                    }
                                }
                            }
                        }
                    }

                    let _ = execute!(stdout, SetForegroundColor(color));
                    println!("{}", display);
                    let _ = execute!(stdout, ResetColor);
                    return Ok(());
                }
                break;
            }

            thread::sleep(Duration::from_secs(2));
        }
    }
}

fn status_name(status: i64) -> &'static str {
    match status {
        2 => "Compilation Error",
        3 => "Output Limit Exceeded",
        4 => "Memory Limit Exceeded",
        5 => "Time Limit Exceeded",
        6 => "Wrong Answer",
        7 => "Runtime Error",
        11 => "Skipped",
        12 => "Accepted",
        14 => "Unaccepted",
        _ => "Unknown",
    }
}

/// Find the first non-accepted test case status in a testCases object
fn first_fail_status(cases: &serde_json::Map<String, serde_json::Value>) -> Option<(i64, i64)> {
    // Sort by test id numerically
    let mut sorted: Vec<_> = cases.iter().collect();
    sorted.sort_by_key(|(k, _)| k.parse::<i64>().unwrap_or(0));
    for (_, c) in &sorted {
        let st = c.get("status").and_then(|v| v.as_i64()).unwrap_or(0);
        if st != 12 && st != 0 && st != 11 {
            let id = c.get("id").and_then(|v| v.as_i64()).unwrap_or(0);
            return Some((id, st));
        }
    }
    None
}

/// Parse Luogu URL into (problem_id, optional contest_id).
/// https://www.luogu.com.cn/problem/P1001 -> ("P1001", None)
/// https://www.luogu.com.cn/problem/P17157?contestId=338569 -> ("P17157", Some("338569"))
fn parse_problem_id(url: &str) -> Option<(String, Option<String>)> {
    let re = Regex::new(r"/problem/([A-Za-z0-9]+)").ok()?;
    let pid = re.captures(url).map(|c| c[1].to_string())?;
    let contest_id = url.split('?').nth(1).and_then(|q| {
        q.split('&').find_map(|kv| {
            kv.strip_prefix("contestId=").map(|s| s.to_string())
        })
    });
    Some((pid, contest_id))
}

/// Extract Luogu's CSRF token from a page's `<meta name="csrf-token" content="...">`.
fn parse_csrf(html: &str) -> Option<String> {
    let re = Regex::new(
        r#"(?is)<meta[^>]*name="csrf-token"[^>]*content="([^"]+)""#,
    )
    .ok()?;
    re.captures(html).map(|c| c[1].to_string())
}

/// Pick a Luogu language id from the user's language string.
///
/// Luogu's compiler IDs are integers. We attempt to pull the current
/// (pid, name) list from the problem page's `_feInjection` blob, matching
/// via `langmatch::pick_option`. If that fails we fall back to a small
/// hardcoded map of the most common languages — good enough for the happy
/// path; anything unmatched returns None and the caller falls back to the
/// browser flow.
fn pick_luogu_language(page: &str, language: &str) -> Option<String> {
    if let Some(options) = extract_luogu_languages(page) {
        if let Some(id) = crate::langmatch::pick_option("luogu", language, &options) {
            return Some(id);
        }
    }
    hardcoded_luogu_lang(language)
}

/// Walk Luogu's `_feInjection` JSON blob for something that looks like a
/// language list — an array of `{id, name}` or a map of `{id: name}`.
fn extract_luogu_languages(html: &str) -> Option<Vec<(String, String)>> {
    let re = Regex::new(
        r#"(?is)JSON\.parse\(decodeURIComponent\("([^"]+)"\)\)"#,
    )
    .ok()?;
    let caps = re.captures(html)?;
    let encoded = &caps[1];
    let decoded = urldecode(encoded);
    let json: serde_json::Value = serde_json::from_str(&decoded).ok()?;

    let mut best: Option<Vec<(String, String)>> = None;
    walk_for_lang_list(&json, &mut best);
    best
}

fn walk_for_lang_list(v: &serde_json::Value, best: &mut Option<Vec<(String, String)>>) {
    match v {
        serde_json::Value::Object(o) => {
            // Object shape: {"1": "Pascal", "2": "C", ...} — string values keyed by numeric id.
            let looks_like_lang_map = o.len() >= 3
                && o.keys().all(|k| k.parse::<u32>().is_ok())
                && o.values().all(|v| v.is_string());
            if looks_like_lang_map {
                let candidate: Vec<(String, String)> = o
                    .iter()
                    .map(|(k, v)| (k.clone(), v.as_str().unwrap().to_string()))
                    .collect();
                if best.as_ref().map_or(true, |b| candidate.len() > b.len()) {
                    *best = Some(candidate);
                }
                return;
            }
            for value in o.values() {
                walk_for_lang_list(value, best);
            }
        }
        serde_json::Value::Array(arr) => {
            // Array shape: [{"id": 15, "name": "Rust"}, ...]
            let looks_like_lang_arr = arr.len() >= 3
                && arr.iter().all(|e| {
                    e.get("id").and_then(|v| v.as_i64()).is_some()
                        && (e.get("name").or_else(|| e.get("title")))
                            .and_then(|v| v.as_str())
                            .is_some()
                });
            if looks_like_lang_arr {
                let candidate: Vec<(String, String)> = arr
                    .iter()
                    .map(|e| {
                        let id = e.get("id").and_then(|v| v.as_i64()).unwrap();
                        let name = e
                            .get("name")
                            .or_else(|| e.get("title"))
                            .and_then(|v| v.as_str())
                            .unwrap();
                        (id.to_string(), name.to_string())
                    })
                    .collect();
                if best.as_ref().map_or(true, |b| candidate.len() > b.len()) {
                    *best = Some(candidate);
                }
                return;
            }
            for e in arr {
                walk_for_lang_list(e, best);
            }
        }
        _ => {}
    }
}

fn urldecode(s: &str) -> String {
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

/// Minimal fallback when the page-scraped language list can't be found or
/// doesn't match. Covers the most common cases; anything else → None and
/// browser fallback.
fn hardcoded_luogu_lang(language: &str) -> Option<String> {
    let l = language.to_lowercase();
    let id = match l.as_str() {
        "rust" | "rust2021" | "rust2024" => 15,
        "cpp" | "c++" | "g++" | "c++20" => 12, // C++20 O2
        "c++17" => 11,
        "c++14" => 10,
        "python" | "python3" | "cpython" => 7,
        "pypy" | "pypy3" => 25,
        _ => return None,
    };
    Some(id.to_string())
}

/// Dump body to a well-known temp file so failure diagnostics point at
/// something concrete on the user's disk.
fn dump_page_for_debug(body: &str, tag: &str) -> String {
    let path = std::env::temp_dir().join(format!("submitter-luogu-{}.html", tag));
    match std::fs::write(&path, body) {
        Ok(()) => format!("saved to {}", path.display()),
        Err(e) => format!("save failed: {}", e),
    }
}

pub fn login() {
    let mut client = LuoguClient::new();
    if let Err(e) = client.login() {
        eprintln!("Login failed: {}", e);
    }
}

pub fn submit(url: String, language: String, source: String) {
    let mut client = LuoguClient::new();

    if let Err(e) = client.login() {
        eprintln!("{}", e);
        return;
    }

    let (pid, contest_id) = match parse_problem_id(&url) {
        Some(p) => p,
        None => {
            eprintln!("Could not parse URL: {}", url);
            return;
        }
    };

    // Record existing submissions — used by poll_verdict to detect the new one
    // whether it came from direct-submit or from the browser fallback.
    let mut known_ids = std::collections::HashSet::new();
    if let Ok(records) = client.get_records(&pid) {
        for r in &records {
            if let Some(id) = r.get("id").and_then(|v| v.as_i64()) {
                known_ids.insert(id);
            }
        }
    }

    // Try direct HTTPS POST first. Any failure = fall back to browser.
    println!("Submitting...");
    match client.try_direct_submit(&pid, contest_id.as_deref(), &language, &source) {
        Ok(_rid) => {
            if let Err(e) = client.poll_verdict(&pid, &known_ids, || {}) {
                eprintln!("Verdict polling failed: {}", e);
            }
            return;
        }
        Err(reason) => {
            eprintln!("Direct submission failed ({}), opening browser instead.", reason);
        }
    }

    // Fallback: existing browser handoff flow.
    let mut ctx: ClipboardContext = ClipboardProvider::new().unwrap();
    ctx.set_contents(source.clone()).unwrap();

    let submit_url = format!("{}#submit", url);
    println!("Source code copied to clipboard");
    let handoff = crate::extbridge::publish(
        crate::extbridge::Job {
            site: "luogu",
            url: submit_url.clone(),
            language,
            source,
        },
        std::time::Duration::from_secs(60),
    ).ok();
    let url_to_open = match handoff.as_ref() {
        Some(h) => format!("{}#submitter={}:{}", url, h.port, h.token),
        None => submit_url.clone(),
    };
    open::that_detached(&url_to_open).ok();

    if let Err(e) = client.poll_verdict(&pid, &known_ids, || {
        if let Some(h) = handoff.as_ref() {
            h.signal_close();
        }
    }) {
        eprintln!("Verdict polling failed: {}", e);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_problem_id_bare_problem() {
        assert_eq!(
            parse_problem_id("https://www.luogu.com.cn/problem/P1001"),
            Some(("P1001".to_string(), None)),
        );
    }

    #[test]
    fn parse_problem_id_contest_url() {
        assert_eq!(
            parse_problem_id("https://www.luogu.com.cn/problem/P17157?contestId=338569"),
            Some(("P17157".to_string(), Some("338569".to_string()))),
        );
    }

    #[test]
    fn parse_csrf_reads_meta_tag() {
        let html = r#"<meta name="csrf-token" content="1785246296:D8PSizK8ZfyI+52wPm+wC7IxNvOMwxPRapS64lzpQng=">"#;
        assert_eq!(
            parse_csrf(html).as_deref(),
            Some("1785246296:D8PSizK8ZfyI+52wPm+wC7IxNvOMwxPRapS64lzpQng="),
        );
    }

    #[test]
    fn parse_csrf_missing_returns_none() {
        assert_eq!(parse_csrf("<html></html>"), None);
    }

    #[test]
    fn hardcoded_luogu_lang_covers_common_cases() {
        assert_eq!(hardcoded_luogu_lang("rust").as_deref(), Some("15"));
        assert_eq!(hardcoded_luogu_lang("cpp").as_deref(), Some("12"));
        assert_eq!(hardcoded_luogu_lang("C++17").as_deref(), Some("11"));
        assert_eq!(hardcoded_luogu_lang("python").as_deref(), Some("7"));
        assert_eq!(hardcoded_luogu_lang("haskell"), None);
    }

    #[test]
    fn urldecode_handles_slash_escape() {
        assert_eq!(urldecode("7847%2F2026"), "7847/2026");
        assert_eq!(urldecode("plain"), "plain");
    }

    #[test]
    fn extract_luogu_languages_walks_map_shape() {
        let html = r#"<script>var x = JSON.parse(decodeURIComponent("%7B%22currentData%22%3A%7B%22langs%22%3A%7B%221%22%3A%22Pascal%22%2C%2215%22%3A%22Rust%22%2C%2212%22%3A%22C%2B%2B17%22%7D%7D%7D"))</script>"#;
        let opts = extract_luogu_languages(html).unwrap();
        assert_eq!(opts.len(), 3);
        assert!(opts.iter().any(|(id, n)| id == "15" && n == "Rust"));
        assert!(opts.iter().any(|(id, n)| id == "12" && n == "C++17"));
    }
}

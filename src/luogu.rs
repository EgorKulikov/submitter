use crate::clear;
use crate::http::HttpClient;
use clipboard::{ClipboardContext, ClipboardProvider};
use crossterm::execute;
use crossterm::style::{Color, ResetColor, SetForegroundColor};
use regex::Regex;
use std::thread;
use std::time::Duration;

const USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36";

struct LuoguClient {
    http: HttpClient,
    cookies: String,
    uid: String,
}

impl LuoguClient {
    fn new() -> Self {
        let http = HttpClient::new("https://www.luogu.com.cn");
        let client_id = http.get_cookie("luogu_client_id").unwrap_or_default();
        let uid = http.get_cookie("luogu_uid").unwrap_or_default();
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
            Ok(body) => {
                body.contains(&format!("\"uid\":{}", self.uid)) || body.contains("currentUser")
            }
            Err(_) => false,
        }
    }

    fn login(&mut self) -> Result<(), String> {
        if self.is_logged_in() {
            println!("Already logged in");
            return Ok(());
        }

        println!("Export your Luogu cookies using EditThisCookie browser extension");
        println!("(click the extension icon on luogu.com.cn, then export)");
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
            self.http.set_cookie("luogu_client_id", &client_id);
            self.http.set_cookie("luogu_uid", &uid);
            println!("Login successful");
            Ok(())
        } else {
            self.cookies.clear();
            self.uid.clear();
            Err("Login failed: invalid cookies".to_string())
        }
    }

    fn get_records(&self, pid: &str) -> Result<Vec<serde_json::Value>, String> {
        let url = format!(
            "https://www.luogu.com.cn/record/list?user={}&pid={}&page=1",
            self.uid, pid
        );
        let body = self.get_page(&url)?;
        let re = Regex::new(r#"decodeURIComponent\("(.*?)"\)"#).unwrap();
        let encoded = re.captures(&body).ok_or("Could not find data in page")?[1].to_string();
        let decoded = urlencoded_decode(&encoded);
        let data: serde_json::Value =
            serde_json::from_str(&decoded).map_err(|e| format!("Failed to parse JSON: {}", e))?;
        Ok(data
            .pointer("/currentData/records/result")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default())
    }

    fn get_record_detail(&self, record_id: &str) -> Result<serde_json::Value, String> {
        let url = format!("https://www.luogu.com.cn/record/{}", record_id);
        let body = self.get_page(&url)?;
        let re = Regex::new(r#"decodeURIComponent\("(.*?)"\)"#).unwrap();
        let encoded = re.captures(&body).ok_or("Could not find data in page")?[1].to_string();
        let decoded = urlencoded_decode(&encoded);
        let data: serde_json::Value =
            serde_json::from_str(&decoded).map_err(|e| format!("Failed to parse JSON: {}", e))?;
        data.pointer("/currentData/record")
            .cloned()
            .ok_or("No record data found".to_string())
    }

    fn poll_verdict(
        &self,
        pid: &str,
        known_ids: &std::collections::HashSet<i64>,
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

fn urlencoded_decode(s: &str) -> String {
    let mut result = String::new();
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '%' {
            let hex: String = chars.by_ref().take(2).collect();
            if let Ok(byte) = u8::from_str_radix(&hex, 16) {
                result.push(byte as char);
            }
        } else if c == '+' {
            result.push(' ');
        } else {
            result.push(c);
        }
    }
    result
}

/// Parse Luogu URL
/// https://www.luogu.com.cn/problem/P1001 -> "P1001"
fn parse_problem_id(url: &str) -> Option<String> {
    let re = Regex::new(r"/problem/([A-Za-z0-9]+)").ok()?;
    re.captures(url).map(|c| c[1].to_string())
}

pub fn login() {
    let mut client = LuoguClient::new();
    if let Err(e) = client.login() {
        eprintln!("Login failed: {}", e);
    }
}

pub fn submit(url: String, _language: String, source: String) {
    let mut client = LuoguClient::new();

    if let Err(e) = client.login() {
        eprintln!("{}", e);
        return;
    }

    let pid = match parse_problem_id(&url) {
        Some(p) => p,
        None => {
            eprintln!("Could not parse problem ID from URL: {}", url);
            return;
        }
    };

    // Record existing submissions
    let mut known_ids = std::collections::HashSet::new();
    if let Ok(records) = client.get_records(&pid) {
        for r in &records {
            if let Some(id) = r.get("id").and_then(|v| v.as_i64()) {
                known_ids.insert(id);
            }
        }
    }

    // Copy source to clipboard and open submit page
    let mut ctx: ClipboardContext = ClipboardProvider::new().unwrap();
    ctx.set_contents(source).unwrap();

    let submit_url = format!("{}#submit", url);
    println!("Source code copied to clipboard");
    open::that(&submit_url).ok();

    // Poll for new submission
    if let Err(e) = client.poll_verdict(&pid, &known_ids) {
        eprintln!("Verdict polling failed: {}", e);
    }
}

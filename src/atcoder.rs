use crate::clear;
use crate::http::HttpClient;
use clipboard::{ClipboardContext, ClipboardProvider};
use crossterm::execute;
use crossterm::style::{Color, ResetColor, SetForegroundColor};
use regex::Regex;
use std::thread;
use std::time::Duration;

const USER_AGENT: &str =
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36";

struct AtcoderClient {
    http: HttpClient,
    revel_session: String,
}

impl AtcoderClient {
    fn new() -> Self {
        let http = HttpClient::new("https://atcoder.jp");
        let revel_session = http.get_cookie("atcoder_revel_session").unwrap_or_default();
        AtcoderClient {
            http,
            revel_session,
        }
    }

    fn get_page(&self, path: &str) -> Result<String, String> {
        reqwest::blocking::Client::builder()
            .redirect(reqwest::redirect::Policy::limited(5))
            .build()
            .unwrap()
            .get(format!("https://atcoder.jp{}", path))
            .header("Cookie", format!("REVEL_SESSION={}", self.revel_session))
            .header("User-Agent", USER_AGENT)
            .send()
            .map_err(|e| format!("GET failed: {}", e))?
            .text()
            .map_err(|e| format!("Failed to read response: {}", e))
    }

    fn is_logged_in(&self) -> bool {
        if self.revel_session.is_empty() {
            return false;
        }
        match self.get_page("/settings") {
            Ok(body) => !body.contains("userScreenName = \"\""),
            Err(_) => false,
        }
    }

    fn login(&mut self) -> Result<(), String> {
        if self.is_logged_in() {
            println!("Already logged in");
            return Ok(());
        }

        println!("Export your AtCoder cookies using EditThisCookie browser extension");
        println!("(click the extension icon, then the export button to copy JSON to clipboard)");
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

        // Parse JSON cookies array
        let cookies: Vec<serde_json::Value> = serde_json::from_str(input.trim())
            .map_err(|e| format!("Failed to parse cookies JSON: {}", e))?;

        let session = cookies
            .iter()
            .find(|c| c.get("name").and_then(|v| v.as_str()) == Some("REVEL_SESSION"))
            .and_then(|c| c.get("value").and_then(|v| v.as_str()))
            .ok_or("REVEL_SESSION cookie not found in input")?
            .to_string();

        self.revel_session = session.clone();
        if self.is_logged_in() {
            self.http.set_cookie("atcoder_revel_session", &session);
            println!("Login successful");
            Ok(())
        } else {
            self.revel_session.clear();
            Err("Login failed: invalid session cookie".to_string())
        }
    }

    fn poll_verdict(
        &self,
        contest_id: &str,
        known_ids: &std::collections::HashSet<String>,
    ) -> Result<(), String> {
        let mut stdout = std::io::stdout();
        let mut last_len = 0;
        let mut tracking_id: Option<String> = None;

        loop {
            let body = self.get_page(&format!("/contests/{}/submissions/me", contest_id))?;

            // Parse submissions table
            let tbody_re = Regex::new(r"(?s)<tbody>(.*?)</tbody>").unwrap();
            let row_re = Regex::new(r"(?s)<tr>(.*?)</tr>").unwrap();
            let sub_re = Regex::new(r"/submissions/(\d+)").unwrap();

            if let Some(tbody) = tbody_re.captures(&body) {
                for row_cap in row_re.captures_iter(&tbody[1]) {
                    let row = &row_cap[1];
                    let sub_id = match sub_re.captures(row) {
                        Some(c) => c[1].to_string(),
                        None => continue,
                    };

                    if let Some(ref tid) = tracking_id {
                        if sub_id != *tid {
                            continue;
                        }
                    } else if known_ids.contains(&sub_id) {
                        continue;
                    } else {
                        // New submission found
                        tracking_id = Some(sub_id.clone());
                        println!(
                            "Submission url: https://atcoder.jp/contests/{}/submissions/{}",
                            contest_id, sub_id
                        );
                    }

                    // Parse verdict from this row
                    let cells: Vec<String> = Regex::new(r"(?s)<td[^>]*>(.*?)</td>")
                        .unwrap()
                        .captures_iter(row)
                        .map(|c| {
                            Regex::new(r"<[^>]+>")
                                .unwrap()
                                .replace_all(&c[1], " ")
                                .trim()
                                .to_string()
                        })
                        .collect();

                    let label = Regex::new(r"label-(\w+)")
                        .unwrap()
                        .captures(row)
                        .map(|c| c[1].to_string())
                        .unwrap_or_default();

                    let verdict = cells.get(6).cloned().unwrap_or_default();
                    let score = cells.get(4).cloned().unwrap_or_default();

                    if label == "default" || verdict == "WJ" {
                        // Still judging
                        let progress = if verdict.contains('/') {
                            format!("Testing {}", verdict)
                        } else {
                            verdict.clone()
                        };
                        clear(last_len);
                        let _ = execute!(stdout, SetForegroundColor(Color::Yellow));
                        print!("{}", progress);
                        let _ = execute!(stdout, ResetColor);
                        last_len = progress.len();
                    } else {
                        // Final verdict
                        clear(last_len);
                        let is_accepted = label == "success";
                        let color = if is_accepted {
                            Color::Green
                        } else {
                            Color::Red
                        };
                        let mut display = verdict.clone();
                        if !is_accepted && !score.is_empty() && score != "0" {
                            display.push_str(&format!(" ({}pts)", score));
                        }
                        let _ = execute!(stdout, SetForegroundColor(color));
                        println!("{}", display);
                        let _ = execute!(stdout, ResetColor);
                        return Ok(());
                    }
                    break; // Only process the tracked submission
                }
            }

            thread::sleep(Duration::from_secs(2));
        }
    }
}

/// Parse AtCoder URL
/// https://atcoder.jp/contests/abc388/tasks/abc388_a -> ("abc388", "abc388_a")
fn parse_url(url: &str) -> Option<(String, String)> {
    let url = url.split('?').next().unwrap_or(url);
    let re = Regex::new(r"contests/([^/]+)/tasks/([^/]+)").ok()?;
    if let Some(caps) = re.captures(url) {
        return Some((caps[1].to_string(), caps[2].to_string()));
    }
    // Just contest URL — no task
    let re = Regex::new(r"contests/([^/]+)").ok()?;
    let caps = re.captures(url)?;
    Some((caps[1].to_string(), String::new()))
}

pub fn login() {
    let mut client = AtcoderClient::new();
    if let Err(e) = client.login() {
        eprintln!("Login failed: {}", e);
    }
}

pub fn submit(url: String, _language: String, source: String) {
    let mut client = AtcoderClient::new();

    if let Err(e) = client.login() {
        eprintln!("{}", e);
        return;
    }

    let (contest_id, task_id) = match parse_url(&url) {
        Some(parsed) => parsed,
        None => {
            eprintln!("Could not parse URL: {}", url);
            return;
        }
    };

    // Record existing submissions before opening browser
    let mut known_ids = std::collections::HashSet::new();
    if let Ok(body) = client.get_page(&format!("/contests/{}/submissions/me", contest_id)) {
        let sub_re = Regex::new(r"/submissions/(\d+)").unwrap();
        for cap in sub_re.captures_iter(&body) {
            known_ids.insert(cap[1].to_string());
        }
    }

    // Copy source to clipboard and open submit page
    let mut ctx: ClipboardContext = ClipboardProvider::new().unwrap();
    ctx.set_contents(source).unwrap();

    let submit_url = if task_id.is_empty() {
        format!("https://atcoder.jp/contests/{}/submit", contest_id)
    } else {
        format!(
            "https://atcoder.jp/contests/{}/submit?taskScreenName={}",
            contest_id, task_id
        )
    };
    println!("Source code copied to clipboard");
    open::that(&submit_url).ok();

    // Poll for new submission
    if let Err(e) = client.poll_verdict(&contest_id, &known_ids) {
        eprintln!("Verdict polling failed: {}", e);
    }
}

use crate::clear;
use crate::http::HttpClient;
use crossterm::execute;
use crossterm::style::{Color, ResetColor, SetForegroundColor};
use dialoguer::console::Term;
use dialoguer::Input;
use regex::Regex;
use reqwest::blocking::multipart;
use std::thread;
use std::time::Duration;

pub struct KattisClient {
    http: HttpClient,
    hostname: String,
    session_cookies: String,
}

impl KattisClient {
    pub fn new(hostname: &str) -> Self {
        let mut http = HttpClient::new(&format!("https://{}", hostname));
        http.disable_cookie_sending();
        KattisClient {
            http,
            hostname: hostname.to_string(),
            session_cookies: String::new(),
        }
    }

    fn login(&mut self) -> Result<(), String> {
        // Check for saved credentials
        let username = self.http.get_cookie("kattis_username");
        let token = self.http.get_cookie("kattis_token");

        let (username, token) = match (username, token) {
            (Some(u), Some(t)) => (u, t),
            _ => {
                println!(
                    "Download your .kattisrc from https://{}/download/kattisrc",
                    self.hostname
                );
                let u: String =
                    Input::with_theme(&dialoguer::theme::ColorfulTheme::default())
                        .with_prompt("Enter your Kattis username")
                        .interact_on(&Term::stdout())
                        .unwrap();
                let t: String =
                    Input::with_theme(&dialoguer::theme::ColorfulTheme::default())
                        .with_prompt("Enter your Kattis token")
                        .interact_on(&Term::stdout())
                        .unwrap();
                (u.trim().to_string(), t.trim().to_string())
            }
        };

        self.do_login(&username, &token)?;
        self.http.set_cookie("kattis_username", &username);
        self.http.set_cookie("kattis_token", &token);
        Ok(())
    }

    pub fn login_with_credentials(
        &mut self,
        username: &str,
        token: &str,
    ) -> Result<(), String> {
        self.do_login(username, token)?;
        self.http.set_cookie("kattis_username", username);
        self.http.set_cookie("kattis_token", token);
        Ok(())
    }

    fn do_login(&mut self, username: &str, token: &str) -> Result<(), String> {
        let client = reqwest::blocking::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .unwrap();
        let resp = client
            .post(format!("https://{}/login", self.hostname))
            .header("User-Agent", "kattis-cli-submit")
            .form(&[("user", username), ("token", token), ("script", "true")])
            .send()
            .map_err(|e| format!("Login request failed: {}", e))?;
        let status = resp.status();
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
        let body = resp.text().unwrap_or_default();
        if status.is_success() && body.contains("Login successful") {
            println!("Login successful");
            Ok(())
        } else if status.as_u16() == 403 {
            Err("Login failed: invalid credentials".to_string())
        } else {
            Err(format!("Login failed ({}): {}", status, body.trim()))
        }
    }

    pub fn submit(
        &mut self,
        problem: &str,
        language: &str,
        source: &str,
    ) -> Result<String, String> {
        self.submit_with_filename(problem, language, source, "solution.cpp")
    }

    pub fn submit_with_filename(
        &mut self,
        problem: &str,
        language: &str,
        source: &str,
        filename: &str,
    ) -> Result<String, String> {
        let form = multipart::Form::new()
            .text("submit", "true")
            .text("submit_ctr", "2")
            .text("language", language.to_string())
            .text("mainclass", "")
            .text("problem", problem.to_string())
            .text("tag", "")
            .text("script", "true")
            .part(
                "sub_file[]",
                multipart::Part::text(source.to_string())
                    .file_name(filename.to_string())
                    .mime_str("application/octet-stream")
                    .unwrap(),
            );

        let url = format!("https://{}/submit", self.hostname);
        let resp = reqwest::blocking::Client::new()
            .post(&url)
            .header("User-Agent", "kattis-cli-submit")
            .header("Cookie", &self.session_cookies)
            .multipart(form)
            .send()
            .map_err(|e| format!("Submit failed: {}", e))?;

        let body = resp.text().unwrap_or_default();
        let re = Regex::new(r"Submission ID: (\d+)").unwrap();
        match re.captures(&body) {
            Some(caps) => Ok(caps[1].to_string()),
            None => Err(format!("Could not find submission ID in response: {}", body.trim())),
        }
    }

    pub fn poll_verdict(&mut self, submission_id: &str) -> Result<String, String> {
        let mut stdout = std::io::stdout();
        let mut last_len = 0;

        loop {
            let resp = reqwest::blocking::Client::new()
                .get(format!(
                    "https://{}/submissions/{}?json",
                    self.hostname, submission_id
                ))
                .header("User-Agent", "kattis-cli-submit")
                .header("Cookie", &self.session_cookies)
                .send()
                .map_err(|e| format!("Poll failed: {}", e))?;
            let json: serde_json::Value = resp
                .json()
                .map_err(|e| format!("Failed to parse JSON: {}", e))?;

            let status_id = json
                .get("status_id")
                .and_then(|v| v.as_i64())
                .unwrap_or(0);

            let row_html = json
                .get("row_html")
                .and_then(|v| v.as_str())
                .unwrap_or("");

            if status_id <= 5 {
                // Still running
                let testcase_index = json
                    .get("testcase_index")
                    .and_then(|v| v.as_i64())
                    .unwrap_or(0);
                let total = count_total_tests(row_html);
                let progress = if testcase_index > 0 && total > 0 {
                    format!("Testing ({}/{})", testcase_index, total)
                } else if status_id <= 3 {
                    "Compiling".to_string()
                } else {
                    "Testing".to_string()
                };
                clear(last_len);
                let _ = execute!(stdout, SetForegroundColor(Color::Yellow));
                print!("{}", progress);
                let _ = execute!(stdout, ResetColor);
                last_len = progress.len();
                thread::sleep(Duration::from_millis(500));
                continue;
            }

            clear(last_len);

            let (verdict_name, is_accepted) = match status_id {
                16 => ("Accepted", true),
                6 => ("Judge Error", false),
                8 => ("Compilation Error", false),
                9 => ("Runtime Error", false),
                10 => ("Memory Limit Exceeded", false),
                11 => ("Output Limit Exceeded", false),
                12 => ("Time Limit Exceeded", false),
                13 => ("Illegal Function", false),
                14 => ("Wrong Answer", false),
                _ => ("Unknown", false),
            };

            let mut display = verdict_name.to_string();

            if !is_accepted && status_id != 8 {
                // Fetch the full submission page to check for test groups
                let groups = self.fetch_group_details(submission_id);
                if groups.len() > 1 {
                    let has_scores = groups.iter().any(|g| g.max_score.is_some());
                    if has_scores {
                        let total_score: usize =
                            groups.iter().filter_map(|g| g.score).sum();
                        let total_max: usize =
                            groups.iter().filter_map(|g| g.max_score).sum();
                        display.push_str(&format!(" ({}/{})", total_score, total_max));
                    }
                    // Print header line
                    let _ = execute!(stdout, SetForegroundColor(Color::Red));
                    println!("{}", display);
                    let _ = execute!(stdout, ResetColor);
                    // Print per-group lines with individual colors
                    for g in &groups {
                        let score_str = if has_scores {
                            format!(
                                " ({}/{})",
                                g.score.unwrap_or(0),
                                g.max_score.unwrap_or(0)
                            )
                        } else {
                            String::new()
                        };
                        let (line, line_color) = if g.passed == g.total {
                            (
                                format!("  Group {}{}: {}/{}", g.index, score_str, g.passed, g.total),
                                Color::Green,
                            )
                        } else if let Some(first_fail) = g.first_fail {
                            (
                                format!(
                                    "  Group {}{}: failed on test {}/{}",
                                    g.index, score_str, first_fail, g.total
                                ),
                                Color::Red,
                            )
                        } else {
                            (
                                format!("  Group {}{}: {}/{}", g.index, score_str, g.passed, g.total),
                                Color::Red,
                            )
                        };
                        let _ = execute!(stdout, SetForegroundColor(line_color));
                        println!("{}", line);
                        let _ = execute!(stdout, ResetColor);
                    }
                    // Skip the single println below
                    println!(
                        "Submission url: https://{}/submissions/{}",
                        self.hostname, submission_id
                    );
                    return Ok(verdict_name.to_string());
                } else {
                    // Single group or no group info — use the JSON test results
                    let test_details = parse_test_results(row_html);
                    if let Some(details) = &test_details {
                        if let Some(g) = details.groups.first() {
                            if let Some(first_fail) = g.first_fail {
                                display.push_str(&format!(
                                    " on test {}/{}",
                                    first_fail, g.total
                                ));
                            }
                        }
                    }
                }
            }

            let color = if is_accepted {
                Color::Green
            } else {
                Color::Red
            };
            let _ = execute!(stdout, SetForegroundColor(color));
            println!("{}", display);
            let _ = execute!(stdout, ResetColor);

            println!(
                "Submission url: https://{}/submissions/{}",
                self.hostname, submission_id
            );

            return Ok(verdict_name.to_string());
        }
    }

    fn fetch_group_details(&self, submission_id: &str) -> Vec<GroupResult> {
        let resp = reqwest::blocking::Client::new()
            .get(format!(
                "https://{}/submissions/{}",
                self.hostname, submission_id
            ))
            .header("User-Agent", "kattis-cli-submit")
            .header("Cookie", &self.session_cookies)
            .send();

        let html = match resp {
            Ok(r) => r.text().unwrap_or_default(),
            Err(_) => return Vec::new(),
        };

        // Extract just the testgroups section
        let tg_start = match html.find("testgroups-list-inner") {
            Some(pos) => pos,
            None => return Vec::new(),
        };
        // End at the next major section
        let tg_html = &html[tg_start..];
        let tg_end = tg_html
            .find("testcases-overall")
            .or_else(|| tg_html.find("submission-info"))
            .unwrap_or(tg_html.len().min(10000));
        let tg_html = &tg_html[..tg_end];

        let mut groups = Vec::new();
        let group_re = Regex::new(r"Group (\d+)").unwrap();
        let icon_re = Regex::new(r"status-icon (is-\w+)").unwrap();
        let mut captures: Vec<(usize, usize)> = Vec::new();
        for cap in group_re.captures_iter(tg_html) {
            let m = cap.get(0).unwrap();
            let group_num: usize = cap[1].parse().unwrap_or(0);
            captures.push((group_num, m.end()));
        }

        for (i, &(group_num, start)) in captures.iter().enumerate() {
            let end = if i + 1 < captures.len() {
                captures[i + 1].1 - format!("Group {}", captures[i + 1].0).len()
            } else {
                tg_html.len()
            };
            let section = &tg_html[start..end.min(tg_html.len())];
            // Only count icons that are testcase-related (status-icon)
            let mut accepted = 0;
            let mut rejected = 0;
            for icon_cap in icon_re.captures_iter(section) {
                match &icon_cap[1] {
                    "is-accepted" => accepted += 1,
                    "is-rejected" => rejected += 1,
                    "is-empty" => {} // don't count unchecked as total for display
                    _ => {}
                }
            }
            let empty_count = icon_re
                .captures_iter(section)
                .filter(|c| &c[1] == "is-empty")
                .count();
            let total = accepted + rejected + empty_count;

            if total == 0 {
                continue;
            }

            let first_fail = if rejected > 0 {
                Some(accepted + 1)
            } else {
                None
            };

            // Extract score: first "N/M" pattern in section is points
            let score_re = Regex::new(r"(\d+)\s*/\s*(\d+)").unwrap();
            let (score, max_score) = score_re
                .captures(section)
                .map(|c| {
                    (
                        c[1].parse::<usize>().ok(),
                        c[2].parse::<usize>().ok(),
                    )
                })
                .unwrap_or((None, None));

            groups.push(GroupResult {
                index: group_num,
                total,
                passed: accepted,
                first_fail,
                score,
                max_score,
            });
        }

        groups
    }
}

struct GroupResult {
    index: usize,
    total: usize,
    passed: usize,
    first_fail: Option<usize>,
    score: Option<usize>,
    max_score: Option<usize>,
}

struct TestResults {
    groups: Vec<GroupResult>,
}

fn count_total_tests(html: &str) -> i64 {
    let re = Regex::new(r"<i ").unwrap();
    let count = re.find_iter(html).count() as i64;
    // Subtract non-testcase icons (the status icon before testcases)
    if count > 0 { count } else { 0 }
}

fn parse_test_results(html: &str) -> Option<TestResults> {
    // Parse test case titles: "Test case 1/22: Wrong Answer" or "Test case 1/22: Accepted"
    // or "Test group 1, test case 2/5: Wrong Answer"
    let re = Regex::new(r#"title="([^"]+)""#).unwrap();
    let mut tests: Vec<(Option<usize>, usize, usize, bool)> = Vec::new(); // (group, index, total, accepted)

    let group_re =
        Regex::new(r"Test group (\d+), test case (\d+)/(\d+): (\w+)").unwrap();
    let simple_re = Regex::new(r"Test case (\d+)/(\d+): (.+)").unwrap();

    for cap in re.captures_iter(html) {
        let title = &cap[1];
        if title == "Test case not checked" {
            continue;
        }
        if let Some(gcap) = group_re.captures(title) {
            let group: usize = gcap[1].parse().unwrap_or(0);
            let index: usize = gcap[2].parse().unwrap_or(0);
            let total: usize = gcap[3].parse().unwrap_or(0);
            let accepted = gcap[4].contains("Accepted");
            tests.push((Some(group), index, total, accepted));
        } else if let Some(scap) = simple_re.captures(title) {
            let index: usize = scap[1].parse().unwrap_or(0);
            let total: usize = scap[2].parse().unwrap_or(0);
            let accepted = scap[3].contains("Accepted");
            tests.push((None, index, total, accepted));
        }
    }

    if tests.is_empty() {
        return None;
    }

    // Check if any test has group info
    let has_groups = tests.iter().any(|(g, _, _, _)| g.is_some());

    if has_groups {
        // Group by group number
        let mut groups: std::collections::BTreeMap<usize, GroupResult> =
            std::collections::BTreeMap::new();
        for (group, _index, total, accepted) in &tests {
            let g = group.unwrap_or(0);
            let entry = groups.entry(g).or_insert(GroupResult {
                index: g,
                total: *total,
                passed: 0,
                first_fail: None,
                score: None,
                max_score: None,
            });
            entry.total = *total;
            if *accepted {
                entry.passed += 1;
            } else if entry.first_fail.is_none() {
                entry.first_fail = Some(entry.passed + 1);
            }
        }
        Some(TestResults {
            groups: groups.into_values().collect(),
        })
    } else {
        // Single group from all tests
        let total = tests.first().map(|(_, _, t, _)| *t).unwrap_or(0);
        let passed = tests.iter().filter(|(_, _, _, a)| *a).count();
        let first_fail = tests
            .iter()
            .enumerate()
            .find(|(_, (_, _, _, a))| !*a)
            .map(|(_, (_, idx, _, _))| *idx);
        Some(TestResults {
            groups: vec![GroupResult {
                index: 1,
                total,
                passed,
                first_fail,
                score: None,
                max_score: None,
            }],
        })
    }
}

/// Parse problem ID from URL
/// https://open.kattis.com/problems/hello -> "hello"
/// https://open.kattis.com/contests/xxx/problems/hello -> "hello"
fn parse_problem_id(url: &str) -> Option<String> {
    let re = Regex::new(r"/problems/([^/?#]+)").unwrap();
    re.captures(url).map(|c| c[1].to_string())
}

/// Extract hostname from URL
/// https://open.kattis.com/problems/hello -> "open.kattis.com"
fn parse_hostname(url: &str) -> Option<String> {
    let re = Regex::new(r"https?://([^/]+)").unwrap();
    re.captures(url).map(|c| c[1].to_string())
}

/// Read .kattisrc config if available
fn read_kattisrc() -> Option<(String, String, String)> {
    let paths = [
        Some(std::path::PathBuf::from(".kattisrc")),
        Some(std::path::PathBuf::from("kattisrc")),
        dirs::home_dir().map(|p| p.join(".kattisrc")),
    ];
    for path in paths.iter().flatten() {
        if let Ok(content) = std::fs::read_to_string(path) {
            let mut username = None;
            let mut token = None;
            let mut hostname = None;
            for line in content.lines() {
                let line = line.trim();
                if line.starts_with("username:") {
                    username = Some(line.trim_start_matches("username:").trim().to_string());
                } else if line.starts_with("token:") {
                    token = Some(line.trim_start_matches("token:").trim().to_string());
                } else if line.starts_with("hostname:") {
                    hostname = Some(line.trim_start_matches("hostname:").trim().to_string());
                }
            }
            if let (Some(u), Some(t)) = (username, token) {
                return Some((
                    hostname.unwrap_or_else(|| "open.kattis.com".to_string()),
                    u,
                    t,
                ));
            }
        }
    }
    None
}

pub fn login() {
    let hostname = "open.kattis.com";
    let mut client = KattisClient::new(hostname);
    if let Some((_, username, token)) = read_kattisrc() {
        if let Err(e) = client.login_with_credentials(&username, &token) {
            eprintln!("Login failed: {}", e);
        }
    } else if let Err(e) = client.login() {
        eprintln!("Login failed: {}", e);
    }
}

pub fn submit(url: String, language: String, source: String, filename: String) {
    let hostname = parse_hostname(&url).unwrap_or_else(|| "open.kattis.com".to_string());
    let mut client = KattisClient::new(&hostname);

    // Try reading .kattisrc first
    if let Some((_, username, token)) = read_kattisrc() {
        println!("Logging in");
        if let Err(e) = client.login_with_credentials(&username, &token) {
            eprintln!("Login failed: {}", e);
            return;
        }
    } else {
        println!("Logging in");
        if let Err(e) = client.login() {
            eprintln!("Login failed: {}", e);
            return;
        }
    }

    let problem = match parse_problem_id(&url) {
        Some(p) => p,
        None => {
            eprintln!("Could not parse problem ID from URL: {}", url);
            return;
        }
    };

    println!("Submitting to problem '{}'", problem);
    let basename = std::path::Path::new(&filename)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("solution.cpp");
    let submission_id = match client.submit_with_filename(&problem, &language, &source, basename) {
        Ok(id) => id,
        Err(e) => {
            eprintln!("{}", e);
            return;
        }
    };

    if let Err(e) = client.poll_verdict(&submission_id) {
        eprintln!("Verdict polling failed: {}", e);
    }
}

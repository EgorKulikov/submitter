use crate::clear;
use crate::http::HttpClient;
use crossterm::execute;
use crossterm::style::{Color, ResetColor, SetForegroundColor};
use dialoguer::console::Term;
use dialoguer::Input;
use std::thread;
use std::time::Duration;

pub struct EolympClient {
    http: HttpClient,
}

impl EolympClient {
    pub fn new() -> Self {
        let http = HttpClient::new("https://api.eolymp.com");
        EolympClient { http }
    }

    pub fn http_mut(&mut self) -> &mut HttpClient {
        &mut self.http
    }

    pub fn login(&mut self) -> Result<(), String> {
        if let Some(key) = self.http.get_cookie("eolymp_api_key") {
            self.http
                .set_header("Authorization", &format!("Bearer {}", key));
            // Verify
            let result = self.http.get_json("/spaces/__lookup/basecamp");
            if result.is_ok() {
                println!("Already logged in");
                return Ok(());
            }
        }

        println!("Generate an API key at https://eolymp.com/developer");
        println!("Required scopes: atlas:problem:read, atlas:submission:read,");
        println!("  atlas:submission:write, judge:contest:read, judge:contest:participate");
        let key: String = Input::with_theme(&dialoguer::theme::ColorfulTheme::default())
            .with_prompt("Enter your eolymp API key")
            .interact_on(&Term::stdout())
            .unwrap();
        let key = key.trim().to_string();

        self.login_with_key(&key)
    }

    pub fn login_with_key(&mut self, key: &str) -> Result<(), String> {
        self.http
            .set_header("Authorization", &format!("Bearer {}", key));
        let result = self.http.get_json("/spaces/__lookup/basecamp");
        match result {
            Ok(_) => {
                self.http.set_cookie("eolymp_api_key", key);
                println!("Login successful");
                Ok(())
            }
            Err(e) => Err(format!("API key validation failed: {}", e)),
        }
    }

    /// Look up space URL by key (e.g. "basecamp")
    fn lookup_space(&mut self, space_key: &str) -> Result<String, String> {
        let json = self
            .http
            .get_json(&format!("/spaces/__lookup/{}", space_key))?;
        json.get("space")
            .and_then(|s| s.get("url"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .ok_or_else(|| "Could not find space URL".to_string())
    }

    /// Find problem ID by letter index (A, B, C...) in a contest
    pub fn find_problem_in_contest(
        &mut self,
        contest_url: &str,
        problem_index: &str,
    ) -> Result<(String, String), String> {
        // Accept both letter ("A") and number ("1") as problem index
        let index_num = if let Ok(n) = problem_index.parse::<i64>() {
            n
        } else {
            problem_index
                .chars()
                .next()
                .map(|c| c.to_ascii_uppercase() as i64 - 'A' as i64 + 1)
                .ok_or("Invalid problem index")?
        };

        let json = self
            .http
            .get_json(&format!("{}/problems?size=100", contest_url))?;

        if let Some(items) = json.get("items").and_then(|v| v.as_array()) {
            for p in items {
                let idx = p.get("index").and_then(|v| v.as_i64()).unwrap_or(0);
                if idx == index_num {
                    let id = p
                        .get("id")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    let title = p
                        .get("title")
                        .and_then(|v| v.as_str())
                        .unwrap_or(problem_index)
                        .to_string();
                    return Ok((id, title));
                }
            }
        }
        Err(format!("Problem {} not found in contest", problem_index))
    }

    /// Submit to a contest problem via judge.SubmissionService
    pub fn submit_contest(
        &mut self,
        contest_url: &str,
        contest_id: &str,
        problem_id: &str,
        language: &str,
        source: &str,
    ) -> Result<String, String> {
        let body = serde_json::json!({
            "contest_id": contest_id,
            "problem_id": problem_id,
            "lang": language,
            "source": source,
        });

        let resp = self.http.post_json(
            &format!("{}/problems/{}/submissions", contest_url, problem_id),
            &body.to_string(),
        )?;

        let resp_body = resp
            .text()
            .map_err(|e| format!("Failed to read submit response: {}", e))?;
        let json: serde_json::Value = serde_json::from_str(&resp_body).map_err(|e| {
            format!(
                "Failed to parse response: {} body: {}",
                e,
                &resp_body[..resp_body.len().min(200)]
            )
        })?;

        json.get("submissionId")
            .or_else(|| json.get("submission_id"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .ok_or_else(|| format!("No submission ID in response: {}", resp_body))
    }

    /// Submit to an archive problem via atlas.SubmissionService
    pub fn submit_archive(
        &mut self,
        space_url: &str,
        problem_id: &str,
        language: &str,
        source: &str,
    ) -> Result<String, String> {
        let body = serde_json::json!({
            "problem_id": problem_id,
            "lang": language,
            "source": source,
        });

        let resp = self
            .http
            .post_json(&format!("{}/submissions", space_url), &body.to_string())?;

        let resp_body = resp
            .text()
            .map_err(|e| format!("Failed to read submit response: {}", e))?;
        let json: serde_json::Value = serde_json::from_str(&resp_body).map_err(|e| {
            format!(
                "Failed to parse response: {} body: {}",
                e,
                &resp_body[..resp_body.len().min(200)]
            )
        })?;

        json.get("submissionId")
            .or_else(|| json.get("submission_id"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .ok_or_else(|| format!("No submission ID in response: {}", resp_body))
    }

    /// Poll verdict. base_url is the contest URL (for judge) or space URL (for atlas)
    pub fn poll_verdict(
        &mut self,
        base_url: &str,
        submission_id: &str,
    ) -> Result<String, String> {
        let mut stdout = std::io::stdout();
        let mut last_len = 0;
        loop {
            clear(last_len);
            let raw = self
                .http
                .get_json(&format!("{}/submissions/{}", base_url, submission_id))?;
            // Response may be wrapped: {"submission": {...}} or flat
            let result = raw.get("submission").unwrap_or(&raw);

            let status_num = result
                .get("status")
                .and_then(|v| {
                    v.as_i64().or_else(|| match v.as_str()? {
                        "PENDING" => Some(1),
                        "TESTING" => Some(2),
                        "TIMEOUT" => Some(3),
                        "COMPLETE" => Some(4),
                        "ERROR" => Some(5),
                        "FAILURE" => Some(6),
                        _ => None,
                    })
                })
                .unwrap_or(0);

            if status_num >= 3 {

                let verdict = result
                    .get("verdict")
                    .and_then(|v| v.as_str())
                    .unwrap_or("UNKNOWN");

                let score = result.get("score").and_then(|v| v.as_f64());
                let cost = result.get("cost").and_then(|v| v.as_f64());

                let (color, text) = match verdict {
                    "ACCEPTED" => (Color::Green, "Accepted".to_string()),
                    "WRONG_ANSWER" => (Color::Red, "Wrong Answer".to_string()),
                    "TIME_LIMIT_EXCEEDED" => (Color::Red, "Time Limit Exceeded".to_string()),
                    "CPU_EXHAUSTED" => (Color::Red, "CPU Exhausted".to_string()),
                    "MEMORY_OVERFLOW" => (Color::Red, "Memory Limit Exceeded".to_string()),
                    "RUNTIME_ERROR" => (Color::Red, "Runtime Error".to_string()),
                    _ => {
                        if status_num == 5 || status_num == 6 {
                            (Color::Red, format!("Error ({})", verdict))
                        } else {
                            (Color::Yellow, verdict.to_string())
                        }
                    }
                };

                let mut display = text;

                let groups = result
                    .get("groups")
                    .and_then(|v| v.as_array())
                    .cloned()
                    .unwrap_or_default();
                // Filter out the "overall" group (no index)
                let named_groups: Vec<_> = groups
                    .iter()
                    .filter(|g| g.get("index").is_some())
                    .collect();

                if let (Some(s), Some(c)) = (score, cost) {
                    if c > 0.0 && (verdict != "ACCEPTED" || named_groups.len() > 1) {
                        display.push_str(&format!(" ({}/{})", s, c));
                    }
                }

                if verdict != "ACCEPTED" && named_groups.len() > 1 {
                    // Print header line in red
                    let _ = execute!(stdout, SetForegroundColor(color));
                    println!("{}", display);
                    let _ = execute!(stdout, ResetColor);
                    // Print per-group lines with individual colors
                    for group in &named_groups {
                        let idx = group.get("index").and_then(|v| v.as_i64()).unwrap_or(0);
                        let gv = group
                            .get("verdict")
                            .and_then(|v| v.as_str())
                            .unwrap_or("");
                        let g_score = group.get("score").and_then(|v| v.as_f64());
                        let g_cost = group.get("cost").and_then(|v| v.as_f64());
                        let score_str = match (g_score, g_cost) {
                            (Some(s), Some(c)) if c > 0.0 => format!(" ({}/{})", s, c),
                            (_, Some(c)) if c > 0.0 => format!(" (0/{})", c),
                            _ => String::new(),
                        };

                        if gv == "ACCEPTED" {
                            let _ = execute!(stdout, SetForegroundColor(Color::Green));
                            println!("  Group {}{}: Accepted", idx, score_str);
                            let _ = execute!(stdout, ResetColor);
                        } else {
                            let short = match gv {
                                "WRONG_ANSWER" => "Wrong Answer",
                                "TIME_LIMIT_EXCEEDED" => "Time Limit Exceeded",
                                "CPU_EXHAUSTED" => "CPU Exhausted",
                                "MEMORY_OVERFLOW" => "Memory Limit Exceeded",
                                "RUNTIME_ERROR" => "Runtime Error",
                                "BLOCKED" => "Blocked",
                                "" | "NO_VERDICT" => "Not tested",
                                other => other,
                            };
                            let first_fail = group
                                .get("runs")
                                .and_then(|v| v.as_array())
                                .and_then(|runs| {
                                    let total = runs.len();
                                    runs.iter()
                                        .find(|r| {
                                            let rv = r
                                                .get("verdict")
                                                .and_then(|v| v.as_str())
                                                .unwrap_or("");
                                            rv != "ACCEPTED" && rv != "NO_VERDICT" && !rv.is_empty()
                                        })
                                        .and_then(|r| r.get("index").and_then(|v| v.as_i64()))
                                        .map(|fail_idx| (fail_idx, total))
                                });
                            let _ = execute!(stdout, SetForegroundColor(Color::Red));
                            if let Some((fail_idx, total)) = first_fail {
                                println!(
                                    "  Group {}{}: {} on test {}/{}",
                                    idx, score_str, short, fail_idx, total
                                );
                            } else {
                                println!("  Group {}{}: {}", idx, score_str, short);
                            }
                            let _ = execute!(stdout, ResetColor);
                        }
                    }
                    return Ok(verdict.to_string());
                } else if verdict != "ACCEPTED" && named_groups.len() == 1 {
                    // Single group — find first failing test
                    let group = &named_groups[0];
                    if let Some(runs) = group.get("runs").and_then(|v| v.as_array()) {
                        let total = runs.len();
                        if let Some(fail_run) = runs.iter().find(|r| {
                            let rv = r.get("verdict").and_then(|v| v.as_str()).unwrap_or("");
                            rv != "ACCEPTED" && rv != "NO_VERDICT" && !rv.is_empty()
                        }) {
                            let fail_idx =
                                fail_run.get("index").and_then(|v| v.as_i64()).unwrap_or(0);
                            display.push_str(&format!(" on test {}/{}", fail_idx, total));
                        }
                    }
                }

                let _ = execute!(stdout, SetForegroundColor(color));
                println!("{}", display);
                let _ = execute!(stdout, ResetColor);
                return Ok(verdict.to_string());
            } else {
                let progress = if status_num == 2 {
                    let groups = result
                        .get("groups")
                        .and_then(|v| v.as_array())
                        .cloned()
                        .unwrap_or_default();
                    let named: Vec<_> = groups
                        .iter()
                        .filter(|g| g.get("index").is_some())
                        .collect();
                    if named.len() > 1 {
                        let done = named
                            .iter()
                            .filter(|g| {
                                g.get("status").and_then(|v| v.as_str()) == Some("COMPLETE")
                            })
                            .count();
                        format!("Testing ({}/{})", done, named.len())
                    } else if named.len() == 1 {
                        if let Some(runs) = named[0].get("runs").and_then(|v| v.as_array()) {
                            let tested = runs
                                .iter()
                                .filter(|r| {
                                    r.get("status").and_then(|v| v.as_str()) == Some("COMPLETE")
                                })
                                .count();
                            format!("Testing ({}/{})", tested, runs.len())
                        } else {
                            "Testing".to_string()
                        }
                    } else {
                        "Testing".to_string()
                    }
                } else {
                    "Pending".to_string()
                };
                let _ = execute!(stdout, SetForegroundColor(Color::Yellow));
                print!("{}", progress);
                let _ = execute!(stdout, ResetColor);
                last_len = progress.len();
            }

            thread::sleep(Duration::from_secs(2));
        }
    }
}

/// Parse an eolymp URL
/// Contest: https://eolymp.com/en/contests/CONTEST_ID/problems/LETTER -> ContestProblem
/// Archive: https://eolymp.com/en/problems/ID -> ArchiveProblem
enum EolympUrl {
    ContestProblem {
        contest_id: String,
        problem_index: String,
    },
    ArchiveProblem {
        problem_id: String,
    },
}

fn parse_url(url: &str) -> Option<EolympUrl> {
    let url = url.trim_end_matches('/');

    // Contest: /contests/CID/problems/IDX or /compete/CID/problem/IDX
    for prefix in ["/contests/", "/compete/"] {
        if let Some(pos) = url.find(prefix) {
            let after = &url[pos + prefix.len()..];
            let parts: Vec<&str> = after.split('/').collect();
            // parts: ["CID", "problems"|"problem", "IDX", ...]
            if parts.len() >= 3 && (parts[1] == "problems" || parts[1] == "problem") {
                return Some(EolympUrl::ContestProblem {
                    contest_id: parts[0].to_string(),
                    problem_index: parts[2].to_string(),
                });
            }
        }
    }

    // Archive: /problems/PID or /problem/PID
    for prefix in ["/problems/", "/problem/"] {
        if let Some(pos) = url.find(prefix) {
            let after = &url[pos + prefix.len()..];
            let pid = after.split('/').next().unwrap_or(after);
            let pid = pid.split('?').next().unwrap_or(pid);
            return Some(EolympUrl::ArchiveProblem {
                problem_id: pid.to_string(),
            });
        }
    }

    None
}

/// Match a language string to an eolymp runtime ID
fn match_language(language: &str) -> Option<&'static str> {
    let lang = language.to_lowercase();
    let mappings: &[(&[&str], &str)] = &[
        (&["c++23", "cpp23", "c++ 23"], "cpp:23-gnu14"),
        (&["c++20", "cpp20", "c++ 20"], "cpp:20-gnu14"),
        (&["c++", "cpp", "c++14", "c++17"], "cpp:23-gnu14"),
        (&["c#", "csharp", "cs"], "csharp:5-dotnet"),
        (&["c", "c17", "c23"], "c:23-gnu14"),
        (&["python3", "python", "py3", "py"], "python:3.14-python"),
        (&["pypy", "pypy3"], "python:3.11-pypy"),
        (&["java"], "java:1.25"),
        (&["kotlin", "kt"], "kotlin:1.9"),
        (&["go", "golang"], "go:1.24"),
        (&["rust", "rs"], "rust:1.78"),
        (&["javascript", "js", "node"], "js:18"),
        (&["pascal", "pas"], "pascal:3.2"),
        (&["haskell", "hs"], "haskell:8.8-ghc"),
        (&["ruby", "rb"], "ruby:2.4"),
        (&["php"], "php:7.4"),
        (&["dart"], "dart:3.6"),
        (&["swift"], "swift:5.6"),
        (&["d"], "d:1-dmd"),
    ];

    for (patterns, runtime) in mappings {
        for p in *patterns {
            if lang == *p || lang.starts_with(p) {
                return Some(runtime);
            }
        }
    }

    None
}

pub fn login() {
    let mut client = EolympClient::new();
    if let Err(e) = client.login() {
        eprintln!("Login failed: {}", e);
    }
}

pub fn submit(url: String, language: String, source: String) {
    let mut client = EolympClient::new();

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

    let lang_id = if language.contains(':') {
        language.clone()
    } else {
        match match_language(&language) {
            Some(id) => id.to_string(),
            None => {
                eprintln!(
                    "Unknown language '{}'. Use a runtime ID like 'cpp:23-gnu14'",
                    language
                );
                return;
            }
        }
    };
    println!("Language: {}", lang_id);

    let space_url = match client.lookup_space("basecamp") {
        Ok(url) => url,
        Err(e) => {
            eprintln!("Failed to look up space: {}", e);
            return;
        }
    };

    match parsed {
        EolympUrl::ContestProblem {
            contest_id,
            problem_index,
        } => {
            let contest_url = format!("{}/contests/{}", space_url, contest_id);

            let (problem_id, problem_title) =
                match client.find_problem_in_contest(&contest_url, &problem_index) {
                    Ok(found) => found,
                    Err(e) => {
                        eprintln!("{}", e);
                        return;
                    }
                };
            println!("Problem: {}", problem_title);

            println!("Submitting");
            let submission_id = match client.submit_contest(
                &contest_url,
                &contest_id,
                &problem_id,
                &lang_id,
                &source,
            ) {
                Ok(id) => id,
                Err(e) => {
                    eprintln!("{}", e);
                    return;
                }
            };
            println!("Submission ID: {}", submission_id);

            if let Err(e) = client.poll_verdict(&contest_url, &submission_id) {
                eprintln!("Verdict polling failed: {}", e);
            }
        }
        EolympUrl::ArchiveProblem { problem_id } => {
            println!("Submitting");
            let submission_id =
                match client.submit_archive(&space_url, &problem_id, &lang_id, &source) {
                    Ok(id) => id,
                    Err(e) => {
                        eprintln!("{}", e);
                        return;
                    }
                };
            println!("Submission ID: {}", submission_id);

            if let Err(e) = client.poll_verdict(&space_url, &submission_id) {
                eprintln!("Verdict polling failed: {}", e);
            }
        }
    }
}

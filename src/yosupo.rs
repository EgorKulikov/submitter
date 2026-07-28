use crate::clear;
use crate::http::HttpClient;
use crossterm::execute;
use crossterm::style::{Color, ResetColor, SetForegroundColor};
use regex::Regex;
use std::thread;
use std::time::Duration;

const API_BASE: &str = "https://v3.api.judge.yosupo.jp";
const SITE_ORIGIN: &str = "https://judge.yosupo.jp";
const USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36";

pub struct YosupoClient {
    http: HttpClient,
}

impl YosupoClient {
    pub fn new() -> Self {
        let mut http = HttpClient::new(API_BASE);
        http.set_header("User-Agent", USER_AGENT);
        // The API is on a subdomain; the browser POSTs it from judge.yosupo.jp
        // and needs the corresponding CORS headers, so Origin/Referer match.
        http.set_header("Origin", SITE_ORIGIN);
        http.set_header("Referer", &format!("{}/", SITE_ORIGIN));
        YosupoClient { http }
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
            let case_count = json
                .get("case_results")
                .and_then(|v| v.as_array())
                .map(|a| a.len())
                .unwrap_or(0);

            if is_terminal(status) {
                let color = if status == "AC" { Color::Green } else { Color::Red };
                let mut display = status_name(status).to_string();
                if status != "AC" && case_count > 0 {
                    // Point at the first non-AC case for a bit of context.
                    if let Some(cases) = json.get("case_results").and_then(|v| v.as_array()) {
                        if let Some(bad) = cases.iter().find(|c| {
                            c.get("status").and_then(|v| v.as_str()).unwrap_or("") != "AC"
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
                format!("Judging ({} cases)", case_count)
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
fn is_terminal(status: &str) -> bool {
    match status {
        "" | "WJ" | "Judging" | "Compiling" | "Waiting" => false,
        _ => true,
    }
}

fn status_name(s: &str) -> &'static str {
    match s {
        "AC" => "Accepted",
        "WA" => "Wrong Answer",
        "TLE" => "Time Limit Exceeded",
        "MLE" => "Memory Limit Exceeded",
        "RE" => "Runtime Error",
        "CE" => "Compilation Error",
        "IE" => "Internal Error",
        "OLE" => "Output Limit Exceeded",
        "WJ" => "Waiting for Judge",
        "Judging" => "Judging",
        "Compiling" => "Compiling",
        "" => "Pending",
        _ => "Unknown",
    }
}

pub fn login() {
    println!("judge.yosupo.jp requires no authentication — nothing to do.");
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
        assert!(!is_terminal("Judging"));
        assert!(is_terminal("AC"));
        assert!(is_terminal("WA"));
        assert!(is_terminal("TLE"));
    }
}

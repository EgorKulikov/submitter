use crate::clear;
use crate::http::HttpClient;
use crossterm::execute;
use crossterm::style::{Color, ResetColor, SetForegroundColor};
use dialoguer::console::Term;
use dialoguer::{Input, Password};
use hmac::{Hmac, Mac};
use md5::{Digest, Md5};
use scraper::{Html, Selector};
use std::thread;
use std::time::Duration;

type HmacMd5 = Hmac<Md5>;

pub struct UojClient {
    http: HttpClient,
    site_name: String,
}

impl UojClient {
    pub fn http_mut(&mut self) -> &mut HttpClient {
        &mut self.http
    }

    pub fn new(base_url: &str, site_name: &str) -> Self {
        UojClient {
            http: HttpClient::new(base_url),
            site_name: site_name.to_string(),
        }
    }

    fn get_html(&mut self, path: &str) -> Result<Html, String> {
        let text = self.http.get_text(path)?;
        Ok(Html::parse_document(&text))
    }

    fn extract_token(doc: &Html) -> Option<String> {
        // Try hidden input first
        let sel = Selector::parse("input[name=\"_token\"]").unwrap();
        if let Some(el) = doc.select(&sel).next() {
            if let Some(val) = el.value().attr("value") {
                return Some(val.to_string());
            }
        }
        // Also look in JavaScript: _token : "..." or _token: "..."
        let sel = Selector::parse("script").unwrap();
        for script in doc.select(&sel) {
            let text = script.text().collect::<String>();
            if let Some(pos) = text.find("_token") {
                let after = &text[pos + 6..];
                let after = after.trim_start();
                let after = if after.starts_with(':') || after.starts_with('=') {
                    after[1..].trim_start()
                } else {
                    continue;
                };
                if after.starts_with('"') {
                    if let Some(end) = after[1..].find('"') {
                        return Some(after[1..1 + end].to_string());
                    }
                } else if after.starts_with('\'') {
                    if let Some(end) = after[1..].find('\'') {
                        return Some(after[1..1 + end].to_string());
                    }
                }
            }
        }
        None
    }

    fn extract_md5_key(doc: &Html) -> Option<String> {
        let sel = Selector::parse("script").unwrap();
        for script in doc.select(&sel) {
            let text = script.text().collect::<String>();
            if let Some(pos) = text.find("md5(") {
                let after_md5 = &text[pos + 4..];
                for quote in ['"', '\''] {
                    let pattern = format!(", {}", quote);
                    if let Some(comma_pos) = after_md5.find(&pattern) {
                        let start = comma_pos + pattern.len();
                        if let Some(end) = after_md5[start..].find(quote) {
                            return Some(after_md5[start..start + end].to_string());
                        }
                    }
                }
            }
        }
        None
    }

    fn hash_password(password: &str, hmac_key: Option<&str>) -> String {
        match hmac_key {
            Some(key) => {
                let mut mac =
                    HmacMd5::new_from_slice(key.as_bytes()).expect("HMAC accepts any key size");
                mac.update(password.as_bytes());
                format!("{:x}", mac.finalize().into_bytes())
            }
            None => {
                let mut hasher = Md5::new();
                hasher.update(password.as_bytes());
                format!("{:x}", hasher.finalize())
            }
        }
    }

    fn extract_matching_language(page_source: &str, language: &str) -> Option<String> {
        let mut matched = None;
        let lang_lower = language.to_lowercase();
        let mut search_from = 0;
        while let Some(pos) = page_source[search_from..].find("option value=\\\"") {
            let start = search_from + pos + 15;
            if let Some(end) = page_source[start..].find("\\\"") {
                let value = &page_source[start..start + end];
                if value.to_lowercase().starts_with(&lang_lower) {
                    matched = Some(value.to_string());
                }
                search_from = start + end;
            } else {
                break;
            }
        }
        matched
    }

    pub fn login(&mut self) -> Result<(), String> {
        if self.is_logged_in()? {
            println!("Already logged in");
            return Ok(());
        }

        let login: String = Input::with_theme(&dialoguer::theme::ColorfulTheme::default())
            .with_prompt(format!("Enter your {} login", self.site_name))
            .interact_on(&Term::stdout())
            .unwrap();
        let password: String = Password::with_theme(&dialoguer::theme::ColorfulTheme::default())
            .with_prompt(format!("Enter your {} password", self.site_name))
            .interact_on(&Term::stdout())
            .unwrap();

        self.login_with_credentials(&login, &password)
    }

    pub fn login_with_credentials(
        &mut self,
        username: &str,
        password: &str,
    ) -> Result<(), String> {
        // Clear stale cookies to ensure clean login
        self.http.clear_cookies();
        let doc = self.get_html("/login")?;
        let token =
            Self::extract_token(&doc).ok_or("Could not find CSRF token on login page")?;
        let hmac_key = Self::extract_md5_key(&doc);

        let hashed = Self::hash_password(password, hmac_key.as_deref());

        let resp = self.http.post_form(
            "/login",
            &[
                ("_token", token.as_str()),
                ("login", ""),
                ("username", username),
                ("password", hashed.as_str()),
            ],
        )?;

        let body = resp
            .text()
            .map_err(|e| format!("Failed to read login response: {}", e))?;
        match body.trim() {
            "ok" => {
                println!("Login successful");
                Ok(())
            }
            "failed" => Err("Login failed: wrong username or password".to_string()),
            "banned" => Err("Login failed: account is banned".to_string()),
            "expired" => Err("Login failed: CSRF token expired".to_string()),
            other => Err(format!("Login failed: unexpected response: {}", other)),
        }
    }

    fn is_logged_in(&mut self) -> Result<bool, String> {
        let doc = self.get_html("/")?;
        let sel = Selector::parse("a[href*=\"logout\"]").unwrap();
        Ok(doc.select(&sel).next().is_some())
    }

    pub fn submit(
        &mut self,
        problem_path: &str,
        language: &str,
        source: &str,
    ) -> Result<String, String> {
        let body = self.http.get_text(problem_path)?;
        let doc = Html::parse_document(&body);
        let token =
            Self::extract_token(&doc).ok_or("Could not find CSRF token on problem page")?;

        let matched_language = Self::extract_matching_language(&body, language)
            .ok_or_else(|| format!("Language '{}' not found", language))?;

        let resp = self.http.post_form(
            problem_path,
            &[
                ("_token", token.as_str()),
                ("answer_answer_language", matched_language.as_str()),
                ("answer_answer_upload_type", "editor"),
                ("answer_answer_editor", source),
                ("submit-answer", "answer"),
            ],
        )?;

        let final_url = resp.url().to_string();
        let body = resp
            .text()
            .map_err(|e| format!("Failed to read response: {}", e))?;

        let submission_id = find_submission_id_in_url(&final_url)
            .or_else(|| find_submission_id_in_html(&body));

        let id = submission_id.ok_or(
            "Submitted but could not find submission ID to track verdict".to_string(),
        )?;
        println!("Submission url: {}/submission/{}", self.http.base_url(), id);
        self.poll_verdict(&id)
    }

    fn fetch_submission_verdict(&mut self, submission_id: &str) -> Result<String, String> {
        let body = self.http.get_text(&format!("/submission/{}", submission_id))?;
        let doc = Html::parse_document(&body);

        let score_sel = Selector::parse(".uoj-score").unwrap();
        let score_el = doc.select(&score_sel).next();
        let score_text = score_el.map(|el| el.text().collect::<String>().trim().to_string());
        let data_score = score_el
            .and_then(|el| el.value().attr("data-score"))
            .map(|s| s.to_string());

        let is_accepted = data_score.as_deref() == Some("100")
            || score_text.as_deref() == Some("100")
            || score_text
                .as_deref()
                .map_or(false, |s| s.starts_with("AC"));

        let display_score = score_text.unwrap_or_else(|| "Unknown".to_string());

        if is_accepted {
            return Ok(display_score);
        }

        Ok(match find_first_failing_test(&doc) {
            Some((verdict, test)) => format!("{} test #{}", verdict, test),
            None => display_score,
        })
    }

    fn poll_verdict(&mut self, submission_id: &str) -> Result<String, String> {
        let mut stdout = std::io::stdout();
        let mut last_len = 0;
        loop {
            clear(last_len);
            let body = self.http.get_text(&format!(
                "/submission-status-details?get[]={}",
                submission_id
            ))?;

            let json: Vec<serde_json::Value> = match serde_json::from_str(&body) {
                Ok(v) => v,
                Err(_) => {
                    thread::sleep(Duration::from_secs(2));
                    continue;
                }
            };

            if let Some(entry) = json.first() {
                let judged = entry.get("judged").and_then(|v| v.as_bool()).unwrap_or(false);
                let html = entry
                    .get("html")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");

                let verdict = extract_verdict_from_html(html);

                if judged {
                    let final_verdict = self.fetch_submission_verdict(submission_id)?;
                    let color = if final_verdict.starts_with("100")
                        || final_verdict.starts_with("AC")
                        || final_verdict.contains("Accepted")
                    {
                        Color::Green
                    } else {
                        Color::Red
                    };
                    let _ = execute!(stdout, SetForegroundColor(color));
                    println!("{}", final_verdict);
                    let _ = execute!(stdout, ResetColor);
                    return Ok(final_verdict);
                } else {
                    let _ = execute!(stdout, SetForegroundColor(Color::Yellow));
                    print!("{}", verdict);
                    let _ = execute!(stdout, ResetColor);
                    last_len = verdict.len();
                }
            }

            thread::sleep(Duration::from_secs(2));
        }
    }
}

fn find_submission_id_in_url(url: &str) -> Option<String> {
    if let Some(pos) = url.find("/submission/") {
        let after = &url[pos + 12..];
        let end = after
            .find(|c: char| !c.is_ascii_digit())
            .unwrap_or(after.len());
        if end > 0 {
            return Some(after[..end].to_string());
        }
    }
    None
}

fn find_submission_id_in_html(body: &str) -> Option<String> {
    let doc = Html::parse_document(body);
    let sel = Selector::parse("a[href*=\"/submission/\"]").unwrap();
    for el in doc.select(&sel) {
        if let Some(href) = el.value().attr("href") {
            if let Some(id) = find_submission_id_in_url(href) {
                return Some(id);
            }
        }
    }
    None
}

/// Find the first failing test on a UOJ submission page and return
/// `(verdict, test_number)`. UOJ wraps each test in a card whose
/// class indicates the outcome (`card-uoj-accepted`, `card-uoj-tle`,
/// `card-uoj-wrong-answer`, …); inside the card-header row the verdict
/// text sits in its own `<div class="col-sm-2">`:
///
/// ```html
/// <div class="card card-uoj-tle">
///   <div class="card-header"><div class="row">
///     <div class="col-sm-2"><h4>Test #2: </h4></div>
///     <div class="col-sm-2">score: -100</div>
///     <div class="col-sm-2">Time Limit Exceeded</div>
///     ...
/// ```
fn find_first_failing_test(doc: &Html) -> Option<(String, String)> {
    let card_sel = Selector::parse(".card").ok()?;
    let h4_sel = Selector::parse("h4").ok()?;
    let col_sel = Selector::parse(".card-header .col-sm-2").ok()?;
    let test_num_re = regex::Regex::new(r"Test\s*#(\d+)").ok()?;
    for card in doc.select(&card_sel) {
        let classes = card.value().attr("class").unwrap_or("");
        let is_test_card = classes.split_whitespace().any(|c| c.starts_with("card-uoj-"));
        if !is_test_card {
            continue;
        }
        let is_accepted = classes
            .split_whitespace()
            .any(|c| c == "card-uoj-accepted");
        if is_accepted {
            continue;
        }
        let h4 = card.select(&h4_sel).next()?;
        let heading = h4.text().collect::<String>();
        let test_num = test_num_re.captures(&heading)?.get(1)?.as_str().to_string();
        // Verdict cell: a .col-sm-2 that holds neither the heading nor
        // the "score: ..." cell.
        let raw_verdict = card
            .select(&col_sel)
            .find(|el| {
                if el.select(&h4_sel).next().is_some() {
                    return false;
                }
                let text = el.text().collect::<String>();
                let trimmed = text.trim();
                !trimmed.is_empty() && !trimmed.to_lowercase().starts_with("score")
            })
            .map(|el| el.text().collect::<String>().trim().to_string())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| {
                classes
                    .split_whitespace()
                    .find_map(|c| c.strip_prefix("card-uoj-"))
                    .unwrap_or("Failed")
                    .replace('-', " ")
            });
        // Strip checker detail like "Wrong Answer : read 5 but expected 3".
        let verdict = raw_verdict
            .split_once(':')
            .map(|(head, _)| head.trim().to_string())
            .unwrap_or(raw_verdict);
        return Some((verdict, test_num));
    }
    None
}

fn extract_verdict_from_html(html: &str) -> String {
    let doc = Html::parse_fragment(html);
    let score_sel = Selector::parse(".uoj-score").ok();
    if let Some(sel) = &score_sel {
        if let Some(el) = doc.select(sel).next() {
            return el.text().collect::<String>().trim().to_string();
        }
    }
    let status_sel = Selector::parse(".uoj-status-details-text-div").ok();
    if let Some(sel) = &status_sel {
        if let Some(el) = doc.select(sel).next() {
            return el.text().collect::<String>().trim().to_string();
        }
    }
    doc.root_element()
        .text()
        .collect::<String>()
        .trim()
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn card(class: &str, num: u32, score: i32, verdict: &str) -> String {
        format!(
            r#"<div class="card {cls} mb-3"><div class="card-header"><div class="row">
                <div class="col-sm-2"><h4 class="card-title">Test #{n}: </h4></div>
                <div class="col-sm-2">score: {s}</div>
                <div class="col-sm-2">{v}</div>
                <div class="col-sm-3">time: 0ms</div>
                <div class="col-sm-3">memory: 2364kb</div>
            </div></div></div>"#,
            cls = class,
            n = num,
            s = score,
            v = verdict,
        )
    }

    #[test]
    fn skips_accepted_cards_and_reports_first_failing_card() {
        let html = format!(
            "<html><body>{}{}{}</body></html>",
            card("card-uoj-accepted", 1, 100, "Accepted"),
            card("card-uoj-tle", 2, -100, "Time Limit Exceeded"),
            card("card-uoj-wrong-answer", 3, -100, "Wrong Answer"),
        );
        let doc = Html::parse_document(&html);
        assert_eq!(
            find_first_failing_test(&doc),
            Some(("Time Limit Exceeded".to_string(), "2".to_string()))
        );
    }

    #[test]
    fn returns_none_when_all_cards_accepted() {
        let html = format!(
            "<html><body>{}{}</body></html>",
            card("card-uoj-accepted", 1, 100, "Accepted"),
            card("card-uoj-accepted", 2, 100, "Accepted"),
        );
        let doc = Html::parse_document(&html);
        assert_eq!(find_first_failing_test(&doc), None);
    }

    #[test]
    fn ignores_non_test_cards() {
        let mut html = String::from(
            r#"<div class="card border-info"><div class="card-header"><h4>Judging History</h4></div></div>"#,
        );
        html.push_str(&card("card-uoj-wrong-answer", 3, -100, "Wrong Answer"));
        let doc = Html::parse_document(&html);
        assert_eq!(
            find_first_failing_test(&doc),
            Some(("Wrong Answer".to_string(), "3".to_string()))
        );
    }

    #[test]
    fn strips_checker_detail_after_colon() {
        let html = card(
            "card-uoj-wrong-answer",
            4,
            -100,
            "Wrong Answer : read 5 but expected 3",
        );
        let doc = Html::parse_document(&html);
        assert_eq!(
            find_first_failing_test(&doc),
            Some(("Wrong Answer".to_string(), "4".to_string()))
        );
    }

    #[test]
    fn falls_back_to_card_class_when_verdict_cell_empty() {
        let html = card("card-uoj-tle", 8, -100, "");
        let doc = Html::parse_document(&html);
        assert_eq!(
            find_first_failing_test(&doc),
            Some(("tle".to_string(), "8".to_string()))
        );
    }
}

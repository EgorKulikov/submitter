use crate::clear;
use crate::http::HttpClient;
use crossterm::execute;
use crossterm::style::{Color, ResetColor, SetForegroundColor};
use reqwest::blocking::multipart;
use std::thread;
use std::time::Duration;

const BASE_URL: &str = "https://doj.kr";

/// Props of the submit form that the page renders as hidden inputs. Each one
/// that is set goes into the submit request as `_1_<name>`; on a plain
/// problem page only the first three are.
const FORM_FIELDS: &[&str] = &[
    "locale",
    "problemId",
    "problemSlug",
    "contestId",
    "contestProblemId",
    "groupPracticeId",
    "groupPracticeProblemId",
    "groupSlug",
    "categoryId",
    "categoryParticipationId",
    "categorySlug",
    "workbookId",
    "workbookParticipationId",
    "dlcVariantId",
];

/// Verdicts that mean the judge has not finished yet.
const PENDING_VERDICTS: &[&str] = &["QUEUED", "PENDING", "WAITING", "COMPILING", "RUNNING", "JUDGING"];

/// What the problem page tells us about its submit form.
#[derive(Debug, PartialEq)]
struct SubmitForm {
    /// Next.js server action ID. It changes with every deployment of the
    /// site, so it is read from the page each time.
    action_id: String,
    /// (form field name, value) pairs for the hidden inputs.
    fields: Vec<(String, String)>,
    locale: String,
    languages: Vec<String>,
    disabled_reason: Option<String>,
}

pub struct DojClient {
    http: HttpClient,
}

impl DojClient {
    pub fn new() -> Self {
        DojClient {
            http: HttpClient::new(BASE_URL),
        }
    }

    pub fn login(&mut self) -> Result<(), String> {
        if self.is_logged_in() {
            println!("Already logged in");
            return Ok(());
        }
        self.prompt_for_cookies()?;
        if self.is_logged_in() {
            println!("Login successful");
            Ok(())
        } else {
            Err("Login failed: pasted cookies did not authenticate".to_string())
        }
    }

    fn is_logged_in(&mut self) -> bool {
        // Anonymous requests get 200 {"authenticated":false,"submissions":[]}.
        match self.http.get_json("/api/my-submissions?locale=en&limit=1&page=1") {
            Ok(json) => json.get("authenticated").and_then(|v| v.as_bool()) == Some(true),
            Err(_) => false,
        }
    }

    fn prompt_for_cookies(&mut self) -> Result<(), String> {
        println!("Export your DOJ cookies using the EditThisCookie browser extension on a logged-in doj.kr tab");
        println!("Paste the JSON cookies array:");
        let mut input = String::new();
        let mut bracket_count: i32 = 0;
        loop {
            let mut line = String::new();
            let read = std::io::stdin()
                .read_line(&mut line)
                .map_err(|e| format!("Failed to read input: {}", e))?;
            if read == 0 {
                // End of input, e.g. when not run from a terminal.
                break;
            }
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
        let mut saved = 0;
        for c in &cookies {
            let name = c.get("name").and_then(|v| v.as_str()).unwrap_or("");
            let value = c.get("value").and_then(|v| v.as_str()).unwrap_or("");
            if name.is_empty() {
                continue;
            }
            self.http.set_cookie(name, value);
            saved += 1;
        }
        if saved == 0 {
            return Err("No cookies found in pasted JSON.".to_string());
        }
        Ok(())
    }

    /// Submit and return (submission id, locale of the page).
    pub fn submit(
        &mut self,
        url: &str,
        language: &str,
        source: &str,
    ) -> Result<(String, String), String> {
        // `RSC: 1` makes Next.js answer with the page's component payload as
        // plain text instead of HTML with the same payload escaped in scripts.
        let page = self
            .http
            .get_with_header(url, "RSC", "1")?
            .text()
            .map_err(|e| format!("Failed to read problem page: {}", e))?;
        let form = parse_submit_form(&page)?;
        if let Some(reason) = &form.disabled_reason {
            return Err(format!("Submitting is disabled for this problem: {}", reason));
        }
        let language_id = find_language(language, &form.languages).ok_or_else(|| {
            format!(
                "Language '{}' not found. Available: {}",
                language,
                form.languages.join(", ")
            )
        })?;
        println!("Language: {}", language_id);

        let mut body = multipart::Form::new();
        for (name, value) in &form.fields {
            body = body.text(format!("_1_{}", name), value.clone());
        }
        body = body
            .text("_1_language", language_id)
            .text("_1_sourceCode", source.to_string())
            // The action's argument list: one argument, the form data above.
            .text("0", "[\"$K1\"]");

        // These must not leak into later requests: the site answers 403 to
        // a GET that carries `next-action`.
        let resp = self.http.post_multipart_with_headers(
            url,
            body,
            &[
                ("next-action", form.action_id.as_str()),
                ("accept", "text/x-component"),
                ("origin", BASE_URL),
                ("referer", url),
            ],
        )?;
        let status = resp.status();
        let text = resp
            .text()
            .map_err(|e| format!("Failed to read submit response: {}", e))?;
        if !status.is_success() {
            return Err(format!(
                "Submit failed ({}): {}",
                status,
                prefix(&text, 300)
            ));
        }
        let id = parse_submit_response(&text)?;
        println!("Submission url: {}/{}/submissions/{}", BASE_URL, form.locale, id);
        Ok((id, form.locale))
    }

    pub fn poll_verdict(&mut self, submission_id: &str) -> Result<String, String> {
        let mut stdout = std::io::stdout();
        let mut last_len = 0;
        let mut missing = 0;
        loop {
            let json = self
                .http
                .get_json("/api/my-submissions?locale=en&limit=20&page=1")?;
            clear(last_len);
            last_len = 0;
            if json.get("authenticated").and_then(|v| v.as_bool()) == Some(false) {
                return Err("Not logged in any more".to_string());
            }
            let submission = json
                .get("submissions")
                .and_then(|v| v.as_array())
                .and_then(|list| {
                    list.iter()
                        .find(|s| s.get("id").and_then(|v| v.as_str()) == Some(submission_id))
                });
            let submission = match submission {
                Some(s) => s,
                None => {
                    missing += 1;
                    if missing >= 15 {
                        return Err(format!(
                            "Submission {} did not appear in the submissions list",
                            submission_id
                        ));
                    }
                    thread::sleep(Duration::from_secs(2));
                    continue;
                }
            };
            let (display, state) = describe(submission);
            let color = match state {
                State::Pending => Color::Yellow,
                State::Accepted => Color::Green,
                State::Rejected => Color::Red,
            };
            let _ = execute!(stdout, SetForegroundColor(color));
            if state == State::Pending {
                print!("{}", display);
                let _ = execute!(stdout, ResetColor);
                let _ = std::io::Write::flush(&mut stdout);
                last_len = display.len();
                thread::sleep(Duration::from_secs(2));
            } else if state == State::Accepted {
                println!("{}", display);
                let _ = execute!(stdout, ResetColor);
                return Ok(display);
            } else {
                let _ = execute!(stdout, ResetColor);
                let subtasks = self.subtasks(submission_id, submission);
                let lines = breakdown(&display, &subtasks);
                for (line, accepted) in &lines {
                    let color = if *accepted { Color::Green } else { Color::Red };
                    let _ = execute!(stdout, SetForegroundColor(color));
                    println!("{}", line);
                    let _ = execute!(stdout, ResetColor);
                }
                return Ok(lines.into_iter().next().map(|l| l.0).unwrap_or(display));
            }
        }
    }

    /// Per-subtask results of a judged submission. Scores come from the
    /// submissions list; the submission page adds which test failed.
    fn subtasks(&mut self, submission_id: &str, submission: &serde_json::Value) -> Vec<Subtask> {
        let mut subtasks = subtasks_from_list(submission);
        let page = self
            .http
            .get_with_header(&format!("/en/submissions/{}", submission_id), "RSC", "1")
            .ok()
            .filter(|resp| resp.status().is_success())
            .and_then(|resp| resp.text().ok());
        let from_page = page.map(|p| parse_subtask_panels(&p)).unwrap_or_default();
        if subtasks.is_empty() {
            return from_page;
        }
        // Only trust the page when it clearly shows the same subtasks.
        if from_page.len() == subtasks.len() {
            for (subtask, panel) in subtasks.iter_mut().zip(from_page) {
                if !panel.verdict.is_empty() {
                    subtask.verdict = panel.verdict;
                }
                subtask.first_fail = panel.first_fail;
            }
        }
        subtasks
    }
}

#[derive(Debug, PartialEq, Clone, Default)]
struct Subtask {
    /// "Accepted", "Wrong Answer", "Skipped", ...
    verdict: String,
    /// "0/100", or empty when unknown.
    score: String,
    /// (1-based index of the first failed test, number of tests).
    first_fail: Option<(usize, usize)>,
}

/// Subtasks as listed in an entry of `/api/my-submissions`:
/// `"subtaskResults":[{"orderIndex":0,"score":0,"maxScore":100,"verdict":"WRONG_ANSWER"},...]`.
fn subtasks_from_list(submission: &serde_json::Value) -> Vec<Subtask> {
    let mut results: Vec<&serde_json::Value> = submission
        .get("subtaskResults")
        .and_then(|v| v.as_array())
        .map(|list| list.iter().collect())
        .unwrap_or_default();
    results.sort_by_key(|r| r.get("orderIndex").and_then(|v| v.as_i64()).unwrap_or(0));
    results
        .into_iter()
        .map(|r| {
            let score = r.get("score").and_then(|v| v.as_f64());
            let max_score = r.get("maxScore").and_then(|v| v.as_f64());
            Subtask {
                verdict: humanize(r.get("verdict").and_then(|v| v.as_str()).unwrap_or("")),
                score: match (score, max_score) {
                    (Some(score), Some(max_score)) => format!("{}/{}", score, max_score),
                    _ => String::new(),
                },
                first_fail: None,
            }
        })
        .collect()
}

/// The lines to print for a rejected submission, each with whether it is
/// green. `header` is the overall verdict, e.g. "Partial (75/475)".
fn breakdown(header: &str, subtasks: &[Subtask]) -> Vec<(String, bool)> {
    let on_test = |s: &Subtask| match s.first_fail {
        Some((index, total)) => format!(" on test {}/{}", index, total),
        None => String::new(),
    };
    // A single "subtask" is really just the flat test list.
    if subtasks.len() <= 1 {
        let suffix = subtasks.first().map(on_test).unwrap_or_default();
        return vec![(format!("{}{}", header, suffix), false)];
    }
    let mut lines = vec![(header.to_string(), false)];
    for (i, subtask) in subtasks.iter().enumerate() {
        let score = if subtask.score.is_empty() {
            String::new()
        } else {
            format!(" ({})", subtask.score)
        };
        let verdict = if subtask.verdict.is_empty() {
            "Not tested"
        } else {
            subtask.verdict.as_str()
        };
        lines.push((
            format!("  Subtask {}{}: {}{}", i + 1, score, verdict, on_test(subtask)),
            subtask.verdict == "Accepted",
        ));
    }
    lines
}

/// Split a component payload into its rows, `<hex id>:<json>\n`. Text rows
/// (`<id>:T<hex byte length>,<text>`) are skipped; they are the only ones
/// that can contain a newline.
fn parse_rows(page: &str) -> std::collections::HashMap<String, serde_json::Value> {
    let bytes = page.as_bytes();
    let mut rows = std::collections::HashMap::new();
    let mut pos = 0;
    while pos < bytes.len() {
        let Some(colon) = bytes[pos..].iter().position(|&b| b == b':').map(|i| pos + i) else {
            break;
        };
        let id = String::from_utf8_lossy(&bytes[pos..colon]).to_string();
        if bytes.get(colon + 1) == Some(&b'T') {
            let length = bytes[colon..]
                .iter()
                .position(|&b| b == b',')
                .map(|i| colon + i)
                .and_then(|comma| {
                    let hex = std::str::from_utf8(&bytes[colon + 2..comma]).ok()?;
                    Some((comma, usize::from_str_radix(hex, 16).ok()?))
                });
            match length {
                Some((comma, length)) => pos = comma + 1 + length,
                None => break,
            }
            continue;
        }
        let end = bytes[colon..]
            .iter()
            .position(|&b| b == b'\n')
            .map(|i| colon + i)
            .unwrap_or(bytes.len());
        if let Ok(value) = serde_json::from_slice(&bytes[colon + 1..end]) {
            rows.insert(id, value);
        }
        pos = end + 1;
    }
    rows
}

/// `["$", tag, key, props]` -> (tag, props).
fn as_element(value: &serde_json::Value) -> Option<(&str, &serde_json::Value)> {
    let items = value.as_array()?;
    if items.len() == 4 && items[0].as_str() == Some("$") && items[3].is_object() {
        Some((items[1].as_str()?, &items[3]))
    } else {
        None
    }
}

fn has_class(props: &serde_json::Value, class: &str) -> bool {
    props
        .get("className")
        .and_then(|v| v.as_str())
        .map(|names| names.split_whitespace().any(|name| name == class))
        .unwrap_or(false)
}

/// Follow `"$<id>"` / `"$L<id>"` references to other rows: big payloads move
/// table rows and whole panels out into rows of their own.
fn follow<'a>(
    value: &'a serde_json::Value,
    rows: &'a std::collections::HashMap<String, serde_json::Value>,
) -> &'a serde_json::Value {
    let mut value = value;
    for _ in 0..20 {
        let target = value
            .as_str()
            .and_then(|s| s.strip_prefix('$'))
            .map(|s| s.strip_prefix('L').unwrap_or(s))
            .and_then(|id| rows.get(id));
        match target {
            Some(target) => value = target,
            None => break,
        }
    }
    value
}

/// Call `visit` on every element under `value`, outermost first; it returns
/// false to skip that element's children.
fn walk<'a>(
    value: &'a serde_json::Value,
    rows: &'a std::collections::HashMap<String, serde_json::Value>,
    depth: usize,
    visit: &mut dyn FnMut(&'a str, &'a serde_json::Value) -> bool,
) {
    if depth > 200 {
        return;
    }
    let value = follow(value, rows);
    if let Some((tag, props)) = as_element(value) {
        if visit(tag, props) {
            walk(props, rows, depth + 1, visit);
        }
    } else if let Some(items) = value.as_array() {
        for item in items {
            walk(item, rows, depth + 1, visit);
        }
    } else if let Some(map) = value.as_object() {
        for item in map.values() {
            walk(item, rows, depth + 1, visit);
        }
    }
}

/// The text an element shows.
fn text_of(
    value: &serde_json::Value,
    rows: &std::collections::HashMap<String, serde_json::Value>,
    depth: usize,
) -> String {
    if depth > 200 {
        return String::new();
    }
    let value = follow(value, rows);
    if let Some((_, props)) = as_element(value) {
        return props
            .get("children")
            .map(|c| text_of(c, rows, depth + 1))
            .unwrap_or_default();
    }
    match value {
        serde_json::Value::String(s) if !s.starts_with('$') => s.clone(),
        serde_json::Value::Number(n) => n.to_string(),
        serde_json::Value::Array(items) => items
            .iter()
            .map(|item| text_of(item, rows, depth + 1))
            .collect(),
        _ => String::new(),
    }
}

/// The site's verdict chips.
fn chip_name(chip: &str) -> String {
    match chip.trim() {
        "AC" => "Accepted".to_string(),
        "WA" => "Wrong Answer".to_string(),
        "TLE" => "Time Limit Exceeded".to_string(),
        "MLE" => "Memory Limit Exceeded".to_string(),
        "OLE" => "Output Limit Exceeded".to_string(),
        "RE" | "RTE" => "Runtime Error".to_string(),
        "CE" => "Compilation Error".to_string(),
        "PE" => "Presentation Error".to_string(),
        "PC" | "PARTIAL" => "Partial".to_string(),
        "IE" => "Internal Error".to_string(),
        // Not run because an earlier test or a required subtask failed.
        "STOP" => "Skipped".to_string(),
        other => other.to_string(),
    }
}

/// Read the per-subtask panels of a submission page: each is a `<details
/// class="doj-subtask-result-panel">` holding a summary with a score label
/// ("0 / 100") and a verdict chip ("WA"), then a table with a row per test
/// whose chip is its verdict; tests that were not run have no chip.
fn parse_subtask_panels(page: &str) -> Vec<Subtask> {
    let rows = parse_rows(page);
    // A panel can sit in a row of its own that the page refers to, so start
    // from every row; `seen` keeps a panel from being counted twice.
    let mut ids: Vec<&String> = rows.keys().collect();
    ids.sort_by_key(|id| (usize::from_str_radix(id, 16).unwrap_or(usize::MAX), id.len()));
    let mut seen = std::collections::HashSet::new();
    let mut panels = Vec::new();
    for id in ids {
        walk(&rows[id], &rows, 0, &mut |tag, props| {
            if tag == "details" && has_class(props, "doj-subtask-result-panel") {
                if seen.insert(props as *const serde_json::Value) {
                    panels.push(props);
                }
                return false;
            }
            true
        });
    }

    let mut subtasks: Vec<(i64, Subtask)> = Vec::new();
    for props in panels {
        let mut subtask = Subtask::default();
        let mut number = None;
        let mut chip = None;
        let mut tests: Vec<Option<String>> = Vec::new();
        walk(props, &rows, 0, &mut |tag, props| {
            if tag == "tr" {
                let mut is_test = false;
                let mut verdict = None;
                walk(props, &rows, 0, &mut |tag, props| {
                    is_test |= tag == "td";
                    if verdict.is_none() && has_class(props, "status-chip") {
                        verdict = props.get("children").map(|c| text_of(c, &rows, 0));
                    }
                    true
                });
                if is_test {
                    tests.push(verdict);
                }
                return false;
            }
            if tag == "h2" && number.is_none() {
                let title = props.get("children").map(|c| text_of(c, &rows, 0));
                number = title.and_then(|t| {
                    t.split_whitespace().last().and_then(|n| n.parse::<i64>().ok())
                });
            } else if chip.is_none() && has_class(props, "status-chip") {
                chip = props.get("children").map(|c| text_of(c, &rows, 0));
            } else if subtask.score.is_empty() && has_class(props, "doj-score-bar-label") {
                subtask.score = props
                    .get("children")
                    .map(|c| text_of(c, &rows, 0))
                    .unwrap_or_default()
                    .split_whitespace()
                    .collect();
            }
            true
        });
        subtask.verdict = chip.map(|c| chip_name(&c)).unwrap_or_default();
        subtask.first_fail = tests
            .iter()
            .position(|t| matches!(t, Some(v) if v.trim() != "AC"))
            .map(|i| (i + 1, tests.len()));
        subtasks.push((number.unwrap_or(subtasks.len() as i64 + 1), subtask));
    }
    subtasks.sort_by_key(|(number, _)| *number);
    subtasks.into_iter().map(|(_, s)| s).collect()
}

#[derive(Debug, PartialEq, Clone, Copy)]
enum State {
    Pending,
    Accepted,
    Rejected,
}

/// Turn one entry of `/api/my-submissions` into the text to show.
fn describe(submission: &serde_json::Value) -> (String, State) {
    let verdict = submission
        .get("verdict")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    if verdict.is_empty() || PENDING_VERDICTS.contains(&verdict) {
        let mut display = if verdict.is_empty() {
            "Pending".to_string()
        } else {
            humanize(verdict)
        };
        if let Some(progress) = submission.get("progress").and_then(|v| v.as_str()) {
            display.push(' ');
            display.push_str(progress);
        }
        return (display, State::Pending);
    }
    if verdict == "ACCEPTED" {
        return ("Accepted".to_string(), State::Accepted);
    }
    // Scoring is by subtask, so a rejected submission can still carry points.
    let mut display = humanize(verdict);
    let score = submission.get("score").and_then(|v| v.as_f64());
    let max_score = submission.get("maxScore").and_then(|v| v.as_f64());
    if let (Some(score), Some(max_score)) = (score, max_score) {
        if max_score > 0.0 {
            display.push_str(&format!(" ({}/{})", score, max_score));
        }
    }
    (display, State::Rejected)
}

/// Parse one JSON value from the start of `text`, ignoring what follows it.
fn json_prefix(text: &str) -> Option<serde_json::Value> {
    serde_json::Deserializer::from_str(text)
        .into_iter::<serde_json::Value>()
        .next()?
        .ok()
}

fn prefix(text: &str, max: usize) -> &str {
    let mut end = text.len().min(max);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[..end]
}

/// Find the submit form in a problem page's component payload.
///
/// The form is a component whose props look like
/// `{"locale":"en","problemId":"...","problemSlug":"5",...,"action":"$ha3",
/// "allowedLanguages":["cpp17",...],...}`, and the action it points to is a
/// separate row `a3:{"id":"<hex>","bound":null}`. Anonymous visitors do not
/// get the form at all.
fn parse_submit_form(page: &str) -> Result<SubmitForm, String> {
    let anchor = page.find("\"allowedLanguages\"").ok_or(
        "No submit form on this page. Check the URL, and that you are logged in (submitter login doj)",
    )?;
    // The props object opens somewhere before the anchor; try each `{` going
    // backwards until one parses into an object that has the anchor's key.
    let mut props = None;
    let mut end = anchor;
    while let Some(start) = page[..end].rfind('{') {
        if anchor - start > 20_000 {
            break;
        }
        if let Some(value) = json_prefix(&page[start..]) {
            if value.get("allowedLanguages").is_some() && value.get("action").is_some() {
                props = Some(value);
                break;
            }
        }
        end = start;
    }
    let props = props.ok_or("Could not read the submit form on this page")?;

    let action_ref = props
        .get("action")
        .and_then(|v| v.as_str())
        .and_then(|v| v.strip_prefix("$h"))
        .ok_or("Submit form has no action reference")?;
    let row = regex::Regex::new(&format!(
        r#"(?:^|[^0-9a-f]){}:\{{"id":"([0-9a-f]+)""#,
        regex::escape(action_ref)
    ))
    .map_err(|e| e.to_string())?;
    let action_id = row
        .captures(page)
        .map(|c| c[1].to_string())
        .ok_or("Could not find the submit action ID on this page")?;

    let mut fields = Vec::new();
    for name in FORM_FIELDS {
        if let Some(value) = props.get(*name).and_then(|v| v.as_str()) {
            // "$undefined" is how the payload spells an unset prop.
            if !value.is_empty() && !value.starts_with('$') {
                fields.push((name.to_string(), value.to_string()));
            }
        }
    }
    if !fields.iter().any(|(name, _)| name == "problemId") {
        return Err("Submit form has no problem ID".to_string());
    }
    let locale = props
        .get("locale")
        .and_then(|v| v.as_str())
        .unwrap_or("en")
        .to_string();
    let languages = props
        .get("allowedLanguages")
        .and_then(|v| v.as_array())
        .map(|list| {
            list.iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default();
    let disabled_reason = if props.get("formalSubmissionDisabled").and_then(|v| v.as_bool())
        == Some(true)
    {
        Some(
            props
                .get("formalSubmissionDisabledReason")
                .and_then(|v| v.as_str())
                .unwrap_or("no reason given")
                .to_string(),
        )
    } else {
        None
    };
    Ok(SubmitForm {
        action_id,
        fields,
        locale,
        languages,
        disabled_reason,
    })
}

/// Pull the submission ID out of the action's response. Among the page rows
/// there is one with the action's return value:
/// `1:{"ok":true,"redirectTo":"/en/submissions?submitted=<id>"}`.
fn parse_submit_response(text: &str) -> Result<String, String> {
    let start = text
        .find(":{\"ok\":")
        .ok_or_else(|| format!("Unexpected submit response: {}", prefix(text, 300)))?;
    let result = json_prefix(&text[start + 1..])
        .ok_or_else(|| format!("Unexpected submit response: {}", prefix(&text[start..], 300)))?;
    if result.get("ok").and_then(|v| v.as_bool()) != Some(true) {
        return Err(format!("Submit rejected: {}", result));
    }
    let redirect = result
        .get("redirectTo")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    regex::Regex::new(r"[?&]submitted=([A-Za-z0-9_-]+)")
        .ok()
        .and_then(|re| re.captures(redirect).map(|c| c[1].to_string()))
        .ok_or_else(|| format!("No submission ID in submit response: {}", result))
}

/// Map a user-supplied language to one of the page's language IDs
/// (`cpp23`, `rust`, `pypy3`, ...). Accepts the ID itself, the usual
/// spellings ("C++23", "C++ 23", "Python 3", "C#"), and bare family names,
/// which pick the newest version the problem allows.
fn find_language(input: &str, allowed: &[String]) -> Option<String> {
    let needle: String = input
        .trim()
        .to_lowercase()
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect();
    if needle.is_empty() {
        return None;
    }
    if let Some(id) = allowed.iter().find(|id| id.to_lowercase() == needle) {
        return Some(id.clone());
    }
    let needle = needle.replace("c++", "cpp").replace("g++", "cpp");
    let candidates: Vec<String> = match needle.as_str() {
        "cpp" => vec!["cpp26", "cpp23", "cpp20", "cpp17"]
            .into_iter()
            .map(String::from)
            .collect(),
        "c" => vec!["c23".to_string(), "c11".to_string()],
        "python" | "python3" | "py" | "py3" | "cpython" => vec!["python3".to_string()],
        "pypy" | "pypy3" => vec!["pypy3".to_string()],
        "java" => vec!["java21".to_string()],
        "kt" => vec!["kotlin".to_string()],
        "ts" => vec!["typescript".to_string()],
        "node" | "node.js" | "js" | "javascript" => vec!["nodejs".to_string()],
        "c#" | "cs" => vec!["csharp".to_string()],
        "rs" => vec!["rust".to_string()],
        "rb" => vec!["ruby".to_string()],
        "plaintext" | "txt" => vec!["text".to_string()],
        // "cpp23" and the like were already tried as exact IDs above.
        other => vec![other.to_string()],
    };
    candidates
        .into_iter()
        .find(|c| allowed.iter().any(|id| id == c))
}

/// "WRONG_ANSWER" -> "Wrong Answer".
fn humanize(s: &str) -> String {
    s.split(|c| c == '_' || c == '-' || c == ' ')
        .filter(|t| !t.is_empty())
        .map(|t| {
            let mut chars = t.chars();
            match chars.next() {
                Some(first) => {
                    first.to_uppercase().collect::<String>() + &chars.as_str().to_lowercase()
                }
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Any page on doj.kr that shows a submit form will do, e.g.
/// `https://doj.kr/en/problems/5`; the form itself says which problem it is.
fn is_doj_url(url: &str) -> bool {
    regex::Regex::new(r"^https://(?:www\.)?doj\.kr/")
        .map(|re| re.is_match(url))
        .unwrap_or(false)
}

pub fn login() {
    let mut client = DojClient::new();
    if let Err(e) = client.login() {
        eprintln!("Login failed: {}", e);
    }
}

pub fn submit(url: String, language: String, source: String) {
    // The fragment never reaches the server.
    let url = url.split('#').next().unwrap_or(&url).to_string();
    if !is_doj_url(&url) {
        eprintln!("Could not parse URL: {}", url);
        return;
    }

    let mut client = DojClient::new();

    println!("Logging in");
    if let Err(e) = client.login() {
        eprintln!("Login failed: {}", e);
        return;
    }

    println!("Submitting");
    let id = match client.submit(&url, &language, &source) {
        Ok((id, _)) => id,
        Err(e) => {
            eprintln!("{}", e);
            return;
        }
    };

    if let Err(e) = client.poll_verdict(&id) {
        eprintln!("Verdict polling failed: {}", e);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Cut down from a real logged-in problem page; the long text row before
    // the action row has no trailing newline, as in the real payload.
    const PAGE: &str = concat!(
        "1:\"$Sreact.fragment\"\n",
        "61:I[339756,[\"$5\"],\"default\"]\n",
        "9f:[\"$\",\"$L61\",\"cmofqz68s0002xdkxd5q2c2l1:u1\",{\"problemId\":\"cmofqz68s0002xdkxd5q2c2l1\",\"locale\":\"en\",\"authenticated\":true}]\n",
        "a1:T5,helloa3:{\"id\":\"40aa90cb51bf741df481818cb913010c05487fb60b\",\"bound\":null}\n",
        "75:[\"$\",\"div\",null,{\"className\":\"stack\",\"children\":[\"$\",\"$La2\",null,{\"locale\":\"en\",",
        "\"problemId\":\"cmofqz68s0002xdkxd5q2c2l1\",\"problemSlug\":\"5\",\"contestId\":\"$undefined\",",
        "\"dlcVariantId\":\"$undefined\",\"action\":\"$ha3\",\"submitLabel\":\"Submit\",\"defaultValue\":\"\",",
        "\"visibleLanguages\":[\"cpp17\",\"cpp23\",\"rust\"],",
        "\"allowedLanguages\":[\"cpp17\",\"cpp20\",\"cpp23\",\"cpp26\",\"c11\",\"c23\",\"python3\",\"pypy3\",\"java21\",\"kotlin\",\"typescript\",\"nodejs\",\"csharp\",\"rust\",\"ruby\",\"golfscript\",\"text\"],",
        "\"codeEditorIndentSize\":4,\"formalSubmissionDisabled\":false,\"formalSubmissionDisabledReason\":null}]}]\n",
    );

    fn allowed() -> Vec<String> {
        parse_submit_form(PAGE).unwrap().languages
    }

    #[test]
    fn parses_submit_form() {
        let form = parse_submit_form(PAGE).unwrap();
        assert_eq!(form.action_id, "40aa90cb51bf741df481818cb913010c05487fb60b");
        assert_eq!(
            form.fields,
            vec![
                ("locale".to_string(), "en".to_string()),
                ("problemId".to_string(), "cmofqz68s0002xdkxd5q2c2l1".to_string()),
                ("problemSlug".to_string(), "5".to_string()),
            ]
        );
        assert_eq!(form.locale, "en");
        assert_eq!(form.languages.len(), 17);
        assert_eq!(form.disabled_reason, None);
    }

    #[test]
    fn contest_fields_are_sent_when_set() {
        let page = PAGE.replace(
            "\"contestId\":\"$undefined\"",
            "\"contestId\":\"cmcontest1\"",
        );
        let form = parse_submit_form(&page).unwrap();
        assert!(form
            .fields
            .contains(&("contestId".to_string(), "cmcontest1".to_string())));
    }

    #[test]
    fn anonymous_page_has_no_form() {
        let page = "9f:[\"$\",\"$L61\",null,{\"problemId\":\"x\",\"authenticated\":false}]\n";
        assert!(parse_submit_form(page).unwrap_err().contains("logged in"));
    }

    #[test]
    fn reports_disabled_submission() {
        let page = PAGE.replace(
            "\"formalSubmissionDisabled\":false,\"formalSubmissionDisabledReason\":null",
            "\"formalSubmissionDisabled\":true,\"formalSubmissionDisabledReason\":\"Contest is over\"",
        );
        assert_eq!(
            parse_submit_form(&page).unwrap().disabled_reason,
            Some("Contest is over".to_string())
        );
    }

    #[test]
    fn parses_submit_response() {
        let text = "2:\"$Sreact.fragment\"\n13:X\n1:{\"ok\":true,\"redirectTo\":\"/en/submissions?submitted=cmwur3it21rdq9le4n84l8nif\"}\n28:I[479520,[\"$8\"],\"\"]\n";
        assert_eq!(
            parse_submit_response(text).unwrap(),
            "cmwur3it21rdq9le4n84l8nif"
        );
    }

    #[test]
    fn rejected_submit_is_an_error() {
        let text = "1:{\"ok\":false,\"error\":\"Too many submissions\"}\n";
        assert!(parse_submit_response(text)
            .unwrap_err()
            .contains("Too many submissions"));
        assert!(parse_submit_response("<!DOCTYPE html>").is_err());
    }

    #[test]
    fn finds_languages() {
        let allowed = allowed();
        let find = |s: &str| find_language(s, &allowed);
        assert_eq!(find("rust"), Some("rust".to_string()));
        assert_eq!(find("Rust"), Some("rust".to_string()));
        assert_eq!(find("C++"), Some("cpp26".to_string()));
        assert_eq!(find("C++23"), Some("cpp23".to_string()));
        assert_eq!(find("c++ 17"), Some("cpp17".to_string()));
        assert_eq!(find("cpp20"), Some("cpp20".to_string()));
        assert_eq!(find("C"), Some("c23".to_string()));
        assert_eq!(find("Python 3"), Some("python3".to_string()));
        assert_eq!(find("pypy"), Some("pypy3".to_string()));
        assert_eq!(find("Java"), Some("java21".to_string()));
        assert_eq!(find("C#"), Some("csharp".to_string()));
        assert_eq!(find("js"), Some("nodejs".to_string()));
        assert_eq!(find("haskell"), None);
        assert_eq!(find("  "), None);
    }

    #[test]
    fn bare_family_respects_allowed_languages() {
        let allowed = vec!["cpp17".to_string(), "cpp20".to_string()];
        assert_eq!(find_language("C++", &allowed), Some("cpp20".to_string()));
        assert_eq!(find_language("C++23", &allowed), None);
    }

    #[test]
    fn describes_submissions() {
        let queued = serde_json::json!({"verdict":"QUEUED","score":0,"maxScore":100});
        assert_eq!(describe(&queued), ("Queued".to_string(), State::Pending));
        let running = serde_json::json!({"verdict":"RUNNING","progress":"10%","score":0,"maxScore":100});
        assert_eq!(describe(&running), ("Running 10%".to_string(), State::Pending));
        let accepted = serde_json::json!({"verdict":"ACCEPTED","score":100,"maxScore":100,"progress":null});
        assert_eq!(describe(&accepted), ("Accepted".to_string(), State::Accepted));
        let partial = serde_json::json!({"verdict":"WRONG_ANSWER","score":19,"maxScore":100});
        assert_eq!(
            describe(&partial),
            ("Wrong Answer (19/100)".to_string(), State::Rejected)
        );
    }

    // The shape of a real submission page: the second panel and one of the
    // test rows live in rows of their own, after a text row with a newline.
    const SUBMISSION_PAGE: &str = concat!(
        "1:\"$Sreact.fragment\"\n",
        "26:T6,fn a\n{}",
        "15:[\"$\",\"section\",null,{\"children\":[[\"$\",\"span\",\"WA-0\",{\"className\":\"status-chip danger\",\"children\":\"WA\"}],\"$L27\"]}]\n",
        "27:[\"$\",\"div\",null,{\"className\":\"stack\",\"children\":[[\"$\",\"details\",\"k1\",{\"className\":\"panel doj-subtask-result-panel\",\"children\":[",
        "[\"$\",\"summary\",null,{\"children\":[[\"$\",\"h2\",null,{\"className\":\"section-title\",\"children\":[\"Subtask\",\" \",1]}],",
        "[\"$\",\"span\",null,{\"className\":\"doj-score-bar-label\",\"children\":[\"0\",\" /\",\" \",\"100\"]}],",
        "[\"$\",\"span\",null,{\"className\":\"status-chip danger\",\"children\":\"WA\"}]]}],",
        "[\"$\",\"table\",null,{\"children\":[[\"$\",\"thead\",null,{\"children\":[\"$\",\"tr\",null,{\"children\":[[\"$\",\"th\",null,{\"children\":\"Test case\"}],[\"$\",\"th\",null,{\"children\":\"Verdict\"}]]}]}],",
        "[\"$\",\"tbody\",null,{\"children\":[[",
        "[\"$\",\"tr\",\"t1\",{\"children\":[[\"$\",\"td\",null,{\"children\":[\"Test case\",\" \",1]}],[\"$\",\"td\",null,{\"children\":[\"$\",\"span\",null,{\"className\":\"status-chip success\",\"children\":\"AC\"}]}]]}],",
        "\"$L2c\",",
        "[\"$\",\"tr\",\"t3\",{\"children\":[[\"$\",\"td\",null,{\"children\":[\"Test case\",\" \",3]}],[\"$\",\"td\",null,{\"children\":\"Skipped\"}]]}]",
        "]]}]]}]]}],\"$L2e\"]}]\n",
        "2c:[\"$\",\"tr\",\"t2\",{\"children\":[[\"$\",\"td\",null,{\"children\":[\"Test case\",\" \",2]}],\"$2d\"]}]\n",
        "2d:[\"$\",\"td\",null,{\"children\":[\"$\",\"span\",null,{\"className\":\"status-chip danger\",\"children\":\"WA\"}]}]\n",
        "2e:[\"$\",\"details\",\"k2\",{\"className\":\"panel doj-subtask-result-panel\",\"children\":[",
        "[\"$\",\"summary\",null,{\"children\":[[\"$\",\"h2\",null,{\"children\":[\"Subtask\",\" \",2]}],",
        "[\"$\",\"span\",null,{\"className\":\"doj-score-bar-label\",\"children\":[\"0\",\" /\",\" \",\"75\"]}],",
        "[\"$\",\"span\",null,{\"className\":\"status-chip warning\",\"children\":\"STOP\"}]]}],",
        "[\"$\",\"tbody\",null,{\"children\":[[\"$\",\"tr\",\"u1\",{\"children\":[[\"$\",\"td\",null,{\"children\":[\"Test case\",\" \",1]}],[\"$\",\"td\",null,{\"children\":\"Skipped\"}]]}]]}]]}]\n",
    );

    #[test]
    fn parses_subtask_panels() {
        assert_eq!(
            parse_subtask_panels(SUBMISSION_PAGE),
            vec![
                Subtask {
                    verdict: "Wrong Answer".to_string(),
                    score: "0/100".to_string(),
                    first_fail: Some((2, 3)),
                },
                Subtask {
                    verdict: "Skipped".to_string(),
                    score: "0/75".to_string(),
                    first_fail: None,
                },
            ]
        );
        assert_eq!(parse_subtask_panels("<!DOCTYPE html>"), vec![]);
    }

    #[test]
    fn reads_subtasks_from_submission_list() {
        let submission = serde_json::json!({"verdict":"PARTIAL","subtaskResults":[
            {"orderIndex":1,"score":75,"maxScore":75,"verdict":"ACCEPTED"},
            {"orderIndex":0,"score":0,"maxScore":100,"verdict":"WRONG_ANSWER"},
        ]});
        let subtasks = subtasks_from_list(&submission);
        assert_eq!(subtasks[0].verdict, "Wrong Answer");
        assert_eq!(subtasks[0].score, "0/100");
        assert_eq!(subtasks[1].verdict, "Accepted");
        assert_eq!(subtasks[1].score, "75/75");
        assert!(subtasks_from_list(&serde_json::json!({"verdict":"WRONG_ANSWER"})).is_empty());
    }

    #[test]
    fn formats_breakdown() {
        let failed = Subtask {
            verdict: "Wrong Answer".to_string(),
            score: "0/100".to_string(),
            first_fail: Some((6, 29)),
        };
        let passed = Subtask {
            verdict: "Accepted".to_string(),
            score: "75/75".to_string(),
            first_fail: None,
        };
        assert_eq!(
            breakdown("Partial (75/175)", &[failed.clone(), passed]),
            vec![
                ("Partial (75/175)".to_string(), false),
                ("  Subtask 1 (0/100): Wrong Answer on test 6/29".to_string(), false),
                ("  Subtask 2 (75/75): Accepted".to_string(), true),
            ]
        );
        assert_eq!(
            breakdown("Wrong Answer (0/100)", &[failed]),
            vec![("Wrong Answer (0/100) on test 6/29".to_string(), false)]
        );
        assert_eq!(
            breakdown("Compilation Error", &[]),
            vec![("Compilation Error".to_string(), false)]
        );
    }

    #[test]
    fn recognises_doj_urls() {
        assert!(is_doj_url("https://doj.kr/en/problems/5"));
        assert!(is_doj_url("https://doj.kr/ko/problems/bfs"));
        assert!(!is_doj_url("https://example.com/doj.kr/"));
    }
}

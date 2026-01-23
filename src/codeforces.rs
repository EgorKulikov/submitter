use crate::clear;
use clipboard::{ClipboardContext, ClipboardProvider};
use crossterm::execute;
use crossterm::style::{Color, ResetColor, SetForegroundColor};
use dialoguer::console::Term;
use dialoguer::Input;
use reqwest::blocking::Client;
use reqwest::header::{ACCEPT, CONTENT_TYPE};
use rs_sha512::{HasherContext, Sha512State};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs::File;
use std::hash::{BuildHasher, Hasher};
use std::io::BufReader;
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

#[derive(Deserialize, Eq, PartialEq, Debug)]
pub enum Verdict {
    #[serde(rename = "FAILED")]
    Failed,
    #[serde(rename = "OK")]
    Accepted,
    #[serde(rename = "PARTIAL")]
    Partial,
    #[serde(rename = "COMPILATION_ERROR")]
    CompilationError,
    #[serde(rename = "RUNTIME_ERROR")]
    RuntimeError,
    #[serde(rename = "WRONG_ANSWER")]
    WrongAnswer,
    #[serde(rename = "TIME_LIMIT_EXCEEDED")]
    TimeLimitExceeded,
    #[serde(rename = "MEMORY_LIMIT_EXCEEDED")]
    MemoryLimitExceeded,
    #[serde(rename = "IDLENESS_LIMIT_EXCEEDED")]
    IdlenessLimitExceeded,
    #[serde(rename = "SECURITY_VIOLATED")]
    SecurityViolated,
    #[serde(rename = "CRASHED")]
    Crashed,
    #[serde(rename = "INPUT_PREPARATION_CRASHED")]
    InputPreparationCrashed,
    #[serde(rename = "CHALLENGED")]
    Challenged,
    #[serde(rename = "SKIPPED")]
    Skipped,
    #[serde(rename = "TESTING")]
    Testing,
    #[serde(rename = "REJECTED")]
    Rejected,
}

#[derive(Deserialize, Eq, PartialEq, Debug)]
pub struct Problem {
    pub name: String,
    #[serde(rename = "contestId")]
    pub contest_id: Option<i64>,
    pub index: String,
}

#[derive(Deserialize, Eq, PartialEq, Debug)]
pub enum TestSet {
    #[serde(rename = "SAMPLES")]
    Samples,
    #[serde(rename = "PRETESTS")]
    Pretests,
    #[serde(rename = "TESTS")]
    Tests,
    #[serde(rename = "CHALLENGES")]
    Challenges,
}

impl TestSet {
    pub(crate) fn test_name(&self) -> &'static str {
        match self {
            TestSet::Samples => "sample",
            TestSet::Pretests => "pretest",
            TestSet::Tests => "test",
            TestSet::Challenges => "challenge",
        }
    }
}

#[derive(Deserialize, PartialEq, Debug)]
pub struct Submission {
    pub id: i64,
    #[serde(rename = "contestId")]
    pub contest_id: Option<i64>,
    pub problem: Problem,
    pub verdict: Option<Verdict>,
    pub testset: TestSet,
    #[serde(rename = "passedTestCount")]
    pub passed_test_count: i32,
    pub points: Option<f64>,
}

impl Submission {
    pub(crate) fn result(&self) -> (Color, String) {
        let color = match self.verdict {
            Some(Verdict::Accepted) => Color::Green,
            Some(Verdict::Partial) => Color::DarkYellow,
            None | Some(Verdict::Testing) => Color::Yellow,
            _ => Color::Red,
        };
        let mut verdict = match self.verdict {
            Some(Verdict::Accepted) => "Accepted".to_string(),
            Some(Verdict::Failed) => "Failed".to_string(),
            Some(Verdict::Partial) => "Partial".to_string(),
            Some(Verdict::CompilationError) => "Compilation Error".to_string(),
            Some(Verdict::RuntimeError) => "Runtime Error".to_string(),
            Some(Verdict::WrongAnswer) => "Wrong Answer".to_string(),
            Some(Verdict::TimeLimitExceeded) => "Time Limit Exceeded".to_string(),
            Some(Verdict::MemoryLimitExceeded) => "Memory Limit Exceeded".to_string(),
            Some(Verdict::IdlenessLimitExceeded) => "Idleness Limit Exceeded".to_string(),
            Some(Verdict::SecurityViolated) => "Security Violated".to_string(),
            Some(Verdict::Crashed) => "Crashed".to_string(),
            Some(Verdict::InputPreparationCrashed) => "Input Preparation Crashed".to_string(),
            Some(Verdict::Challenged) => "Challenged".to_string(),
            Some(Verdict::Skipped) => "Skipped".to_string(),
            Some(Verdict::Testing) => "Testing".to_string(),
            Some(Verdict::Rejected) => "Rejected".to_string(),
            None => "Queued".to_string(),
        };
        match self.verdict {
            Some(Verdict::Accepted)
            | None
            | Some(Verdict::Partial)
            | Some(Verdict::CompilationError) => {}
            _ => {
                verdict.push_str(&format!(
                    " on {} {}",
                    self.testset.test_name(),
                    self.passed_test_count + 1
                ));
            }
        }
        if let Some(pts) = self.points {
            verdict.push_str(&format!(" ({:.4} pts)", pts));
        }
        (color, verdict)
    }

    pub fn print_header(&self) {
        let url = if let Some(contest_id) = self.contest_id {
            format!(
                "https://codeforces.com/contest/{}/submission/{}",
                contest_id, self.id
            )
        } else {
            format!("https://codeforces.com/problemset/submission/{}", self.id)
        };
        println!("Submission url: {}", url);
    }

    pub fn is_final_result(&self) -> bool {
        match self.verdict {
            None | Some(Verdict::Testing) => false,
            _ => true,
        }
    }
}

#[derive(Serialize, Deserialize)]
struct Data {
    user: String,
    api_key: String,
    api_secret: String,
}

fn read_data() -> Option<Data> {
    let file = File::open(".cf_api.json").ok()?;
    let reader = BufReader::new(file);
    serde_json::from_reader(reader).ok()
}

fn get_data() -> Data {
    let user: String = Input::with_theme(&dialoguer::theme::ColorfulTheme::default())
        .with_prompt("Enter your codeforces login")
        .interact_on(&Term::stdout())
        .unwrap();
    let api_key: String = Input::with_theme(&dialoguer::theme::ColorfulTheme::default())
        .with_prompt("Enter your codeforces api key")
        .interact_on(&Term::stdout())
        .unwrap();
    let api_secret: String = Input::with_theme(&dialoguer::theme::ColorfulTheme::default())
        .with_prompt("Enter your codeforces api secret")
        .interact_on(&Term::stdout())
        .unwrap();
    let data = Data {
        user,
        api_key,
        api_secret,
    };
    let file = File::create(".cf_api.json").unwrap();
    serde_json::to_writer(file, &data).unwrap();
    data
}

fn url(method: String, mut params: Vec<(String, String)>, data: &Data) -> String {
    params.push(("apiKey".to_string(), data.api_key.clone()));
    params.push((
        "time".to_string(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs()
            .to_string(),
    ));
    params.sort();
    let rand = format!("{:06}", rand::random::<u64>() % 1_000_000);
    let mut sig = format!("{}/{}?", rand, method);
    let mut first = true;
    for (key, value) in &params {
        if first {
            first = false;
        } else {
            sig.push('&');
        }
        sig.push_str(&format!("{}={}", key, value));
    }
    sig.push('#');
    sig.push_str(data.api_secret.as_str());
    let mut sha512hasher = Sha512State::default().build_hasher();
    sha512hasher.write(sig.as_bytes());
    let res = HasherContext::finish(&mut sha512hasher);
    let api_sig = format!("{}{:02x}", rand, res);
    params.push(("apiSig".to_string(), api_sig));
    let mut url = format!("https://codeforces.com/api/{}?", method);
    let mut first = true;
    for (key, value) in &params {
        if first {
            first = false;
        } else {
            url.push('&');
        }
        url.push_str(&format!("{}={}", key, value));
    }
    url
}

pub fn submit(task_url: String, source: String) {
    let data = if let Some(data) = read_data() {
        data
    } else {
        get_data()
    };
    let mut ctx: ClipboardContext = ClipboardProvider::new().unwrap();
    ctx.set_contents(source).unwrap();
    let pos = match task_url.rfind("/problem/") {
        None => {
            eprintln!("Bad url");
            return;
        }
        Some(pos) => pos,
    };
    let submit_url = if task_url.contains("problemset") {
        let slash = task_url[pos + 9..].find('/').unwrap();
        let end = task_url[pos + 10 + slash..]
            .find(|c: char| c == '/' || c == '?' || c == '#')
            .unwrap_or(task_url.len() - (pos + 10 + slash));
        let contest_id = task_url[pos + 9..pos + 9 + slash].parse::<i64>().unwrap();
        let problem_id = &task_url[pos + 10 + slash..pos + 10 + slash + end];
        format!(
            "https://codeforces.com/problemset/submit/{}/{}",
            contest_id, problem_id
        )
    } else {
        let end = task_url[pos + 9..]
            .find(|c: char| c == '/' || c == '?' || c == '#')
            .unwrap_or(task_url.len() - (pos + 9));
        let problem_id = &task_url[pos + 9..pos + 9 + end];
        format!(
            "{}/submit/{}",
            task_url[..pos].replace("https://codeforces.com", "https://codeforces.com"),
            problem_id
        )
    };
    open::that(&submit_url).ok();
    let mut submission_map = HashMap::new();
    let mut last_id = None;
    let mut last_len = 0;
    let mut first = true;
    let mut stdout = std::io::stdout();
    let mut tries = 0;
    'outer: loop {
        let url = url(
            "user.status".to_string(),
            vec![
                ("handle".to_string(), data.user.clone()),
                ("from".to_string(), "1".to_string()),
                ("count".to_string(), "10".to_string()),
            ],
            &data,
        );
        let bytes = Client::builder()
            .timeout(None)
            .build()
            .unwrap()
            .get(url)
            .header(ACCEPT, "application/json")
            .header(CONTENT_TYPE, "application/json")
            .send()
            .unwrap()
            .text()
            .unwrap();
        #[derive(Deserialize, Debug)]
        struct RequestResult {
            result: Vec<Submission>,
        }
        let request_result: RequestResult = match serde_json::from_reader(bytes.as_bytes()) {
            Ok(res) => {
                tries = 0;
                res
            }
            Err(_) => {
                tries += 1;
                if tries >= 10 {
                    eprintln!("Failed to get submission status from codeforces");
                    return;
                } else {
                    thread::sleep(Duration::from_secs(2));
                    continue;
                }
            }
        };
        let mut submissions = request_result.result;
        for submission in submissions.into_iter() {
            let updated = if let Some(old_submission) = submission_map.get(&submission.id) {
                old_submission != &submission
            } else {
                true
            };
            if !first && !updated {
                continue;
            }
            if updated {
                if !first {
                    let is_last_shown = last_id.map(|id| id == submission.id).unwrap_or(false);
                    if is_last_shown {
                        clear(last_len);
                    } else {
                        if last_id.is_some() {
                            println!();
                        }
                        submission.print_header();
                    }
                    let (color, outcome) = submission.result();
                    let _ = execute!(stdout, SetForegroundColor(color));
                    print!("{}", outcome);
                    let _ = execute!(stdout, ResetColor);
                    if submission.is_final_result() {
                        println!();
                        last_id = None;
                        last_len = 0;
                        break 'outer;
                    } else {
                        last_id = Some(submission.id);
                        last_len = outcome.len();
                    }
                }
                submission_map.insert(submission.id.clone(), submission);
            }
        }
        first = false;
        thread::sleep(Duration::from_secs(2));
    }
}

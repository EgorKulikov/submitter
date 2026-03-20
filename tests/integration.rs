/// Integration tests for online judge submitters.
///
/// These tests actually submit solutions to real judges and verify verdicts.
/// Credentials are read from environment variables (set via GitHub Actions secrets).
///
/// Run a specific test:
///   UOJ_USER=x UOJ_PASS=y cargo test --test integration test_uoj -- --nocapture
///
/// Run all tests (needs all env vars set):
///   cargo test --test integration -- --nocapture
use submitter::codechef::CodechefClient;
use submitter::eolymp::EolympClient;
use submitter::kattis::KattisClient;
use submitter::uoj::UojClient;
use submitter::yandex::YandexClient;

fn env_or_skip(name: &str) -> String {
    match std::env::var(name) {
        Ok(val) if !val.is_empty() => val,
        _ => {
            eprintln!("Skipping: {} not set", name);
            std::process::exit(0);
        }
    }
}

// ── UOJ ──────────────────────────────────────────────────────────────────

#[test]
fn test_uoj() {
    let user = env_or_skip("UOJ_USER");
    let pass = env_or_skip("UOJ_PASS");
    let mut client = UojClient::new("https://uoj.ac", "UOJ");
    client.login_with_credentials(&user, &pass).unwrap();
    let verdict = client
        .submit(
            "/problem/1",
            "C++14",
            r#"#include <iostream>
using namespace std;
int main() { int a, b; cin >> a >> b; cout << a + b << endl; }"#,
        )
        .unwrap();
    assert!(
        verdict.starts_with("100") || verdict.starts_with("AC"),
        "Expected AC, got: {}",
        verdict
    );
}

// ── CodeChef ─────────────────────────────────────────────────────────────

#[test]
fn test_codechef() {
    let user = env_or_skip("CODECHEF_USER");
    let pass = env_or_skip("CODECHEF_PASS");
    let mut client = CodechefClient::new();
    client.login_with_credentials(&user, &pass).unwrap();
    let (lang_id, _) = client.find_language_id("Rust").unwrap();
    let solution_id = client
        .submit_solution(
            "TEST",
            "PRACTICE",
            &lang_id,
            r#"use std::io::{self, BufRead, Write, BufWriter};
fn main() {
    let stdout = io::stdout();
    let mut out = BufWriter::new(stdout.lock());
    for line in io::stdin().lock().lines() {
        let n: i32 = line.unwrap().trim().parse().unwrap();
        if n == 42 { break; }
        writeln!(out, "{}", n).unwrap();
    }
}"#,
        )
        .unwrap();
    let verdict = client.poll_verdict(&solution_id).unwrap();
    assert_eq!(verdict, "accepted", "Expected AC, got: {}", verdict);
}

// ── Toph ─────────────────────────────────────────────────────────────────

// ── Yandex ───────────────────────────────────────────────────────────────

#[test]
fn test_yandex() {
    let refresh_token = env_or_skip("YANDEX_REFRESH_TOKEN");
    let mut client = YandexClient::new();
    client.set_refresh_token(&refresh_token);
    assert!(client.try_refresh_token(), "Failed to refresh Yandex token");
    let (compiler_id, _) = client.find_compiler("C++").unwrap();
    let run_id = client
        .submit(
            "3",
            "B",
            &compiler_id,
            r#"#include <fstream>
using namespace std;
int main() {
    ifstream in("input.txt");
    ofstream out("output.txt");
    long long a, b;
    in >> a >> b;
    out << a + b << endl;
}"#,
        )
        .unwrap();
    let verdict = client.poll_verdict("3", run_id).unwrap();
    assert!(
        verdict == "OK" || verdict == "Accepted",
        "Expected AC, got: {}",
        verdict
    );
}

#[test]
fn test_yandex_rust() {
    let refresh_token = env_or_skip("YANDEX_REFRESH_TOKEN");
    let mut client = YandexClient::new();
    client.set_refresh_token(&refresh_token);
    assert!(client.try_refresh_token(), "Failed to refresh Yandex token");
    let (compiler_id, _) = client.find_compiler("Rust").unwrap();
    let run_id = client
        .submit(
            "3",
            "B",
            &compiler_id,
            r#"use std::io::{Read, Write};
use std::fs;
fn main() {
    let mut input = String::new();
    fs::File::open("input.txt").unwrap().read_to_string(&mut input).unwrap();
    let v: Vec<i64> = input.trim().split_whitespace().map(|x| x.parse().unwrap()).collect();
    let mut out = fs::File::create("output.txt").unwrap();
    writeln!(out, "{}", v[0] + v[1]).unwrap();
}"#,
        )
        .unwrap();
    let verdict = client.poll_verdict("3", run_id).unwrap();
    assert!(
        verdict == "OK" || verdict == "Accepted",
        "Expected AC, got: {}",
        verdict
    );
}

// ── Kattis ───────────────────────────────────────────────────────────────

#[test]
fn test_kattis() {
    let user = env_or_skip("KATTIS_USER");
    let token = env_or_skip("KATTIS_TOKEN");
    let mut client = KattisClient::new("open.kattis.com");
    client.login_with_credentials(&user, &token).unwrap();
    let submission_id = client
        .submit_with_filename(
            "hello",
            "C++",
            r#"#include <iostream>
using namespace std;
int main() { cout << "Hello World!" << endl; }"#,
            "hello.cpp",
        )
        .unwrap();
    let verdict = client.poll_verdict(&submission_id).unwrap();
    assert_eq!(verdict, "Accepted", "Expected AC, got: {}", verdict);
}

#[test]
fn test_kattis_rust() {
    let user = env_or_skip("KATTIS_USER");
    let token = env_or_skip("KATTIS_TOKEN");
    let mut client = KattisClient::new("open.kattis.com");
    client.login_with_credentials(&user, &token).unwrap();
    let submission_id = client
        .submit_with_filename(
            "hello",
            "Rust",
            r#"fn main() { println!("Hello World!"); }"#,
            "hello.rs",
        )
        .unwrap();
    let verdict = client.poll_verdict(&submission_id).unwrap();
    assert_eq!(verdict, "Accepted", "Expected AC, got: {}", verdict);
}

// ── Eolymp ───────────────────────────────────────────────────────────────

#[test]
fn test_eolymp() {
    let api_key = env_or_skip("EOLYMP_API_KEY");
    let mut client = EolympClient::new();
    client.login_with_key(&api_key).unwrap();

    let space_url = client
        .http_mut()
        .get_json("/spaces/__lookup/basecamp")
        .unwrap()
        .get("space")
        .and_then(|s| s.get("url"))
        .and_then(|v| v.as_str())
        .unwrap()
        .to_string();

    // Problem 1 is "Simple problem" — split a 2-digit number into digits
    let submission_id = client
        .submit_archive(
            &space_url,
            "1",
            "cpp:23-gnu14",
            r#"#include <iostream>
using namespace std;
int main() { int n; cin >> n; cout << n/10 << " " << n%10 << endl; }"#,
        )
        .unwrap();
    let verdict = client.poll_verdict(&space_url, &submission_id).unwrap();
    assert_eq!(verdict, "ACCEPTED", "Expected AC, got: {}", verdict);
}

#[test]
fn test_eolymp_rust() {
    let api_key = env_or_skip("EOLYMP_API_KEY");
    let mut client = EolympClient::new();
    client.login_with_key(&api_key).unwrap();

    let space_url = client
        .http_mut()
        .get_json("/spaces/__lookup/basecamp")
        .unwrap()
        .get("space")
        .and_then(|s| s.get("url"))
        .and_then(|v| v.as_str())
        .unwrap()
        .to_string();

    let submission_id = client
        .submit_archive(
            &space_url,
            "1",
            "rust:1.78",
            r#"use std::io;
fn main() {
    let mut s = String::new();
    io::stdin().read_line(&mut s).unwrap();
    let n: i32 = s.trim().parse().unwrap();
    println!("{} {}", n / 10, n % 10);
}"#,
        )
        .unwrap();
    let verdict = client.poll_verdict(&space_url, &submission_id).unwrap();
    assert_eq!(verdict, "ACCEPTED", "Expected AC, got: {}", verdict);
}

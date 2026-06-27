use std::time::{Duration, Instant};

#[derive(Debug, Clone)]
pub struct Job {
    pub site: &'static str,
    pub url: String,
    pub language: String,
    pub source: String,
}

pub struct Handoff {
    pub port: u16,
    pub token: String,
}

fn gen_token() -> String {
    let bytes: [u8; 16] = rand::random();
    let mut s = String::with_capacity(32);
    for b in bytes {
        s.push(hex_nibble(b >> 4));
        s.push(hex_nibble(b & 0x0f));
    }
    s
}

fn hex_nibble(n: u8) -> char {
    match n {
        0..=9 => (b'0' + n) as char,
        10..=15 => (b'a' + n - 10) as char,
        _ => unreachable!(),
    }
}

use std::collections::HashMap;
use std::sync::Mutex;

struct Entry {
    job: Job,
    expires_at: Instant,
}

pub(crate) struct Store {
    entries: Mutex<HashMap<String, Entry>>,
}

impl Store {
    pub(crate) fn new() -> Self {
        Store { entries: Mutex::new(HashMap::new()) }
    }

    pub(crate) fn insert(&self, job: Job, ttl: Duration) -> String {
        let token = gen_token();
        let entry = Entry { job, expires_at: Instant::now() + ttl };
        self.entries.lock().unwrap().insert(token.clone(), entry);
        token
    }

    pub(crate) fn take(&self, token: &str) -> Option<Job> {
        let mut guard = self.entries.lock().unwrap();
        let entry = guard.remove(token)?;
        if Instant::now() > entry.expires_at {
            return None;
        }
        Some(entry.job)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn gen_token_is_32_lowercase_hex_chars() {
        let t = gen_token();
        assert_eq!(t.len(), 32);
        assert!(t.chars().all(|c| c.is_ascii_hexdigit() && (c.is_ascii_digit() || c.is_ascii_lowercase())));
    }

    #[test]
    fn gen_token_is_unique_across_many_calls() {
        let mut seen = HashSet::new();
        for _ in 0..1000 {
            assert!(seen.insert(gen_token()));
        }
    }

    fn sample_job() -> Job {
        Job { site: "atcoder", url: "https://atcoder.jp/x".into(), language: "C++".into(), source: "int main(){}".into() }
    }

    #[test]
    fn store_take_returns_job_once_then_none() {
        let store = Store::new();
        let token = store.insert(sample_job(), Duration::from_secs(30));
        assert!(store.take(&token).is_some());
        assert!(store.take(&token).is_none());
    }

    #[test]
    fn store_take_returns_none_after_ttl_expires() {
        let store = Store::new();
        let token = store.insert(sample_job(), Duration::from_millis(50));
        std::thread::sleep(Duration::from_millis(120));
        assert!(store.take(&token).is_none());
    }

    #[test]
    fn store_take_with_unknown_token_returns_none() {
        let store = Store::new();
        assert!(store.take("nonexistent").is_none());
    }
}

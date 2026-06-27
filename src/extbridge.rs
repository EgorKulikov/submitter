use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

const SERVER_LIFETIME_SECS: u64 = 600; // 10 minutes

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
    close_flag: Arc<AtomicBool>,
}

impl Handoff {
    pub fn signal_close(&self) {
        self.close_flag.store(true, Ordering::SeqCst);
    }
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

pub fn publish(job: Job, ttl: Duration) -> Result<Handoff, String> {
    let listener = TcpListener::bind("127.0.0.1:0")
        .map_err(|e| format!("bind 127.0.0.1:0 failed: {}", e))?;
    let port = listener.local_addr()
        .map_err(|e| format!("local_addr failed: {}", e))?.port();

    let store = Arc::new(Store::new());
    let token = store.insert(job, ttl);

    let store_t = Arc::clone(&store);
    let close_flag = Arc::new(AtomicBool::new(false));
    let close_flag_t = Arc::clone(&close_flag);
    let deadline = Instant::now() + Duration::from_secs(SERVER_LIFETIME_SECS);

    thread::spawn(move || {
        listener.set_nonblocking(true).ok();
        loop {
            if Instant::now() > deadline {
                return; // Overall cap: 10 minutes.
            }
            match listener.accept() {
                Ok((stream, _)) => {
                    handle(stream, &store_t, &*close_flag_t);
                    // Keep the server running — /close/<token> may follow.
                }
                Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(50));
                }
                Err(_) => return,
            }
        }
    });

    Ok(Handoff { port, token, close_flag })
}

fn handle(mut stream: TcpStream, store: &Store, close_flag: &AtomicBool) -> bool {
    stream.set_read_timeout(Some(Duration::from_secs(2))).ok();
    let reader_stream = match stream.try_clone() {
        Ok(s) => s,
        Err(_) => { write_404(&mut stream); return false; }
    };
    let mut reader = BufReader::new(reader_stream);

    let mut request_line = String::new();
    if reader.read_line(&mut request_line).is_err() {
        write_404(&mut stream);
        return false;
    }
    // Drain headers (we don't need them, but the client expects us to read them).
    let mut header = String::new();
    loop {
        header.clear();
        if reader.read_line(&mut header).is_err() { break; }
        if header == "\r\n" || header == "\n" || header.is_empty() { break; }
    }

    let parts: Vec<&str> = request_line.split_whitespace().collect();
    if parts.len() < 2 {
        write_404(&mut stream);
        return false;
    }
    let method = parts[0];
    let path = parts[1];

    match method {
        "OPTIONS" => {
            // Chrome's Private Network Access preflight: respond with PNA + CORS
            // headers but do NOT consume the token — the actual GET follows.
            if path.strip_prefix("/job/").is_some() || path.strip_prefix("/close/").is_some() {
                let _ = stream.write_all(
                    b"HTTP/1.1 204 No Content\r\n\
                      Access-Control-Allow-Origin: *\r\n\
                      Access-Control-Allow-Methods: GET\r\n\
                      Access-Control-Allow-Headers: *\r\n\
                      Access-Control-Allow-Private-Network: true\r\n\
                      Access-Control-Max-Age: 60\r\n\
                      Connection: close\r\n\r\n",
                );
                false // keep the server thread alive for the real GET
            } else {
                write_404(&mut stream);
                false
            }
        }
        "GET" => {
            if let Some(token) = path.strip_prefix("/job/") {
                match store.take(token) {
                    Some(job) => {
                        let body = serde_json::json!({
                            "site": job.site,
                            "url": job.url,
                            "language": job.language,
                            "source": job.source,
                        }).to_string();
                        let response = format!(
                            "HTTP/1.1 200 OK\r\n\
                             Content-Type: application/json\r\n\
                             Content-Length: {}\r\n\
                             Access-Control-Allow-Origin: *\r\n\
                             Access-Control-Allow-Private-Network: true\r\n\
                             Connection: close\r\n\r\n{}",
                            body.len(), body
                        );
                        let _ = stream.write_all(response.as_bytes());
                        false // Keep running for the /close endpoint.
                    }
                    None => { write_404(&mut stream); false }
                }
            } else if path.strip_prefix("/close/").is_some() {
                if close_flag.load(Ordering::SeqCst) {
                    let _ = stream.write_all(
                        b"HTTP/1.1 204 No Content\r\n\
                          Access-Control-Allow-Origin: *\r\n\
                          Access-Control-Allow-Private-Network: true\r\n\
                          Connection: close\r\n\r\n",
                    );
                } else {
                    write_404(&mut stream);
                }
                false // server keeps running
            } else {
                write_404(&mut stream);
                false
            }
        }
        _ => { write_404(&mut stream); false }
    }
}

fn write_404(stream: &mut TcpStream) {
    let _ = stream.write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
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

    #[test]
    fn publish_serves_job_once_then_404s() {
        let job = Job {
            site: "atcoder",
            url: "https://atcoder.jp/test".into(),
            language: "C++".into(),
            source: "int main(){}".into(),
        };
        let handoff = publish(job.clone(), Duration::from_secs(5)).unwrap();
        let url = format!("http://127.0.0.1:{}/job/{}", handoff.port, handoff.token);

        let r1 = reqwest::blocking::get(&url).unwrap();
        assert_eq!(r1.status(), 200);
        let v: serde_json::Value = r1.json().unwrap();
        assert_eq!(v["site"], "atcoder");
        assert_eq!(v["url"], "https://atcoder.jp/test");
        assert_eq!(v["language"], "C++");
        assert_eq!(v["source"], "int main(){}");

        // Second request: 404 (token consumed; server stays alive for /close endpoint).
        let r2 = reqwest::blocking::get(&url);
        match r2 {
            Ok(resp) => assert_eq!(resp.status(), 404),
            Err(_) => {} // Connection refused is also acceptable.
        }
    }

    #[test]
    fn publish_returns_404_for_wrong_token() {
        let job = Job {
            site: "atcoder", url: "x".into(), language: "y".into(), source: "z".into(),
        };
        let handoff = publish(job, Duration::from_secs(5)).unwrap();
        let url = format!("http://127.0.0.1:{}/job/deadbeef", handoff.port);
        let r = reqwest::blocking::get(&url).unwrap();
        assert_eq!(r.status(), 404);
        // After the 404, the server thread must still be alive and serve the correct token.
        let correct_url = format!("http://127.0.0.1:{}/job/{}", handoff.port, handoff.token);
        let r2 = reqwest::blocking::get(&correct_url).unwrap();
        assert_eq!(r2.status(), 200);
        let v: serde_json::Value = r2.json().unwrap();
        assert_eq!(v["site"], "atcoder");
    }

    #[test]
    fn options_preflight_returns_pna_headers_and_preserves_token() {
        let job = Job {
            site: "atcoder",
            url: "https://atcoder.jp/x".into(),
            language: "C++".into(),
            source: "int main(){}".into(),
        };
        let handoff = publish(job, Duration::from_secs(5)).unwrap();
        let url = format!("http://127.0.0.1:{}/job/{}", handoff.port, handoff.token);

        let client = reqwest::blocking::Client::new();
        let r1 = client.request(reqwest::Method::OPTIONS, &url).send().unwrap();
        assert_eq!(r1.status(), 204);
        assert_eq!(
            r1.headers().get("access-control-allow-private-network").and_then(|v| v.to_str().ok()),
            Some("true")
        );
        assert_eq!(
            r1.headers().get("access-control-allow-origin").and_then(|v| v.to_str().ok()),
            Some("*")
        );

        // Token must still be valid for the actual GET after a preflight.
        let r2 = reqwest::blocking::get(&url).unwrap();
        assert_eq!(r2.status(), 200);
    }

    #[test]
    fn get_response_carries_pna_headers() {
        let job = Job {
            site: "luogu",
            url: "https://luogu/x".into(),
            language: "Rust".into(),
            source: "fn main(){}".into(),
        };
        let handoff = publish(job, Duration::from_secs(5)).unwrap();
        let url = format!("http://127.0.0.1:{}/job/{}", handoff.port, handoff.token);
        let r = reqwest::blocking::get(&url).unwrap();
        assert_eq!(r.status(), 200);
        assert_eq!(
            r.headers().get("access-control-allow-private-network").and_then(|v| v.to_str().ok()),
            Some("true")
        );
        assert_eq!(
            r.headers().get("access-control-allow-origin").and_then(|v| v.to_str().ok()),
            Some("*")
        );
    }

    #[test]
    fn close_endpoint_returns_404_until_signaled_then_204() {
        let job = Job {
            site: "atcoder", url: "x".into(), language: "y".into(), source: "z".into(),
        };
        let handoff = publish(job, Duration::from_secs(30)).unwrap();
        let close_url = format!("http://127.0.0.1:{}/close/{}", handoff.port, handoff.token);

        // Before signaling, /close returns 404
        let r1 = reqwest::blocking::get(&close_url).unwrap();
        assert_eq!(r1.status(), 404);

        handoff.signal_close();

        // After signaling, /close returns 204
        let r2 = reqwest::blocking::get(&close_url).unwrap();
        assert_eq!(r2.status(), 204);
    }

    #[test]
    fn options_preflight_accepts_close_path() {
        let job = Job {
            site: "atcoder", url: "x".into(), language: "y".into(), source: "z".into(),
        };
        let handoff = publish(job, Duration::from_secs(30)).unwrap();
        let close_url = format!("http://127.0.0.1:{}/close/{}", handoff.port, handoff.token);

        let client = reqwest::blocking::Client::new();
        let r = client.request(reqwest::Method::OPTIONS, &close_url).send().unwrap();
        assert_eq!(r.status(), 204);
        assert_eq!(
            r.headers().get("access-control-allow-private-network").and_then(|v| v.to_str().ok()),
            Some("true")
        );
    }
}

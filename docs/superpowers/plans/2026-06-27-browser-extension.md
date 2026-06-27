# Browser Extension Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Ship a Chrome+Firefox Manifest V3 extension that automates paste-language-click on AtCoder, Codeforces, and Luogu submit pages when launched by `submitter`, plus the loopback HTTP bridge in `submitter` itself that hands the job off.

**Architecture:** `submitter` exposes a tiny in-process HTTP server bound to `127.0.0.1` (OS-assigned port) that serves a single `GET /job/<token>` endpoint with a 128-bit, single-use, 30-second-TTL token. When opening the submit URL, submitter appends `#submitter=<port>:<token>`. A content script reads that fragment, scrubs it via `history.replaceState`, fetches the job from localhost, and dispatches to a per-site filler. The extension is purely additive — if it's absent, submitter's existing clipboard + open-browser flow runs unchanged.

**Tech Stack:** Rust (existing crate, edition 2021), `serde_json` and `rand` (already in `Cargo.toml`), `std::net::TcpListener` (no new server framework, no tokio). Browser side: vanilla JavaScript, Manifest V3, no bundler, no framework.

## Global Constraints

- **Manifest V3 only** — no MV2 fallback, no background service worker.
- **Firefox 109+** — required for `browser_specific_settings.gecko.id` with MV3.
- **Submitter must never depend on the extension** — if `extbridge::publish` fails or returns Err, every existing code path keeps working.
- **Token:** 128 random bits, lowercase hex-encoded (32 chars), single-use, 30-second TTL.
- **Bind:** `127.0.0.1` only — never `0.0.0.0`. The server listens on an OS-assigned ephemeral port.
- **No CORS headers** on the loopback response — exemption comes from the extension's `host_permissions: ["http://127.0.0.1/*"]`.
- **No new heavy Rust deps** — no `tokio`, no `axum`, no `hyper`, no `hex` crate (hand-roll the 6-line encoder).
- **Token transport:** URL fragment only (never query string). Scrub via `history.replaceState` before any other script runs in the page.
- **Spec source of truth:** `docs/superpowers/specs/2026-06-27-browser-extension-design.md`. Tasks below cite section numbers from it.

---

## File Structure

**Rust side (submitter crate):**

- Create: `src/extbridge.rs` — token store, HTTP server thread, single-use semantics.
- Modify: `src/lib.rs` — register the new module.
- Modify: `src/atcoder.rs`, `src/codeforces.rs`, `src/luogu.rs` — wrap the existing `open::that(&submit_url)` call with the extbridge publish + fragment-append.
- Modify: `CHANGELOG.md` — Unreleased entry for the bridge + extension.

**Extension (new top-level directory):**

- Create: `extension/manifest.json`
- Create: `extension/shared/notify.js` — in-page banner.
- Create: `extension/shared/languages.js` — per-site language matchers (data + lookup function).
- Create: `extension/content/atcoder.js` — `window.__submitterFill` for ACE editor.
- Create: `extension/content/codeforces.js` — `window.__submitterFill` for plain textarea.
- Create: `extension/content/luogu.js` — `window.__submitterFill` for Vue hidden textarea.
- Create: `extension/content/activator.js` — fragment parse → fetch → dispatch.
- Create: `extension/icons/icon-16.png`, `icon-32.png`, `icon-48.png`, `icon-128.png` — solid-color placeholders are fine for Stage 1.
- Create: `extension/README.md` — user-facing install instructions (linked from project README).

**Packaging / docs:**

- Modify: `.github/workflows/release.yml` — zip the `extension/` directory into a release asset.
- Modify: `README.md` — new "Browser extension (optional)" section.

---

## Task 1: extbridge token store (pure, no I/O)

**Files:**
- Create: `src/extbridge.rs`
- Modify: `src/lib.rs` (one line: `pub mod extbridge;`)
- Test: inline `#[cfg(test)] mod tests` in `src/extbridge.rs`

**Interfaces:**
- Consumes: nothing from prior tasks.
- Produces:
  - `pub struct Job { pub site: &'static str, pub url: String, pub language: String, pub source: String }`
  - `pub struct Handoff { pub port: u16, pub token: String }`
  - Internal `struct Store` with methods `new()`, `insert(job, ttl) -> token`, `take(token) -> Option<Job>` — `take` returns `Some(job)` once then `None` forever after (single-use), and also returns `None` if TTL has passed.
  - Internal helper `gen_token() -> String` returning 32 lowercase hex chars from 128 random bits.

- [ ] **Step 1: Add the module declaration**

Edit `src/lib.rs` to add the line (place it alongside the other `pub mod ...;` declarations — file ordering is alphabetical in this crate):

```rust
pub mod extbridge;
```

- [ ] **Step 2: Write the failing tests for `gen_token`**

Create `src/extbridge.rs` with this initial content:

```rust
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
}
```

- [ ] **Step 3: Run the tests to verify they fail**

Run: `cargo test --lib extbridge::tests::gen_token 2>&1 | tail -20`
Expected: compile errors — `gen_token` not defined.

- [ ] **Step 4: Implement `gen_token`**

Append to `src/extbridge.rs` (above the `#[cfg(test)]` block):

```rust
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
```

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cargo test --lib extbridge::tests::gen_token 2>&1 | tail -10`
Expected: `test result: ok. 2 passed`.

- [ ] **Step 6: Add the failing tests for the Store**

Append inside the `tests` module (above the closing `}`):

```rust
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
```

- [ ] **Step 7: Run the new tests to verify they fail**

Run: `cargo test --lib extbridge::tests::store 2>&1 | tail -20`
Expected: compile errors — `Store` not defined.

- [ ] **Step 8: Implement `Store`**

Insert above the `#[cfg(test)]` block:

```rust
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
```

- [ ] **Step 9: Run all extbridge tests to verify they pass**

Run: `cargo test --lib extbridge 2>&1 | tail -10`
Expected: `test result: ok. 5 passed`.

- [ ] **Step 10: Commit**

```bash
git add src/extbridge.rs src/lib.rs
git commit -m "$(cat <<'EOF'
(feat) Add extbridge token store skeleton

128-bit single-use token with TTL-checked retrieval. Pure Rust, no I/O —
the HTTP server arrives in the next commit.

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>
EOF
)"
```

---

## Task 2: extbridge HTTP server

**Files:**
- Modify: `src/extbridge.rs` (append server thread + `publish` + integration test)

**Interfaces:**
- Consumes: `Job`, `Handoff`, `Store` from Task 1.
- Produces:
  - `pub fn publish(job: Job, ttl: Duration) -> Result<Handoff, String>` — binds `127.0.0.1:0`, spawns a daemon thread serving exactly one `GET /job/<token>` and shutting down afterward. Returns the bound port and token. On any bind/spawn error, returns `Err(string)` and submitter falls back to the no-fragment URL.
- Server contract:
  - Path `GET /job/<token>` only.
  - On match (and unused, unexpired): `200 OK`, `Content-Type: application/json`, body `{"site":..., "url":..., "language":..., "source":...}`. Connection closes. Server thread exits.
  - Anything else (wrong path, wrong method, expired, already taken, malformed request): `404 Not Found` with empty body. Connection closes. Server thread exits after one response either way (single-shot is fine — token is single-use).
- No CORS headers — see Global Constraints.

- [ ] **Step 1: Write the failing integration test**

Append inside the `tests` module:

```rust
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

        // Second request: 404, server has shut down so this may also fail to connect.
        let r2 = reqwest::blocking::get(&url);
        match r2 {
            Ok(resp) => assert_eq!(resp.status(), 404),
            Err(_) => {} // Connection refused is also acceptable — server exited.
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
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --lib extbridge::tests::publish 2>&1 | tail -20`
Expected: compile errors — `publish` not defined.

- [ ] **Step 3: Implement the server**

Append to `src/extbridge.rs` (above the `#[cfg(test)]` block):

```rust
use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::Arc;
use std::thread;

pub fn publish(job: Job, ttl: Duration) -> Result<Handoff, String> {
    let listener = TcpListener::bind("127.0.0.1:0")
        .map_err(|e| format!("bind 127.0.0.1:0 failed: {}", e))?;
    let port = listener.local_addr()
        .map_err(|e| format!("local_addr failed: {}", e))?.port();

    let store = Arc::new(Store::new());
    let token = store.insert(job, ttl);

    let store_t = Arc::clone(&store);
    let deadline = Instant::now() + ttl + Duration::from_secs(1);

    thread::spawn(move || {
        listener.set_nonblocking(true).ok();
        loop {
            if Instant::now() > deadline {
                return; // TTL+1s: give up so the thread doesn't leak forever.
            }
            match listener.accept() {
                Ok((stream, _)) => {
                    let served = handle(stream, &store_t);
                    if served {
                        return; // Single-shot: served the job, we're done.
                    }
                    // 404s don't terminate the thread — a wrong path before the
                    // real request shouldn't strand the submitter.
                }
                Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(50));
                }
                Err(_) => return,
            }
        }
    });

    Ok(Handoff { port, token })
}

fn handle(mut stream: TcpStream, store: &Store) -> bool {
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
    if parts.len() < 2 || parts[0] != "GET" {
        write_404(&mut stream);
        return false;
    }
    let path = parts[1];
    let token = match path.strip_prefix("/job/") {
        Some(t) => t,
        None => { write_404(&mut stream); return false; }
    };

    match store.take(token) {
        Some(job) => {
            let body = serde_json::json!({
                "site": job.site,
                "url": job.url,
                "language": job.language,
                "source": job.source,
            }).to_string();
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(), body
            );
            let _ = stream.write_all(response.as_bytes());
            true
        }
        None => { write_404(&mut stream); false }
    }
}

fn write_404(stream: &mut TcpStream) {
    let _ = stream.write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test --lib extbridge 2>&1 | tail -10`
Expected: `test result: ok. 7 passed`.

If a test hangs: the server thread isn't shutting down — check the single-shot logic. If 404s come back for the right token: check the `strip_prefix("/job/")` path matches what `reqwest::blocking::get` sends (it should — `/job/<token>` is the path component for `http://127.0.0.1:P/job/T`).

- [ ] **Step 5: Run the full test suite to catch any regressions**

Run: `cargo test 2>&1 | tail -15`
Expected: all tests pass (existing atcoder tests + the 5+2 new ones).

- [ ] **Step 6: Commit**

```bash
git add src/extbridge.rs
git commit -m "$(cat <<'EOF'
(feat) extbridge HTTP server on 127.0.0.1

Hand-rolled HTTP/1.1 on std::net::TcpListener, no tokio/axum. Single
endpoint GET /job/<token>, single-shot: serves the job once then the
thread exits. Bound to OS-assigned ephemeral port on the loopback only.

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>
EOF
)"
```

---

## Task 3: Wire all three submitter sites

**Files:**
- Modify: `src/atcoder.rs` (around line 271–280, just before `open::that`).
- Modify: `src/codeforces.rs` (around line 271–293, just before `open::that`).
- Modify: `src/luogu.rs` (around line 387–392, just before `open::that`).

**Interfaces:**
- Consumes: `extbridge::publish`, `extbridge::Job`, `extbridge::Handoff` from Task 2.
- Produces: nothing for later tasks (terminal in Rust dependency chain).

Each site does the same thing: after the clipboard copy, just before opening the browser, try `extbridge::publish`; if it succeeds, append `#submitter=<port>:<token>` to the URL, otherwise open the URL as-is.

**AtCoder note:** the current signature is `pub fn submit(url: String, _language: String, source: String)`. The language is currently ignored (the `_` prefix). For the extension to pick the right dropdown entry we need to pass it through — change `_language` to `language` and use it in the Job.

**Codeforces note:** `submit` takes the language. Pass it through.

**Luogu note:** same — language is already available.

- [ ] **Step 1: Wire AtCoder**

In `src/atcoder.rs`, change the function signature line:

```rust
pub fn submit(url: String, _language: String, source: String) {
```

to:

```rust
pub fn submit(url: String, language: String, source: String) {
```

Then replace the lines:

```rust
    println!("Source code copied to clipboard");
    open::that(&submit_url).ok();
```

with:

```rust
    println!("Source code copied to clipboard");
    let url_to_open = match crate::extbridge::publish(
        crate::extbridge::Job {
            site: "atcoder",
            url: submit_url.clone(),
            language: language.clone(),
            source: source.clone(),
        },
        std::time::Duration::from_secs(30),
    ) {
        Ok(h) => format!("{}#submitter={}:{}", submit_url, h.port, h.token),
        Err(_) => submit_url.clone(),
    };
    open::that(&url_to_open).ok();
```

Note: `source` was previously moved into `ctx.set_contents(source)`. Change that line to `ctx.set_contents(source.clone()).unwrap();` so `source` is still in scope for the publish call below.

- [ ] **Step 2: Wire Codeforces**

In `src/codeforces.rs`, locate the `open::that(&submit_url).ok();` line (currently line 293) and apply the same pattern. Use `site: "codeforces"`. Pass through `language` (verify the parameter name in the existing `submit` signature — should already be a binding, not `_language`).

Same caveat about `source.clone()` for the clipboard line preceding it.

- [ ] **Step 3: Wire Luogu**

In `src/luogu.rs`, locate the `open::that(&submit_url).ok();` line (currently line 392) and apply the same pattern. Use `site: "luogu"`.

Same caveat about `source.clone()` for the clipboard line preceding it.

- [ ] **Step 4: Verify it builds**

Run: `cargo build 2>&1 | tail -10`
Expected: clean build, no warnings about unused `language`.

- [ ] **Step 5: Verify existing tests still pass**

Run: `cargo test 2>&1 | tail -10`
Expected: all tests pass.

- [ ] **Step 6: Manual smoke (optional but recommended)**

Run a real submit against any of the three sites. Watch for: clipboard still copies, browser still opens. The fragment will be present in the URL bar momentarily until the extension (when installed in later tasks) scrubs it. Without the extension installed, the fragment is harmless — sites ignore unknown fragments.

- [ ] **Step 7: Commit**

```bash
git add src/atcoder.rs src/codeforces.rs src/luogu.rs
git commit -m "$(cat <<'EOF'
(feat) Append #submitter=PORT:TOKEN to AtCoder/Codeforces/Luogu submit URLs

When extbridge::publish succeeds, the opened URL carries a one-shot
fragment that the optional browser extension uses to fetch the source +
language + URL from the loopback server and paste-and-submit
automatically. If publish fails, the plain URL is opened unchanged —
existing clipboard-only flow keeps working.

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>
EOF
)"
```

---

## Task 4: Extension scaffold — manifest, notify, icons

**Files:**
- Create: `extension/manifest.json`
- Create: `extension/shared/notify.js`
- Create: `extension/icons/icon-16.png`, `icon-32.png`, `icon-48.png`, `icon-128.png`

**Interfaces:**
- Consumes: nothing from prior tasks.
- Produces:
  - Global function `notify(message)` (defined at top of every content script bundle via `shared/notify.js` ordering in `content_scripts`). Shows a fixed-position banner at the top of the page for 6 seconds.

- [ ] **Step 1: Write the manifest**

Create `extension/manifest.json`:

```json
{
  "manifest_version": 3,
  "name": "Submitter Helper",
  "version": "0.1.0",
  "description": "Automates paste + language select + click Submit on AtCoder, Codeforces, and Luogu when launched by the submitter CLI.",
  "icons": {
    "16": "icons/icon-16.png",
    "32": "icons/icon-32.png",
    "48": "icons/icon-48.png",
    "128": "icons/icon-128.png"
  },
  "host_permissions": [
    "http://127.0.0.1/*",
    "https://atcoder.jp/contests/*/submit*",
    "https://codeforces.com/contest/*/submit*",
    "https://codeforces.com/problemset/submit*",
    "https://www.luogu.com.cn/problem/*"
  ],
  "content_scripts": [
    {
      "matches": [
        "https://atcoder.jp/contests/*/submit*",
        "https://codeforces.com/contest/*/submit*",
        "https://codeforces.com/problemset/submit*",
        "https://www.luogu.com.cn/problem/*"
      ],
      "js": [
        "shared/notify.js",
        "shared/languages.js",
        "content/atcoder.js",
        "content/codeforces.js",
        "content/luogu.js",
        "content/activator.js"
      ],
      "run_at": "document_idle"
    }
  ],
  "browser_specific_settings": {
    "gecko": {
      "id": "submitter-helper@egork.net",
      "strict_min_version": "109.0"
    }
  }
}
```

- [ ] **Step 2: Write `shared/notify.js`**

Create `extension/shared/notify.js`:

```javascript
(function () {
  if (window.__submitterNotify) return;
  window.__submitterNotify = function notify(message) {
    const banner = document.createElement('div');
    banner.textContent = `[submitter] ${message}`;
    Object.assign(banner.style, {
      position: 'fixed',
      top: '0',
      left: '0',
      right: '0',
      zIndex: '2147483647',
      padding: '10px 16px',
      background: '#fff3cd',
      color: '#664d03',
      borderBottom: '1px solid #ffecb5',
      font: '14px/1.4 system-ui, sans-serif',
      boxShadow: '0 2px 6px rgba(0,0,0,0.15)',
    });
    document.documentElement.appendChild(banner);
    setTimeout(() => banner.remove(), 6000);
  };
  window.notify = window.__submitterNotify;
})();
```

- [ ] **Step 3: Create placeholder icons**

Generate four solid-color PNGs (any tool — ImageMagick, GIMP, an online generator). Plain green squares are fine for Stage 1.

Run:

```bash
mkdir -p extension/icons
for SZ in 16 32 48 128; do
  convert -size ${SZ}x${SZ} xc:'#2da44e' extension/icons/icon-${SZ}.png
done
ls extension/icons/
```

Expected: four PNGs listed. If ImageMagick isn't installed, the user can substitute any 16/32/48/128 PNGs with a single solid color — the artwork doesn't matter for Stage 1.

- [ ] **Step 4: Verify the manifest loads in Chrome**

Open Chrome → `chrome://extensions` → enable Developer mode → Load unpacked → select `extension/`. Expect: extension appears with the green icon and no errors. (Content scripts referenced in the manifest haven't been created yet — Chrome only warns about missing files at content-script injection time, not load time, so the manifest itself loads cleanly.)

Note: this step is optional smoke if you don't have a Chrome instance handy — the real validation comes in Task 5 once activator.js exists.

- [ ] **Step 5: Commit**

```bash
git add extension/manifest.json extension/shared/notify.js extension/icons/
git commit -m "$(cat <<'EOF'
(feat) Scaffold submitter-helper browser extension

MV3 manifest with three site-submit match patterns, loopback
host_permission, and a notify() helper for in-page banners. Solid-color
placeholder icons — artwork to follow at store-publish time. Per-site
fillers + activator land in subsequent commits.

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>
EOF
)"
```

---

## Task 5: activator.js

**Files:**
- Create: `extension/content/activator.js`
- Create: `extension/shared/languages.js` (stub — real maps land per-site in Tasks 6–8)
- Create: `extension/content/atcoder.js`, `codeforces.js`, `luogu.js` (stubs that don't define `__submitterFill` yet, just to make manifest load with no missing files)

**Interfaces:**
- Consumes: `window.__submitterNotify` (alias `window.notify`) from Task 4.
- Produces:
  - Content-script behavior: on any matched page, if `location.hash` contains `#submitter=<port>:<hex32>`, the activator strips the fragment via `history.replaceState`, fetches `http://127.0.0.1:<port>/job/<token>`, and calls `await window.__submitterFill(job)` if defined. On any error, shows a `notify()` banner and returns (clipboard fallback by user).
  - Token regex: `/(?:^|[#&])submitter=(\d{1,5}):([0-9a-f]{32})(?:&|$)/i` — restrict to 32-char lowercase hex to reject malformed fragments.

- [ ] **Step 1: Write `shared/languages.js` stub**

Create `extension/shared/languages.js`:

```javascript
// Per-site language matchers. Each entry: { name, match }
// - `name` is the exact label the page's <select> uses.
// - `match` is an array of substrings (case-insensitive) or RegExp objects.
// `pickLanguage(site, requested)` returns the first entry whose match[] contains
// a substring/regex that matches `requested`, or null if nothing matches.
//
// Real entries land in tasks 6/7/8 — this stub keeps activator.js loadable.
window.__submitterLanguages = {
  atcoder: [],
  codeforces: [],
  luogu: [],
};

window.__submitterPickLanguage = function pickLanguage(site, requested) {
  const list = window.__submitterLanguages[site] || [];
  const needle = String(requested || '').trim();
  if (!needle) return null;
  for (const entry of list) {
    for (const m of entry.match) {
      if (m instanceof RegExp) {
        if (m.test(needle)) return entry;
      } else {
        if (needle.toLowerCase().includes(String(m).toLowerCase())) return entry;
      }
    }
  }
  return null;
};
```

- [ ] **Step 2: Write per-site filler stubs**

Create three near-identical files. `extension/content/atcoder.js`:

```javascript
if (location.hostname === 'atcoder.jp') {
  // Filler implementation lands in a later task.
  // Defining nothing here means activator's "typeof __submitterFill !== 'function'"
  // branch fires → user sees the "no filler" banner → clipboard fallback.
}
```

`extension/content/codeforces.js`:

```javascript
if (location.hostname === 'codeforces.com') {
  // Filler implementation lands in a later task.
}
```

`extension/content/luogu.js`:

```javascript
if (location.hostname === 'www.luogu.com.cn') {
  // Filler implementation lands in a later task.
}
```

- [ ] **Step 3: Write `activator.js`**

Create `extension/content/activator.js`:

```javascript
(async function () {
  const re = /(?:^|[#&])submitter=(\d{1,5}):([0-9a-f]{32})(?:&|$)/i;
  const m = location.hash.match(re);
  if (!m) return;

  const port = m[1];
  const token = m[2].toLowerCase();

  // Scrub the fragment before any other script can read it.
  history.replaceState(null, '', location.pathname + location.search);

  let job;
  try {
    const r = await fetch(`http://127.0.0.1:${port}/job/${token}`);
    if (!r.ok) throw new Error(`HTTP ${r.status}`);
    job = await r.json();
  } catch (e) {
    window.notify(`couldn't reach helper (${e.message}). Paste from clipboard.`);
    return;
  }

  if (typeof window.__submitterFill !== 'function') {
    window.notify(`no filler for this site yet. Paste from clipboard.`);
    return;
  }

  try {
    await window.__submitterFill(job);
  } catch (e) {
    window.notify(`${e.message}. Paste from clipboard.`);
  }
})();
```

- [ ] **Step 4: Reload the extension and verify activator runs**

Reload the extension in `chrome://extensions`. Then:

1. Start submitter to submit any AtCoder problem. It will open the submit page with `#submitter=PORT:TOKEN`.
2. The activator should: fetch the job, see no `__submitterFill` defined yet, show the "no filler for this site yet" banner, scrub the fragment from the URL.
3. Verify the URL no longer contains `#submitter=...`.
4. Verify the banner appears for ~6s.

If the activator silently does nothing: open DevTools → Console on the submit page. Look for "Failed to fetch" (port issue), token mismatch, or absent matches.

- [ ] **Step 5: Commit**

```bash
git add extension/content/activator.js extension/content/atcoder.js extension/content/codeforces.js extension/content/luogu.js extension/shared/languages.js
git commit -m "$(cat <<'EOF'
(feat) activator.js + language-matcher scaffolding

Content script reads #submitter=PORT:TOKEN, scrubs the fragment via
history.replaceState, fetches the job from the loopback server, and
dispatches to a per-site __submitterFill. Without a filler defined it
falls back to a banner asking the user to paste manually.

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>
EOF
)"
```

---

## Task 6: AtCoder filler

**Files:**
- Modify: `extension/content/atcoder.js`
- Modify: `extension/shared/languages.js` (populate the `atcoder` array)

**Interfaces:**
- Consumes: `window.__submitterPickLanguage`, `window.notify`.
- Produces: `window.__submitterFill = async (job) => { ... }` defined when `location.hostname === 'atcoder.jp'`.

**Page model (AtCoder submit page):**
- The code area is an ACE editor wrapped in `<div id="editor-...">`. The ACE instance is accessed via `ace.edit(<div>)`. Setting code: `editor.setValue(source, -1)` (the `-1` puts the cursor at the start).
- The language `<select>` has id `select-lang-N` where `N` is a numeric task index — there are typically multiple selects on the page (one per task), only one visible. The visible one is inside a `<div>` whose ancestor `<div class="form-group">` is not hidden. Easier path: query `select[id^="select-lang"]` and pick the one whose `offsetParent` is non-null (visible).
- Submit button: `<input type="submit" id="submit" value="Submit">` or `<button id="submit">`. `form.querySelector('#submit')` then click.

- [ ] **Step 1: Populate the AtCoder language map**

In `extension/shared/languages.js`, replace `atcoder: [],` with the populated array. Pull current option labels from a live AtCoder submit page (right-click language select → Inspect → copy each `<option>` text). Initial set:

```javascript
  atcoder: [
    { name: "C++ 23 (gcc 12.2)",       match: ["c++23 (gcc", "c++23"] },
    { name: "C++ 23 (Clang 16.0.6)",   match: ["c++23 (clang"] },
    { name: "C++ 20 (gcc 12.2)",       match: ["c++20 (gcc", "c++20"] },
    { name: "C++ 20 (Clang 16.0.6)",   match: ["c++20 (clang"] },
    { name: "C++ 17 (gcc 12.2)",       match: ["c++17", /^c\+\+$/i, "g++"] },
    { name: "Python (CPython 3.11.4)", match: [/^python\s*3/i, "cpython", "python"] },
    { name: "PyPy3 (7.3.12)",          match: [/^pypy/i] },
    { name: "Rust (1.70.0)",           match: [/^rust/i] },
    { name: "Java (OpenJDK 17)",       match: [/^java/i] },
    { name: "C# 11.0 (.NET 7.0.7)",    match: [/^c\#/i, "csharp", ".net"] },
    { name: "Go (1.20.6)",             match: [/^go$/i, "golang"] },
    { name: "Kotlin (1.8.20)",         match: [/^kotlin/i] },
  ],
```

(These names are illustrative — verify against a live page before publishing. The plan's contract is "first label that exactly matches an `<option>`"; missing entries land in stretch issues.)

- [ ] **Step 2: Write the AtCoder filler**

Replace `extension/content/atcoder.js` with:

```javascript
if (location.hostname === 'atcoder.jp') {
  window.__submitterFill = async function (job) {
    const editor = await waitFor(() => findEditor(), 10000);
    const select = await waitFor(() => findLanguageSelect(), 10000);
    const submitBtn = await waitFor(() => document.querySelector('#submit'), 10000);

    if (!editor) throw new Error('code editor not found');
    if (!select) throw new Error('language select not found');
    if (!submitBtn) throw new Error('submit button not found');

    editor.setValue(job.source, -1);

    const entry = window.__submitterPickLanguage('atcoder', job.language);
    if (!entry) {
      window.notify(`unknown language "${job.language}". Pick one and click Submit.`);
      return;
    }
    const option = Array.from(select.options).find(o => o.textContent.trim() === entry.name);
    if (!option) {
      window.notify(`language "${entry.name}" not in dropdown. Pick one and click Submit.`);
      return;
    }
    select.value = option.value;
    select.dispatchEvent(new Event('change', { bubbles: true }));

    submitBtn.click();
  };

  function findEditor() {
    if (typeof ace === 'undefined') return null;
    const div = Array.from(document.querySelectorAll('div[id^="editor"]'))
      .find(d => d.offsetParent !== null);
    if (!div) return null;
    return ace.edit(div);
  }

  function findLanguageSelect() {
    return Array.from(document.querySelectorAll('select[id^="select-lang"]'))
      .find(s => s.offsetParent !== null) || null;
  }

  function waitFor(fn, timeoutMs) {
    return new Promise((resolve) => {
      const start = Date.now();
      const tick = () => {
        const r = fn();
        if (r) return resolve(r);
        if (Date.now() - start > timeoutMs) return resolve(null);
        setTimeout(tick, 100);
      };
      tick();
    });
  }
}
```

- [ ] **Step 3: Manual smoke**

1. Reload the extension.
2. `submitter submit <atcoder-problem-url> <source-file>` with C++.
3. Expected: AtCoder submit page opens, brief pause, code appears in the editor, language picked, submit clicked, verdict line lights up in submitter's terminal as usual.
4. Test the unknown-language path: run with a language not in the map (e.g. "C++14 (gcc 5.4.1)" if it's been retired). Expected: banner "unknown language ...", source still pasted, no auto-click.

- [ ] **Step 4: Commit**

```bash
git add extension/content/atcoder.js extension/shared/languages.js
git commit -m "$(cat <<'EOF'
(feat) AtCoder filler — ACE editor + language select + click submit

window.__submitterFill on atcoder.jp waits up to 10s (MutationObserver-
style polling) for the editor / select / button to render, then pastes
the source, picks the matching <option> via the languages map, and
clicks Submit. Unknown language → banner, source pasted, no auto-click.

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>
EOF
)"
```

---

## Task 7: Codeforces filler

**Files:**
- Modify: `extension/content/codeforces.js`
- Modify: `extension/shared/languages.js` (populate the `codeforces` array)

**Interfaces:**
- Consumes: `window.__submitterPickLanguage`, `window.notify`.
- Produces: `window.__submitterFill` defined when `location.hostname === 'codeforces.com'`.

**Page model (Codeforces submit page):**
- Code textarea: `<textarea id="sourceCodeTextarea" name="source">`. Setting code: `textarea.value = source; textarea.dispatchEvent(new Event('input', { bubbles: true }));`. Codeforces also has a CodeMirror skin that some users enable — for now, target the plain textarea (CodeMirror copies into it on submit anyway). If CodeMirror is present, additionally call `editor.setValue(source)` on the CodeMirror instance accessible via the wrapping `.CodeMirror` element.
- Language `<select>` has `name="programTypeId"`. Pick the visible one (`offsetParent !== null`).
- Submit button: `<input type="submit" class="submit" value="Submit">`. Easier: `document.querySelector('input[type=submit][value=Submit]')` inside the form.

- [ ] **Step 1: Populate the Codeforces language map**

In `extension/shared/languages.js`, replace `codeforces: [],` with:

```javascript
  codeforces: [
    { name: "GNU G++23 14.2 (64 bit, msys2)",   match: ["c++23"] },
    { name: "GNU G++20 13.2 (64 bit, winlibs)", match: ["c++20", "c++"] },
    { name: "GNU G++17 7.3.0",                  match: ["c++17"] },
    { name: "Python 3.8.10",                    match: [/^python\s*3/i, "cpython"] },
    { name: "PyPy 3.10 (7.3.15, 64bit)",        match: [/^pypy/i] },
    { name: "Rust 2021 (1.75.0)",               match: [/^rust/i] },
    { name: "Java 21 64bit",                    match: [/^java/i] },
    { name: "C# 10, .NET SDK 6.0",              match: [/^c\#/i, "csharp", ".net"] },
    { name: "Go 1.22.2",                        match: [/^go$/i, "golang"] },
    { name: "Kotlin 1.9.21",                    match: [/^kotlin/i] },
  ],
```

(Same caveat — verify against the live dropdown before publishing.)

- [ ] **Step 2: Write the Codeforces filler**

Replace `extension/content/codeforces.js` with:

```javascript
if (location.hostname === 'codeforces.com') {
  window.__submitterFill = async function (job) {
    const textarea = await waitFor(() => document.querySelector('textarea[name="source"]'), 10000);
    const select = await waitFor(() => visible(document.querySelectorAll('select[name="programTypeId"]')), 10000);
    const submitBtn = await waitFor(() => document.querySelector('input[type=submit][value="Submit"]'), 10000);

    if (!textarea) throw new Error('source textarea not found');
    if (!select) throw new Error('language select not found');
    if (!submitBtn) throw new Error('submit button not found');

    textarea.value = job.source;
    textarea.dispatchEvent(new Event('input', { bubbles: true }));
    textarea.dispatchEvent(new Event('change', { bubbles: true }));

    // If CodeMirror is enabled, mirror into it so the user sees the code.
    const cmEl = document.querySelector('.CodeMirror');
    if (cmEl && cmEl.CodeMirror) cmEl.CodeMirror.setValue(job.source);

    const entry = window.__submitterPickLanguage('codeforces', job.language);
    if (!entry) {
      window.notify(`unknown language "${job.language}". Pick one and click Submit.`);
      return;
    }
    const option = Array.from(select.options).find(o => o.textContent.trim() === entry.name);
    if (!option) {
      window.notify(`language "${entry.name}" not in dropdown. Pick one and click Submit.`);
      return;
    }
    select.value = option.value;
    select.dispatchEvent(new Event('change', { bubbles: true }));

    submitBtn.click();
  };

  function visible(nodes) {
    return Array.from(nodes).find(n => n.offsetParent !== null) || null;
  }

  function waitFor(fn, timeoutMs) {
    return new Promise((resolve) => {
      const start = Date.now();
      const tick = () => {
        const r = fn();
        if (r) return resolve(r);
        if (Date.now() - start > timeoutMs) return resolve(null);
        setTimeout(tick, 100);
      };
      tick();
    });
  }
}
```

- [ ] **Step 3: Manual smoke**

Same as Task 6 step 3 but against a Codeforces problem.

- [ ] **Step 4: Commit**

```bash
git add extension/content/codeforces.js extension/shared/languages.js
git commit -m "$(cat <<'EOF'
(feat) Codeforces filler — textarea (+ CodeMirror mirror) + select + submit

window.__submitterFill on codeforces.com writes into the plain textarea,
also mirroring to CodeMirror if the user has the editor skin enabled,
then picks the language and clicks Submit. Unknown language → banner,
source pasted, no auto-click.

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>
EOF
)"
```

---

## Task 8: Luogu filler

**Files:**
- Modify: `extension/content/luogu.js`
- Modify: `extension/shared/languages.js` (populate the `luogu` array)

**Interfaces:**
- Consumes: `window.__submitterPickLanguage`, `window.notify`.
- Produces: `window.__submitterFill` defined when `location.hostname === 'www.luogu.com.cn'`.

**Page model (Luogu problem page with `#submit`):**
- Luogu is a Vue SPA. The submit panel reveals when the URL fragment is `#submit`. After the activator strips the fragment, the panel may collapse — so the filler should re-add `#submit` to `location.hash` before doing its work (do this BEFORE the `replaceState` runs… but `replaceState` already happened in activator). Workaround: the filler sets `location.hash = 'submit'` itself, then waits for the editor.
- The code editor is Monaco. Access via the wrapping `<div class="monaco-editor">`'s data — the cleanest path is to find the hidden `<textarea>` Monaco uses for input and dispatch keyboard events… which is fragile. Better: Luogu exposes the textarea via `monaco.editor.getEditors()[0].setValue(source)` once Monaco's global is loaded.
- Language select: a custom Vue component, not a native `<select>`. The filler clicks the dropdown trigger, then clicks the matching `<li>` by visible text. Inspect a live page to capture the selectors; expected shape `.lg-dropdown` button and `.lg-dropdown-menu li`.
- Submit button: `<button>` with text "提交" (Submit). Match by `textContent.trim() === '提交'` or by visible button inside the submit form.

Because the Luogu page is the most dynamic, give the MutationObserver wait a longer ceiling (15s) and prefer Monaco's API over DOM-poking.

- [ ] **Step 1: Populate the Luogu language map**

In `extension/shared/languages.js`, replace `luogu: [],` with:

```javascript
  luogu: [
    { name: "C++ 20 (GCC 13)",            match: ["c++20", "c++"] },
    { name: "C++ 17 (GCC 13)",            match: ["c++17"] },
    { name: "C++ 14 (GCC 13)",            match: ["c++14"] },
    { name: "Python 3.11",                match: [/^python\s*3/i, "cpython"] },
    { name: "PyPy 3 (8.0)",               match: [/^pypy/i] },
    { name: "Rust (rustc 1.74)",          match: [/^rust/i] },
    { name: "Java 17 (OpenJDK)",          match: [/^java/i] },
    { name: "Go 1.22",                    match: [/^go$/i, "golang"] },
    { name: "Kotlin",                     match: [/^kotlin/i] },
  ],
```

- [ ] **Step 2: Write the Luogu filler**

Replace `extension/content/luogu.js` with:

```javascript
if (location.hostname === 'www.luogu.com.cn') {
  window.__submitterFill = async function (job) {
    // The activator stripped #submit; restore it so the submit panel opens.
    if (location.hash !== '#submit') {
      location.hash = 'submit';
    }

    const editor = await waitFor(() => findMonacoEditor(), 15000);
    if (!editor) throw new Error('Monaco editor not found');
    editor.setValue(job.source);

    const entry = window.__submitterPickLanguage('luogu', job.language);
    if (entry) {
      const picked = await pickLanguageOption(entry.name, 10000);
      if (!picked) {
        window.notify(`language "${entry.name}" not in dropdown. Pick one and click 提交.`);
        return;
      }
    } else {
      window.notify(`unknown language "${job.language}". Pick one and click 提交.`);
      return;
    }

    const submitBtn = await waitFor(() => findSubmitButton(), 10000);
    if (!submitBtn) throw new Error('Submit button not found');
    submitBtn.click();
  };

  function findMonacoEditor() {
    if (typeof monaco === 'undefined' || !monaco.editor) return null;
    const editors = monaco.editor.getEditors();
    return editors.length > 0 ? editors[0] : null;
  }

  async function pickLanguageOption(name, timeoutMs) {
    // Click the dropdown trigger to open the menu, then click the matching item.
    // Luogu's exact selectors should be verified on a live page — adjust as needed.
    const trigger = await waitFor(
      () => document.querySelector('.lg-dropdown[data-v-]') || document.querySelector('.lg-select'),
      timeoutMs
    );
    if (!trigger) return false;
    trigger.click();
    const item = await waitFor(() => Array.from(document.querySelectorAll('.lg-dropdown-menu li, .lg-select-option'))
      .find(li => li.textContent.trim() === name), timeoutMs);
    if (!item) return false;
    item.click();
    return true;
  }

  function findSubmitButton() {
    return Array.from(document.querySelectorAll('button'))
      .find(b => b.textContent.trim() === '提交' && b.offsetParent !== null) || null;
  }

  function waitFor(fn, timeoutMs) {
    return new Promise((resolve) => {
      const start = Date.now();
      const tick = () => {
        const r = fn();
        if (r) return resolve(r);
        if (Date.now() - start > timeoutMs) return resolve(null);
        setTimeout(tick, 100);
      };
      tick();
    });
  }
}
```

**Note on selectors:** Luogu's class names contain Vue scoped-CSS data attributes that change per build. Capture current selectors from a live page via DevTools right before implementing — the `.lg-dropdown` / `.lg-dropdown-menu` names above are a best guess. If they don't match, replace with whatever the current page uses.

- [ ] **Step 3: Manual smoke**

Same pattern as Tasks 6/7 but against a Luogu problem. If the language dropdown click fails, leave the source pasted and the banner — the user can pick the language manually.

- [ ] **Step 4: Commit**

```bash
git add extension/content/luogu.js extension/shared/languages.js
git commit -m "$(cat <<'EOF'
(feat) Luogu filler — Monaco editor + custom dropdown + 提交 button

window.__submitterFill on www.luogu.com.cn re-sets the #submit hash (it
was scrubbed by the activator), waits for Monaco to mount, calls
setValue on the editor, opens Luogu's custom language dropdown and
picks the matching item, then clicks the 提交 button. Selectors for the
dropdown are best-effort and may need an update if Luogu redesigns.

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>
EOF
)"
```

---

## Task 9: Release packaging + README

**Files:**
- Modify: `.github/workflows/release.yml`
- Modify: `README.md`
- Create: `extension/README.md`
- Modify: `CHANGELOG.md`

**Interfaces:**
- Consumes: everything above.
- Produces: a `submitter-extension-<version>.zip` asset on every GitHub release; user-facing install docs.

- [ ] **Step 1: Add an `extension` sibling job**

The existing `.github/workflows/release.yml` has two jobs: `build` (matrix of deb/exe/dmg, each uploading its artifact via `actions/upload-artifact`) and `release` (downloads everything via `actions/download-artifact` and publishes with `softprops/action-gh-release`). The cleanest insertion is a new sibling job that produces another artifact, then make `release` wait on it too.

Insert this new job between the `build` job and the `release` job (paste after the `build` job's closing brace, before `release:`):

```yaml
  extension:
    name: Package browser extension
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - name: Build zip
        shell: bash
        run: |
          mkdir -p dist
          cd extension
          zip -r "../dist/submitter-extension-${{ inputs.version }}.zip" . -x "README.md"
      - uses: actions/upload-artifact@v4
        with:
          name: extension
          path: dist/*
          if-no-files-found: error
```

Then change the `release` job's `needs:` line from:

```yaml
    needs: build
```

to:

```yaml
    needs: [build, extension]
```

The existing `softprops/action-gh-release@v2` step already takes `files: dist/*`, and `download-artifact` with `merge-multiple: true` will pull the extension zip into the same `dist/` directory — no further changes to the publish step are needed.

- [ ] **Step 2: Write `extension/README.md`**

Create `extension/README.md`:

```markdown
# Submitter Helper (browser extension)

Optional companion to the `submitter` CLI. Automates paste + language
select + click on the AtCoder, Codeforces, and Luogu submit pages.

## Install (unpacked, recommended for now)

### Chrome / Edge / Brave

1. Download `submitter-extension-<version>.zip` from the latest
   [submitter release](https://github.com/EgorKulikov/submitter/releases).
2. Unzip it to a folder you'll keep around (the extension loads from
   that folder).
3. Open `chrome://extensions`, enable **Developer mode**, click **Load
   unpacked**, select the unzipped folder.

### Firefox

1. Download and unzip as above.
2. Open `about:debugging#/runtime/this-firefox`.
3. Click **Load Temporary Add-on**, select the unzipped folder's
   `manifest.json`.

Note: Firefox temporary add-ons disappear on browser restart. To make
it persistent, build a signed `.xpi` with `web-ext build` (see Mozilla
docs).

## How it works

When `submitter` opens the submit page, it appends a one-shot URL
fragment like `#submitter=PORT:TOKEN`. The extension reads it, fetches
the source from a short-lived loopback server in `submitter`, then
pastes + picks the language + clicks Submit. If anything goes wrong, a
yellow banner explains and the source is still on your clipboard.

The extension is purely additive — uninstall it and `submitter` works
exactly as before.

## Privacy

- No network requests except to `http://127.0.0.1:<port>` (your own
  machine) and the three site domains the extension is permitted on.
- No tracking, no analytics, no remote config.

## Languages

The map of language names lives in `shared/languages.js`. If your
language isn't matched, the extension pastes the code and leaves
language picking to you. PRs to extend the map are welcome.
```

- [ ] **Step 3: Add a section to the project README**

Open `README.md`, find a suitable spot (probably after the install instructions for the CLI), and append:

```markdown
## Browser extension (optional, AtCoder / Codeforces / Luogu)

The three judges above don't have a usable submit API for normal users,
so `submitter` opens a browser tab and copies the source to the
clipboard. The optional [Submitter Helper extension](extension/README.md)
automates the paste-and-submit step in Chrome and Firefox.

Without the extension, nothing changes — the page opens, you paste, you
submit. The extension is purely additive.

Download `submitter-extension-<version>.zip` from a
[release](https://github.com/EgorKulikov/submitter/releases) and follow
the install instructions in `extension/README.md`.
```

- [ ] **Step 4: Add CHANGELOG entry**

Edit `CHANGELOG.md`. Replace the `## [Unreleased]` section with:

```markdown
## [Unreleased]

### Added
- AtCoder verdict line now appends the score column for accepted
  submissions, with digits grouped by apostrophe (e.g.
  `Accepted (1'234'567)`).
- Optional browser extension (Chrome + Firefox, Manifest V3) that
  automates paste + language select + click Submit on AtCoder,
  Codeforces, and Luogu. Submitter opens the submit URL with a one-shot
  `#submitter=PORT:TOKEN` fragment that the extension uses to fetch the
  source from a short-lived loopback server. The CLI works unchanged
  when the extension is absent. Released as
  `submitter-extension-<version>.zip` alongside the existing binaries.
```

- [ ] **Step 5: Verify the workflow file parses**

Run: `cat .github/workflows/release.yml | python3 -c "import sys, yaml; yaml.safe_load(sys.stdin)"`
Expected: no output (clean parse). If yaml lib isn't installed, use any online YAML linter.

- [ ] **Step 6: Commit**

```bash
git add .github/workflows/release.yml README.md extension/README.md CHANGELOG.md
git commit -m "$(cat <<'EOF'
(ci) Package browser extension into release zip + install docs

New ubuntu-latest job zips the extension/ directory into
submitter-extension-<tag>.zip and uploads it to each GitHub release.
README and extension/README.md document the unpacked install for
Chrome, Edge, Brave, and Firefox; store publishing remains a Stage 2
follow-up.

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>
EOF
)"
```

---

## Done

After Task 9, the extension is downloadable from any release, installs in unpacked mode on all four targeted browsers, and the submitter CLI works exactly as before for users who don't install it. Store publication (Chrome Web Store + Firefox AMO) is a separate effort and intentionally not in this plan — see spec §4.

# AtCoder direct-submit with browser fallback — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a direct HTTPS POST submission path to `atcoder::submit` that runs before the current browser handoff, falls back to browser on any failure, and shares a new language matcher module with the future Luogu extension.

**Architecture:** Two-layer flow inside `atcoder::submit`: try `try_direct_submit` first; on `Ok`, poll verdict and skip browser; on `Err`, print the reason and drop into the existing browser handoff (unchanged). The direct-submit path reuses the existing `HttpClient` (with a real-browser `User-Agent` now set globally on the client) and a new site-parameterised `langmatch::pick_option` that mirrors the browser extension's `pickOption`.

**Tech Stack:** Rust 2021, `reqwest::blocking`, `regex`. No new dependencies.

## Global Constraints

- No new dependencies in `Cargo.toml`. Everything uses `reqwest`, `regex`, `serde_json` already present.
- No feature flag — the fallback IS the safety net.
- Extension side is unchanged; do not touch `extension/` at all.
- No live-AtCoder integration tests — same policy as current AtCoder code.
- Fallback triggers on **any** failure of the direct path (network, HTTP, parse, unmatched language). Do not attempt to classify Cloudflare specifically.
- User-Agent constant reused: `Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36` (already at the top of `src/atcoder.rs`).
- Every commit runs `cargo test` and `cargo build` cleanly before landing.

**Reference:** the design spec is `docs/superpowers/specs/2026-07-25-atcoder-direct-submit-design.md`. Read it if any of the rationale here is unclear.

## File Structure

Files created:
- `src/langmatch.rs` — site-parameterised language matcher, ~120 LoC + tests.

Files modified:
- `src/lib.rs` — one line: `pub mod langmatch;`.
- `src/http.rs` — add `post_form_raw` that returns the response without following the 302.
- `src/atcoder.rs` — set UA on `self.http`; rewrite `get_page` to go through `self.http`; add `parse_csrf`, `parse_language_options`, `classify_submit_response`, `try_direct_submit`; wire into `submit()`.

Extension side: nothing.

---

### Task 1: Language matcher module

Port of `pickOption` from `extension/shared/languages.js` — a site-parameterised picker with per-site tables of `{when, options}` entries and a version-tuple tiebreaker. This is a pure module: no I/O, only string matching.

**Files:**
- Create: `src/langmatch.rs`
- Modify: `src/lib.rs` (add module declaration)

**Interfaces:**
- Consumes: nothing (uses only `regex` from stdlib deps)
- Produces: `pub fn pick_option(site: &str, requested: &str, options: &[(String, String)]) -> Option<String>` — returns the winning option `value`, or `None` if no candidates match.

- [ ] **Step 1: Write the failing tests**

Create `src/langmatch.rs` with only the test module (no `pub fn` yet):

```rust
// src/langmatch.rs
// Site-parameterised language matcher. Port of pickOption from
// extension/shared/languages.js. Keep the two in sync when adding languages.

#[cfg(test)]
mod tests {
    use super::pick_option;

    fn opt(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
        pairs.iter().map(|(v, t)| (v.to_string(), t.to_string())).collect()
    }

    #[test]
    fn atcoder_cpp_picks_latest_by_version_tuple() {
        let options = opt(&[
            ("5028", "C++ 20 (gcc 12.2)"),
            ("5001", "C++ 17 (gcc 9.2.1)"),
            ("5029", "C++ 23 (gcc 12.2)"),
        ]);
        assert_eq!(pick_option("atcoder", "cpp", &options).as_deref(), Some("5029"));
    }

    #[test]
    fn atcoder_specific_cpp_standard_wins() {
        let options = opt(&[
            ("5028", "C++ 20 (gcc 12.2)"),
            ("5001", "C++ 17 (gcc 9.2.1)"),
            ("5029", "C++ 23 (gcc 12.2)"),
        ]);
        assert_eq!(pick_option("atcoder", "c++17", &options).as_deref(), Some("5001"));
    }

    #[test]
    fn atcoder_pypy_before_python() {
        let options = opt(&[
            ("5063", "Python (CPython 3.11.4)"),
            ("5078", "Python (PyPy 3.10-v7.3.12)"),
        ]);
        assert_eq!(pick_option("atcoder", "pypy", &options).as_deref(), Some("5078"));
        assert_eq!(pick_option("atcoder", "python", &options).as_deref(), Some("5063"));
    }

    #[test]
    fn atcoder_rust_single_option() {
        let options = opt(&[("5054", "Rust (rustc 1.70.0)")]);
        assert_eq!(pick_option("atcoder", "rust", &options).as_deref(), Some("5054"));
    }

    #[test]
    fn atcoder_no_matching_language() {
        let options = opt(&[("5028", "C++ 20 (gcc 12.2)")]);
        assert_eq!(pick_option("atcoder", "haskell", &options), None);
    }

    #[test]
    fn empty_requested_returns_none() {
        let options = opt(&[("5028", "C++ 20 (gcc 12.2)")]);
        assert_eq!(pick_option("atcoder", "", &options), None);
    }

    #[test]
    fn codeforces_cpp_picks_latest() {
        // Codeforces table entry for bare cpp uses /^gnu g\+\+|^c\+\+/i
        let options = opt(&[
            ("54", "GNU G++17 7.3.0"),
            ("89", "GNU G++20 13.2 (64 bit, winlibs)"),
            ("91", "GNU G++23 13.2 (64 bit, winlibs)"),
        ]);
        assert_eq!(pick_option("codeforces", "cpp", &options).as_deref(), Some("91"));
    }

    #[test]
    fn luogu_cpp_picks_latest() {
        let options = opt(&[
            ("11", "C++11 (gcc 9.5.0)"),
            ("14", "C++14 (gcc 9.5.0)"),
            ("17", "C++17 (gcc 9.5.0)"),
            ("20", "C++20 (gcc 9.5.0)"),
        ]);
        assert_eq!(pick_option("luogu", "cpp", &options).as_deref(), Some("20"));
    }

    #[test]
    fn unknown_site_returns_none() {
        let options = opt(&[("1", "C++ 20")]);
        assert_eq!(pick_option("elsewhere", "cpp", &options), None);
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

```bash
cd /home/egor/proj/submitter
# Add module declaration temporarily so cargo sees the file:
```

Also add to `src/lib.rs`:

```rust
pub mod langmatch;
```

Then:

```bash
cargo test --lib langmatch 2>&1 | tail -20
```

Expected: compile error `cannot find function 'pick_option'`.

- [ ] **Step 3: Implement `pick_option`**

Prepend to `src/langmatch.rs` (before the `#[cfg(test)]` block):

```rust
use regex::Regex;

/// One row in a site's language matcher table.
/// - `when`: patterns matched against the user's requested language string.
///   Each entry is either a regex or a case-insensitive substring.
/// - `options_pat`: regex matched against a dropdown option's display text.
struct Entry {
    when: &'static [When],
    options_pat: &'static str,
}

enum When {
    Regex(&'static str),
    Substring(&'static str),
}

/// Per-site tables. Ordered specific-to-general — first match wins.
/// Keep in sync with extension/shared/languages.js.
fn table_for(site: &str) -> Option<&'static [Entry]> {
    match site {
        "atcoder" => Some(ATCODER),
        "codeforces" => Some(CODEFORCES),
        "luogu" => Some(LUOGU),
        _ => None,
    }
}

const ATCODER: &[Entry] = &[
    Entry { when: &[When::Regex(r"(?i)^c\+\+\s*23\b")],  options_pat: r"(?i)c\+\+\s*23" },
    Entry { when: &[When::Regex(r"(?i)^c\+\+\s*20\b")],  options_pat: r"(?i)c\+\+\s*20" },
    Entry { when: &[When::Regex(r"(?i)^c\+\+\s*17\b")],  options_pat: r"(?i)c\+\+\s*17" },
    Entry { when: &[When::Regex(r"(?i)^c\+\+\s*14\b")],  options_pat: r"(?i)c\+\+\s*14" },
    Entry { when: &[When::Regex(r"(?i)^c\+\+$"), When::Regex(r"(?i)^g\+\+$"), When::Substring("cpp")],
            options_pat: r"(?i)^c\+\+" },
    Entry { when: &[When::Regex(r"(?i)^pypy")], options_pat: r"(?i)pypy" },
    Entry { when: &[When::Regex(r"(?i)^python\s*3"), When::Regex(r"(?i)^python$"), When::Substring("cpython")],
            options_pat: r"(?i)python.*3|^python\s*\(3" },
    Entry { when: &[When::Regex(r"(?i)^rust\s*2024"), When::Substring("rust2024")], options_pat: r"(?i)rust\s*2024" },
    Entry { when: &[When::Regex(r"(?i)^rust\s*2021"), When::Substring("rust2021")], options_pat: r"(?i)rust\s*2021" },
    Entry { when: &[When::Regex(r"(?i)^rust")], options_pat: r"(?i)rust" },
    Entry { when: &[When::Regex(r"(?i)^java")], options_pat: r"(?i)^java" },
    Entry { when: &[When::Regex(r"(?i)^kotlin")], options_pat: r"(?i)kotlin" },
    Entry { when: &[When::Regex(r"(?i)^go$"), When::Substring("golang")], options_pat: r"(?i)^go\b" },
    Entry { when: &[When::Regex(r"(?i)^c\#"), When::Substring("csharp"), When::Substring(".net")],
            options_pat: r"(?i)c\#|csharp|\.net" },
];

const CODEFORCES: &[Entry] = &[
    Entry { when: &[When::Regex(r"(?i)^c\+\+\s*23\b")], options_pat: r"(?i)c\+\+23" },
    Entry { when: &[When::Regex(r"(?i)^c\+\+\s*20\b")], options_pat: r"(?i)c\+\+20" },
    Entry { when: &[When::Regex(r"(?i)^c\+\+\s*17\b")], options_pat: r"(?i)c\+\+17" },
    Entry { when: &[When::Regex(r"(?i)^c\+\+$"), When::Regex(r"(?i)^g\+\+$"), When::Substring("cpp")],
            options_pat: r"(?i)^gnu g\+\+|^c\+\+" },
    Entry { when: &[When::Regex(r"(?i)^pypy")], options_pat: r"(?i)pypy" },
    Entry { when: &[When::Regex(r"(?i)^python\s*3"), When::Regex(r"(?i)^python$"), When::Substring("cpython")],
            options_pat: r"(?i)^python\s*3" },
    Entry { when: &[When::Regex(r"(?i)^rust\s*2024"), When::Substring("rust2024")], options_pat: r"(?i)rust\s*2024" },
    Entry { when: &[When::Regex(r"(?i)^rust\s*2021"), When::Substring("rust2021")], options_pat: r"(?i)rust\s*2021" },
    Entry { when: &[When::Regex(r"(?i)^rust")], options_pat: r"(?i)^rust" },
    Entry { when: &[When::Regex(r"(?i)^java")], options_pat: r"(?i)^java" },
    Entry { when: &[When::Regex(r"(?i)^kotlin")], options_pat: r"(?i)^kotlin" },
    Entry { when: &[When::Regex(r"(?i)^go$"), When::Substring("golang")], options_pat: r"(?i)^go\s" },
    Entry { when: &[When::Regex(r"(?i)^c\#"), When::Substring("csharp"), When::Substring(".net")],
            options_pat: r"(?i)c\#|csharp|\.net" },
];

const LUOGU: &[Entry] = &[
    Entry { when: &[When::Regex(r"(?i)^c\+\+\s*23\b")], options_pat: r"(?i)c\+\+\s*23" },
    Entry { when: &[When::Regex(r"(?i)^c\+\+\s*20\b")], options_pat: r"(?i)c\+\+\s*20" },
    Entry { when: &[When::Regex(r"(?i)^c\+\+\s*17\b")], options_pat: r"(?i)c\+\+\s*17" },
    Entry { when: &[When::Regex(r"(?i)^c\+\+\s*14\b")], options_pat: r"(?i)c\+\+\s*14" },
    Entry { when: &[When::Regex(r"(?i)^c\+\+\s*11\b")], options_pat: r"(?i)c\+\+\s*11" },
    Entry { when: &[When::Regex(r"(?i)^c\+\+\s*98\b")], options_pat: r"(?i)c\+\+\s*98" },
    Entry { when: &[When::Regex(r"(?i)^c\+\+$"), When::Regex(r"(?i)^g\+\+$"), When::Substring("cpp")],
            options_pat: r"(?i)^c\+\+" },
    Entry { when: &[When::Regex(r"(?i)^pypy")], options_pat: r"(?i)pypy" },
    Entry { when: &[When::Regex(r"(?i)^python\s*3"), When::Regex(r"(?i)^python$"), When::Substring("cpython")],
            options_pat: r"(?i)python\s*3" },
    Entry { when: &[When::Regex(r"(?i)^rust")], options_pat: r"(?i)rust" },
    Entry { when: &[When::Regex(r"(?i)^java")], options_pat: r"(?i)^java" },
    Entry { when: &[When::Regex(r"(?i)^kotlin")], options_pat: r"(?i)kotlin" },
    Entry { when: &[When::Regex(r"(?i)^go$"), When::Substring("golang")], options_pat: r"(?i)^go\b" },
];

fn when_matches(w: &When, needle: &str) -> bool {
    match w {
        When::Regex(pat) => Regex::new(pat).map(|r| r.is_match(needle)).unwrap_or(false),
        When::Substring(sub) => needle.to_lowercase().contains(&sub.to_lowercase()),
    }
}

fn version_tuple(text: &str) -> Vec<u32> {
    let re = Regex::new(r"\d+").unwrap();
    re.find_iter(text).filter_map(|m| m.as_str().parse().ok()).collect()
}

fn cmp_tuples(a: &[u32], b: &[u32]) -> std::cmp::Ordering {
    let n = a.len().max(b.len());
    for i in 0..n {
        let ai = a.get(i).copied().unwrap_or(0);
        let bi = b.get(i).copied().unwrap_or(0);
        match ai.cmp(&bi) {
            std::cmp::Ordering::Equal => continue,
            other => return other,
        }
    }
    std::cmp::Ordering::Equal
}

pub fn pick_option(site: &str, requested: &str, options: &[(String, String)]) -> Option<String> {
    let needle = requested.trim();
    if needle.is_empty() {
        return None;
    }
    let table = table_for(site)?;
    let entry = table.iter().find(|e| e.when.iter().any(|w| when_matches(w, needle)))?;
    let opts_re = Regex::new(entry.options_pat).ok()?;

    let mut matches: Vec<&(String, String)> =
        options.iter().filter(|(_, text)| opts_re.is_match(text)).collect();
    if matches.is_empty() {
        return None;
    }
    if matches.len() == 1 {
        return Some(matches[0].0.clone());
    }
    // Default: last in document order.
    let mut best = matches.pop().unwrap();
    let mut best_tuple = version_tuple(&best.1);
    for m in matches {
        let t = version_tuple(&m.1);
        if t.is_empty() {
            continue;
        }
        if best_tuple.is_empty() || cmp_tuples(&t, &best_tuple).is_gt() {
            best = m;
            best_tuple = t;
        }
    }
    Some(best.0.clone())
}
```

- [ ] **Step 4: Run tests to verify they pass**

```bash
cargo test --lib langmatch 2>&1 | tail -20
```

Expected: `test result: ok. 8 passed`.

- [ ] **Step 5: Verify full build still passes**

```bash
cargo build 2>&1 | tail -5
cargo test 2>&1 | tail -5
```

Expected: build succeeds; existing tests still pass.

- [ ] **Step 6: Commit**

```bash
git add src/langmatch.rs src/lib.rs
git commit -m "$(cat <<'EOF'
(feat) Add langmatch module — Rust port of extension pickOption

Site-parameterised (atcoder/codeforces/luogu). Same table shape,
regex families, and version-tuple tiebreaker as the JS side.
Standalone module — no consumers yet; AtCoder direct-submit will
wire it in.

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>
EOF
)"
```

---

### Task 2: `HttpClient::post_form_raw`

The existing `post_form` always follows the 302 via `follow_redirect`, so the caller can never see the original response's `Location` header. Direct-submit needs that header to distinguish "302 → /submissions" (success) from "302 → /login" (session died). Add a variant that returns the raw response.

**Files:**
- Modify: `src/http.rs`

**Interfaces:**
- Consumes: internal `send_with_retry`, `save_response_cookies`
- Produces: `pub fn post_form_raw(&mut self, path: &str, form: &[(&str, &str)]) -> Result<reqwest::blocking::Response, String>` on `HttpClient`.

- [ ] **Step 1: Write the failing test**

Add to the bottom of `src/http.rs`:

```rust
#[cfg(test)]
mod post_form_raw_tests {
    // We can't easily spin up a server in unit tests without extra deps.
    // Compile-time test: verify the method exists with the expected signature.
    #[allow(dead_code)]
    fn _signature_check() {
        fn takes<F>(_: F) where F: for<'a> Fn(&'a mut super::HttpClient, &'a str, &'a [(&'a str, &'a str)])
            -> Result<reqwest::blocking::Response, String> {}
        takes(|c, p, f| c.post_form_raw(p, f));
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

```bash
cargo test --lib post_form_raw 2>&1 | tail -10
```

Expected: compile error `no method named 'post_form_raw'`.

- [ ] **Step 3: Implement `post_form_raw`**

Insert directly after the existing `post_form` (around line 337 in `src/http.rs`):

```rust
    /// Like `post_form`, but returns the raw response without following any 302.
    /// Callers that need to inspect the `Location` header (e.g. to distinguish
    /// success-redirect from session-expired-redirect) use this.
    pub fn post_form_raw(
        &mut self,
        path: &str,
        form: &[(&str, &str)],
    ) -> Result<reqwest::blocking::Response, String> {
        let url = self.resolve_url(path);
        let cookie_header = self.cookie_header();
        let headers = self.extra_headers.clone();
        let send_cookies = self.send_cookies;
        let form_owned: Vec<(String, String)> =
            form.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
        let resp = self.send_with_retry(
            || {
                let mut req = self.client.post(&url);
                if send_cookies && !cookie_header.is_empty() {
                    req = req.header(COOKIE, &cookie_header);
                }
                for (name, value) in &headers {
                    req = req.header(name.clone(), value.clone());
                }
                let form_ref: Vec<(&str, &str)> =
                    form_owned.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
                req.form(&form_ref)
            },
            &url,
        )?;
        self.save_response_cookies(&resp);
        Ok(resp)
    }
```

- [ ] **Step 4: Run test to verify it passes**

```bash
cargo test --lib post_form_raw 2>&1 | tail -5
cargo build 2>&1 | tail -5
```

Expected: tests pass, build succeeds.

- [ ] **Step 5: Commit**

```bash
git add src/http.rs
git commit -m "$(cat <<'EOF'
(feat) HttpClient: add post_form_raw that skips redirect following

AtCoder direct-submit needs to inspect the POST's 302 Location header
to distinguish success-redirect from /login-redirect. post_form always
follows, so add a raw variant.

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>
EOF
)"
```

---

### Task 3: AtCoder parsers + response classifier

Three pure helpers used by `try_direct_submit`. Ship them with tests and no wiring yet; Task 5 wires them into the submit flow.

**Files:**
- Modify: `src/atcoder.rs`

**Interfaces:**
- Consumes: `regex::Regex` (already used in this file)
- Produces (all module-private, `fn` — not `pub`):
  - `fn parse_csrf(html: &str) -> Option<String>`
  - `fn parse_language_options(html: &str) -> Vec<(String, String)>`
  - `fn classify_submit_response(status: u16, location: Option<&str>) -> Result<(), String>`

- [ ] **Step 1: Write the failing tests**

Append to the existing `#[cfg(test)] mod tests` block at the bottom of `src/atcoder.rs`:

```rust
    use super::{parse_csrf, parse_language_options, classify_submit_response};

    #[test]
    fn parse_csrf_extracts_token() {
        let html = r#"<form><input type="hidden" name="csrf_token" value="abc123XYZ=="></form>"#;
        assert_eq!(parse_csrf(html).as_deref(), Some("abc123XYZ=="));
    }

    #[test]
    fn parse_csrf_handles_attribute_order() {
        let html = r#"<input value="tok999" name="csrf_token" type="hidden">"#;
        // Same-line, name comes after value — our regex looks for name= first,
        // so this variant is expected to miss. Document that: if AtCoder ever
        // reorders attributes, we fall back to browser (safe) rather than parse.
        assert_eq!(parse_csrf(html), None);
    }

    #[test]
    fn parse_csrf_missing_returns_none() {
        let html = "<form></form>";
        assert_eq!(parse_csrf(html), None);
    }

    #[test]
    fn parse_language_options_extracts_pairs() {
        let html = r#"
          <select name="data.LanguageId" class="form-control">
            <option value="5001">C++ 17 (gcc 9.2.1)</option>
            <option value="5054">Rust (rustc 1.70.0)</option>
          </select>
        "#;
        let opts = parse_language_options(html);
        assert_eq!(opts.len(), 2);
        assert_eq!(opts[0], ("5001".to_string(), "C++ 17 (gcc 9.2.1)".to_string()));
        assert_eq!(opts[1], ("5054".to_string(), "Rust (rustc 1.70.0)".to_string()));
    }

    #[test]
    fn parse_language_options_missing_select_returns_empty() {
        assert!(parse_language_options("<html></html>").is_empty());
    }

    #[test]
    fn parse_language_options_empty_select_returns_empty() {
        let html = r#"<select name="data.LanguageId"></select>"#;
        assert!(parse_language_options(html).is_empty());
    }

    #[test]
    fn classify_302_to_submissions_is_ok() {
        assert!(classify_submit_response(302, Some("/contests/abc463/submissions/me")).is_ok());
    }

    #[test]
    fn classify_302_to_login_is_err() {
        assert!(classify_submit_response(302, Some("/login")).is_err());
    }

    #[test]
    fn classify_302_without_location_is_err() {
        assert!(classify_submit_response(302, None).is_err());
    }

    #[test]
    fn classify_200_is_err() {
        assert!(classify_submit_response(200, None).is_err());
    }

    #[test]
    fn classify_403_is_err() {
        assert!(classify_submit_response(403, None).is_err());
    }
```

- [ ] **Step 2: Run tests to verify they fail**

```bash
cargo test --lib atcoder 2>&1 | tail -20
```

Expected: compile error `cannot find function 'parse_csrf'` (and friends).

- [ ] **Step 3: Implement the three helpers**

Insert into `src/atcoder.rs` just above the existing `fn group_digits(...)` (near line 207):

```rust
fn parse_csrf(html: &str) -> Option<String> {
    let re = Regex::new(r#"(?is)<input[^>]*name="csrf_token"[^>]*value="([^"]+)""#).ok()?;
    re.captures(html).map(|c| c[1].to_string())
}

fn parse_language_options(html: &str) -> Vec<(String, String)> {
    let sel_re = Regex::new(
        r#"(?is)<select[^>]*name="data\.LanguageId"[^>]*>(.*?)</select>"#,
    )
    .unwrap();
    let opt_re =
        Regex::new(r#"(?is)<option[^>]*value="([^"]+)"[^>]*>(.*?)</option>"#).unwrap();
    let inner = match sel_re.captures(html) {
        Some(c) => c[1].to_string(),
        None => return Vec::new(),
    };
    opt_re
        .captures_iter(&inner)
        .map(|c| (c[1].to_string(), c[2].trim().to_string()))
        .collect()
}

/// Given the raw POST response's status + `Location` header, decide whether
/// the direct submission succeeded. Success = 302 whose Location contains
/// "/submissions". Everything else = fallback trigger.
fn classify_submit_response(status: u16, location: Option<&str>) -> Result<(), String> {
    if status == 200 {
        return Err("POST returned 200 (form re-rendered — validation failed?)".to_string());
    }
    if !(300..400).contains(&status) {
        return Err(format!("POST returned {}", status));
    }
    let loc = location.ok_or_else(|| "POST redirected with no Location header".to_string())?;
    if loc.contains("/submissions") {
        Ok(())
    } else if loc.contains("/login") {
        Err("POST redirected to /login (session expired?)".to_string())
    } else {
        Err(format!("POST redirected to unexpected Location: {}", loc))
    }
}
```

- [ ] **Step 4: Run tests to verify they pass**

```bash
cargo test --lib atcoder 2>&1 | tail -10
```

Expected: all new tests pass; existing tests still pass.

- [ ] **Step 5: Commit**

```bash
git add src/atcoder.rs
git commit -m "$(cat <<'EOF'
(feat) AtCoder: add parse_csrf, parse_language_options, classify_submit_response

Pure helpers for the upcoming direct-submit path. Not wired in yet;
that comes next. Full unit-test coverage — no live-network tests.

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>
EOF
)"
```

---

### Task 4: Prepare `self.http` for real HTTP calls — UA + cookie-name migration

Before we can make direct-submit calls through `self.http`, two things need fixing:

1. **User-Agent** — reqwest's default is `reqwest/x.y.z`, which AtCoder could easily filter on. Set the same `Mozilla/5.0…` UA the existing `get_page` uses.
2. **Cookie name** — the existing code stores the AtCoder session cookie under key `"atcoder_revel_session"` in the on-disk cookie store, but AtCoder's actual cookie is named `REVEL_SESSION`. Today this doesn't matter because `self.http` is only used as a storage backend; the actual `get_page` manually builds a `Cookie: REVEL_SESSION=…` header. But once direct-submit uses `self.http` for real requests, its auto-cookie-injection would send `atcoder_revel_session=…` (an unrecognized name) and AtCoder would treat us as logged out. Migrate to storing under `REVEL_SESSION`, with a read-fallback from the old key so existing users don't have to re-login.

**We deliberately do not rewrite `get_page` in this task.** `get_page` currently uses an ad-hoc `reqwest::blocking::Client` with `Policy::limited(5)` (up to 5 auto-follow redirects); `HttpClient` uses `Policy::none()` + one manual follow. Rewriting would silently change redirect behaviour with no upside for this feature. `get_page` stays as-is; direct-submit rides on `self.http`.

**Files:**
- Modify: `src/atcoder.rs`

**Interfaces:** No new public API. `AtcoderClient::new` and `AtcoderClient::login` change internally; the `AtcoderClient` struct keeps its `revel_session: String` field (still used by `get_page`).

- [ ] **Step 1: Set the User-Agent on the HttpClient at construction, and migrate the cookie key**

Replace the current `AtcoderClient::new` (starts around line 19) with:

```rust
    fn new() -> Self {
        let mut http = HttpClient::new("https://atcoder.jp");
        http.set_header("User-Agent", USER_AGENT);
        // Read the session cookie under its canonical name (REVEL_SESSION), and
        // fall back to the legacy key "atcoder_revel_session" for existing users.
        // If we found it under the legacy key, migrate it forward so self.http's
        // auto-injection sends the right cookie name to AtCoder.
        let revel_session = if let Some(v) = http.get_cookie("REVEL_SESSION") {
            v
        } else if let Some(v) = http.get_cookie("atcoder_revel_session") {
            http.set_cookie("REVEL_SESSION", &v);
            v
        } else {
            String::new()
        };
        AtcoderClient {
            http,
            revel_session,
        }
    }
```

- [ ] **Step 2: Update `login()` to store under the canonical name**

Find the line in `fn login` (around line 93) that reads `self.http.set_cookie("atcoder_revel_session", &session);` and change the key:

```rust
            self.http.set_cookie("REVEL_SESSION", &session);
```

That is the only edit in `login`.

- [ ] **Step 3: Run all tests + build**

```bash
cargo test 2>&1 | tail -10
cargo build 2>&1 | tail -5
```

Expected: all tests pass; build succeeds. Existing users' cookie files carry `atcoder_revel_session`; the migration in `new` will move it to `REVEL_SESSION` on next run.

- [ ] **Step 4: Commit**

```bash
git add src/atcoder.rs
git commit -m "$(cat <<'EOF'
(refactor) AtCoder: set browser UA on HttpClient + migrate cookie key to REVEL_SESSION

Direct-submit is coming and will use self.http for real HTTP calls,
not just as a cookie storage backend. Two prep changes:

  * Real browser UA on self.http (reqwest's default would stand out).
  * Cookie stored under REVEL_SESSION (the actual name AtCoder expects)
    with a one-time read-migration from the legacy "atcoder_revel_session"
    key so existing users don't have to re-login.

get_page stays on its ad-hoc client — rewriting would change redirect
policy for no gain here.

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>
EOF
)"
```

---

### Task 5: `try_direct_submit` — the direct POST path

Compose everything from tasks 1–4 into the actual submit attempt. Not wired into `submit()` yet; that's task 6.

**Files:**
- Modify: `src/atcoder.rs`

**Interfaces:**
- Consumes: `parse_csrf`, `parse_language_options`, `classify_submit_response`, `HttpClient::post_form_raw`, `langmatch::pick_option`.
- Produces: method on `AtcoderClient`:
  `fn try_direct_submit(&mut self, contest_id: &str, task_id: &str, language: &str, source: &str) -> Result<(), String>`

- [ ] **Step 1: Add the method to `AtcoderClient`**

Insert into the existing `impl AtcoderClient` block in `src/atcoder.rs`, right before `fn poll_verdict`:

```rust
    fn try_direct_submit(
        &mut self,
        contest_id: &str,
        task_id: &str,
        language: &str,
        source: &str,
    ) -> Result<(), String> {
        if task_id.is_empty() {
            return Err("no task in URL".to_string());
        }

        let submit_path = format!("/contests/{}/submit", contest_id);
        let page = self.http.get_text(&submit_path)
            .map_err(|e| format!("GET submit page failed: {}", e))?;

        let csrf = parse_csrf(&page)
            .ok_or_else(|| "CSRF token not found on submit page".to_string())?;

        let options = parse_language_options(&page);
        if options.is_empty() {
            return Err("no language options found on submit page".to_string());
        }
        let lang_id = crate::langmatch::pick_option("atcoder", language, &options)
            .ok_or_else(|| format!("no matching language for {:?}", language))?;

        let resp = self.http.post_form_raw(
            &submit_path,
            &[
                ("csrf_token", &csrf),
                ("data.TaskScreenName", task_id),
                ("data.LanguageId", &lang_id),
                ("sourceCode", source),
            ],
        )?;

        let status = resp.status().as_u16();
        let location = resp
            .headers()
            .get("location")
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string());
        classify_submit_response(status, location.as_deref())
    }
```

- [ ] **Step 2: Build to verify it compiles**

```bash
cargo build 2>&1 | tail -10
```

Expected: clean build. `try_direct_submit` is unused (dead-code warning is fine — Task 6 wires it in).

- [ ] **Step 3: Commit**

```bash
git add src/atcoder.rs
git commit -m "$(cat <<'EOF'
(feat) AtCoder: add try_direct_submit — the direct POST path

Composes parse_csrf + parse_language_options + langmatch::pick_option
+ post_form_raw + classify_submit_response. Not wired into submit()
yet; that's the next commit so the wiring is reviewable on its own.

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>
EOF
)"
```

---

### Task 6: Wire into `submit()`

Add the direct-submit attempt before the existing browser handoff. On success, skip browser and poll directly. On failure, print the reason and drop into the current browser flow verbatim.

**Files:**
- Modify: `src/atcoder.rs`

**Interfaces:** No new public API; changes internal control flow of the existing `pub fn submit(url, language, source)`.

- [ ] **Step 1: Rewrite `submit()`**

Replace the current `pub fn submit` (starts around line 244) with:

```rust
pub fn submit(url: String, language: String, source: String) {
    let mut client = AtcoderClient::new();

    if let Err(e) = client.login() {
        eprintln!("{}", e);
        return;
    }

    let (contest_id, task_id) = match parse_url(&url) {
        Some(parsed) => parsed,
        None => {
            eprintln!("Could not parse URL: {}", url);
            return;
        }
    };

    // Record existing submissions before we submit — used by poll_verdict
    // to detect the new one whether it came via direct POST or via browser.
    let mut known_ids = std::collections::HashSet::new();
    if let Ok(body) = client.get_page(&format!("/contests/{}/submissions/me", contest_id)) {
        let sub_re = Regex::new(r"/submissions/(\d+)").unwrap();
        for cap in sub_re.captures_iter(&body) {
            known_ids.insert(cap[1].to_string());
        }
    }

    // Try the direct HTTPS POST first. Any failure = fall back to browser.
    println!("Submitting...");
    match client.try_direct_submit(&contest_id, &task_id, &language, &source) {
        Ok(()) => {
            // Direct submit succeeded — no browser needed, no clipboard needed.
            if let Err(e) = client.poll_verdict(&contest_id, &known_ids, || {}) {
                eprintln!("Verdict polling failed: {}", e);
            }
            return;
        }
        Err(reason) => {
            eprintln!("Direct submission failed ({}), opening browser instead.", reason);
        }
    }

    // Fallback: existing browser handoff flow.
    let mut ctx: ClipboardContext = ClipboardProvider::new().unwrap();
    ctx.set_contents(source.clone()).unwrap();

    let submit_url = if task_id.is_empty() {
        format!("https://atcoder.jp/contests/{}/submit", contest_id)
    } else {
        format!(
            "https://atcoder.jp/contests/{}/submit?taskScreenName={}",
            contest_id, task_id
        )
    };
    println!("Source code copied to clipboard");
    let handoff = crate::extbridge::publish(
        crate::extbridge::Job {
            site: "atcoder",
            url: submit_url.clone(),
            language,
            source,
        },
        std::time::Duration::from_secs(60),
    ).ok();
    let url_to_open = match handoff.as_ref() {
        Some(h) => format!("{}#submitter={}:{}", submit_url, h.port, h.token),
        None => submit_url.clone(),
    };
    open::that_detached(&url_to_open).ok();

    if let Err(e) = client.poll_verdict(&contest_id, &known_ids, || {
        if let Some(h) = handoff.as_ref() {
            h.signal_close();
        }
    }) {
        eprintln!("Verdict polling failed: {}", e);
    }
}
```

- [ ] **Step 2: Build + run all tests**

```bash
cargo build 2>&1 | tail -5
cargo test 2>&1 | tail -10
```

Expected: clean build; all tests pass; no dead-code warnings for `try_direct_submit`.

- [ ] **Step 3: Smoke-check the fallback path manually**

Since no live-network test is planned, verify the fallback wiring by hand:

```bash
# Point at a bogus URL so parse_url succeeds but the site GET will 404/fail.
# We're just checking the printed messages come out in the right order —
# don't actually submit.
```

Skip this step if you don't have a convenient way to trigger a failure. The unit tests + build cover the mechanical wiring; end-to-end confidence comes from the user's contest testing.

- [ ] **Step 4: Commit**

```bash
git add src/atcoder.rs
git commit -m "$(cat <<'EOF'
(feat) AtCoder: try direct HTTPS POST first, fall back to browser on any failure

atcoder::submit now tries a direct submission via HttpClient before
falling back to the existing browser-extension handoff. On success,
no browser tab opens and no clipboard copy happens. On any failure
— network, HTTP, parse, unmatched language — we print the reason
and drop into the current browser flow verbatim.

Design: docs/superpowers/specs/2026-07-25-atcoder-direct-submit-design.md

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>
EOF
)"
```

---

## Verification checklist (run before declaring the branch done)

- [ ] `cargo build` clean, no warnings.
- [ ] `cargo test` all pass — new `langmatch` tests + new AtCoder helper tests + existing tests.
- [ ] Manual smoke test: run `cargo run -- https://atcoder.jp/contests/<some-contest>/tasks/<some-task> cpp <file>.cpp` against a live AtCoder session where you have logged in via the CLI cookie paste. Verify one of:
  - Direct path: prints `Submitting...` → submission URL → verdict, no browser tab opens.
  - Fallback path: prints `Submitting...` → `Direct submission failed (…), opening browser instead.` → existing browser flow.

- [ ] Extension side still works (Codeforces / Luogu unaffected; AtCoder browser-fallback still works when direct fails).

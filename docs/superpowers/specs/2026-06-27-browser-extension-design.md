# Browser extension for AtCoder, Codeforces, and Luogu submissions

**Date:** 2026-06-27
**Status:** Design approved, implementation pending
**Scope:** Chrome + Firefox (Manifest V3) extension that automates the paste+select-language+click-submit step for the three judges that today require a browser tab.

## 1. Motivation and overall flow

Three of submitter's supported judges (AtCoder, Codeforces, Luogu) don't have a usable submit API for non-privileged users, so today submitter:

1. Copies the source to the clipboard.
2. Opens the submit URL in the default browser.
3. The user manually pastes, selects the language, and clicks Submit.
4. Submitter polls the site for the new submission and prints the verdict.

The extension automates step 3 without changing 1, 2, or 4. The browser still opens; if the extension is missing or fails for any reason, the user pastes manually exactly as today.

### Flow (with the extension installed)

```
submitter                                       browser + extension
─────────                                       ───────────────────
1. build Job{site, url, language, source}
2. extbridge::publish(job)
       ─ binds 127.0.0.1:0 (OS picks port)
       ─ generates 128-bit token
       ─ stores Job in memory under that token (TTL 30s, single-use)
       ─ returns Handoff{port, token}
3. open::that("<submit_url>#submitter=<port>:<token>")
                                                4. browser loads submit page
                                                5. content script activator.js
                                                   reads #submitter=…, then
                                                   history.replaceState clears it
                                                6. fetch http://127.0.0.1:<port>/job/<token>
                                                   ─ submitter serves Job once, then 404s
                                                7. activator.js dispatches to
                                                   atcoder.js / codeforces.js / luogu.js
                                                8. site script waits (MutationObserver,
                                                   up to 10s) for the form to render
                                                   — handles Cloudflare interstitial
                                                9. pastes source into editor
                                               10. selects language (per-site map);
                                                   if no match → banner, no click
                                               11. clicks Submit
12. polls site (unchanged from today)          12.   ↑ submitter never knew the
                                                       extension existed
```

### Invariants

- Submitter never depends on the extension. Every code path that exists today still works.
- One job per token, single-use, 30s TTL. Token has 128 bits of entropy.
- No changes to verdict polling on any site.
- Token lives in the URL fragment, never in query string. Fragment is scrubbed via `history.replaceState` before any other script can read it.

## 2. Submitter side — `src/extbridge.rs`

New module. Roughly 100 lines, hand-rolled HTTP/1.1 on `std::net::TcpListener`, no tokio/axum.

### Public API

```rust
pub struct Job {
    pub site: &'static str,   // "atcoder" | "codeforces" | "luogu"
    pub url: String,          // the submit URL submitter would open today
    pub language: String,     // language name as submitter knows it
    pub source: String,       // file contents
}

pub struct Handoff {
    pub port: u16,
    pub token: String,        // hex-encoded 128-bit value
}

/// Spawn a short-lived loopback HTTP server bound to 127.0.0.1:0.
/// The job is served exactly once and then the server shuts down.
/// Returns Err if the port can't be bound; callers should fall back to
/// the existing clipboard-only flow.
pub fn publish(job: Job, ttl: Duration) -> Result<Handoff, String>;
```

### Single endpoint

`GET /job/<token>` → `200 application/json` with `{site, url, language, source}` on first hit; `404` on subsequent hits or after TTL expires. No other paths, no POST, no CORS headers (intentional — see security).

### Per-site wiring

About five lines per file. Example for AtCoder (analogous changes in `codeforces.rs` and `luogu.rs`):

```rust
// before open::that(&submit_url)
let url_to_open = match extbridge::publish(
    extbridge::Job { site: "atcoder", url: submit_url.clone(), language, source: source.clone() },
    Duration::from_secs(30),
) {
    Ok(h) => format!("{}#submitter={}:{}", submit_url, h.port, h.token),
    Err(_) => submit_url.clone(),
};
open::that(&url_to_open).ok();
```

Clipboard copy and verdict polling are unchanged.

### Tests

- Unit: token entropy, TTL expiry, single-use semantics, JSON shape.
- Integration: spawn `publish()`, hit the loopback endpoint with `reqwest`, assert round-trip.

## 3. Extension structure

Manifest V3, single codebase, works on Chrome and Firefox 109+.

### Layout

```
extension/
  manifest.json
  content/
    activator.js       # runs on the three submit URLs; reads fragment, fetches job, dispatches
    atcoder.js         # window.__submitterFill: ACE editor + language <select> + submit button
    codeforces.js      # window.__submitterFill: plain textarea + language <select> + submit button
    luogu.js           # window.__submitterFill: Vue component (hidden textarea) + select + button
  shared/
    languages.js       # per-site language matchers (substring/regex)
    notify.js          # in-page banner helper (used for Cloudflare wait, language miss, etc.)
  icons/
    icon-{16,32,48,128}.png
```

No background service worker — all logic lives in content scripts.

### manifest.json (sketch)

```json
{
  "manifest_version": 3,
  "name": "Submitter Helper",
  "version": "0.1.0",
  "description": "Automates paste+submit for AtCoder, Codeforces, Luogu when launched by submitter.",
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
      "js": ["shared/notify.js", "shared/languages.js",
             "content/atcoder.js", "content/codeforces.js", "content/luogu.js",
             "content/activator.js"],
      "run_at": "document_idle"
    }
  ],
  "browser_specific_settings": {
    "gecko": { "id": "submitter-helper@egork.net", "strict_min_version": "109.0" }
  }
}
```

### activator.js (sketch)

```js
(async () => {
  const m = location.hash.match(/(?:^|[#&])submitter=(\d+):([0-9a-f]+)/i);
  if (!m) return;
  const [, port, token] = m;
  history.replaceState(null, '', location.pathname + location.search);
  let job;
  try {
    const r = await fetch(`http://127.0.0.1:${port}/job/${token}`);
    if (!r.ok) throw new Error(`HTTP ${r.status}`);
    job = await r.json();
  } catch (e) {
    notify(`Submitter: couldn't reach helper (${e.message}). Paste from clipboard.`);
    return;
  }
  if (typeof window.__submitterFill !== 'function') {
    notify('Submitter: no filler for this page. Paste from clipboard.');
    return;
  }
  try { await window.__submitterFill(job); }
  catch (e) { notify(`Submitter: ${e.message}. Paste from clipboard.`); }
})();
```

### Per-site fillers

Each defines a single `window.__submitterFill = async (job) => { … }`. The site dispatch is implicit — only one of the three site scripts matches the current host, so only that one's `__submitterFill` is defined when activator runs.

Each filler:

1. Uses MutationObserver (up to 10s) to wait for the editor + language select + submit button. Survives Cloudflare interstitial.
2. Pastes the source into the editor (AtCoder: `ace.edit(...).setValue`; Codeforces: textarea + dispatch input event; Luogu: hidden textarea + dispatch input + framework reflow).
3. Looks up the language in `shared/languages.js`'s per-site map. If no match, calls `notify(...)` and returns without clicking — source is already in the editor.
4. Clicks Submit.

### Language matching

`shared/languages.js` exports:

```js
const LANGUAGES = {
  atcoder: [
    { name: "C++17 (gcc 9.2.1)", match: ["c++17", "gcc 9.2"] },
    { name: "Python (3.8.2)",    match: [/^python\s*3/i] },
    // ...
  ],
  codeforces: [ /* ... */ ],
  luogu: [ /* ... */ ],
};
```

`match` entries can be substrings (case-insensitive) or regexes. The filler picks the first option whose `name` exactly equals an option in the page's `<select>`. Maintained by hand; adding a new language is one entry.

## 4. Edge cases, security, distribution, testing

### Edge cases

| Case | Behavior |
|---|---|
| Extension not installed | Submitter never knows; user pastes from clipboard. |
| Port-bind fails | `publish()` returns Err, submitter opens the plain URL without a fragment. |
| Token expired before extension fetches (>30s) | Server returns 404; banner; clipboard fallback. |
| Cloudflare challenge | MutationObserver waits 10s for the form. If still not present, banner. |
| Language not in dropdown | Source is pasted, banner names the requested language, user picks and clicks. |
| User opens the submit page in a second tab | Fragment was stripped in tab 1; tab 2 has no fragment → no-op. |
| Two concurrent `submitter` invocations | Independent port + token per run. |
| Submitter dies between `open::that` and fetch | Server thread dies with it; fetch fails → banner + manual paste. |

### Security model

- Server binds `127.0.0.1` only — never `0.0.0.0`.
- Token = 128 random bits, hex-encoded, single-use, 30s TTL.
- Token travels in the URL fragment, so it's not in HTTP referer, not in server logs of the target site, not in browser history (we `replaceState` to clear `history.state` too).
- No CORS headers on the response. A malicious page that guesses port+token can't read the response (browser blocks the read). The extension reads fine because `host_permissions` exempts it from CORS.
- Source code lives in submitter's memory briefly. Never written to disk.

### Distribution (staged)

**Stage 1 — unpacked zip:**
- Add a `pack-extension` step to `.github/workflows/release.yml` that zips `extension/` into `submitter-extension-<version>.zip` and uploads it to the GitHub release alongside the existing binaries.
- README gets a "Browser extension (optional, for AtCoder/Codeforces/Luogu)" section with install instructions for Chrome (`chrome://extensions` → Developer mode → Load unpacked) and Firefox (`about:debugging` → Load Temporary Add-on, or `.xpi` via `web-ext build`).

**Stage 2 — stores (once stable):**
- Chrome Web Store: $5 one-time dev account, MV3 ZIP, 1–3 day review.
- Firefox AMO: free, `.xpi` upload, auto-signed.
- README swaps unpacked instructions for store links.

Extension source lives in a top-level `extension/` directory inside the submitter repo. Versioned in lockstep with submitter for Stage 1 (zip artifact rides the same release). May diverge once stores are involved.

### Testing

- **Submitter (Rust):** unit-test `extbridge` token generator, TTL expiry, single-use, JSON shape. Integration test round-trips a publish + reqwest fetch.
- **Extension (JS):** `shared/languages.js` matchers are pure — can be tested with `node --test` if we add one. Otherwise: manual smoke checklist (one solution to each of AtCoder, Codeforces, Luogu using a real account) before tagging each release.
- No CI for browser orchestration — too much overhead for the scope.

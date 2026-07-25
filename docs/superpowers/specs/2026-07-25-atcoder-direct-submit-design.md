# AtCoder direct-submit with browser fallback

**Status:** Draft — 2026-07-25
**Related work:** browser-extension design (`2026-06-27-browser-extension-design.md`), which established the current always-browser flow for AtCoder / Codeforces / Luogu.

## Motivation

AtCoder does not always sit behind Cloudflare — outside of contest start and a few high-load windows, the plain HTTPS API accepts submissions from any well-behaved client. The current always-browser flow spawns a submit tab even when a direct POST would have worked, adding a few seconds of latency and requiring the browser extension to be installed at all.

Goal: submit directly from the CLI when we can, and fall back to the existing browser handoff whenever the direct path fails for any reason.

Non-goals:
- Changing Codeforces (always CF-walled — browser stays the only path).
- Changing Luogu in this iteration. Luogu is a candidate for the same treatment later; this spec deliberately covers AtCoder only so we can validate the fallback pattern on one site first.
- Detecting Cloudflare specifically. We treat *any* failure of the direct path as a fallback trigger.

## Overview

`atcoder::submit(url, language, source)` gains a new prelude before the existing browser flow:

1. Log in (unchanged — prompts for pasted cookies if the CLI has none).
2. Snapshot known submission IDs (unchanged).
3. **Attempt direct submission via HTTPS POST.** On success, poll the verdict as we do today and skip the browser entirely. On any failure (network, HTTP, parse, unmatched language), print a one-line reason and fall through to the current browser handoff flow, which is left unchanged.

The user sees one of two paths:

```
$ submitter https://atcoder.jp/... rust solution.rs
Submitting...
Submission url: https://atcoder.jp/contests/abc463/submissions/12345678
AC (1'234'567)
```

```
$ submitter https://atcoder.jp/... rust solution.rs
Submitting...
Direct submission failed (POST returned 403), opening browser instead.
Source code copied to clipboard
[browser tab opens, extension pastes+submits, CLI polls]
```

## Failure classification (the "when to fall back" contract)

Any of the following returns `Err(reason)` from `try_direct_submit`, which is the sole trigger for falling back to the browser flow:

| Stage | Trigger |
|---|---|
| GET submit page | non-2xx status; network error / timeout after `HttpClient::send_with_retry` exhausts; CSRF regex missed; `<select name="data.LanguageId">` missing or empty |
| Language matching | matcher returns `None` for the requested language string |
| POST submit | non-3xx status; network error / timeout after retries; `Location` header missing; `Location` points to `/login` (session died); `Location` does not contain `/submissions` |

We deliberately do **not** try to distinguish "Cloudflare wall" from "AtCoder returned a validation error" from "AtCoder is down for 20 seconds." From the user's perspective they all mean "CLI didn't work this time, use the browser." Doing more granular classification adds complexity for zero UX benefit — the browser is a working answer to all three.

We do **not** wrap `try_direct_submit` in additional retry logic. `HttpClient::send_with_retry` already handles transient connect/timeout errors. If those exhaust, that surfaces as `Err` and we fall back to browser — which itself gives the user a working path.

## Direct-submit flow

`try_direct_submit(contest_id, task_id, language, source) -> Result<(), String>`:

0. If `task_id` is empty (user passed a bare contest URL, e.g. `/contests/abc463`), return `Err("no task in URL")` immediately. The browser flow can still handle this (user picks a task on the page); the direct POST cannot.
1. GET `https://atcoder.jp/contests/{contest_id}/submit`.
2. Regex-extract `csrf_token` from the response body:
   `<input[^>]*name="csrf_token"[^>]*value="([^"]+)"`.
3. Parse the language `<select>`:
   - Locate `<select[^>]*name="data.LanguageId"[^>]*>...</select>`.
   - Extract each `<option value="ID">TEXT</option>` pair into `Vec<(String, String)>`.
4. Call `langmatch::pick_option("atcoder", &language, &options)`. If `None`, return `Err("no matching language")`.
5. POST `https://atcoder.jp/contests/{contest_id}/submit` form-encoded, body:
   - `csrf_token=<extracted>`
   - `data.TaskScreenName=<task_id>`
   - `data.LanguageId=<matched id>`
   - `sourceCode=<source>`
6. Success = status 302 with `Location` header containing `/submissions`. Anything else = `Err`.

On success, return `Ok(())`. The caller then falls into the existing `poll_verdict` loop, which watches for a new submission ID not in `known_ids` — same as today's browser-flow polling. AtCoder's POST 302 goes to `/contests/{id}/submissions/me`, not to a specific submission, so we do not gain the submission ID from the POST. `poll_verdict` picks it up on the first poll after the POST.

## Language matcher (`src/langmatch.rs`)

New module. Rust port of `pickOption` from `extension/shared/languages.js`, and of the per-site tables next to it. Shape:

```rust
pub fn pick_option(site: &str, requested: &str, options: &[(String, String)]) -> Option<String>
```

`options` is `(value, display_text)` per `<option>` — the `value` is what we POST back, the `display_text` is what we match against. `site` selects which table to use (`"atcoder"`, `"codeforces"`, `"luogu"`); the site parameter mirrors the JS API so the Luogu extension of this feature drops in cleanly. Returns the winning `value` (not index, not the text).

Algorithm — behaviour-equivalent to the JS `pickOption`:

1. Look up the site's entry list. Each entry is `{when: [pattern...], options: /regex/}`, ordered specific-to-general (e.g. `c++17` before bare `cpp`, PyPy before Python).
2. Find the first entry whose `when` list matches `requested` (case-insensitive substring or regex).
3. Filter `options` down to those whose `display_text` matches the entry's `options` regex.
4. If zero: `None`. If one: return its value. If more than one, pick the highest by **version-tuple comparison**: extract every run of digits from `display_text` in order into a `Vec<u32>`, then lex-compare tuples (shorter tuple padded with zeros). If no tuples have any digits, fall back to last-in-document-order — same as JS.
5. Return the winning `value`.

Placement: a fresh `src/langmatch.rs` (not inlined into `atcoder.rs`) because (a) Luogu's later direct-submit work will reuse it, and (b) it makes unit-testing trivial.

Source-of-truth note: the JS table stays canonical for the browser flow, the Rust table is a manual port. When we add/update a language pattern we update both. That's a low-frequency edit (contests add new compilers rarely) and both tables are ~15 lines each; a lint or shared config isn't worth the machinery.

## HTTP hygiene

The current `AtcoderClient` builds an ad-hoc `reqwest::blocking::Client` inside `get_page` with a real-browser `User-Agent`, while `self.http` (an `HttpClient`) sends reqwest's default UA (`reqwest/x.y.z`) — a red flag AtCoder could easily filter on. As part of this work:

- `AtcoderClient::new` sets `self.http.set_header("User-Agent", USER_AGENT)` using the same `Mozilla/5.0…` constant already at the top of the file.
- `get_page` is rewritten to use `self.http.get_text` instead of its own reqwest client. Same UA, same cookies, redirect handling comes from `HttpClient::follow_redirect`.
- Direct-submit GET + POST go through the same `self.http`.

Result: one HTTP client, one UA, one cookie store, consistently applied.

## Error output and UX

Success path: unchanged output, minus the "Source code copied to clipboard" line — we don't need to copy to clipboard when we submitted directly. First line printed is `Submitting...` (immediately before the direct POST), replaced by the submission URL once `poll_verdict` finds the new submission.

Fallback path: prints exactly one extra line before dropping into the current browser flow:
```
Direct submission failed (<reason>), opening browser instead.
```
where `<reason>` is the short `Err` string from `try_direct_submit` (e.g. `POST returned 403`, `no matching language`, `network error after 5 retries`). Then the existing flow runs verbatim — "Source code copied to clipboard", extension handoff, browser tab, poll.

Rationale: we want the user to know *why* the fallback happened when it does, but we don't want to spam them in the happy path.

## Files touched

- `src/atcoder.rs` — add `try_direct_submit`, `parse_csrf`, `parse_language_options`, `classify_submit_response`; wire into `submit()`; drop ad-hoc reqwest client in `get_page`; set UA on `self.http`.
- `src/langmatch.rs` — new file (~80 LoC), port of the JS matcher.
- `src/lib.rs` — add `pub mod langmatch;`.
- `src/http.rs` — add `post_form_raw` (POST without following the 302). We need to inspect `Location` on the POST response to distinguish "redirected to /submissions" (success) from "redirected to /login" (session died). The existing `post_form` auto-follows the redirect via `follow_redirect`, which loses the 302 signal.
- Extension side: unchanged.

## Testing

Unit tests, no live network. We test the pure functions that do the parsing and matching, and the response-shape classifier. We do **not** mock `HttpClient` — the flow is a thin composition of those pure functions plus HTTP calls, and end-to-end validation happens by hand during contest testing.

- `langmatch::pick_option`:
  - `("atcoder", "cpp", ...)` against a realistic option list picks the option with the highest version tuple.
  - `("atcoder", "c++17", ...)` picks the C++17 option even when C++20/23 are also present.
  - `("atcoder", "rust", ...)` against a list with only one Rust option picks it.
  - `("atcoder", "haskell", ...)` against a list without Haskell returns `None`.
  - `("atcoder", "", ...)` returns `None`.
  - `("codeforces", "cpp", ...)` and `("luogu", "cpp", ...)` — one smoke test per site so the table wiring is covered.
- `parse_csrf(html) -> Option<String>` on a saved AtCoder submit-page HTML snippet — success + a variant with no CSRF.
- `parse_language_options(html) -> Vec<(String, String)>` on the same snippet — asserts count and a couple of specific pairs.
- `classify_submit_response(status, location) -> Result<(), String>` (the tiny helper that decides success from the POST response):
  - `(302, Some("/contests/abc463/submissions/me"))` → `Ok`
  - `(302, Some("/login"))` → `Err`
  - `(302, None)` → `Err`
  - `(200, _)` → `Err` (form re-rendered = validation failed)
  - `(403, _)` → `Err`

No live-AtCoder integration test — same policy as the existing browser-flow code.

## Rollout

Single commit, merged directly to `main`. No feature flag: the fallback IS the safety net.

If direct-submit turns out to be unreliable in ways this design didn't anticipate, the fix is to tighten failure classification and/or fall back more aggressively — the browser path is always available. Rollback path (if truly needed) is a one-line change: skip the `try_direct_submit` call and go straight to the browser flow.

After a week or two of contest testing, we decide whether to extend the same pattern to Luogu.

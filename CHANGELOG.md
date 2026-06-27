# Changelog

All notable changes to this project are documented here.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added
- AtCoder verdict line now appends the score column for accepted
  submissions, with digits grouped by apostrophe (e.g.
  `Accepted (1'234'567)`).

## [1.1] - 2026-06-09

### Added
- Repovive (`repovive.com`) support: cookie-import login, JSON submit
  API, polling, language-ID table. Short aliases: `repovive` / `rv`.
  Browser-mimicking headers added so requests get past Repovive's WAF.
- README install instructions for the pre-built `.deb` / `.dmg` / `.exe`
  binaries published to GitHub releases.

### Changed
- Daily integration test workflow now runs every judge regardless of
  earlier failures and reports a per-judge pass/fail summary, instead
  of stopping at the first failed step.
- Standardized user-facing messages across every judge module:
  uniform "Could not parse URL: …", "Language: …", and
  EditThisCookie paste prompts.

## [1.0] - 2026-05-19

First stable release.

### Added
- Submit + verdict-polling support for AtCoder, Codeforces, CodeChef,
  Yandex Contest, UOJ, Universal Cup, Toph, Kattis, Eolymp, Luogu,
  CodeRun, and KEP.uz.
- `submitter login <site>` for pre-contest authentication.
- Pre-built `.deb`, `.dmg` (aarch64), and `.exe` binaries published
  via a manual GitHub Actions release workflow.

[Unreleased]: https://github.com/EgorKulikov/submitter/compare/v1.1...HEAD
[1.1]: https://github.com/EgorKulikov/submitter/compare/v1.0...v1.1
[1.0]: https://github.com/EgorKulikov/submitter/releases/tag/v1.0

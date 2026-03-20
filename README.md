# submitter

Tool to submit solutions to online judges directly from the command line.

## Prerequisites

You need [Rust](https://www.rust-lang.org/tools/install).

## Installation

```
cargo install --git https://github.com/EgorKulikov/submitter
```

## Usage

```
submitter <task url> <language> <path to solution>
submitter login <site>
```

Credentials are prompted on first use and saved for subsequent runs in `.submitter_cookies.json`.

### Pre-contest login

Use `submitter login <site>` to authenticate before a contest starts, so you don't waste time during the contest:

```bash
submitter login ucup
submitter login atcoder
submitter login codeforces
submitter login eolymp
submitter login yandex
```

Accepted short names: `ac`, `atcoder`, `cf`, `codeforces`, `cc`, `codechef`, `ucup`, `uoj`, `qoj`, `yandex`, `ya`,
`toph`, `kattis`, `eolymp`, `eol`. Full URLs also work.

## Supported sites

| Site                                                                            | Submit                                        | Verdict      | Auth                            |
|---------------------------------------------------------------------------------|-----------------------------------------------|--------------|---------------------------------|
| [AtCoder](https://atcoder.jp)                                                   | via browser (opens page, copies to clipboard) | HTTP polling | browser cookie (EditThisCookie) |
| [Codeforces](https://codeforces.com)                                            | via browser (opens page, copies to clipboard) | API polling  | API key + secret                |
| [CodeChef](https://codechef.com)                                                | API                                           | API          | username + password             |
| [Yandex Contest](https://contest.yandex.com)                                    | API                                           | API          | OAuth (device code flow)        |
| [UOJ](https://uoj.ac) / [UCup](https://contest.ucup.ac) / [QOJ](https://qoj.ac) | HTTP                                          | HTTP         | username + password             |
| [Toph](https://toph.co)                                                         | API                                           | API          | username + password             |
| [Kattis](https://open.kattis.com)                                               | API                                           | API          | username + token (.kattisrc)    |
| [Eolymp](https://eolymp.com)                                                    | API                                           | API          | API key                         |

### Notes

- **AtCoder**: on first use, export cookies from browser using [EditThisCookie](https://www.editthiscookie.com/)
  extension and paste the JSON when prompted
- **Codeforces**: requires API key and secret from https://codeforces.com/settings/api
- **Yandex Contest**: on first use, opens browser to authorize via Yandex account
- **Kattis**: download your `.kattisrc` from https://open.kattis.com/download/kattisrc and place it in your project or
  home directory
- **Eolymp**: requires API key from https://eolymp.com/developer with scopes: `atlas:problem:read`,
  `atlas:submission:read`, `atlas:submission:write`, `judge:contest:read`, `judge:contest:participate`

## Examples

```bash
# AtCoder
submitter "https://atcoder.jp/contests/abc388/tasks/abc388_a" "Rust" solution.rs

# Codeforces
submitter "https://codeforces.com/contest/1/problem/A" "C++" solution.cpp

# CodeChef
submitter "https://www.codechef.com/problems/TEST" "C++" solution.cpp

# Yandex Contest
submitter "https://contest.yandex.com/contest/3/problems/B/" "C++" solution.cpp

# UOJ
submitter "https://uoj.ac/problem/1" "C++14" solution.cpp

# UCup
submitter "https://contest.ucup.ac/contest/1106/problem/A" "C++17" solution.cpp

# QOJ
submitter "https://qoj.ac/problem/1" "C++14" solution.cpp

# Toph
submitter "https://toph.co/p/add-them-up" "C++" solution.cpp

# Kattis
submitter "https://open.kattis.com/problems/hello" "C++" solution.cpp

# Eolymp (archive)
submitter "https://eolymp.com/en/problems/1" "C++" solution.cpp

# Eolymp (contest)
submitter "https://eolymp.com/en/contests/CONTEST_ID/problems/A" "C++" solution.cpp
```

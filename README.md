# submitter

Tool to submit to online judges dirrectly from command line

## Prerequisites

You would need [rust](https://www.rust-lang.org/tools/install) and [docker](https://docs.docker.com/desktop/)

## Installation

```
cargo install --git https://github.com/EgorKulikov/submitter
```

## Usage

```
submitter <task url> <language> <path to solution>
```

## Supported sites

At the moment the following is supported:

- Codeforces
- Codechef
- Yandex Contest
- AtCoder
- Universal Cup
- Toph*

*no support for specifying language, language of the last submit is used

Luogu support is discontinued due to cloudflare captcha

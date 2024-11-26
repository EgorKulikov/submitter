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

- Codefoces
- Codechef
- Yandex Contest
- AtCoder
- Universal Cup
- Luogu (no support for changing language, language of the last submit is used)

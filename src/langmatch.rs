// src/langmatch.rs
// Site-parameterised language matcher. Port of pickOption from
// extension/shared/languages.js. Keep the two in sync when adding languages.

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

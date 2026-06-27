'use strict';
// Per-site language matchers. Each entry: { name, match }
// - `name` is the exact label the page's <select> uses.
// - `match` is an array of substrings (case-insensitive) or RegExp objects.
// `pickLanguage(site, requested)` returns the first entry whose match[] contains
// a substring/regex that matches `requested`, or null if nothing matches.
//
// Real entries land in tasks 6/7/8 — this stub keeps activator.js loadable.
window.__submitterLanguages = {
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

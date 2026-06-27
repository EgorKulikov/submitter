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

'use strict';
// Per-site language matchers.
// Each entry: { when, options }
//   `when`    – array of substrings (case-insensitive) or RegExp that match the USER's language string.
//   `options` – substring or RegExp that matches a DROPDOWN OPTION's text.
// Entries ordered specific-to-general; the picker takes the first `when` match.
window.__submitterLanguages = {
  atcoder: [
    // Specific C++ standards first
    { when: [/^c\+\+\s*23\b/i], options: /c\+\+\s*23/i },
    { when: [/^c\+\+\s*20\b/i], options: /c\+\+\s*20/i },
    { when: [/^c\+\+\s*17\b/i], options: /c\+\+\s*17/i },
    { when: [/^c\+\+\s*14\b/i], options: /c\+\+\s*14/i },
    // Bare "c++" / "g++" → newest C++ available
    { when: [/^c\+\+$/i, /^g\+\+$/i, "cpp"], options: /^c\+\+/i },
    // PyPy before Python (otherwise /python/i could match PyPy)
    { when: [/^pypy/i], options: /pypy/i },
    // Python 3 only (we never want to silently submit as Python 2)
    { when: [/^python\s*3/i, /^python$/i, "cpython"], options: /python.*3|^python\s*\(3/i },
    // Rust editions first, bare rust last
    { when: [/^rust\s*2024/i, "rust2024"], options: /rust\s*2024/i },
    { when: [/^rust\s*2021/i, "rust2021"], options: /rust\s*2021/i },
    { when: [/^rust/i], options: /rust/i },
    { when: [/^java/i], options: /^java/i },
    { when: [/^kotlin/i], options: /kotlin/i },
    { when: [/^go$/i, "golang"], options: /^go\b/i },
    { when: [/^c\#/i, "csharp", ".net"], options: /c\#|csharp|\.net/i },
  ],
  codeforces: [
    { when: [/^c\+\+\s*23\b/i], options: /c\+\+23/i },
    { when: [/^c\+\+\s*20\b/i], options: /c\+\+20/i },
    { when: [/^c\+\+\s*17\b/i], options: /c\+\+17/i },
    { when: [/^c\+\+$/i, /^g\+\+$/i, "cpp"], options: /^gnu g\+\+|^c\+\+/i },
    { when: [/^pypy/i], options: /pypy/i },
    { when: [/^python\s*3/i, /^python$/i, "cpython"], options: /^python\s*3/i },
    { when: [/^rust\s*2024/i, "rust2024"], options: /rust\s*2024/i },
    { when: [/^rust\s*2021/i, "rust2021"], options: /rust\s*2021/i },
    { when: [/^rust/i], options: /^rust/i },
    { when: [/^java/i], options: /^java/i },
    { when: [/^kotlin/i], options: /^kotlin/i },
    { when: [/^go$/i, "golang"], options: /^go\s/i },
    { when: [/^c\#/i, "csharp", ".net"], options: /c\#|csharp|\.net/i },
  ],
  luogu: [
    { when: [/^c\+\+\s*20\b/i], options: /c\+\+\s*20/i },
    { when: [/^c\+\+\s*17\b/i], options: /c\+\+\s*17/i },
    { when: [/^c\+\+\s*14\b/i], options: /c\+\+\s*14/i },
    { when: [/^c\+\+$/i, /^g\+\+$/i, "cpp"], options: /^c\+\+/i },
    { when: [/^pypy/i], options: /pypy/i },
    { when: [/^python\s*3/i, /^python$/i, "cpython"], options: /python.*3/i },
    { when: [/^rust/i], options: /rust/i },
    { when: [/^java/i], options: /^java/i },
    { when: [/^kotlin/i], options: /kotlin/i },
    { when: [/^go$/i, "golang"], options: /^go\s/i },
  ],
};

window.__submitterPickOption = function pickOption(site, requested, optionTexts) {
  const list = window.__submitterLanguages[site] || [];
  const needle = String(requested || '').trim();
  if (!needle) return null;
  const entry = list.find(e => (e.when || []).some(w =>
    w instanceof RegExp ? w.test(needle) : needle.toLowerCase().includes(String(w).toLowerCase())
  ));
  if (!entry) return null;
  const opt = entry.options;
  const matches = []; // [{index, text}]
  for (let i = 0; i < optionTexts.length; i++) {
    const t = optionTexts[i];
    const hit = opt instanceof RegExp ? opt.test(t) : t.toLowerCase().includes(String(opt).toLowerCase());
    if (hit) matches.push({ index: i, text: t });
  }
  if (matches.length === 0) return null;
  if (matches.length === 1) return matches[0].index;
  // Pick latest by version-tuple comparison; fall back to last document-order if no numbers.
  const tupleOf = t => {
    const nums = [];
    const re = /\d+/g;
    let m;
    while ((m = re.exec(t)) !== null) nums.push(parseInt(m[0], 10));
    return nums;
  };
  const cmp = (a, b) => {
    const n = Math.max(a.length, b.length);
    for (let i = 0; i < n; i++) {
      const ai = a[i] !== undefined ? a[i] : 0, bi = b[i] !== undefined ? b[i] : 0;
      if (ai !== bi) return ai - bi;
    }
    return 0;
  };
  let best = matches[matches.length - 1]; // default: last in document order
  let bestTuple = tupleOf(best.text);
  for (const m of matches) {
    const t = tupleOf(m.text);
    if (t.length === 0) continue;
    if (bestTuple.length === 0 || cmp(t, bestTuple) > 0) {
      best = m;
      bestTuple = t;
    }
  }
  return best.index;
};

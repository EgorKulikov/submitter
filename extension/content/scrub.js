'use strict';
(function () {
  const KEY = '__submitterHandoff';
  const re = /(?:^|[#&])submitter=(\d{1,5}):([0-9a-f]{32})(?:&|$)/i;
  const m = location.hash.match(re);
  if (m) {
    const handoff = { port: m[1], token: m[2].toLowerCase() };
    window.__submitterHandoff = handoff;
    try { sessionStorage.setItem(KEY, JSON.stringify(handoff)); } catch (_) {}
    history.replaceState(null, '', location.pathname + location.search);
    return;
  }
  // No fragment on this page — see if a prior page load stashed one.
  try {
    const raw = sessionStorage.getItem(KEY);
    if (!raw) return;
    const handoff = JSON.parse(raw);
    if (handoff && handoff.port && handoff.token) {
      window.__submitterHandoff = handoff;
    }
  } catch (_) {}
})();

'use strict';
(function () {
  const re = /(?:^|[#&])submitter=(\d{1,5}):([0-9a-f]{32})(?:&|$)/i;
  const m = location.hash.match(re);
  if (!m) return;
  window.__submitterHandoff = { port: m[1], token: m[2].toLowerCase() };
  history.replaceState(null, '', location.pathname + location.search);
})();

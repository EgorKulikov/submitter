(async function () {
  const re = /(?:^|[#&])submitter=(\d{1,5}):([0-9a-f]{32})(?:&|$)/i;
  const m = location.hash.match(re);
  if (!m) return;

  const port = m[1];
  const token = m[2].toLowerCase();

  // Scrub the fragment before any other script can read it.
  history.replaceState(null, '', location.pathname + location.search);

  let job;
  try {
    const r = await fetch(`http://127.0.0.1:${port}/job/${token}`);
    if (!r.ok) throw new Error(`HTTP ${r.status}`);
    job = await r.json();
  } catch (e) {
    window.notify(`couldn't reach helper (${e.message}). Paste from clipboard.`);
    return;
  }

  if (typeof window.__submitterFill !== 'function') {
    window.notify(`no filler for this site yet. Paste from clipboard.`);
    return;
  }

  try {
    await window.__submitterFill(job);
  } catch (e) {
    window.notify(`${e.message}. Paste from clipboard.`);
  }
})();

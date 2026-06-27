'use strict';
(async function () {
  const handoff = window.__submitterHandoff;
  if (!handoff) return;
  const { port, token } = handoff;

  let job;
  try {
    const r = await fetch(`http://127.0.0.1:${port}/job/${token}`);
    if (!r.ok) throw new Error(`HTTP ${r.status}`);
    job = await r.json();
    try { sessionStorage.removeItem('__submitterHandoff'); } catch (_) {}
    chrome.runtime.sendMessage({
      type: 'submitter:close-when-ready',
      port: handoff.port,
      token: handoff.token,
    }).catch(() => {}); // background not loaded yet shouldn't break anything
  } catch (e) {
    window.notify(`couldn't reach helper (${e.message || 'unknown error'}). Paste from clipboard.`);
    return;
  }

  if (typeof window.__submitterFill !== 'function') {
    window.notify(`no filler for this site yet. Paste from clipboard.`);
    return;
  }

  try {
    await window.__submitterFill(job);
  } catch (e) {
    window.notify(`${e.message || 'filler error'}. Paste from clipboard.`);
  }
})();

'use strict';
chrome.runtime.onMessage.addListener((msg, sender) => {
  if (!msg || msg.type !== 'submitter:close-when-ready') return;
  const tabId = sender.tab && sender.tab.id;
  if (!tabId || !msg.port || !msg.token) return;
  pollAndClose(tabId, String(msg.port), String(msg.token));
});

async function pollAndClose(tabId, port, token) {
  const deadline = Date.now() + 10 * 60 * 1000; // 10 minutes
  while (Date.now() < deadline) {
    let status;
    try {
      const r = await fetch(`http://127.0.0.1:${port}/close/${token}`);
      status = r.status;
    } catch (_) {
      // Connection refused = submitter exited. Stop polling, leave tab open.
      return;
    }
    if (status === 204) {
      chrome.tabs.remove(tabId).catch(() => {});
      return;
    }
    // 404 means "not yet". Anything else: also wait + retry.
    await new Promise((r) => setTimeout(r, 2000));
  }
}

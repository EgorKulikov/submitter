'use strict';
window.__submitterSetSource = async function setSource(source, timeoutMs) {
  timeoutMs = timeoutMs || 10000;
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    const result = await new Promise((resolve) => {
      const handler = (e) => {
        if (e.source !== window) return;
        if (!e.data || e.data.__submitter !== 'set-source-result') return;
        window.removeEventListener('message', handler);
        resolve(e.data);
      };
      window.addEventListener('message', handler);
      window.postMessage({ __submitter: 'set-source', source: source }, '*');
      setTimeout(() => {
        window.removeEventListener('message', handler);
        resolve(null);
      }, 1000);
    });
    if (result) return result;
  }
  return { ok: false, error: 'editor bridge timed out' };
};

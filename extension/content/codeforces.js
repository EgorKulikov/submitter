'use strict';
if (location.hostname === 'codeforces.com') {
  window.__submitterFill = async function (job) {
    // Set source via MAIN-world bridge (handles textarea + CodeMirror mirror)
    const result = await window.__submitterSetSource(job.source);
    if (!result.ok) throw new Error(result.error || 'failed to set editor source');

    const select = await waitFor(() => visible(document.querySelectorAll('select[name="programTypeId"]')), 10000);
    if (!select) throw new Error('language select not found');

    const optionTexts = Array.from(select.options).map(o => o.textContent.trim());
    const idx = window.__submitterPickOption('codeforces', job.language, optionTexts);
    if (idx === null) {
      window.notify(`couldn't pick a language for "${job.language}". Pick one and click Submit.`);
      return;
    }
    select.value = select.options[idx].value;
    select.dispatchEvent(new Event('change', { bubbles: true }));

    // Submit: find by form structure (locale-agnostic)
    const submitBtn = await waitFor(() => {
      const ta = document.querySelector('textarea[name="source"]');
      if (!ta) return null;
      const form = ta.closest('form');
      return form ? form.querySelector('input[type=submit]') : null;
    }, 10000);
    if (!submitBtn) throw new Error('submit button not found');
    submitBtn.click();
  };

  function visible(nodes) {
    return Array.from(nodes).find(n => n.offsetParent !== null) || null;
  }

  function waitFor(fn, timeoutMs) {
    return new Promise((resolve) => {
      const start = Date.now();
      const tick = () => {
        const r = fn();
        if (r) return resolve(r);
        if (Date.now() - start > timeoutMs) return resolve(null);
        setTimeout(tick, 100);
      };
      tick();
    });
  }
}

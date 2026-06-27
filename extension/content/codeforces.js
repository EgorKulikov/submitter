'use strict';
if (location.hostname === 'codeforces.com') {
  window.__submitterFill = async function (job) {
    const textarea = await waitFor(() => document.querySelector('textarea[name="source"]'), 10000);
    const select = await waitFor(() => visible(document.querySelectorAll('select[name="programTypeId"]')), 10000);
    const submitBtn = await waitFor(() => {
      if (!textarea) return null;
      const form = textarea.closest('form');
      return form ? form.querySelector('input[type=submit]') : null;
    }, 10000);

    if (!textarea) throw new Error('source textarea not found');
    if (!select) throw new Error('language select not found');
    if (!submitBtn) throw new Error('submit button not found');

    textarea.value = job.source;
    textarea.dispatchEvent(new Event('input', { bubbles: true }));
    textarea.dispatchEvent(new Event('change', { bubbles: true }));

    // If CodeMirror is enabled, mirror into it so the user sees the code.
    const cmEl = document.querySelector('.CodeMirror');
    if (cmEl && cmEl.CodeMirror) cmEl.CodeMirror.setValue(job.source);

    const entry = window.__submitterPickLanguage('codeforces', job.language);
    if (!entry) {
      window.notify(`unknown language "${job.language}". Pick one and click Submit.`);
      return;
    }
    const option = Array.from(select.options).find(o => o.textContent.trim() === entry.name);
    if (!option) {
      window.notify(`language "${entry.name}" not in dropdown. Pick one and click Submit.`);
      return;
    }
    select.value = option.value;
    select.dispatchEvent(new Event('change', { bubbles: true }));

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

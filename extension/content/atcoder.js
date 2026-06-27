'use strict';
if (location.hostname === 'atcoder.jp') {
  window.__submitterFill = async function (job) {
    // Set source via MAIN-world bridge (ACE editor lives in page world)
    const result = await window.__submitterSetSource(job.source);
    if (!result.ok) throw new Error(result.error || 'failed to set editor source');

    // Language select — visible <select name="data.LanguageId">
    const select = await waitFor(() => findLanguageSelect(), 10000);
    if (!select) throw new Error('language select not found');

    const optionTexts = Array.from(select.options).map(o => o.textContent.trim());
    const idx = window.__submitterPickOption('atcoder', job.language, optionTexts);
    if (idx === null) {
      window.notify(`couldn't pick a language for "${job.language}". Pick one and click Submit.`);
      return;
    }
    select.value = select.options[idx].value;
    select.dispatchEvent(new Event('change', { bubbles: true }));

    // Submit button
    const submitBtn = await waitFor(() => document.querySelector('#submit'), 10000);
    if (!submitBtn) throw new Error('submit button not found');
    submitBtn.click();
  };

  function findLanguageSelect() {
    return Array.from(document.querySelectorAll('select[name="data.LanguageId"]'))
      .find(s => s.offsetParent !== null) || null;
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

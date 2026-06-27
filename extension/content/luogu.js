'use strict';
if (location.hostname === 'www.luogu.com.cn') {
  window.__submitterFill = async function (job) {
    // The activator stripped #submit; restore it so the submit panel opens.
    if (location.hash !== '#submit') {
      location.hash = 'submit';
    }

    // Set source via MAIN-world bridge (Monaco lives in page world)
    const result = await window.__submitterSetSource(job.source, 15000);
    if (!result.ok) throw new Error(result.error || 'failed to set editor source');

    const picked = await pickLanguageOption(job.language, 10000);
    if (!picked) {
      window.notify(`couldn't pick a language for "${job.language}". Pick one and click 提交.`);
      return;
    }

    const submitBtn = await waitFor(() => findSubmitButton(), 10000);
    if (!submitBtn) throw new Error('Submit button not found');
    submitBtn.click();
  };

  async function pickLanguageOption(requested, timeoutMs) {
    const start = Date.now();
    const remaining = () => Math.max(0, timeoutMs - (Date.now() - start));
    const trigger = await waitFor(
      () => document.querySelector('.lg-dropdown') || document.querySelector('.lg-select'),
      remaining()
    );
    if (!trigger) return false;
    trigger.click();
    const items = await waitFor(() => {
      const list = Array.from(document.querySelectorAll('.lg-dropdown-menu li, .lg-select-option'));
      return list.length > 0 ? list : null;
    }, remaining());
    if (!items) return false;
    const texts = items.map(li => li.textContent.trim());
    const idx = window.__submitterPickOption('luogu', requested, texts);
    if (idx === null) return false;
    items[idx].click();
    return true;
  }

  function findSubmitButton() {
    return Array.from(document.querySelectorAll('button'))
      .find(b => b.textContent.trim() === '提交' && b.offsetParent !== null) || null;
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

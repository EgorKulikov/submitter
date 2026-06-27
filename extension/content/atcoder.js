'use strict';
if (location.hostname === 'atcoder.jp') {
  window.__submitterFill = async function (job) {
    const editor = await waitFor(() => findEditor(), 10000);
    const select = await waitFor(() => findLanguageSelect(), 10000);
    const submitBtn = await waitFor(() => document.querySelector('#submit'), 10000);

    if (!editor) throw new Error('code editor not found');
    if (!select) throw new Error('language select not found');
    if (!submitBtn) throw new Error('submit button not found');

    editor.setValue(job.source, -1);

    const optionTexts = Array.from(select.options).map(o => o.textContent.trim());
    const idx = window.__submitterPickOption('atcoder', job.language, optionTexts);
    if (idx === null) {
      window.notify(`couldn't pick a language for "${job.language}". Pick one and click Submit.`);
      return;
    }
    select.value = select.options[idx].value;
    select.dispatchEvent(new Event('change', { bubbles: true }));
    submitBtn.click();
  };

  function findEditor() {
    if (typeof ace === 'undefined') return null;
    const div = Array.from(document.querySelectorAll('div[id^="editor"]'))
      .find(d => d.offsetParent !== null);
    if (!div) return null;
    return ace.edit(div);
  }

  function findLanguageSelect() {
    return Array.from(document.querySelectorAll('select[id^="select-lang"]'))
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

if (location.hostname === 'atcoder.jp') {
  window.__submitterFill = async function (job) {
    const editor = await waitFor(() => findEditor(), 10000);
    const select = await waitFor(() => findLanguageSelect(), 10000);
    const submitBtn = await waitFor(() => document.querySelector('#submit'), 10000);

    if (!editor) throw new Error('code editor not found');
    if (!select) throw new Error('language select not found');
    if (!submitBtn) throw new Error('submit button not found');

    editor.setValue(job.source, -1);

    const entry = window.__submitterPickLanguage('atcoder', job.language);
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

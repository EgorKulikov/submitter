if (location.hostname === 'www.luogu.com.cn') {
  window.__submitterFill = async function (job) {
    // The activator stripped #submit; restore it so the submit panel opens.
    if (location.hash !== '#submit') {
      location.hash = 'submit';
    }

    const editor = await waitFor(() => findMonacoEditor(), 15000);
    if (!editor) throw new Error('Monaco editor not found');
    editor.setValue(job.source);

    const entry = window.__submitterPickLanguage('luogu', job.language);
    if (entry) {
      const picked = await pickLanguageOption(entry.name, 10000);
      if (!picked) {
        window.notify(`language "${entry.name}" not in dropdown. Pick one and click 提交.`);
        return;
      }
    } else {
      window.notify(`unknown language "${job.language}". Pick one and click 提交.`);
      return;
    }

    const submitBtn = await waitFor(() => findSubmitButton(), 10000);
    if (!submitBtn) throw new Error('Submit button not found');
    submitBtn.click();
  };

  function findMonacoEditor() {
    if (typeof monaco === 'undefined' || !monaco.editor) return null;
    const editors = monaco.editor.getEditors();
    return editors.length > 0 ? editors[0] : null;
  }

  async function pickLanguageOption(name, timeoutMs) {
    // Click the dropdown trigger to open the menu, then click the matching item.
    // Luogu's exact selectors should be verified on a live page — adjust as needed.
    const trigger = await waitFor(
      () => document.querySelector('.lg-dropdown[data-v-]') || document.querySelector('.lg-select'),
      timeoutMs
    );
    if (!trigger) return false;
    trigger.click();
    const item = await waitFor(() => Array.from(document.querySelectorAll('.lg-dropdown-menu li, .lg-select-option'))
      .find(li => li.textContent.trim() === name), timeoutMs);
    if (!item) return false;
    item.click();
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

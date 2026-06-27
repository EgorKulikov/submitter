'use strict';
(function () {
  function handle(source) {
    // Try ACE first (AtCoder)
    if (typeof ace !== 'undefined') {
      const div = Array.from(document.querySelectorAll('div[id^="editor"]'))
        .find(d => d.offsetParent !== null);
      if (div) {
        try { ace.edit(div).setValue(source, -1); return { ok: true, kind: 'ace' }; }
        catch (e) { return { ok: false, error: 'ace.setValue failed: ' + e.message }; }
      }
    }
    // Try Monaco (Luogu)
    if (typeof monaco !== 'undefined' && monaco.editor && typeof monaco.editor.getEditors === 'function') {
      const editors = monaco.editor.getEditors();
      if (editors.length > 0) {
        try { editors[0].setValue(source); return { ok: true, kind: 'monaco' }; }
        catch (e) { return { ok: false, error: 'monaco.setValue failed: ' + e.message }; }
      }
    }
    // CodeMirror 6 (Luogu)
    const cm6 = document.querySelector('.cm-editor');
    if (cm6) {
      const cmContent = cm6.querySelector('.cm-content');
      if (cmContent) {
        try {
          cmContent.focus();
          const sel = window.getSelection();
          const range = document.createRange();
          range.selectNodeContents(cmContent);
          sel.removeAllRanges();
          sel.addRange(range);
          const inserted = document.execCommand('insertText', false, source);
          if (!inserted) return { ok: false, error: 'cm6 execCommand returned false' };
          return { ok: true, kind: 'codemirror6' };
        } catch (e) {
          return { ok: false, error: 'cm6 insertText failed: ' + e.message };
        }
      }
    }
    // Fall back to a plain source textarea
    const ta = document.querySelector('textarea[name="source"], textarea[name="sourceCode"]');
    if (ta) {
      try {
        ta.value = source;
        ta.dispatchEvent(new Event('input', { bubbles: true }));
        ta.dispatchEvent(new Event('change', { bubbles: true }));
        // If a CodeMirror skin is rendered over the textarea, mirror there too
        const cmEl = document.querySelector('.CodeMirror');
        if (cmEl && cmEl.CodeMirror) cmEl.CodeMirror.setValue(source);
        return { ok: true, kind: 'textarea' };
      } catch (e) {
        return { ok: false, error: 'textarea set failed: ' + e.message };
      }
    }
    return { ok: false, error: 'no editor found' };
  }

  window.addEventListener('message', function (e) {
    if (e.source !== window) return;
    if (!e.data || e.data.__submitter !== 'set-source') return;
    const result = handle(e.data.source);
    window.postMessage(Object.assign({ __submitter: 'set-source-result' }, result), '*');
  });
})();

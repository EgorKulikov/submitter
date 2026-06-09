// Paste this into the Chrome DevTools console (or load via Sources >
// Snippets) on a Repovive problem page, press Enter, then click the
// language dropdown so it's visibly open. After ~5 seconds a textarea
// appears at the top of the page with the language list as JSON.
// Ctrl+A, Ctrl+C, paste into chat.
//
// Reads numeric language IDs by walking each option element's React
// fiber props (Radix Select doesn't surface them as DOM attributes).

setTimeout(() => {
  const items = [...document.querySelectorAll('[role="option"]')].map(el => {
    const propKey = Object.keys(el).find(k => k.startsWith('__reactProps$'));
    const props = propKey ? el[propKey] : null;
    let value = props && (props.value ?? props['data-value']);
    // Some Radix wrappers nest the value inside children-props or a context. As
    // a fallback, traverse the React fiber a few levels and look for a
    // numeric-looking value prop.
    if (value == null) {
      const fiberKey = Object.keys(el).find(k => k.startsWith('__reactFiber$'));
      let f = fiberKey ? el[fiberKey] : null;
      for (let i = 0; f && i < 8; i++) {
        const v = f.memoizedProps && (f.memoizedProps.value ?? f.memoizedProps.itemValue);
        if (v != null) { value = v; break; }
        f = f.return;
      }
    }
    return { label: el.textContent.trim(), value };
  });
  const ta = document.createElement('textarea');
  ta.value = JSON.stringify(items);
  ta.style.cssText = 'position:fixed;top:0;left:0;width:100%;height:60%;z-index:99999;font:12px monospace;';
  document.body.appendChild(ta);
  ta.select();
  console.log(items.length);
}, 5000);

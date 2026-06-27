(function () {
  if (window.__submitterNotify) return;
  window.__submitterNotify = function notify(message) {
    const banner = document.createElement('div');
    banner.textContent = `[submitter] ${message}`;
    Object.assign(banner.style, {
      position: 'fixed',
      top: '0',
      left: '0',
      right: '0',
      zIndex: '2147483647',
      padding: '10px 16px',
      background: '#fff3cd',
      color: '#664d03',
      borderBottom: '1px solid #ffecb5',
      font: '14px/1.4 system-ui, sans-serif',
      boxShadow: '0 2px 6px rgba(0,0,0,0.15)',
    });
    document.documentElement.appendChild(banner);
    setTimeout(() => banner.remove(), 6000);
  };
  window.notify = window.__submitterNotify;
})();

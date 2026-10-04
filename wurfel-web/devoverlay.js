// Development only: shows the compiler output of a failed client build in the page, instead of
// Trunk's "cargo exited with status 101". devcheck.sh writes /build-error.txt; it is absent in
// production, so this does nothing there.
(function () {
  if (!/^(localhost|127\.|\[::1\])/.test(location.hostname)) return;
  var box = null;
  function show(text) {
    if (!box) {
      box = document.createElement('pre');
      box.style.cssText = 'position:fixed;inset:0;z-index:99999;margin:0;padding:16px 20px;overflow:auto;' +
        'background:rgba(30,8,8,.96);color:#ffd6d6;font:13px/1.45 ui-monospace,Menlo,monospace;white-space:pre-wrap';
      document.body.appendChild(box);
    }
    box.textContent = 'Client build failed (wurfel-web, wasm). Fix and save, this closes by itself.\n\n' + text;
  }
  function poll() {
    fetch('/build-error.txt', { cache: 'no-store' }).then(function (r) {
      var type = r.headers.get('content-type') || '';
      return r.ok && type.indexOf('text/plain') === 0 ? r.text() : '';
    }).then(function (text) {
      if (text.trim()) show(text);
      else if (box) { box.remove(); box = null; location.reload(); }
    }).catch(function () {});
  }
  setInterval(poll, 1500);
  poll();
})();

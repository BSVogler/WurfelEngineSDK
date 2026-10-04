/*
 * Tab player list: hold the "players" key (default Tab) in the game to see everybody on the
 * server with their ping. The wasm client publishes `window.wurfelPlayers` (a list of
 * { id, name, color, pingMs, me }, pingMs null until known) and `window.wurfelStatus.map` about
 * twice a second; this file only draws them. The key is the `players` action of
 * `window.wurfelSettings.keys` (rebindable in the menu), a lowercased KeyboardEvent.key.
 */
(function () {
  'use strict';
  var DEFAULT_KEY = 'tab';
  var el = null;

  // Green up to 80 ms, yellow up to 160 ms, red above.
  function pingClass(ms) {
    if (ms === null || ms === undefined) return 'unknown';
    return ms <= 80 ? 'good' : ms <= 160 ? 'ok' : 'bad';
  }

  function boundKeys() {
    var s = window.wurfelSettings, k = s && s.keys && s.keys.players;
    var keys = Array.isArray(k) ? k.filter(Boolean) : [DEFAULT_KEY];
    return keys.map(function (x) { return String(x).toLowerCase(); });
  }

  function typing(e) {
    var t = e.target;
    return !!t && (t.tagName === 'INPUT' || t.tagName === 'TEXTAREA' || t.tagName === 'SELECT' || t.isContentEditable);
  }

  function inGame() {
    return !window.wurfelMenuOpen && !window.wurfelConsoleOpen;
  }

  function ensure() {
    if (el) return el;
    el = document.createElement('div');
    el.id = 'scoreboard';
    el.hidden = true;
    el.setAttribute('role', 'status');
    document.body.appendChild(el);
    return el;
  }

  function render() {
    var players = Array.isArray(window.wurfelPlayers) ? window.wurfelPlayers : [];
    var map = (window.wurfelStatus && window.wurfelStatus.map) || '';
    var box = ensure();
    box.textContent = '';
    var head = document.createElement('header');
    var title = document.createElement('span');
    title.textContent = map || 'Players';
    var count = document.createElement('span');
    count.textContent = players.length + (players.length === 1 ? ' player' : ' players');
    head.appendChild(title);
    head.appendChild(count);
    box.appendChild(head);
    var list = document.createElement('ul');
    players.forEach(function (p) {
      var li = document.createElement('li');
      if (p.me) li.className = 'me';
      var swatch = document.createElement('span');
      swatch.className = 'swatch';
      swatch.style.background = /^#[0-9a-f]{6}$/i.test(p.color) ? p.color : '#888';
      var name = document.createElement('span');
      name.className = 'name';
      name.textContent = p.name;
      var ping = document.createElement('span');
      ping.className = 'ping ' + pingClass(p.pingMs);
      ping.textContent = p.pingMs === null || p.pingMs === undefined ? '–' : p.pingMs + ' ms';
      li.appendChild(swatch);
      li.appendChild(name);
      li.appendChild(ping);
      list.appendChild(li);
    });
    box.appendChild(list);
  }

  var timer = null;
  function show() {
    if (timer !== null) return;
    render();
    ensure().hidden = false;
    timer = setInterval(render, 250);
  }
  function hide() {
    if (timer !== null) { clearInterval(timer); timer = null; }
    if (el) el.hidden = true;
  }

  window.addEventListener('keydown', function (e) {
    if (e.ctrlKey || e.altKey || e.metaKey || typing(e)) return;
    if (boundKeys().indexOf(e.key.toLowerCase()) === -1) return;
    if (!inGame()) return;
    e.preventDefault(); // Tab would move the keyboard focus
    show();
  });
  window.addEventListener('keyup', function (e) {
    if (boundKeys().indexOf(e.key.toLowerCase()) !== -1) hide();
  });
  // Menu or console opening, or focus leaving the page, while the key is held.
  window.addEventListener('blur', hide);
  window.addEventListener('wurfel:pause', hide);
  setInterval(function () { if (timer !== null && !inGame()) hide(); }, 250);
})();

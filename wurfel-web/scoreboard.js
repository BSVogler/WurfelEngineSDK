/*
 * Tab player list: hold the "players" key (default Tab) in the game to see everybody on the
 * server with their ping. The wasm client publishes `window.wurfelPlayers` (a list of
 * { id, name, color, pingMs, me, friend, invited, invitesMe }, pingMs null until known) and
 * `window.wurfelStatus.map` about twice a second; this file only draws them.
 *
 * Every other player has a heart. Clicking it (the mouse works while the key is held) invites
 * that player to be friends, or accepts their invite; clicking it again withdraws, declines or
 * ends. The page sets `window.wurfelScoreboardOpen` meanwhile so the click does not also place a
 * block, and sends the click through `window.wurfelNet.heart(id, on)`. The key is the `players` action of
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

  // What a heart shows and what clicking it asks for, per relationship.
  function heartState(p) {
    if (p.friend) return { cls: 'friend', glyph: '\u2665', title: 'Friends. Click to end', on: false };
    if (p.invitesMe) return { cls: 'accept', glyph: '\u2665', title: 'Wants to be friends. Click to accept', on: true };
    if (p.invited) return { cls: 'sent', glyph: '\u2661', title: 'Invite sent. Click to withdraw', on: false };
    return { cls: 'none', glyph: '\u2661', title: 'Invite to be friends', on: true };
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
      var heart = document.createElement('span');
      if (p.me) {
        heart.className = 'heart none';
        heart.style.visibility = 'hidden';
        heart.textContent = '\u2661';
      } else {
        var h = heartState(p);
        heart.className = 'heart ' + h.cls;
        heart.textContent = h.glyph;
        heart.title = h.title;
        heart.dataset.player = String(p.id);
        heart.dataset.on = h.on ? '1' : '0';
      }
      li.appendChild(swatch);
      li.appendChild(name);
      li.appendChild(heart);
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
    window.wurfelScoreboardOpen = true;
    timer = setInterval(render, 250);
  }
  function hide() {
    window.wurfelScoreboardOpen = false;
    if (timer !== null) { clearInterval(timer); timer = null; }
    if (el) el.hidden = true;
  }

  // The list is rebuilt several times a second, so act on the press (not on a click, which needs
  // the same element to still be there on release).
  document.addEventListener('mousedown', function (e) {
    if (timer === null || e.button !== 0) return;
    var t = e.target;
    if (!t || !t.dataset || !t.dataset.player) return;
    e.preventDefault();
    var net = window.wurfelNet;
    if (net && typeof net.heart === 'function') net.heart(Number(t.dataset.player), t.dataset.on === '1');
  });

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

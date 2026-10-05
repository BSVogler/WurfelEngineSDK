/*
 * Main menu, options and pause overlay for the Wurfel Engine browser client. Plain JS, no build step.
 *
 * CONTRACT WITH THE GAME (wasm) SIDE
 * ----------------------------------
 * window.wurfelSettings   Live object, always current. Fields (all validated and clamped):
 *     playerName       string, 1-16 chars (control characters removed). The game reads this when it joins.
 *     playerColor      "#rrggbb" (lowercase), default random from the 10 swatches. Read when it joins.
 *     serverUrl        the active server as typed: "localhost" (default), "host", "host:port", a ws(s)://
 *                      or http(s):// URL, or "" for the server this page came from. Use the `server` field
 *                      of wurfel:play, which is already a full ws:// or wss:// URL.
 *     servers          up to 8 remembered servers (strings, most recent first, no duplicates); a server is
 *                      remembered after its lobby answered once.
 *     masterVolume, musicVolume, effectsVolume   0..1
 *     renderScale      0.5..1   (advisory: fraction of the native canvas resolution to render at)
 *     zoom             0.2..2   default camera zoom (1 = the engine's native 100 px per block)
 *     generator        string: id of the generator last used in the "Create map" form
 *     seed             integer >= 0: the seed last used in that form
 *                      (both only remember the form; the world you play in is described by wurfel:play)
 *     limitFps         bool     cap at 60 FPS
 *     ambientOcclusion bool     for the light engine, once it exists
 *     showFps, showHelp bool    (JS handles the FPS counter and hides #info itself)
 *     keys             { action: [primary, alternate] } with actions
 *                      up, down, left, right, jump, place, break, zoomIn, zoomOut, players.
 *                      Values are KeyboardEvent.key lowercased (" " is space, "arrowup"...), or
 *                      "mouse0" / "mouse1" / "mouse2" for mouse buttons. An empty string means
 *                      unbound: ignore it. Number keys 1-4 (hotbar) are fixed and not listed.
 *
 * Events dispatched on window (CustomEvent):
 *     wurfel:play      detail { name, color, server, generator, seed, create }
 *                      The player joined the server's world: "Connect" joins the one already running, "Host a
 *                      map" loads a save first. name / color = the player's name and "#rrggbb" colour, server =
 *                      ws(s):// URL to connect to (no
 *                      query string). generator / seed describe the world the server is running now (read
 *                      from the server, so the game can generate the same terrain). create is true when
 *                      this click created a new save slot. The server has already switched worlds when
 *                      this fires (the menu did the POST); the game must not POST anything.
 *     wurfel:pause     the pause overlay opened while playing (release held keys, stop the player).
 *     wurfel:resume    the pause overlay closed, back to the game.
 *     wurfel:leave     the player left the game; connection should be closed (menu shows main screen).
 *     wurfel:error     (game -> menu) detail { message }: joining or creating a world failed, or the
 *                      connection was lost for good (the game retries for about a minute first, keeping the
 *                      world on screen). The menu shows the message on the main menu.
 *     window.wurfelUpdate.show(serverBuild)   (game -> page) the server runs another build than this page:
 *                      shows a small "Update available - Reload" notice. Does not block input.
 *     window.wurfelPlayRequest   the detail of the last wurfel:play (null after leaving). A game that
 *                      was not listening yet when the event fired reads this at startup.
 *     wurfel:settings  detail = window.wurfelSettings; fired once at startup and after every change.
 *
 * Choosing the server (resolveServer): an address without a scheme gets the game server's default port 3000,
 *     NOT the page's: "localhost" -> ws://localhost:3000/ws, "myserver.example.org" -> ws://myserver.example.org:3000/ws
 *     (or wss://myserver.example.org/ws when this page is https). "host:port" keeps its port (ws, or wss on an
 *     https page), "" is the server this page came from, ws(s):// and http(s):// URLs are used as given
 *     (http -> ws). A path defaults to /ws. Everything below belongs to the selected server.
 *
 * Connect, Host a map and the lobby WebSocket
 *     The main menu has a "Connect" button and a small Server field (default localhost). Connect asks the
 *     server's lobby for the running world, then joins it. "Host a map" lists all maps on the server's disk with generator, seed and saves;
 *     loading one switches what the server runs, then joins it.
 *     The server holds exactly one (map, save slot) in memory, like the Java engine's single Map. It can
 *     only switch it while nobody is joined. The menu never starts a server process.
 *     For both, the menu opens its OWN short-lived WebSocket to the `server`
 *     URL (ws(s)://host/ws, no query string). It is closed when the player leaves the Host screens or
 *     joins; the game then opens a separate socket and joins there (the menu never sends
 *     Join). Refresh reconnects. Frames are JSON text with a "type" field. The menu must be able to
 *     connect, and receive a Lobby, within 4 s, else it shows "couldn't reach a game server".
 *     server -> menu
 *       Lobby         { world: { map (display name), map_id, slot, generator, seed, players, gamemode }, generators: [...] }
 *                     sent right after connecting, and as the answer to GetWorld. `players` counts joined
 *                     players only. `generators` is [{ id, name, description, uses_seed }].
 *       Maps          { maps: [{ id, name, description, generator, seed, gamemode, saves: [{ slot, modified|null }] }] }
 *       WorldChanged  { world }   a LoadMap succeeded (sent to every lobby connection). This is how the menu
 *                     learns that its own LoadMap worked.
 *       MapCreated    { map }     a CreateMap succeeded.
 *       Failed        { request, message }   a refused request; request is the type of the client message.
 *                     The message is shown to the player as it is.
 *     menu -> server
 *       ListMaps, GetWorld, LoadMap { map, slot: <n> | "new" }, CreateMap { id, name, description,
 *       generator, seed, gamemode: "" | "engine" | "caveland" } (id: 1-32 of [a-z0-9_-]). Frames must stay under 1 KB.
 *     Requests that get no answer give up after 20 s (LoadMap) and 15 s (CreateMap); the maps list after 4 s.
 *     window.wurfelGenerators   optional fallback list of { id, name, description, uses_seed }, used when the
 *                      lobby cannot be reached or sends no generators. Without it a built-in list is used:
 *                      island, air, blocktest, fullmap, arena, caveland.
 *
 * State flags:
 *     window.wurfelMenuOpen   true while any menu is open. The game must ignore gameplay input then.
 *                             As a safety net this script also swallows keyboard, mouse and wheel events
 *                             in the capture phase on window while the menu is open, and dispatches a
 *                             `blur` event on window when the menu opens so held keys are released.
 *     window.wurfelStatus     optional, set by the game and read by this script, e.g.
 *                             { connected: bool, players: n, fps: n, backend: "WebGPU" }.
 *
 * Escape opens the pause overlay while playing and goes back / resumes while a menu is open.
 */
(() => {
  'use strict';

  const STORAGE_KEY = 'wurfel.settings.v1';
  const NAME_MAX = 16;
  const DEFAULT_SERVER = 'localhost';
  const DEFAULT_PORT = 3000;
  const MAX_SERVERS = 8;
  const HEX = /^#[0-9a-f]{6}$/i;
  const SWATCHES = [
    ['Red', '#e64d4d'], ['Yellow', '#f2bf33'], ['Purple', '#9959d9'], ['Teal', '#33bfbf'], ['Orange', '#f28026'],
    ['White', '#e6e6f2'], ['Green', '#4caf50'], ['Blue', '#3f7fe0'], ['Pink', '#e86fa8'], ['Brown', '#8b6b4a'],
  ];
  const cleanName = (v) => String(v).replace(/[\u0000-\u001f\u007f]/g, '').trim().slice(0, NAME_MAX);

  // ---------------------------------------------------------------------------------- settings
  const ACTIONS = [
    ['up', 'Walk up'], ['down', 'Walk down'], ['left', 'Walk left'], ['right', 'Walk right'],
    ['jump', 'Jump'], ['place', 'Place block'], ['break', 'Break block'],
    ['zoomIn', 'Zoom in'], ['zoomOut', 'Zoom out'], ['players', 'Player list'],
  ];
  const DEFAULT_KEYS = {
    up: ['w', 'arrowup'], down: ['s', 'arrowdown'], left: ['a', 'arrowleft'], right: ['d', 'arrowright'],
    jump: [' ', ''], place: ['mouse0', ''], break: ['mouse2', ''], zoomIn: ['e', ''], zoomOut: ['q', ''], players: ['tab', ''],
  };
  const RANGES = {
    masterVolume: [0, 1], musicVolume: [0, 1], effectsVolume: [0, 1], renderScale: [0.5, 1], zoom: [0.2, 2],
  };
  const clone = (o) => JSON.parse(JSON.stringify(o));

  function defaults() {
    return {
      playerName: 'Player' + String(100 + Math.floor(Math.random() * 900)),
      playerColor: SWATCHES[Math.floor(Math.random() * SWATCHES.length)][1],
      serverUrl: DEFAULT_SERVER, servers: [DEFAULT_SERVER],
      masterVolume: 0.8, musicVolume: 0.6, effectsVolume: 0.8,
      renderScale: 1, zoom: 0.5,
      generator: 'island', seed: Math.floor(Math.random() * 1000000),
      limitFps: true, ambientOcclusion: false, showFps: false, showHelp: true,
      keys: clone(DEFAULT_KEYS),
    };
  }

  /** Merge untrusted stored data over the defaults, keeping only values of the right type and range. */
  function sanitize(raw, base) {
    const out = base;
    if (!raw || typeof raw !== 'object') return out;
    if (typeof raw.playerName === 'string') out.playerName = cleanName(raw.playerName);
    if (typeof raw.playerColor === 'string' && HEX.test(raw.playerColor)) out.playerColor = raw.playerColor.toLowerCase();
    if (Array.isArray(raw.servers)) out.servers = cleanServers(raw.servers);
    if (typeof raw.serverUrl === 'string') out.serverUrl = raw.serverUrl.trim().slice(0, 200);
    if (typeof raw.generator === 'string' && /^[\w-]{1,32}$/.test(raw.generator)) out.generator = raw.generator;
    if (Number.isSafeInteger(raw.seed) && raw.seed >= 0) out.seed = raw.seed;
    for (const [key, [lo, hi]] of Object.entries(RANGES)) {
      if (typeof raw[key] === 'number' && Number.isFinite(raw[key])) out[key] = Math.min(hi, Math.max(lo, raw[key]));
    }
    for (const key of ['limitFps', 'ambientOcclusion', 'showFps', 'showHelp']) {
      if (typeof raw[key] === 'boolean') out[key] = raw[key];
    }
    if (raw.keys && typeof raw.keys === 'object') {
      for (const [action] of ACTIONS) {
        const slots = raw.keys[action];
        if (Array.isArray(slots)) {
          out.keys[action] = [0, 1].map((i) => (typeof slots[i] === 'string' ? slots[i].toLowerCase().slice(0, 24) : ''));
        }
      }
    }
    return out;
  }

  /** Remembered servers: valid, unique (by the URL they resolve to) and at most MAX_SERVERS. */
  function cleanServers(list) {
    const seen = new Set();
    const out = [];
    for (const item of list) {
      if (typeof item !== 'string') continue;
      const text = item.trim().slice(0, 200);
      const url = text ? resolve(text, false, '') : null;
      if (!url || seen.has(url)) continue;
      seen.add(url);
      out.push(text);
      if (out.length === MAX_SERVERS) break;
    }
    return out;
  }

  let storageWorks = true;
  function loadSettings() {
    const settings = defaults();
    try {
      const text = window.localStorage.getItem(STORAGE_KEY);
      if (text) sanitize(JSON.parse(text), settings);
    } catch (_) { /* corrupt JSON or storage blocked: keep the defaults */ }
    return settings;
  }
  function saveSettings() {
    try {
      window.localStorage.setItem(STORAGE_KEY, JSON.stringify(S));
      storageWorks = true;
    } catch (_) {
      storageWorks = false;
    }
    $('#storage-note').hidden = storageWorks;
  }

  const S = loadSettings();
  window.wurfelSettings = S;

  // ---------------------------------------------------------------------------------- helpers
  const $ = (sel, root = document) => root.querySelector(sel);
  const $$ = (sel, root = document) => Array.from(root.querySelectorAll(sel));
  const emit = (name, detail) => window.dispatchEvent(new CustomEvent(name, { detail }));
  const menu = $('#menu');
  const root = document.documentElement;

  /**
   * Full ws(s):// URL for what the player typed, or null. Pure: the page's protocol and host are
   * parameters. An address without a scheme gets the game server's port 3000 (not the page's).
   */
  function resolve(input, pageSecure, pageHost) {
    const text = (input || '').trim();
    if (/\s/.test(text)) return null; // a space is never part of an address
    let url;
    if (!text) {
      if (!pageHost) return null; // opened from a file: there is no "same origin"
      url = (pageSecure ? 'wss://' : 'ws://') + pageHost;
    } else if (/^https?:\/\//i.test(text)) {
      url = text.replace(/^http/i, 'ws');
    } else if (/^wss?:\/\//i.test(text)) {
      url = text;
    } else {
      let u;
      try { u = new URL((pageSecure ? 'wss://' : 'ws://') + text); } catch (_) { return null; }
      const explicitPort = /^(\[[^\]]*\]|[^:/?#\s]+):\d+(?=$|[/?#])/.test(text);
      if (!explicitPort) {
        const local = u.hostname === 'localhost' || u.hostname === '127.0.0.1' || u.hostname === '[::1]';
        if (local) { u.protocol = 'ws:'; u.port = String(DEFAULT_PORT); } // this machine is not behind TLS
        else if (!pageSecure) u.port = String(DEFAULT_PORT);
      }
      url = u.toString();
    }
    try {
      const parsed = new URL(url);
      if (!parsed.hostname) return null;
      if (parsed.pathname === '/' || parsed.pathname === '') parsed.pathname = '/ws';
      return parsed.toString();
    } catch (_) {
      return null;
    }
  }
  const resolveServer = (input) => resolve(input, location.protocol === 'https:', location.host);

  // Small UI sounds, like the Java menu's select/confirm/abort. Generated, so no asset files.
  let audio = null;
  function blip(freq, ms = 55) {
    const volume = S.masterVolume * S.effectsVolume;
    if (volume <= 0) return;
    try {
      audio = audio || new (window.AudioContext || window.webkitAudioContext)();
      const osc = audio.createOscillator();
      const gain = audio.createGain();
      osc.type = 'triangle';
      osc.frequency.value = freq;
      gain.gain.setValueAtTime(0.12 * volume, audio.currentTime);
      gain.gain.exponentialRampToValueAtTime(0.0001, audio.currentTime + ms / 1000);
      osc.connect(gain).connect(audio.destination);
      osc.start();
      osc.stop(audio.currentTime + ms / 1000);
    } catch (_) { /* no audio available */ }
  }

  const KEY_NAMES = {
    ' ': 'Space', arrowup: '↑', arrowdown: '↓', arrowleft: '←', arrowright: '→', enter: 'Enter', tab: 'Tab',
    shift: 'Shift', control: 'Ctrl', alt: 'Alt', meta: 'Meta', mouse0: 'Left click', mouse1: 'Middle click', mouse2: 'Right click',
  };
  function keyLabel(key) {
    if (!key) return 'unbound';
    return KEY_NAMES[key] || (key.length === 1 ? key.toUpperCase() : key);
  }


  // ---------------------------------------------------------------------------------- server world
  // The server holds exactly one (map, save slot) in memory. Before joining, the menu talks to it over
  // a short-lived "lobby" WebSocket of its own: it shows the running world, lists the maps and saves,
  // loads a save and creates maps. The game opens its own socket after wurfel:play; the menu never
  // sends Join.
  const BUILTIN_GENERATORS = [
    { id: 'island', name: 'Island', description: 'One mountain rising from a shallow sea over a sand floor.', uses_seed: true },
    { id: 'air', name: 'Air', description: 'Nothing but air. Build everything yourself.', uses_seed: false },
    { id: 'blocktest', name: 'Block test', description: 'A single layer in which every row shows a different block type, for checking all blocks.', uses_seed: false },
    { id: 'fullmap', name: 'Full map', description: 'The whole world filled with one solid block type.', uses_seed: false },
    { id: 'arena', name: 'Arena', description: 'A flat sand arena with scattered pillars.', uses_seed: true },
    { id: 'caveland', name: 'Caveland', description: 'An overworld with cave rooms underground.', uses_seed: true },
  ];
  const MAP_ID = /^[a-z0-9_-]{1,32}$/;
  const FRAME_LIMIT = 1000; // the server drops connections that send frames above 1 KB
  const TIMEOUTS = { ready: 4000, answer: 4000, load: 20000, create: 15000 };
  const UNREACHABLE = "Can't reach the server.";
  const LOST = 'Connection lost. Press Refresh.';
  const MAPS_FAILED = "Couldn't load the maps.";

  /** Keep only well-formed generator entries; the list comes from outside this file. */
  function cleanGenerators(given) {
    if (!Array.isArray(given)) return [];
    return given
      .filter((g) => g && typeof g.id === 'string' && /^[\w-]{1,32}$/.test(g.id))
      .map((g) => ({
        id: g.id,
        name: typeof g.name === 'string' && g.name ? g.name.slice(0, 40) : g.id,
        description: typeof g.description === 'string' ? g.description.slice(0, 300) : '',
        uses_seed: g.uses_seed !== false,
      }));
  }

  /** The server's own list (from the Lobby), else what the game provided, else the built-in one. */
  let lobbyGenerators = null;
  function generators() {
    if (lobbyGenerators && lobbyGenerators.length) return lobbyGenerators;
    const given = cleanGenerators(window.wurfelGenerators);
    return given.length ? given : BUILTIN_GENERATORS;
  }
  const generatorById = (id) => generators().find((g) => g.id === id) || null;

  /** The chosen server as host:port, for display. */
  function serverLabel() {
    const url = resolveServer(S.serverUrl);
    try { return url ? new URL(url).host : ''; } catch (_) { return ''; }
  }

  const asSeed = (v) => (Number.isSafeInteger(v) && v >= 0 ? v : null);
  const asCount = (v) => (Number.isFinite(v) && v >= 0 ? Math.floor(v) : 0);

  /** The rules a map is played by; older servers send none, which is the plain engine. */
  const cleanMode = (v) => (v === 'caveland' ? 'caveland' : 'engine');

  /** The `world` of a Lobby or WorldChanged message: { map (display name), map_id, slot, generator, seed, players }. */
  function parseWorld(data) {
    if (!data || typeof data !== 'object' || typeof data.generator !== 'string' || !/^[\w-]{1,32}$/.test(data.generator)) throw new Error('bad world');
    return {
      id: typeof data.map_id === 'string' ? data.map_id.slice(0, 40) : '',
      name: typeof data.map === 'string' ? data.map.slice(0, 60) : '',
      slot: Number.isSafeInteger(data.slot) && data.slot >= 0 ? data.slot : null,
      generator: data.generator,
      seed: asSeed(data.seed),
      gamemode: cleanMode(data.gamemode),
      players: asCount(data.players),
    };
  }

  function parseMap(m) {
    if (!m || typeof m !== 'object' || typeof m.id !== 'string' || !/^[\w-]{1,32}$/.test(m.id)) return null;
    const saves = Array.isArray(m.saves) ? m.saves : [];
    return {
      id: m.id,
      name: typeof m.name === 'string' ? m.name.trim().slice(0, 60) : '',
      description: typeof m.description === 'string' ? m.description.trim().slice(0, 200) : '',
      generator: typeof m.generator === 'string' ? m.generator.slice(0, 32) : '',
      seed: asSeed(m.seed),
      gamemode: cleanMode(m.gamemode),
      saves: saves
        .filter((v) => v && Number.isSafeInteger(v.slot) && v.slot >= 0)
        .slice(0, 30)
        .map((v) => ({ slot: v.slot, modified: typeof v.modified === 'string' ? v.modified : null }))
        .sort((a, b) => a.slot - b.slot),
    };
  }

  /** The `maps` of a Maps message. Entries that are not well-formed are dropped. */
  function parseMaps(data) {
    if (!Array.isArray(data)) throw new Error('not a list');
    return data.slice(0, 100).map(parseMap).filter(Boolean);
  }

  function formatModified(iso) {
    if (!iso) return 'not modified yet';
    const date = new Date(iso);
    if (Number.isNaN(date.getTime())) return 'unknown date';
    try { return date.toLocaleString(undefined, { dateStyle: 'medium', timeStyle: 'short' }); } catch (_) { return date.toISOString(); }
  }

  const playersText = (n) => (n === 0 ? 'Empty' : `${n} player${n === 1 ? '' : 's'}`);
  const mapTitle = (id, name) => (id ? `/${id}/ ${name || 'no map name set'}` : name || 'no map name set');

  function generatorLine(generatorId, seed, mode) {
    const gen = generatorById(generatorId);
    const parts = [gen ? gen.name : (generatorId || 'unknown generator')];
    if (seed !== null && (!gen || gen.uses_seed)) parts.push('seed ' + seed);
    if (mode === 'caveland') parts.push('Caveland rules');
    return parts.join(' · ');
  }


  // ------ server: a short field on the main menu; everything else belongs to the chosen server
  const sameServer = (a, b) => { const x = resolveServer(a); return !!x && x === resolveServer(b); };

  function showServerError(text) { const e = $('#server-error'); e.textContent = text; e.hidden = false; $('#server-state').textContent = ''; }
  function hideServerError() { $('#server-error').hidden = true; }

  function renderServerPicker() {
    const input = $('#server-input');
    if (document.activeElement !== input) input.value = S.serverUrl;
    input.placeholder = location.host ? 'this server' : 'host or host:port';
    const options = $('#server-options');
    options.textContent = '';
    for (const entry of S.servers) { const o = document.createElement('option'); o.value = entry; options.append(o); }
  }

  /** The field was edited: it is now the server, and anything open for the old one is dropped. */
  function applyServerInput() {
    const value = $('#server-input').value.trim().slice(0, 200);
    if (value === S.serverUrl || (value && !resolveServer(value))) return; // an invalid address is never stored
    S.serverUrl = value;
    closeLobby();
    serverWorld = null;
    maps = null;
    changed();
  }

  /** Connect: ask the server what it is running, then join that. */
  function connectToServer() {
    hideServerError();
    const typed = $('#server-input').value.trim();
    if (typed && !resolveServer(typed)) { showServerError("That doesn't look like a server address."); return; }
    applyServerInput();
    if (!resolveServer(S.serverUrl)) { showServerError('Enter a server address.'); return; }
    $('#server-state').textContent = 'Connecting…';
    $('#connect-btn').disabled = true;
    openLobby({ autoJoin: true });
  }

  /** The Connect attempt ended without joining. */
  function connectEnded(message) {
    $('#connect-btn').disabled = false;
    $('#server-state').textContent = '';
    if (message) showServerError(message);
  }

  /** Remember a server that answered, most recent first, without duplicates, at most MAX_SERVERS. */
  function rememberServer(entry) {
    const next = [entry, ...S.servers.filter((x) => !sameServer(x, entry))].slice(0, MAX_SERVERS);
    if (JSON.stringify(next) !== JSON.stringify(S.servers)) { S.servers = next; changed(); }
  }

  // ------ player panel: name and colour
  function shade(hex, factor) {
    const n = parseInt(hex.slice(1), 16);
    return '#' + [(n >> 16) & 255, (n >> 8) & 255, n & 255].map((v) => Math.round(v * factor).toString(16).padStart(2, '0')).join('');
  }

  function buildSwatches() {
    const box = $('#swatches');
    for (const [label, hex] of SWATCHES) {
      const b = el('button', 'swatch');
      b.type = 'button';
      b.setAttribute('role', 'radio');
      b.setAttribute('aria-label', label);
      b.dataset.action = 'swatch';
      b.dataset.color = hex;
      b.style.background = hex;
      box.append(b);
    }
  }

  function syncPlayerPanel() {
    const color = S.playerColor;
    $('#pv-top').setAttribute('fill', color);
    $('#pv-left').setAttribute('fill', shade(color, 0.78)); // the same face shading the game uses
    $('#pv-right').setAttribute('fill', shade(color, 0.58));
    if ($('#color-input').value !== color) $('#color-input').value = color;
    const anySelected = SWATCHES.some(([, hex]) => hex === color);
    $$('#swatches .swatch').forEach((b, i) => {
      const selected = b.dataset.color === color;
      b.setAttribute('aria-checked', String(selected));
      // Roving focus: one tab stop for the group, the arrow keys move the selection.
      const entry = selected || (!anySelected && i === 0);
      b.tabIndex = entry ? 0 : -1;
      if (entry) b.dataset.nav = ''; else delete b.dataset.nav;
    });
    $('#player-toggle-dot').style.background = color;
    $('#player-toggle-text').textContent = cleanName(S.playerName) || 'Player';
  }

  function selectColor(hex) {
    if (!HEX.test(hex)) return;
    S.playerColor = hex.toLowerCase();
    changed();
  }

  function stepSwatch(direction) {
    const index = SWATCHES.findIndex(([, hex]) => hex === S.playerColor);
    const next = (Math.max(index, 0) + direction + SWATCHES.length) % SWATCHES.length;
    selectColor(SWATCHES[next][1]);
    $$('#swatches .swatch')[next].focus();
  }

  function setPlayerPanelOpen(open) {
    $('#player-panel').classList.toggle('open', open);
    $('#player-toggle').setAttribute('aria-expanded', String(open));
  }

  function showNameError(text) { const e = $('#name-error'); e.textContent = text; e.hidden = false; }
  function hideNameError() { $('#name-error').hidden = true; }

  // ------ state
  let lobby = null; // the socket in use: { ws, timers, timer(ms, fn) }
  let lobbyPhase = 'idle'; // idle | connecting | ready | failed (never worked) | closed (was working)
  let serverWorld = null; // the running world, or null if unknown
  let maps = null; // the maps list, or null if it is not known
  let mapsState = 'loading'; // loading | ok | failed
  let mapsProblem = '';
  let pending = null; // the request waiting for its answer: { type, map?, slot?, id?, timer }
  let busy = false; // a LoadMap or CreateMap is waiting for the server
  let highlightMap = null; // id of the map that was just created
  let focusHighlight = false;
  let autoFocused = null; // what showScreen focused on its own, so the Lobby may move focus on to Join
  let focusedByRender = false; // the render already put focus where it belongs, showScreen must not override it

  const lobbyOpen = () => !!lobby && lobby.ws.readyState === WebSocket.OPEN;

  const el = (tag, className, text) => {
    const node = document.createElement(tag);
    if (className) node.className = className;
    if (text !== undefined) node.textContent = text; // textContent: everything here comes from the server
    return node;
  };

  function setWorldsStatus(text) { $('#worlds-status').textContent = text; }
  function showMapsError(text) { const e = $('#maps-error'); e.textContent = text; e.hidden = false; }
  function hideMapsError() { $('#maps-error').hidden = true; }
  function showMapError(text) { const e = $('#newmap-error'); e.textContent = text; e.hidden = false; }
  function hideMapError() { $('#newmap-error').hidden = true; }

  /** Enable or disable what depends on who is playing, the connection and running requests. */
  function updateWorldControls() {
    const blocked = !!serverWorld && serverWorld.players > 0;
    const connected = lobbyOpen() && lobbyPhase === 'ready';
    $('#maps-blocked').hidden = !blocked;
    $('#join-btn').disabled = busy || lobbyPhase === 'connecting' || !serverWorld;
    $('#refresh-btn').disabled = busy;
    $('#map-submit').disabled = busy;
    for (const b of $$('#map-list .map-actions button')) {
      b.disabled = busy || blocked || !connected;
      if (blocked) b.setAttribute('aria-describedby', 'maps-blocked'); else b.removeAttribute('aria-describedby');
    }
  }

  function renderMapRow(m) {
    const row = el('div', 'map');
    row.setAttribute('role', 'listitem');
    row.dataset.mapId = m.id;
    if (m.id === highlightMap) { row.classList.add('highlight'); row.setAttribute('aria-current', 'true'); }
    row.append(el('h4', 'map-title', mapTitle(m.id, m.name)));
    row.append(el('p', m.description ? 'map-desc' : 'map-desc fallback', m.description || 'no description found'));
    row.append(el('p', 'map-meta', generatorLine(m.generator, m.seed, m.gamemode)));

    const actions = el('div', 'map-actions');
    const title = m.name || m.id;
    const add = (slot, label, aria) => {
      const b = el('button', 'item compact mini', label);
      b.type = 'button';
      b.dataset.nav = '';
      b.dataset.action = 'map-load';
      b.dataset.map = m.id;
      b.dataset.slot = String(slot);
      b.setAttribute('aria-label', aria);
      actions.append(b);
    };
    for (const save of m.saves) {
      add(save.slot, `Continue save ${save.slot} · ${formatModified(save.modified)}`, `Continue save ${save.slot} of ${title}, last changed ${formatModified(save.modified)}`);
    }
    add('new', 'New save', `Start a new save of ${title}`);
    row.append(actions);
    return row;
  }

  function renderServerState() {
    renderServerPicker();
    $('#card-server').textContent = serverLabel();
    const mapEl = $('#card-map');
    if (lobbyPhase === 'connecting') {
      mapEl.textContent = 'Loading…';
      $('#card-detail').textContent = '';
      $('#card-players').textContent = '';
    } else if (serverWorld) {
      const w = serverWorld;
      mapEl.textContent = mapTitle(w.id, w.name);
      $('#card-detail').textContent = [w.slot === null ? null : `Save slot ${w.slot}`, generatorLine(w.generator, w.seed, w.gamemode)].filter(Boolean).join(' · ');
      $('#card-players').textContent = playersText(w.players);
    } else {
      mapEl.textContent = 'Not available';
      $('#card-detail').textContent = '';
      $('#card-players').textContent = '';
    }

    const list = $('#map-list');
    list.textContent = '';
    let note;
    if (lobbyPhase === 'connecting' || (mapsState === 'loading' && lobbyPhase !== 'failed')) note = 'Loading…';
    else if (mapsState === 'failed' || maps === null) note = mapsProblem || MAPS_FAILED;
    else note = maps.length ? '' : 'No maps yet.';
    $('#maps-status').textContent = note;
    if (maps && mapsState === 'ok') for (const m of maps) list.append(renderMapRow(m));
    updateWorldControls();

    if (focusHighlight && highlightMap) {
      const row = $(`#map-list .map[data-map-id="${CSS.escape(highlightMap)}"]`);
      if (row) {
        focusHighlight = false;
        row.scrollIntoView({ block: 'nearest' });
        const target = $$('button', row).find((b) => !b.disabled);
        if (target) { target.focus({ preventScroll: true }); focusedByRender = true; }
      }
    }
  }

  function clearPending() {
    if (pending) clearTimeout(pending.timer);
    pending = null;
  }

  /** Drop the lobby socket and everything waiting on it. Safe to call at any time. */
  function closeLobby() {
    clearPending();
    busy = false;
    if (!lobby) { lobbyPhase = 'idle'; return; }
    const { ws, timers } = lobby;
    lobby = null;
    timers.forEach(clearTimeout);
    ws.onopen = ws.onmessage = ws.onclose = ws.onerror = null;
    try { ws.close(); } catch (_) { /* already closed */ }
    lobbyPhase = 'idle';
  }

  /** The lobby could not be reached or does not speak the protocol. */
  function lobbyFailed(message = UNREACHABLE) {
    const connecting = !!(lobby && lobby.autoJoin);
    closeLobby();
    lobbyPhase = 'failed';
    serverWorld = null;
    maps = null;
    mapsState = 'failed';
    mapsProblem = '';
    lobbyGenerators = null;
    setWorldsStatus(message);
    if (connecting) connectEnded(message);
    renderServerState();
    syncMapForm();
  }

  /** Send a lobby message. Returns false if there is no open connection. */
  function sendLobby(message) {
    if (!lobbyOpen()) return false;
    try { lobby.ws.send(JSON.stringify(message)); return true; } catch (_) { return false; }
  }

  function openLobby({ autoJoin = false } = {}) {
    closeLobby();
    serverWorld = null;
    maps = null;
    mapsState = 'loading';
    mapsProblem = '';
    lobbyGenerators = null;
    hideMapsError();
    setWorldsStatus('');
    const url = resolveServer(S.serverUrl);
    if (!url) {
      lobbyFailed('No server address.');
      return;
    }
    let ws;
    try { ws = new WebSocket(url); } catch (_) { lobbyFailed(); return; } // e.g. ws:// from an https page
    const mine = { ws, timers: [], autoJoin };
    mine.timer = (ms, fn) => { mine.timers.push(setTimeout(() => { if (lobby === mine) fn(); }, ms)); };
    lobby = mine;
    lobbyPhase = 'connecting';
    // One deadline for connecting and for the first answer: a server that accepts but stays silent is as
    // useless as one that is not there.
    mine.timer(TIMEOUTS.ready, () => { if (lobbyPhase === 'connecting') lobbyFailed(); });
    ws.onopen = () => { sendLobby({ type: 'ListMaps' }); };
    ws.onmessage = (event) => onLobbyMessage(mine, event);
    ws.onerror = () => { /* the close event follows and does the work */ };
    ws.onclose = () => { if (lobby === mine) onLobbyClosed(); };
    renderServerState();
  }

  function onLobbyClosed() {
    const was = lobbyPhase;
    const hadPending = pending;
    if (was === 'connecting') return lobbyFailed(); // refused, or closed before the Lobby arrived
    const connecting = !!(lobby && lobby.autoJoin);
    closeLobby();
    if (connecting) connectEnded(LOST);
    lobbyPhase = 'closed';
    setWorldsStatus(LOST);
    if (hadPending && hadPending.type === 'LoadMap') showMapsError('The connection was lost before the server answered.');
    if (hadPending && hadPending.type === 'CreateMap') showMapError('The connection was lost before the server answered.');
    renderServerState();
  }

  function onLobbyMessage(mine, event) {
    if (lobby !== mine || typeof event.data !== 'string' || event.data.length > 300000) return;
    let msg;
    try { msg = JSON.parse(event.data); } catch (_) { return; }
    if (!msg || typeof msg.type !== 'string') return;

    switch (msg.type) {
      case 'Lobby': {
        try { serverWorld = parseWorld(msg.world); } catch (_) { return lobbyFailed('The server sent a lobby that this page cannot read.'); }
        const gens = cleanGenerators(msg.generators);
        lobbyGenerators = gens.length ? gens : null;
        const first = lobbyPhase === 'connecting';
        lobbyPhase = 'ready';
        if (first && S.serverUrl !== '') rememberServer(S.serverUrl);
        if (first && mine.autoJoin) {
          startGame({ generator: serverWorld.generator, seed: serverWorld.seed ?? 0, create: false });
          if (!playing) { closeLobby(); connectEnded(''); } // no name yet: stay on the menu
          return;
        }
        if (mapsState === 'loading') {
          mine.timer(TIMEOUTS.answer, () => {
            if (mapsState === 'loading') { mapsState = 'failed'; mapsProblem = 'The server did not send the list of maps.'; renderServerState(); }
          });
        }
        renderServerState();
        syncMapForm();
        // Prefer Join once it is available, unless the player has already moved on.
        const active = document.activeElement;
        if (first && !focusHighlight && menuOpen && screen === 'worlds' && (active === document.body || active === autoFocused) && !$('#join-btn').disabled) {
          $('#join-btn').focus({ preventScroll: true });
        }
        break;
      }
      case 'Maps':
        try { maps = parseMaps(msg.maps); mapsState = 'ok'; } catch (_) { maps = null; mapsState = 'failed'; mapsProblem = MAPS_FAILED; }
        renderServerState();
        break;
      case 'WorldChanged': {
        let world;
        try { world = parseWorld(msg.world); } catch (_) { return; }
        serverWorld = world;
        // The answer to our own LoadMap: the server switched, so join what it is running now.
        if (pending && pending.type === 'LoadMap' && (world.id === pending.map || world.id === '')
            && (pending.slot === 'new' || world.slot === null || world.slot === pending.slot)) {
          const created = pending.slot === 'new';
          clearPending();
          busy = false;
          startGame({ generator: world.generator, seed: world.seed ?? 0, create: created });
          return;
        }
        renderServerState();
        break;
      }
      case 'MapCreated': {
        const created = parseMap(msg.map);
        if (!created) return;
        if (maps === null || mapsState !== 'ok') { maps = []; mapsState = 'ok'; }
        maps = [...maps.filter((m) => m.id !== created.id), created];
        if (pending && pending.type === 'CreateMap' && pending.id === created.id) {
          clearPending();
          busy = false;
          highlightMap = created.id;
          focusHighlight = true;
          for (const field of ['#map-id', '#map-name', '#map-desc']) $(field).value = '';
          if (previous[previous.length - 1] === 'worlds') previous.pop();
          showScreen('worlds', { push: false });
          announce(`Map ${created.id} created.`);
          blip(520);
        } else {
          renderServerState(); // created by someone else
        }
        break;
      }
      case 'Failed': {
        const message = typeof msg.message === 'string' && msg.message.trim() ? msg.message.trim().slice(0, 200) : 'The server refused the request.';
        switch (msg.request) {
          case 'LoadMap':
            if (pending && pending.type === 'LoadMap') clearPending();
            busy = false;
            showMapsError(message);
            // The refusal usually means the state changed (someone joined, a map is gone): look again.
            sendLobby({ type: 'GetWorld' });
            sendLobby({ type: 'ListMaps' });
            updateWorldControls();
            break;
          case 'CreateMap':
            if (pending && pending.type === 'CreateMap') clearPending();
            busy = false;
            showMapError(message);
            updateWorldControls();
            break;
          case 'ListMaps':
            mapsState = 'failed';
            mapsProblem = `${MAPS_FAILED} ${message}`;
            renderServerState();
            break;
          case 'GetWorld':
            setWorldsStatus(message);
            break;
          default:
            showMapsError(message);
        }
        break;
      }
      default: break; // unknown messages are ignored so newer servers keep working
    }
  }

  /** Open the lobby for the Host screens, unless a working one is already there. */
  function enterWorlds() {
    hideMapsError();
    if (lobby && (lobbyPhase === 'ready' || lobbyPhase === 'connecting')) renderServerState();
    else openLobby();
  }

  /** Join the world that is running right now. */
  function joinRunningWorld() {
    if (!serverWorld || busy) return;
    startGame({ generator: serverWorld.generator, seed: serverWorld.seed ?? 0, create: false });
  }

  /** Ask the server to load a save (or create a new one); it answers with WorldChanged or Failed. */
  function loadMap(mapId, slot) {
    if (busy) return;
    hideMapsError();
    if (!lobbyOpen() || lobbyPhase !== 'ready') { showMapsError('Not connected to the server. Press Refresh to reconnect.'); return; }
    busy = true;
    $('#maps-status').textContent = slot === 'new' ? 'Creating a new save…' : 'Loading the save…';
    pending = {
      type: 'LoadMap', map: mapId, slot,
      timer: setTimeout(() => {
        if (!pending || pending.type !== 'LoadMap') return;
        clearPending();
        busy = false;
        showMapsError('The server did not answer.');
        renderServerState();
      }, TIMEOUTS.load),
    };
    updateWorldControls();
    sendLobby({ type: 'LoadMap', map: mapId, slot });
  }

  // ------ create map
  /** Fill the generator dropdown and the seed field from the current settings. */
  function syncMapForm() {
    const select = $('#gen-select');
    const list = generators();
    select.textContent = '';
    for (const g of list) {
      const option = document.createElement('option');
      option.value = g.id;
      option.textContent = g.name;
      select.append(option);
    }
    if (!list.some((g) => g.id === S.generator)) S.generator = list[0].id;
    select.value = S.generator;
    const gen = generatorById(S.generator);
    $('#gen-desc').textContent = gen ? gen.description : '';
    const usesSeed = !gen || gen.uses_seed;
    const seedInput = $('#seed-input');
    seedInput.disabled = !usesSeed;
    $('#seed-random').disabled = !usesSeed;
    $('#seed-help').textContent = usesSeed ? 'The same seed always gives the same map. Whole numbers only.' : 'This generator does not use a seed.';
    if (document.activeElement !== seedInput) seedInput.value = String(S.seed);
  }

  function randomSeed() {
    try {
      const buf = new Uint32Array(1);
      crypto.getRandomValues(buf);
      return buf[0];
    } catch (_) {
      return Math.floor(Math.random() * 4294967295);
    }
  }

  function enterNewMap() {
    hideMapError();
    syncMapForm();
  }

  function onGeneratorChange() {
    S.generator = $('#gen-select').value;
    changed();
    syncMapForm();
  }

  function onSeedInput() {
    const text = $('#seed-input').value.trim();
    if (/^\d{1,15}$/.test(text)) { S.seed = Number(text); hideMapError(); changed(); }
  }

  function submitMap(e) {
    e.preventDefault();
    if (busy) return;
    hideMapError();
    const id = $('#map-id').value.trim();
    if (!MAP_ID.test(id)) return showMapError('The id must be 1 to 32 characters: lowercase letters, digits, - and _.');
    if (maps && maps.some((m) => m.id.toLowerCase() === id)) return showMapError('A map with that id already exists. Choose another id.');
    const gen = generatorById($('#gen-select').value);
    if (!gen) return showMapError('Please choose a generator.');
    let seed = S.seed;
    if (gen.uses_seed) {
      const text = $('#seed-input').value.trim();
      if (!/^\d{1,15}$/.test(text)) return showMapError('The seed must be a whole number, for example 12345.');
      seed = Number(text);
    }
    const name = $('#map-name').value.trim().slice(0, 60) || id;
    const description = $('#map-desc').value.trim().slice(0, 200);
    const frame = { type: 'CreateMap', id, name, description, generator: gen.id, seed, gamemode: $('#mode-select').value };
    if (new TextEncoder().encode(JSON.stringify(frame)).length > FRAME_LIMIT) return showMapError('The name and description are too long for the server. Please shorten them.');
    S.generator = gen.id;
    S.seed = seed;
    changed();
    if (!lobbyOpen() || lobbyPhase !== 'ready') return showMapError('Not connected to the server. Press Refresh on the Host screen.');

    busy = true;
    pending = {
      type: 'CreateMap', id,
      timer: setTimeout(() => {
        if (!pending || pending.type !== 'CreateMap') return;
        clearPending();
        busy = false;
        showMapError('The server did not answer, so the map may not have been created.');
        updateWorldControls();
      }, TIMEOUTS.create),
    };
    updateWorldControls();
    sendLobby(frame); // the answer is MapCreated or Failed
  }

  // ---------------------------------------------------------------------------------- state
  let screen = 'main';
  let previous = []; // screens to return to with Back
  let menuOpen = false;
  let playing = false;
  let capturing = null; // { action, slot, button } while waiting for a key to bind

  function status() { return window.wurfelStatus && typeof window.wurfelStatus === 'object' ? window.wurfelStatus : null; }

  function setMenuOpen(open) {
    menuOpen = open;
    window.wurfelMenuOpen = open;
    menu.hidden = !open;
    root.classList.toggle('menu-open', open);
    refreshHud();
  }

  function showScreen(name, { push = true } = {}) {
    if (push && screen && screen !== name) previous.push(screen);
    screen = name;
    menu.dataset.view = name; // the player panel is only shown beside some screens (not data-screen: sections use that)
    for (const el of $$('.screen', menu)) el.hidden = el.dataset.screen !== name;
    capturing = null;
    renderBindings();
    syncControls();
    renderServerPicker();
    refreshStatus();
    // The lobby connection lives only while the Host screens are open.
    if (name !== 'worlds' && name !== 'newmap') closeLobby();
    if (name !== 'worlds') highlightMap = null;
    if (name === 'worlds') enterWorlds();
    if (name === 'newmap') enterNewMap();
    if (!focusedByRender) focusFirst();
    focusedByRender = false;
    autoFocused = document.activeElement;
  }

  function openMenu(name) {
    previous = [];
    closeLobby(); // a fresh entry always starts from a fresh lobby connection
    setMenuOpen(true);
    showScreen(name, { push: false });
    if (playing) emit('wurfel:pause');
    // The game keeps its own set of held keys; make it let go of them.
    window.dispatchEvent(new Event('blur'));
  }

  function resume() {
    setMenuOpen(false);
    previous = [];
    emit('wurfel:resume');
    blip(660);
  }

  function back() {
    if (!$('#confirm').hidden) return closeConfirm();
    if (capturing) { capturing = null; return renderBindings(); }
    if (previous.length) {
      showScreen(previous.pop(), { push: false });
    } else if (playing && screen === 'pause') {
      resume();
    }
    blip(300);
  }

  /** Join the server's world. Fires wurfel:play and closes the menu. */
  function startGame({ generator, seed, create }) {
    const server = resolveServer(S.serverUrl);
    if (!server) { showServerError("That doesn't look like a server address."); return; }
    connectEnded('');
    const name = cleanName(S.playerName);
    if (!name) {
      setPlayerPanelOpen(true);
      showNameError('Enter a name (1 to 16 characters).');
      $('#player-name').focus();
      return;
    }
    closeLobby(); // the game opens its own connection
    if (name !== S.playerName) { S.playerName = name; changed(); }
    menuError = '';
    playing = true;
    previous = [];
    setMenuOpen(false);
    const request = { name, color: S.playerColor, server, generator, seed, create: !!create };
    // Remembered so a game that is still starting up (wasm not loaded yet) can pick it up.
    window.wurfelPlayRequest = request;
    emit('wurfel:play', request);
    blip(520);
  }

  function leave() {
    playing = false;
    window.wurfelPlayRequest = null;
    emit('wurfel:leave');
    openMenu('main');
    blip(260);
  }

  // ---------------------------------------------------------------------------------- settings UI
  function changed() {
    saveSettings();
    syncControls();
    applySettings();
    emit('wurfel:settings', S);
  }

  const info = $('#info');
  function applySettings() {
    // The wasm side writes help text and also error messages into #info; hide help but keep errors.
    info.hidden = !S.showHelp && !/^failed|error/i.test(info.textContent || '');
    $('#fps').hidden = !(S.showFps && (playing || menuOpen));
  }
  new MutationObserver(applySettings).observe(info, { childList: true, characterData: true, subtree: true });

  /** Push current settings into the form controls. */
  function syncControls() {
    for (const el of $$('[data-setting]', menu)) {
      const key = el.dataset.setting;
      if (el.type === 'checkbox') el.checked = !!S[key];
      else if (el.type === 'range') {
        const scale = Number(el.dataset.scale) || 1;
        el.value = Math.round(S[key] * scale);
        const out = el.parentElement.querySelector('output');
        if (out) out.textContent = Math.round(S[key] * scale) + '%';
        el.setAttribute('aria-valuetext', Math.round(S[key] * scale) + ' percent');
      } else if (document.activeElement !== el) el.value = S[key];
    }
    syncPlayerPanel();
    const fs = $('#fullscreen-btn .label');
    if (fs) fs.textContent = document.fullscreenElement ? 'Leave fullscreen' : 'Enter fullscreen';
    const st = status();
    const backend = st && st.backend ? `Graphics backend: ${st.backend}. ` : '';
    $('#backend-note').textContent = backend + 'V-Sync is controlled by the browser. WebGPU is used when available, otherwise WebGL2.';
    $('#storage-note').hidden = storageWorks;
  }

  function onSettingInput(e) {
    const el = e.target;
    const key = el.dataset && el.dataset.setting;
    if (!key) return;
    if (el.type === 'checkbox') S[key] = el.checked;
    else if (el.type === 'range') {
      const [lo, hi] = RANGES[key] || [0, 1];
      S[key] = Math.min(hi, Math.max(lo, Number(el.value) / (Number(el.dataset.scale) || 1)));
    } else {
      S[key] = key === 'playerName' ? el.value.replace(/[\u0000-\u001f\u007f]/g, '').slice(0, NAME_MAX) : el.value.slice(0, 200);
      for (const other of $$(`[data-setting="${key}"]`, menu)) if (other !== el) other.value = S[key];
      if (key === 'playerName') hideNameError();
    }
    changed();
  }

  // ---------------------------------------------------------------------------------- key bindings
  function renderBindings() {
    const box = $('#bindings');
    box.textContent = '';
    for (const [action, label] of ACTIONS) {
      const row = document.createElement('div');
      row.className = 'binding';
      row.setAttribute('role', 'row');
      const name = document.createElement('span');
      name.textContent = label;
      name.setAttribute('role', 'rowheader');
      row.append(name);
      for (const slot of [0, 1]) {
        const btn = document.createElement('button');
        btn.type = 'button';
        btn.className = 'slot';
        btn.dataset.nav = '';
        btn.dataset.action = 'rebind';
        btn.dataset.bind = action;
        btn.dataset.slot = String(slot);
        const key = S.keys[action][slot];
        const listening = capturing && capturing.action === action && capturing.slot === slot;
        btn.textContent = listening ? 'Press a key…' : keyLabel(key);
        btn.classList.toggle('empty', !key && !listening);
        btn.classList.toggle('listening', !!listening);
        btn.setAttribute('role', 'cell');
        btn.setAttribute('aria-label', `${label}, ${slot ? 'alternate' : 'primary'} key: ${listening ? 'waiting for input' : keyLabel(key)}. Activate to change.`);
        row.append(btn);
        if (listening) capturing.button = btn;
      }
      box.append(row);
    }
    if (capturing && capturing.button) capturing.button.focus();
  }

  function startCapture(action, slot) {
    capturing = { action, slot, button: null };
    renderBindings();
    announce(`Press a key or mouse button for ${ACTIONS.find((a) => a[0] === action)[1]}. Escape cancels.`);
  }

  function finishCapture(value) {
    const { action, slot } = capturing;
    let note = '';
    if (value) {
      // A key can only do one thing: take it away from any other slot.
      for (const [other] of ACTIONS) {
        S.keys[other].forEach((k, i) => {
          if (k === value && !(other === action && i === slot)) {
            S.keys[other][i] = '';
            note = ` (removed from ${ACTIONS.find((a) => a[0] === other)[1]})`;
          }
        });
      }
    }
    S.keys[action][slot] = value;
    capturing = null;
    renderBindings();
    changed();
    announce(`${ACTIONS.find((a) => a[0] === action)[1]}: ${keyLabel(value)}${note}`);
    blip(520);
    const again = $(`[data-bind="${action}"][data-slot="${slot}"]`);
    if (again) again.focus();
  }

  function announce(text) { $('#menu-status').textContent = text; }

  // The game reports failures (could not create or join a world, connection lost): show them and go
  // back to the main menu so the player can try again.
  let menuError = '';
  window.addEventListener('wurfel:error', (e) => {
    menuError = String((e.detail && e.detail.message) || 'Could not join.');
    playing = false;
    window.wurfelPlayRequest = null;
    openMenu('main');
    blip(260);
  });

  // The game calls this when the server runs another build than this page: a small banner offers a
  // reload. It never takes focus or blocks input, and the game only asks once per server build.
  window.wurfelUpdate = {
    show(serverBuild) {
      let box = $('#update-notice');
      if (!box) {
        box = document.createElement('div');
        box.id = 'update-notice';
        box.className = 'update-notice';
        box.setAttribute('role', 'status');
        const text = document.createElement('span');
        text.textContent = 'Update available – reload to get the new version.';
        const reload = document.createElement('button');
        reload.type = 'button';
        reload.textContent = 'Reload';
        reload.addEventListener('click', () => location.reload());
        const dismiss = document.createElement('button');
        dismiss.type = 'button';
        dismiss.className = 'dismiss';
        dismiss.setAttribute('aria-label', 'Dismiss');
        dismiss.textContent = '×';
        dismiss.addEventListener('click', () => { box.hidden = true; });
        box.append(text, reload, dismiss);
        document.body.appendChild(box);
      }
      box.title = serverBuild ? `Server build ${serverBuild}` : '';
      box.hidden = false;
    },
  };

  // ---------------------------------------------------------------------------------- status / HUD
  function refreshStatus() {
    const st = status();
    let text;
    if (capturing) return;
    if (menuError) { $('#menu-status').textContent = menuError; return; }
    if (playing && st) text = st.connected ? `Connected · ${st.players ?? 1} player${st.players === 1 ? '' : 's'}` : 'Connecting…';
    else if (playing) text = 'Playing';
    else if (st && st.connected) text = `Connected · ${st.players ?? 1} player${st.players === 1 ? '' : 's'}`;
    else text = 'Offline preview';
    $('#menu-status').textContent = text;
  }

  let frames = 0;
  let lastFpsTime = performance.now();
  let fps = 0;
  function refreshHud() {
    $('#menubtn').hidden = !(playing && !menuOpen);
    const pill = $('#netpill');
    const st = status();
    pill.hidden = !(playing && !menuOpen);
    if (!pill.hidden) {
      pill.textContent = st ? (st.connected ? `● online · ${st.players ?? 1} player${st.players === 1 ? '' : 's'}` : '○ connecting…') : '● playing';
    }
    applySettings();
    const fpsEl = $('#fps');
    const st2 = status();
    fpsEl.textContent = `${Math.round(st2 && Number.isFinite(st2.fps) ? st2.fps : fps)} FPS`;
  }

  // ---------------------------------------------------------------------------------- navigation
  function navItems() {
    const modal = $('#confirm');
    const visible = (el) => !el.disabled && el.offsetParent !== null;
    if (!modal.hidden) return $$('button', modal).filter(visible);
    const scope = $(`.screen[data-screen="${screen}"]`, menu);
    if (!scope) return [];
    // The player panel (when shown beside this screen) comes after the screen's own items.
    return [...$$('[data-nav]', scope), ...$$('[data-nav]', $('#player-panel'))].filter(visible);
  }

  function focusFirst() {
    if (!menuOpen) return;
    const items = navItems();
    // Prefer something to act on over a text field, so typing W/S does not start in a form.
    const first = items.find((el) => el.tagName === 'BUTTON') || items[0];
    if (first) first.focus({ preventScroll: true });
  }

  function move(delta) {
    const items = navItems();
    if (!items.length) return;
    const index = items.indexOf(document.activeElement);
    const next = items[(index + delta + items.length) % items.length];
    next.focus();
    next.scrollIntoView({ block: 'nearest' });
    blip(380 + 20 * (index % 4), 35);
  }

  function stepRange(el, direction) {
    const step = Number(el.step) || 1;
    el.value = String(Math.min(Number(el.max), Math.max(Number(el.min), Number(el.value) + direction * step)));
    el.dispatchEvent(new Event('input', { bubbles: true }));
  }

  function stepSelect(el, direction) {
    const next = Math.min(el.options.length - 1, Math.max(0, el.selectedIndex + direction));
    if (next === el.selectedIndex) return;
    el.selectedIndex = next;
    el.dispatchEvent(new Event('change', { bubbles: true }));
  }

  function isTyping(el) { return el && el.tagName === 'INPUT' && el.type === 'text'; }

  function trapTab(e) {
    const items = navItems();
    if (!items.length) return;
    const first = items[0];
    const last = items[items.length - 1];
    if (e.shiftKey && document.activeElement === first) { e.preventDefault(); last.focus(); }
    else if (!e.shiftKey && document.activeElement === last) { e.preventDefault(); first.focus(); }
  }

  // ---------------------------------------------------------------------------------- confirm dialog
  let confirmAction = null;
  function askConfirm(then) {
    confirmAction = then;
    $('#confirm').hidden = false;
    $('#confirm [data-action="confirm-no"]').focus();
  }
  function closeConfirm() {
    $('#confirm').hidden = true;
    confirmAction = null;
    focusFirst();
  }

  // ---------------------------------------------------------------------------------- actions
  function onClick(e) {
    const target = e.target.closest('[data-action]');
    if (!target) return;
    const action = target.dataset.action;
    switch (action) {
      case 'join-world': joinRunningWorld(); break;
      case 'connect': blip(440); connectToServer(); break;
      case 'swatch': selectColor(target.dataset.color); break;
      case 'map-load': loadMap(target.dataset.map, target.dataset.slot === 'new' ? 'new' : Number(target.dataset.slot)); break;
      case 'worlds-refresh': blip(440); openLobby(); break;
      case 'seed-random': S.seed = randomSeed(); $('#seed-input').value = String(S.seed); changed(); blip(440); break;
      case 'goto': blip(440); showScreen(target.dataset.target); break;
      case 'back': back(); break;
      case 'resume': resume(); break;
      case 'leave': leave(); break;
      case 'fullscreen':
        try {
          if (document.fullscreenElement) document.exitFullscreen();
          else document.documentElement.requestFullscreen();
        } catch (_) { announce('Fullscreen is not available here.'); }
        break;
      case 'reset': askConfirm(() => {
        Object.assign(S, defaults(), { keys: clone(DEFAULT_KEYS) });
        changed();
        announce('Settings reset to defaults.');
      }); break;
      case 'reset-keys':
        S.keys = clone(DEFAULT_KEYS);
        renderBindings();
        changed();
        announce('Default keys restored.');
        break;
      case 'rebind': startCapture(target.dataset.bind, Number(target.dataset.slot)); break;
      case 'credits-tab': selectCreditsTab(target.dataset.tab); break;
      case 'confirm-yes': { const run = confirmAction; closeConfirm(); if (run) run(); break; }
      case 'confirm-no': closeConfirm(); break;
      default: break;
    }
  }

  function selectCreditsTab(tab) {
    for (const t of $$('.tab', menu)) {
      const selected = t.dataset.tab === tab;
      t.setAttribute('aria-selected', String(selected));
      t.tabIndex = selected ? 0 : -1;
    }
    $('#credits-engine').hidden = tab !== 'engine';
    $('#credits-game').hidden = tab !== 'game';
  }

  $('#map-form').addEventListener('submit', submitMap);
  $('#gen-select').addEventListener('change', onGeneratorChange);
  $('#seed-input').addEventListener('input', onSeedInput);
  $('#server-form').addEventListener('submit', (e) => { e.preventDefault(); connectToServer(); });
  $('#server-input').addEventListener('input', hideServerError);
  $('#server-input').addEventListener('change', applyServerInput);
  $('#color-input').addEventListener('input', (e) => selectColor(e.target.value));
  $('#player-toggle').addEventListener('click', () => setPlayerPanelOpen(!$('#player-panel').classList.contains('open')));
  menu.addEventListener('click', onClick);
  $('#menubtn').addEventListener('click', () => openMenu('pause'));
  menu.addEventListener('input', onSettingInput);
  document.addEventListener('fullscreenchange', syncControls);

  // ---------------------------------------------------------------------------------- input capture
  const swallow = (e) => { e.stopImmediatePropagation(); };

  window.addEventListener('keydown', (e) => {
    const key = e.key.toLowerCase();

    if (capturing) {
      swallow(e);
      e.preventDefault();
      if (e.repeat) return; // the Enter/Space press that started the capture must not bind itself
      if (key === 'escape') { capturing = null; renderBindings(); refreshStatus(); }
      else if (key === 'backspace' || key === 'delete') finishCapture('');
      else if (!['dead', 'process', 'unidentified'].includes(key)) finishCapture(key);
      return;
    }

    if (!menuOpen) {
      if (key === 'escape' && playing && !e.repeat) { swallow(e); openMenu('pause'); }
      return;
    }

    // From here on a menu is open: the game never sees the key.
    swallow(e);
    const typing = isTyping(e.target);
    const modalOpen = !$('#confirm').hidden;

    if (key === 'escape') { e.preventDefault(); if (!e.repeat) back(); }
    else if (key === 'arrowdown' || (key === 's' && !typing)) { e.preventDefault(); move(1); }
    else if (key === 'arrowup' || (key === 'w' && !typing)) { e.preventDefault(); move(-1); }
    else if (key === 'tab') trapTab(e);
    else if ((key === 'arrowleft' || key === 'arrowright' || (!typing && (key === 'a' || key === 'd'))) && !modalOpen) {
      const el = document.activeElement;
      const dir = key === 'arrowleft' || key === 'a' ? -1 : 1;
      if (el && el.type === 'range' && (key === 'a' || key === 'd')) { e.preventDefault(); stepRange(el, dir); }
      else if (el && el.tagName === 'SELECT') { e.preventDefault(); stepSelect(el, dir); }
      else if (el && el.getAttribute('role') === 'radio') { e.preventDefault(); stepSwatch(dir); }
      else if (el && el.getAttribute('role') === 'tab') { e.preventDefault(); selectCreditsTab(el.dataset.tab === 'engine' ? 'game' : 'engine'); $(`#tab-${$('#credits-engine').hidden ? 'game' : 'engine'}`).focus(); }
      else if (el && !typing && el.type !== 'range') { e.preventDefault(); move(dir); }
    }
  }, true);

  window.addEventListener('keyup', (e) => { if (menuOpen || capturing) swallow(e); }, true);

  for (const type of ['mousedown', 'mouseup', 'mousemove', 'wheel', 'contextmenu']) {
    window.addEventListener(type, (e) => {
      if (capturing && type === 'mousedown') {
        e.preventDefault();
        swallow(e);
        return finishCapture('mouse' + e.button);
      }
      if (menuOpen) swallow(e); // clicks on the menu must not place blocks underneath
    }, true);
  }

  // ---------------------------------------------------------------------------------- gamepad
  const pad = { held: new Map(), lastStart: false };
  function pollGamepad(now) {
    let pads = [];
    try { pads = Array.from(navigator.getGamepads ? navigator.getGamepads() : []).filter(Boolean); } catch (_) { /* blocked */ }
    const note = $('#gamepad-note');
    note.textContent = pads.length ? `Gamepad: ${pads[0].id.slice(0, 60)}. D-pad or stick to move, A to select, B to go back, Start to pause.` : 'Gamepad: none detected. Press a button on it to wake it up.';
    const gp = pads[0];
    if (!gp) return;

    const down = (i) => !!(gp.buttons[i] && gp.buttons[i].pressed);
    const axis = (i) => gp.axes[i] || 0;
    const inputs = {
      up: down(12) || axis(1) < -0.6, down: down(13) || axis(1) > 0.6,
      left: down(14) || axis(0) < -0.6, right: down(15) || axis(0) > 0.6,
      a: down(0), b: down(1), start: down(9),
    };

    if (inputs.start && !pad.lastStart) {
      if (menuOpen && screen === 'pause') resume();
      else if (!menuOpen && playing) openMenu('pause');
    }
    pad.lastStart = inputs.start;
    if (!menuOpen) { pad.held.clear(); return; }

    for (const [name, pressed] of Object.entries(inputs)) {
      if (name === 'start') continue;
      if (!pressed) { pad.held.delete(name); continue; }
      const state = pad.held.get(name);
      // Fire on press, then repeat directions while held (A and B only fire once).
      if (!state) pad.held.set(name, { next: now + 400 });
      else if (name !== 'a' && name !== 'b' && now >= state.next) state.next = now + 120;
      else continue;
      if (name === 'up') move(-1);
      else if (name === 'down') move(1);
      else if (name === 'left' || name === 'right') {
        const el = document.activeElement;
        if (el && el.type === 'range') stepRange(el, name === 'left' ? -1 : 1);
        else if (el && el.tagName === 'SELECT') stepSelect(el, name === 'left' ? -1 : 1);
        else if (el && el.getAttribute('role') === 'radio') stepSwatch(name === 'left' ? -1 : 1);
        else move(name === 'left' ? -1 : 1);
      } else if (name === 'a') { const el = document.activeElement; if (el) el.click(); }
      else if (name === 'b') back();
    }
  }

  // ---------------------------------------------------------------------------------- loop
  let lastStatusRefresh = 0;
  function tick(now) {
    frames++;
    if (now - lastFpsTime >= 1000) {
      fps = (frames * 1000) / (now - lastFpsTime);
      frames = 0;
      lastFpsTime = now;
    }
    pollGamepad(now);
    if (now - lastStatusRefresh > 400) {
      lastStatusRefresh = now;
      if (menuOpen) refreshStatus();
      refreshHud();
    }
    requestAnimationFrame(tick);
  }

  // ---------------------------------------------------------------------------------- start
  buildSwatches();
  $('#player-toggle').dataset.nav = '';
  applySettings();
  syncControls();
  setMenuOpen(true);
  showScreen('main', { push: false });
  saveSettings(); // also reveals whether storage is usable
  emit('wurfel:settings', S);
  requestAnimationFrame(tick);

  // Handy for debugging and for tests.
  window.wurfelMenu = { openMenu, resume, leave, startGame, resolveServer, resolve, sanitize, defaults, generators, timeouts: TIMEOUTS };
})();

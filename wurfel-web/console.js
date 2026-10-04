/*
 * Wurfel Engine HTML console: a drop-down command line over the game, ported from the Java
 * engine's libGDX console (F1 toggles it, Tab completes, Up/Down walk the history).
 *
 * Open it with F1 or the backquote/tilde key (the key above Tab). Esc closes it.
 *
 * ---- Contract with the game (wasm side) ------------------------------------------------------
 *
 *   window.wurfelConsoleHost = {
 *     // Run one line. Resolves with {lines: [{level, text}], clear?: bool} or just the lines array.
 *     // level is 'info' | 'warn' | 'error' | 'echo' (case-insensitive). If no 'echo' line is
 *     // returned the console prints the typed line itself. `clear: true` wipes the log first.
 *     // The promise may take a while (commands forwarded to the server); the console shows a "..."
 *     // after the prompt meanwhile and gives up after 15 s.
 *     execute(line) -> Promise<Result> | Result,
 *
 *     // Tab completion. `prefix` is the text before the caret. Return complete replacement lines
 *     // (for prefix "ki" return ["killall"]; for "man ki" return ["man killall"]).
 *     suggest(prefix) -> string[] | Promise<string[]>,
 *
 *     // Optional: the prompt shown before the input, e.g. "alpha $ " (the path of `cd`).
 *     prompt: string | () => string,
 *   }
 *
 * The host may be installed or replaced at any time; until it exists the console says so.
 *
 * ---- What the console offers the game ---------------------------------------------------------
 *
 *   window.wurfelConsole.print(level, text)   append a line (network events, warnings...), also
 *                                             while the console is closed
 *   window.wurfelConsole.open() / close() / toggle() / clear() / isOpen()
 *   window.wurfelConsoleOpen                  true while it is open; the game must ignore keys then
 *                                             (this script also keeps gameplay from seeing the keys)
 *   'wurfel:console' event on window          detail: {open: bool}
 *
 * ---- Cooperation with the menu ----------------------------------------------------------------
 *
 * It does not open while window.wurfelMenuOpen is true and closes when the menu opens
 * ('wurfel:pause' / 'wurfel:leave'). This script must be loaded BEFORE menu.js so that Esc
 * closes the console instead of opening the pause menu.
 *
 * Development without the game: add ?consolemock to the URL to load console-mock.js.
 */
(() => {
  'use strict';
  if (window.wurfelConsole) return;

  const MAX_LINES = 1000;
  const MAX_HISTORY = 100;
  const HISTORY_KEY = 'wurfel.console.history';
  const ANSWER_TIMEOUT_MS = 15000;
  const LEVELS = new Set(['info', 'warn', 'error', 'echo']);

  // ------------------------------------------------------------------------------------ helpers

  function el(tag, attrs, text) {
    const node = document.createElement(tag);
    for (const [k, v] of Object.entries(attrs || {})) node.setAttribute(k, v);
    if (text != null) node.textContent = text;
    return node;
  }

  function normaliseLevel(level) {
    const l = String(level || 'info').toLowerCase();
    return LEVELS.has(l) ? l : 'info';
  }

  function withTimeout(promise, ms) {
    return new Promise((resolve, reject) => {
      const timer = setTimeout(() => reject(new Error(`no answer after ${ms / 1000} s`)), ms);
      promise.then(
        (v) => { clearTimeout(timer); resolve(v); },
        (e) => { clearTimeout(timer); reject(e); }
      );
    });
  }

  function commonPrefix(words) {
    let prefix = words[0] || '';
    for (const w of words) {
      let i = 0;
      while (i < prefix.length && i < w.length && prefix[i] === w[i]) i++;
      prefix = prefix.slice(0, i);
    }
    return prefix;
  }

  // Browsers can throw on storage access (private windows, blocked site data).
  function loadHistory() {
    try {
      const parsed = JSON.parse(localStorage.getItem(HISTORY_KEY) || '[]');
      return Array.isArray(parsed) ? parsed.filter((s) => typeof s === 'string').slice(-MAX_HISTORY) : [];
    } catch (_) {
      return [];
    }
  }

  function saveHistory(history) {
    try { localStorage.setItem(HISTORY_KEY, JSON.stringify(history)); } catch (_) { /* not persisted */ }
  }

  // --------------------------------------------------------------------------------------- DOM

  const root = el('section', { id: 'wconsole', class: 'wconsole', role: 'region', 'aria-label': 'Console' });
  root.setAttribute('inert', ''); // closed: not focusable, not read out
  const log = el('div', {
    id: 'wconsole-log', class: 'wc-log', role: 'log', 'aria-live': 'polite', 'aria-relevant': 'additions',
  });
  const form = el('form', { class: 'wc-form', autocomplete: 'off' });
  const label = el('label', { for: 'wconsole-input', class: 'wc-sr' }, 'Console command');
  const promptEl = el('span', { class: 'wc-prompt', 'aria-hidden': 'true' });
  const input = el('input', {
    id: 'wconsole-input', class: 'wc-input', type: 'text', spellcheck: 'false', autocomplete: 'off',
    autocapitalize: 'off', autocorrect: 'off', enterkeyhint: 'send',
  });
  const hint = el('div', { class: 'wc-hint' },
    'F1 or ` closes · Tab completes · ↑↓ history · Ctrl+L clears · Esc closes');
  form.append(label, promptEl, input);
  root.append(log, form, hint);

  // ----------------------------------------------------------------------------------- state

  const state = {
    open: false,
    pending: 0, // commands waiting for an answer
    queue: Promise.resolve(), // commands run one after the other
    history: loadHistory(),
    historyPos: -1, // -1 = not browsing; else index into history
    draft: '',
    tab: null, // {before, after, list, index, value}
    previousFocus: null,
    poll: 0,
  };

  const host = () => window.wurfelConsoleHost;

  function promptText() {
    const h = host();
    let p = h && (typeof h.prompt === 'function' ? h.prompt() : h.prompt);
    p = typeof p === 'string' ? p : ' $ ';
    return p;
  }

  function refreshPrompt() {
    promptEl.textContent = promptText();
  }

  // ------------------------------------------------------------------------------------- log

  function atBottom() {
    return log.scrollHeight - log.scrollTop - log.clientHeight < 24;
  }

  function addLine(level, text, extraClass) {
    const stick = atBottom();
    const line = el('div', { class: `wc-line wc-${normaliseLevel(level)}${extraClass ? ' ' + extraClass : ''}` });
    line.textContent = String(text); // never innerHTML: log text can contain anything
    log.append(line);
    while (log.childElementCount > MAX_LINES) log.firstElementChild.remove();
    if (stick) log.scrollTop = log.scrollHeight;
  }

  function clearLog() {
    log.replaceChildren();
  }

  // ------------------------------------------------------------------------------ open / close

  function dispatchState() {
    window.dispatchEvent(new CustomEvent('wurfel:console', { detail: { open: state.open } }));
  }

  function open() {
    if (state.open) return true;
    if (window.wurfelMenuOpen) return false; // the menu owns the keyboard
    state.previousFocus = document.activeElement;
    state.open = true;
    window.wurfelConsoleOpen = true;
    refreshPrompt();
    root.removeAttribute('inert');
    root.classList.add('open');
    log.scrollTop = log.scrollHeight;
    input.focus({ preventScroll: true });
    // Notice the menu opening in a way that dispatches no event.
    state.poll = setInterval(() => { if (window.wurfelMenuOpen) close(false); }, 300);
    dispatchState();
    return true;
  }

  function close(restoreFocus = true) {
    if (!state.open) return;
    state.open = false;
    window.wurfelConsoleOpen = false;
    clearInterval(state.poll);
    root.classList.remove('open');
    root.setAttribute('inert', '');
    input.blur();
    const back = state.previousFocus;
    state.previousFocus = null;
    if (restoreFocus && back && back !== document.body && document.contains(back) && !root.contains(back)) {
      back.focus({ preventScroll: true });
    }
    dispatchState();
  }

  function toggle() {
    if (state.open) close();
    else open();
  }

  // ------------------------------------------------------------------------------ running lines

  function setPending(delta) {
    state.pending = Math.max(0, state.pending + delta);
    root.classList.toggle('busy', state.pending > 0);
    log.setAttribute('aria-busy', String(state.pending > 0));
  }

  function normaliseResult(result) {
    if (Array.isArray(result)) return { lines: result, clear: false };
    return { lines: (result && result.lines) || [], clear: !!(result && result.clear) };
  }

  function render(result, typed, secret) {
    const { lines, clear } = normaliseResult(result);
    if (clear) clearLog();
    const echoed = lines.some((l) => normaliseLevel(l.level) === 'echo');
    if (!echoed && !clear) addLine('echo', promptText() + (secret ? 'auth ********' : typed));
    for (const l of lines) addLine(l.level, l.text);
    refreshPrompt(); // `cd` changes it
  }

  async function run(typed, secret) {
    const h = host();
    if (!h || typeof h.execute !== 'function') {
      addLine('echo', promptText() + (secret ? 'auth ********' : typed));
      addLine('error', 'No console host is connected yet: the game is still loading, or it was built without one.');
      return;
    }
    setPending(1);
    let result;
    try {
      result = await withTimeout(Promise.resolve(h.execute(typed)), ANSWER_TIMEOUT_MS);
    } catch (e) {
      result = { lines: [{ level: 'error', text: `console host error: ${(e && e.message) || e}` }] };
    }
    setPending(-1);
    render(result, typed, secret);
  }

  function submit() {
    const typed = input.value;
    input.value = '';
    state.historyPos = -1;
    state.tab = null;
    if (!typed.trim()) {
      addLine('echo', promptText()); // like the Java console: an empty line just shows the prompt
      return;
    }
    // Admin tokens are neither shown nor remembered.
    const secret = /^\s*auth(\s|$)/i.test(typed);
    if (!secret && state.history[state.history.length - 1] !== typed) {
      state.history.push(typed);
      if (state.history.length > MAX_HISTORY) state.history.shift();
      saveHistory(state.history);
    }
    state.queue = state.queue.then(() => run(typed, secret));
  }

  // ---------------------------------------------------------------------------------- history

  function historyUp() {
    if (!state.history.length) return;
    if (state.historyPos === -1) {
      state.draft = input.value;
      state.historyPos = state.history.length - 1;
    } else if (state.historyPos > 0) {
      state.historyPos -= 1;
    }
    input.value = state.history[state.historyPos];
  }

  function historyDown() {
    if (state.historyPos === -1) return;
    if (state.historyPos < state.history.length - 1) {
      state.historyPos += 1;
      input.value = state.history[state.historyPos];
    } else {
      state.historyPos = -1;
      input.value = state.draft; // back to what was being typed
    }
  }

  // --------------------------------------------------------------------------- tab completion

  function flashBad() {
    form.classList.add('wc-bad');
    setTimeout(() => form.classList.remove('wc-bad'), 160);
  }

  function setValue(value, caretAtEnd = true) {
    input.value = value;
    if (caretAtEnd) input.setSelectionRange(value.length, value.length);
  }

  async function complete() {
    const h = host();
    if (!h || typeof h.suggest !== 'function') return flashBad();

    // Pressing Tab again on the text a previous Tab produced cycles through the candidates.
    const t = state.tab;
    if (t && t.value === input.value && t.list.length > 1) {
      t.index = (t.index + 1) % t.list.length;
      t.value = t.list[t.index] + t.after;
      return setValue(t.value);
    }

    const caret = input.selectionStart == null ? input.value.length : input.selectionStart;
    const before = input.value.slice(0, caret);
    const after = input.value.slice(caret);
    let list;
    try {
      list = await Promise.resolve(h.suggest(before));
    } catch (_) {
      list = [];
    }
    if (input.value !== before + after) return; // the user typed on while we waited
    list = [...new Set((Array.isArray(list) ? list : []).filter((s) => typeof s === 'string'))];
    if (!list.length) return flashBad();

    if (list.length === 1) {
      state.tab = null;
      return setValue(list[0] + ' ' + after.replace(/^\s+/, ''));
    }

    // Several candidates: list them once, complete the part they share, and let further Tab
    // presses cycle through them.
    const shared = commonPrefix(list);
    addLine('info', list.map((s) => s.slice(s.lastIndexOf(' ') + 1)).join('  '), 'wc-cands');
    if (shared.length > before.length) {
      // index at the end so that the next Tab wraps round to the first candidate
      state.tab = { after, list, index: list.length - 1, value: shared + after };
    } else {
      state.tab = { after, list, index: 0, value: list[0] + after };
    }
    setValue(state.tab.value);
  }

  // ------------------------------------------------------------------------------------ input

  input.addEventListener('keydown', (e) => {
    // Typing must not reach the game, whose key listeners sit on `window`. Stopping here, on the
    // target, keeps the key out of window's bubble phase but still lets the field use it.
    e.stopPropagation();

    switch (e.key) {
      case 'Enter':
        e.preventDefault();
        return submit();
      case 'ArrowUp':
        e.preventDefault();
        return historyUp();
      case 'ArrowDown':
        e.preventDefault();
        return historyDown();
      case 'Tab':
        e.preventDefault();
        return void complete();
      case 'PageUp':
        e.preventDefault();
        log.scrollTop -= log.clientHeight * 0.9;
        return;
      case 'PageDown':
        e.preventDefault();
        log.scrollTop += log.clientHeight * 0.9;
        return;
      default:
    }
    if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === 'l') {
      e.preventDefault(); // otherwise the browser jumps to the address bar
      return clearLog();
    }
    // Editing the text ends a completion cycle; pure modifier presses do not.
    if (e.key.length === 1 || e.key === 'Backspace' || e.key === 'Delete') state.tab = null;
  });
  // keyup is NOT stopped: a key held down when the console opened (running left, say) must still be
  // released in the game, or the player would keep walking.
  input.addEventListener('keypress', (e) => e.stopPropagation());
  form.addEventListener('submit', (e) => e.preventDefault());

  // Clicking the log selects text for copying; only a plain click puts the caret back in the field.
  root.addEventListener('mouseup', () => {
    const selection = window.getSelection && window.getSelection();
    if (!selection || selection.isCollapsed) input.focus({ preventScroll: true });
  });

  // ---------------------------------------------------------------------------- global shortcuts

  function isTypingElsewhere(target) {
    if (!target || root.contains(target) || !target.tagName) return false;
    const tag = target.tagName;
    return tag === 'INPUT' || tag === 'TEXTAREA' || tag === 'SELECT' || target.isContentEditable;
  }

  // Capture phase on window: runs before the game's and the menu's key handlers.
  window.addEventListener('keydown', (e) => {
    const isToggle = e.key === 'F1' || (e.code === 'Backquote' && !e.ctrlKey && !e.metaKey && !e.altKey);
    if (isToggle) {
      // The backquote must stay typeable in other text fields (server address, world name...).
      if (!state.open && (window.wurfelMenuOpen || (e.key !== 'F1' && isTypingElsewhere(e.target)))) return;
      e.preventDefault(); // F1 would open the browser's help
      e.stopImmediatePropagation();
      toggle();
      return;
    }
    if (!state.open) return;
    if (window.wurfelMenuOpen) return close(false);
    if (e.key === 'Escape') {
      e.preventDefault();
      e.stopImmediatePropagation(); // so the menu does not take the same Esc
      return close();
    }
    if (!root.contains(e.target) && !['Shift', 'Control', 'Alt', 'Meta'].includes(e.key)) {
      // Focus drifted (a click on the canvas, say). Keep keys away from the game and bring the
      // caret back; the browser delivers the character to whatever is focused when it types.
      e.stopImmediatePropagation();
      input.focus({ preventScroll: true });
    }
  }, true);

  window.addEventListener('wurfel:pause', () => close(false));
  window.addEventListener('wurfel:leave', () => close(false));

  // ---------------------------------------------------------------------------------- public API

  window.wurfelConsoleOpen = false;
  window.wurfelConsole = {
    open,
    close: () => close(),
    toggle,
    clear: clearLog,
    isOpen: () => state.open,
    print(level, text) {
      addLine(level, text);
    },
  };

  document.body.append(root);
  refreshPrompt();
  addLine('info', 'Wurfel Engine console. Type `help` to list the commands. F1 or ` toggles this window.');

  // Development without the game: ?consolemock loads a stand-in host.
  if (/[?&]consolemock(?:[=&]|$)/.test(location.search)) {
    document.head.append(el('script', { src: 'console-mock.js' }));
  }
})();

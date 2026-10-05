// The console's connection to the game (see the contract at the top of console.js): lines that
// start with one of the Caveland commands go to the server, which answers on its own.
//
// The engine's own console commands (cvars, `ls`, `man`...) are not connected to the running game
// yet; `help` says so. The server only lets the host (the player who has been there longest) run
// the commands: they are cheats.
(function () {
  'use strict';
  const COMMANDS = {
    give: 'give <item>: puts a collectible into your pack (Wood, Coal, Torch, Explosives, Iron...)',
    tpplayer: 'tpplayer <x> <y> <z> [player]: teleports a player to a block',
    portaltarget: 'portaltarget <x> <y> <z>: sets where the nearest portal leads',
  };
  const ANSWER_TIMEOUT_MS = 8000;
  const waiting = [];

  const lines = (...texts) => ({ lines: texts.map(([level, text]) => ({ level, text })) });

  window.wurfelConsoleHost = {
    prompt: () => (window.wurfelHud && window.wurfelHud.active ? 'caveland $ ' : '$ '),

    execute(line) {
      const text = String(line).trim().replace(/^[\/:]/, '');
      const name = text.split(/\s+/)[0];
      if (name === 'help' || name === '?') {
        return lines(['info', 'editor [on|off]: switch the map editor on or off (F2); only there the mouse changes blocks'], ...Object.values(COMMANDS).map((m) => ['info', m]),
          ['info', 'Commands only work in Caveland maps, for the host. Engine commands are not connected yet.']);
      }
      if (name === 'editor') {
        // Java `editor`: switch to the map editor (again to leave it). `editor on|off` is explicit.
        const mode = text.split(/\s+/)[1];
        if (!window.wurfelEditor) return lines(['error', 'The editor is not available in this page.']);
        const refused = window.wurfelEditor.toggle(mode);
        if (refused) return lines(['error', refused]);
        return lines(['info', window.wurfelEditor.active ? 'Editor on. Use F2 or `editor` to leave.' : 'Editor off.']);
      }
      if (!Object.prototype.hasOwnProperty.call(COMMANDS, name)) {
        return lines(['error', `${text}: command not found (try help)`]);
      }
      if (!(window.wurfelHud && window.wurfelHud.active)) {
        return lines(['error', 'This map does not use Caveland rules: no game commands here.']);
      }
      if (!window.wurfelNet || typeof window.wurfelNet.command !== 'function') {
        return lines(['error', 'Not connected to a game.']);
      }
      return new Promise((resolve) => {
        const entry = { resolve, timer: setTimeout(() => {
          waiting.splice(waiting.indexOf(entry), 1);
          resolve(lines(['error', 'The server did not answer.']));
        }, ANSWER_TIMEOUT_MS) };
        waiting.push(entry);
        window.wurfelNet.command(text);
      });
    },

    suggest(prefix) {
      const typed = String(prefix).replace(/^[\/:]/, '');
      if (/\s/.test(typed)) return [];
      return ['help', 'editor', ...Object.keys(COMMANDS)].filter((c) => c.startsWith(typed));
    },

    /** The server's answer, as JSON {ok, text}, called by the client. */
    reply(json) {
      let answer;
      try { answer = typeof json === 'string' ? JSON.parse(json) : json; } catch (e) { return; }
      const entry = waiting.shift();
      if (!entry) return;
      clearTimeout(entry.timer);
      entry.resolve(lines([answer.ok ? 'info' : 'error', String(answer.text || (answer.ok ? 'done' : 'failed'))]));
    },
  };
})();

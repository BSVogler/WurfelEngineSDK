// The console's connection to the game (see the contract at the top of console.js). The game runs
// every line itself (the engine's console, `wurfel-web/src/web/console.rs`): client commands and
// this browser's cvars are answered at once; commands that change the shared world and the game
// mode's own commands (Caveland: give, tpplayer, portaltarget, place) go to the server, which
// answers through `reply`. The server lets only the host (the player who has been there longest)
// change the world or use the cheats.
(function () {
  'use strict';
  const ANSWER_TIMEOUT_MS = 8000;
  const waiting = [];

  const lines = (...texts) => ({ lines: texts.map(([level, text]) => ({ level, text })) });
  const net = (name) => window.wurfelNet && typeof window.wurfelNet[name] === 'function' ? window.wurfelNet[name] : null;

  window.wurfelConsoleHost = {
    prompt: () => {
      const prompt = net('prompt');
      return prompt ? prompt() : '$ ';
    },

    execute(line) {
      const command = net('command');
      if (!command) return lines(['error', 'The game is not loaded yet.']);
      let result;
      try { result = JSON.parse(command(String(line))); } catch (e) { return lines(['error', `Command crashed: ${e}`]); }
      if (!result.forwarded) return result;
      const echo = result.lines || [];
      return new Promise((resolve) => {
        const entry = {
          echo,
          resolve,
          timer: setTimeout(() => {
            waiting.splice(waiting.indexOf(entry), 1);
            resolve({ lines: [...echo, { level: 'error', text: 'The server did not answer.' }] });
          }, ANSWER_TIMEOUT_MS),
        };
        waiting.push(entry);
      });
    },

    suggest(prefix) {
      const suggest = net('suggest');
      return suggest ? Array.from(suggest(String(prefix))) : [];
    },

    /** The server's answer to a forwarded line, as JSON {lines}, called by the client. */
    reply(json) {
      let answer;
      try { answer = typeof json === 'string' ? JSON.parse(json) : json; } catch (e) { return; }
      const answered = Array.isArray(answer.lines) ? answer.lines : [];
      const entry = waiting.shift();
      if (entry) {
        clearTimeout(entry.timer);
        entry.resolve({ lines: [...entry.echo, ...answered] });
      } else if (window.wurfelConsole) {
        // Nobody typed it (`?auth=` in the address): just show it.
        for (const line of answered) window.wurfelConsole.print(line.level, line.text);
      }
    },
  };
})();

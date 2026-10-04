/*
 * Stand-in console host for developing and testing the console UI without the game.
 * Only loaded when the page URL contains ?consolemock (see console.js). The real host comes from
 * the wasm client and is backed by wurfel_sim::console.
 *
 * Commands: help, get/set <cvar> [value], cd, clear, echo <text>, slow (answers after 1.5 s),
 * fail (a host error), big (200 lines), multi (one line with line breaks), auth <token> (secret: "open").
 */
(() => {
  'use strict';
  if (window.wurfelConsoleHost) return;

  const cvars = { gravity: '9.81', friction: '0.001', music: '1.0', sound: '1.0', generator: 'island', enableHSD: 'true' };
  const commands = ['auth', 'big', 'cd', 'clear', 'echo', 'fail', 'get', 'help', 'killall', 'multi', 'set', 'slow'];
  const worlds = ['alpha', 'beta'];
  let path = '';
  let admin = false;

  const info = (text) => ({ level: 'info', text });
  const error = (text) => ({ level: 'error', text });
  const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

  function cvarLine(name) {
    const key = Object.keys(cvars).find((k) => k.toLowerCase() === name.toLowerCase());
    return key ? info(`cvar ${key.toLowerCase()} has value ${cvars[key]}`) : null;
  }

  window.wurfelConsoleHost = {
    get prompt() { return `${path} $ `; },

    async execute(line) {
      const echo = { level: 'echo', text: `${path} $ ${/^\s*auth(\s|$)/i.test(line) ? 'auth ********' : line}` };
      const [first = '', ...rest] = line.trim().split(/\s+/);
      const name = first.toLowerCase();
      const out = [echo];
      switch (name) {
        case 'help':
          out.push(info(commands.map((c) => c.padEnd(8)).join('')));
          out.push({ level: 'warn', text: 'mock host: these commands only exist for testing the UI' });
          break;
        case 'clear':
          return { lines: [], clear: true };
        case 'echo':
          out.push(info(rest.join(' ')));
          break;
        case 'slow':
          await sleep(1500);
          out.push(info('done after 1.5 s'));
          break;
        case 'fail':
          throw new Error('the mock host failed on purpose');
        case 'big':
          for (let i = 1; i <= 200; i++) out.push(info(`line ${i} of 200`));
          break;
        case 'multi':
          out.push(info('first line\nsecond line\n  indented third line'));
          break;
        case 'killall':
          out.push(admin ? info('disposed 3 entities') : error('killall: permission denied. Log in with `auth <token>`'));
          if (!admin) out.push(error('Failed executing command.'));
          break;
        case 'auth':
          admin = rest[0] === 'open';
          out.push(admin ? info('logged in as administrator') : error('wrong token'));
          break;
        case 'cd': {
          const target = rest[0] === '/' || rest[0] === '..' ? '' : rest[0];
          if (target === undefined) { out.push(error('Parameter missing'), error('Failed executing command.')); break; }
          if (target !== '' && !worlds.includes(target)) { out.push(info('not a valid path')); break; }
          path = target;
          break;
        }
        case 'set':
        case 'get': {
          const [cvar, ...value] = rest;
          if (cvar && value.length && name === 'set') cvars[Object.keys(cvars).find((k) => k.toLowerCase() === cvar.toLowerCase()) || cvar] = value.join(' ');
          out.push(cvar && cvarLine(cvar) || error(`${cvar || ''}: command not found`), ...(cvar && cvarLine(cvar) ? [] : [error('Failed executing command.')]));
          break;
        }
        case '':
          break;
        default: {
          const asCvar = cvarLine(name);
          if (asCvar) {
            if (rest.length) cvars[Object.keys(cvars).find((k) => k.toLowerCase() === name)] = rest.join(' ');
            out.push(cvarLine(name));
          } else {
            out.push(error(`${line}: command not found`), error('Failed executing command.'));
          }
        }
      }
      return { lines: out };
    },

    async suggest(prefix) {
      const m = /^(.*?)(\S*)$/.exec(prefix);
      const head = m[1];
      const word = m[2].toLowerCase();
      const words = head.trim().split(/\s+/).filter(Boolean);
      let pool;
      if (words.length === 0) pool = [...commands, ...Object.keys(cvars).map((k) => k.toLowerCase())];
      else if (words[0] === 'cd') pool = ['/', '..', ...worlds];
      else if (words[0] === 'set' || words[0] === 'get') pool = Object.keys(cvars).map((k) => k.toLowerCase());
      else if (words[0] === 'generator') pool = ['island', 'air', 'caveland'];
      else pool = [];
      return [...new Set(pool.filter((c) => c.toLowerCase().startsWith(word)))].sort().map((c) => head + c);
    },
  };

  if (window.wurfelConsole) window.wurfelConsole.print('warn', 'Using the mock console host (?consolemock).');
})();

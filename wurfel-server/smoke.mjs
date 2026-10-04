// End-to-end check against a running server:  node smoke.mjs [http://127.0.0.1:3000]
// The server should be started on an empty maps folder (e.g. --maps-dir /tmp/x). Set LAG_MS to the server's
// --lag-ms value to also check that the simulated delay shows up in pings.
const base = (process.argv[2] ?? 'http://127.0.0.1:3000').replace(/\/$/, '');
const url = base.replace(/^http/, 'ws') + '/ws';
const lag = Number(process.env.LAG_MS ?? 0);
const sleep = ms => new Promise(r => setTimeout(r, ms));
let failures = 0;
const check = (ok, what) => { console.log(ok ? 'ok  ' : 'FAIL', what); if (!ok) failures++; };

function decodeChunk(buf) { // header: kind, cx, cy; then runs of (u16 count, id, value, health) in layer order
  const v = new DataView(buf.buffer, buf.byteOffset, buf.byteLength);
  const cells = new Array(4000); let index = 0;
  for (let p = 9; p < buf.length; p += 5) { const n = v.getUint16(p, true); for (let i = 0; i < n; i++) cells[index++] = [buf[p + 2], buf[p + 3]]; }
  return { kind: buf[0], cx: v.getInt32(1, true), cy: v.getInt32(5, true), count: index, at: (lx, ly, z) => cells[z * 400 + lx * 40 + ly] };
}

function client() {
  const ws = new WebSocket(url); ws.binaryType = 'arraybuffer';
  const c = { ws, log: [], chunks: [], send: m => ws.send(JSON.stringify(m)) };
  ws.onmessage = e => { if (typeof e.data === 'string') c.log.push(JSON.parse(e.data)); else c.chunks.push(decodeChunk(new Uint8Array(e.data))); };
  c.ready = new Promise((res, rej) => { ws.onopen = res; ws.onerror = () => rej(new Error('cannot connect')); });
  c.find = type => c.log.find(m => m.type === type);
  c.last = type => [...c.log].reverse().find(m => m.type === type);
  c.me = () => c.last('Snapshot')?.players.find(p => p.id === c.find('Welcome').your_id);
  c.wait = async (type, ms = 1500) => { for (let t = 0; t < ms; t += 50) { const m = c.log.find(x => x.type === type); if (m) return m; await sleep(50); } };
  return c;
}
let inputSeq = 0;
const keys = (o = {}) => ({ type: 'Input', seq: ++inputSeq, input: { up: false, down: false, left: false, right: false, jump: false, ...o } });

// ---- lobby
const lobby = client(); await lobby.ready;
const hello = await lobby.wait('Lobby');
check(hello && hello.world.players === 0 && hello.generators.some(g => g.id === 'island') && hello.generators.length >= 6, 'a new connection starts in the lobby and learns the world and the generators');
lobby.send({ type: 'ListMaps' });
const maps = await lobby.wait('Maps');
check(maps && maps.maps.length >= 1 && maps.maps.every(m => m.id && Array.isArray(m.saves)), 'ListMaps lists the maps with their saves');
lobby.send({ type: 'CreateMap', id: 'smoke-map', name: 'Smoke map', description: 'test', generator: 'caveland', seed: 3, gamemode: 'engine' });
const made = await lobby.wait('MapCreated');
check(made?.map.id === 'smoke-map' && made.map.gamemode === 'engine', 'CreateMap makes a map and remembers the game mode');
lobby.send({ type: 'CreateMap', id: '../evil', name: 'x', description: '', generator: 'island', seed: 1 });
check((await lobby.wait('Failed'))?.request === 'CreateMap', 'a hostile map id is refused');
lobby.send({ type: 'CreateMap', id: 'bad-gen', name: 'x', description: '', generator: 'nope', seed: 1 });
await sleep(300);
check(lobby.log.filter(m => m.type === 'Failed').length === 2, 'an unknown generator is refused');
lobby.send({ type: 'CreateMap', id: 'bad-mode', name: 'x', description: '', generator: 'island', seed: 1, gamemode: 'chess' });
await sleep(300);
check(lobby.log.filter(m => m.type === 'Failed').length === 3 && lobby.last('Failed').message.includes('chess'), 'an unknown game mode is refused');

// ---- load a map while the server is empty
lobby.send({ type: 'LoadMap', map: 'smoke-map', slot: 'new' });
const changed = await lobby.wait('WorldChanged');
check(changed && changed.world.map_id === 'smoke-map' && changed.world.generator === 'caveland', 'LoadMap (new save) switches the world and tells the lobby');

// ---- join and stream
const a = client(); await a.ready; await a.wait('Lobby');
a.send({ type: 'Join', name: '  Ann  ', color: [10, 20, 30] });
const welcome = await a.wait('Welcome');
check(welcome && welcome.map === 'Smoke map' && welcome.generator === 'caveland' && welcome.tick_rate === 60 && !('edits' in welcome), 'Join answers with a Welcome for the loaded map');
await sleep(2000);
check(a.chunks.length >= 9 && a.chunks.every(c => c.kind === 1 && c.count === 4000), `terrain is streamed as binary chunks (${a.chunks.length} so far, each a full chunk)`);
check(new Set(a.chunks.map(c => c.cx + ',' + c.cy)).size === a.chunks.length, 'no chunk is sent twice');
const mine = welcome.players.find(p => p.id === welcome.your_id).pos;
const col = (() => { const gx = Math.round(mine[0]), gy = Math.round(mine[1]); const yy = gx + gy; return [(gx - gy - (((yy % 2) + 2) % 2)) / 2, yy]; })();
check(a.chunks[0].cx === Math.floor(col[0] / 10) && a.chunks[0].cy === Math.floor(col[1] / 40), 'the chunk the player stands in comes first');
check(welcome.roster.some(p => p.id === welcome.your_id && p.name === 'Ann' && p.color.join() === '10,20,30'), 'the roster has the cleaned name and the chosen colour');
await sleep(100);
check(a.log.some(m => m.type === 'PlayerJoined' && m.player.name === 'Ann'), 'everybody is told when a player joins');

// lobby operations are refused while someone is playing
const b = client(); await b.ready; await b.wait('Lobby');
b.send({ type: 'LoadMap', map: 'smoke-map', slot: 0 });
check((await b.wait('Failed'))?.message.includes('playing'), 'loading another save is refused while somebody is playing');
check(!b.log.some(m => m.type === 'Snapshot'), 'a lobby connection does not receive world traffic');

// ---- network debug messages
a.send({ type: 'Ping', client_time: 4242.5, rtt_ms: 37.4 });
const pong = await a.wait('Pong', 1000 + lag);
check(pong && pong.client_time === 4242.5, 'Ping is answered with a Pong carrying the same client_time');
await sleep(1200 + lag);
const stats = a.find('Stats');
check(stats && stats.players === 1 && stats.loaded_chunks >= 9, 'Stats arrive about once a second');
const pings = a.find('Pings');
check(pings && pings.list.some(([id, ms]) => id === a.find('Welcome').your_id && ms === 37), 'the ping a client reports is shared with everybody in a Pings list');

// ---- movement, jumping, blocks
const start = [...a.me().pos];
a.send(keys({ right: true })); await sleep(700 + lag); a.send(keys()); await sleep(400 + lag);
check(Math.hypot(...a.me().pos.map((v, i) => v - start[i])) > 1, 'the player walks on the generated terrain');
check(a.me().input_seq >= 2 && typeof a.me().input_ticks === 'number', 'snapshots acknowledge the input sequence');
a.send(keys({ jump: true })); await sleep(150 + lag); a.send(keys());
check(a.me().pos[2] > start[2] + 0.1 || a.me().vel[2] > 0, 'jumping lifts the player');
await sleep(1500 + lag);
check(a.me().vel[2] === 0, 'and gravity brings the player back down');

const me = a.me();
const gx = Math.round(me.pos[0]) + 1, gy = Math.round(me.pos[1]);
const y = gx + gy, x = (gx - gy - ((y % 2) + 2) % 2) / 2;
a.send({ type: 'SetBlock', x, y, z: 9, block: 3 });
check((await a.wait('BlockSet', 1000 + lag))?.x === x, 'a block placement is broadcast');

// ---- robustness
a.ws.send('not json'); a.ws.send('{"type":"Nonsense"}'); a.ws.send('{"type":"Input","seq":99,"input":{"upp":true}}');
await sleep(300);
const d = client(); await d.ready;
check(!!(await d.wait('Lobby')), 'the server still accepts connections after garbage input');
const big = client(); await big.ready; await big.wait('Lobby'); big.ws.send('x'.repeat(5000)); await sleep(400 + lag);
check(big.ws.readyState !== 1, 'an oversized message disconnects the sender');

// ---- persistence: leave, reload the save, the edit is still in the chunk
a.ws.close(); b.ws.close(); await sleep(500 + lag);
check(lobby.log.filter(m => m.type === 'WorldChanged').length >= 1, 'the lobby was told about the world');
lobby.send({ type: 'LoadMap', map: 'smoke-map', slot: changed.world.slot });
await sleep(500);
const e = client(); await e.ready; await e.wait('Lobby'); e.send({ type: 'Join', name: 'Eve', color: [1, 2, 3] }); await e.wait('Welcome'); await sleep(1500);
const cxy = [Math.floor(x / 10), Math.floor(y / 40)];
const chunk = e.chunks.find(c => c.cx === cxy[0] && c.cy === cxy[1]);
check(chunk && chunk.at(((x % 10) + 10) % 10, ((y % 40) + 40) % 40, 9)[0] === 3, 'a block placed earlier is still there after the save was reloaded');
e.ws.close();

// ---- the Caveland game mode
await sleep(500 + lag);
lobby.send({ type: 'CreateMap', id: 'smoke-cave', name: 'Smoke cave', description: '', generator: 'island', seed: 1, gamemode: 'caveland' });
await sleep(400);
const cave = lobby.last('MapCreated');
check(cave?.map.id === 'smoke-cave' && cave.map.gamemode === 'caveland', 'a map can be created with Caveland rules');
lobby.send({ type: 'LoadMap', map: 'smoke-cave', slot: 'new' });
await sleep(600);
check(lobby.last('WorldChanged')?.world.gamemode === 'caveland', 'loading it reports the game mode');
const f = client(); await f.ready; await f.wait('Lobby'); f.send({ type: 'Join', name: 'Cave', color: [5, 6, 7] });
const cw = await f.wait('Welcome');
check(cw?.gamemode === 'caveland', 'Welcome tells the client the game mode');
await sleep(1500);
const things = f.last('Things');
check(things && things.things.some(t => t.kind === 'Torch') && things.things.some(t => t.kind === 'robot'), 'the mode sends its things (items, an enemy)');
const state = f.log.filter(m => m.type === 'Rules' && m.kind === 'state').pop();
check(state && state.data[cw.your_id]?.health === 100 && Array.isArray(state.data[cw.your_id].items), 'Rules state carries our health and pack');
check(f.last('Snapshot').players.length === 1, 'items and robots are not players in the snapshot');
const me2 = f.me();
const cgx = Math.round(me2.pos[0]) + 1, cgy = Math.round(me2.pos[1]), cy = cgx + cgy;
f.send({ type: 'SetBlock', x: (cgx - cgy - ((cy % 2) + 2) % 2) / 2, y: cy, z: 9, block: 3 });
await sleep(400 + lag);
check(!f.log.some(m => m.type === 'BlockSet'), 'clients cannot place blocks directly in Caveland');
const before = f.log.length;
f.send({ type: 'Action', name: 'attack' }); await sleep(100); f.send({ type: 'Action', name: 'release_attack' });
f.send({ type: 'Action', name: 'nonsense', arg: -3 });
await sleep(800 + lag);
check(f.ws.readyState === 1 && f.log.slice(before).some(m => m.type === 'Snapshot'), 'actions are accepted and the world keeps running');
f.ws.close();

for (const c of [lobby, d]) c.ws.close();
console.log(failures ? `\n${failures} check(s) failed` : '\nall checks passed');
process.exit(failures ? 1 : 0);

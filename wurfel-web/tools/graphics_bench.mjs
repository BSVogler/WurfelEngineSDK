// Graphics settings benchmark: drives the real game in Chrome (WebGPU) and times every graphics
// setting, alone on top of a "minimal" baseline, then writes a text report.
//
//   cd /some/dir && npm i puppeteer-core          # once, outside the repo
//   ./dev.sh                                      # game server + client on :8080 / :3000
//   node <repo>/wurfel-web/tools/graphics_bench.mjs [report.txt]
//
// Env: URL (default http://127.0.0.1:8080/), SERVER (ws://127.0.0.1:3000/ws), DSF (device scale
// factor, default 2: renders 3840x2160 so the GPU, not the browser's frame pacing, is the limit),
// BENCH_SECONDS (measure time per sample, default 3), ROUNDS (default 2, the best round counts),
// CHROME (path to Chrome).
//
// How it measures: the frame cap is lifted (`--disable-frame-rate-limit`, vsync off, fps limit 0)
// and the page's animation frames are counted over a few seconds. The page's frames are held back
// by the GPU, so the count is the GPU throughput for that setting. Close other GPU-heavy apps.
import { createRequire } from 'node:module';
import { readFileSync, writeFileSync } from 'node:fs';
import os from 'node:os';
const require = createRequire(process.cwd() + '/');
const puppeteer = require('puppeteer-core');

// FULL=1: every level of every setting and longer samples (about 15 minutes). Default: about 5 minutes.
const FULL = process.env.FULL === '1';
const URL_ = process.env.URL || 'http://127.0.0.1:8080/';
const SERVER = process.env.SERVER || 'ws://127.0.0.1:3000/ws';
const DSF = +(process.env.DSF || 2);
const SECONDS = +(process.env.BENCH_SECONDS || (FULL ? 5 : 3));
const ROUNDS = +(process.env.ROUNDS || (FULL ? 2 : 1));
const CHROME = process.env.CHROME || '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome';
const OUT = process.argv[2] || 'graphics_bench.txt';

// Everything optional off, and no sound (music and effects would only add CPU noise).
const MINIMAL = {
  masterVolume: 0, musicVolume: 0, effectsVolume: 0,
  ambientOcclusion: false, sunShadows: false, shadowMethod: 'map', shadowSoftness: 0.4, shadowQuality: 'medium',
  cloudShadows: false, cloudSpeed: 1, linearBlend: false, bloom: 0, depthOfField: 0, fxaa: false,
  grass: false, grassDensity: 10, atmosphere: false, atmosphereDensity: 1, weather: 'clear', weatherDensity: 1,
  volumetrics: false, spriteShadows: false, waterReflection: false,
};
// [group, label, patch on top of MINIMAL]. A patch that needs a parent switch includes it.
const CASES = [
  ['baseline', 'minimal (everything optional off)', {}],
  ['Ambient occlusion', 'on', { ambientOcclusion: true }],
  ['Linear blending', 'on', { linearBlend: true }],
  ['Anti-aliasing (FXAA)', 'on', { fxaa: true }],
  ['Bloom', '10%', { bloom: 0.1 }],
  ['Bloom', '50%', { bloom: 0.5 }],
  ['Depth of field', '50%', { depthOfField: 0.5 }],
  ['Depth of field', '100%', { depthOfField: 1 }],
  ['Sprite shadows', 'on', { spriteShadows: true }],
  ['Cloud shadows', 'on, speed 100%', { cloudShadows: true }],
  ['Sun shadows, map', 'low', { sunShadows: true, shadowMethod: 'map', shadowQuality: 'low' }],
  ['Sun shadows, map', 'medium', { sunShadows: true, shadowMethod: 'map', shadowQuality: 'medium' }],
  ['Sun shadows, map', 'high', { sunShadows: true, shadowMethod: 'map', shadowQuality: 'high' }],
  ['Sun shadows, voxel', 'low, softness 0%', { sunShadows: true, shadowMethod: 'voxel', shadowQuality: 'low', shadowSoftness: 0 }],
  ['Sun shadows, voxel', 'low, softness 40%', { sunShadows: true, shadowMethod: 'voxel', shadowQuality: 'low', shadowSoftness: 0.4 }],
  ['Sun shadows, voxel', 'medium, softness 40%', { sunShadows: true, shadowMethod: 'voxel', shadowQuality: 'medium', shadowSoftness: 0.4 }],
  ['Sun shadows, voxel', 'medium, softness 100%', { sunShadows: true, shadowMethod: 'voxel', shadowQuality: 'medium', shadowSoftness: 1 }],
  ['Sun shadows, voxel', 'high, softness 40%', { sunShadows: true, shadowMethod: 'voxel', shadowQuality: 'high', shadowSoftness: 0.4 }],
  ['Sun shadows, voxel', 'high, softness 100%', { sunShadows: true, shadowMethod: 'voxel', shadowQuality: 'high', shadowSoftness: 1 }],
  ['Grass blades', 'density 5', { grass: true, grassDensity: 5 }],
  ['Grass blades', 'density 10', { grass: true, grassDensity: 10 }],
  ['Grass blades', 'density 20', { grass: true, grassDensity: 20 }],
  ['Ambient life', 'density 100%', { atmosphere: true, atmosphereDensity: 1 }],
  ['Ambient life', 'density 200%', { atmosphere: true, atmosphereDensity: 2 }],
  ['Weather', 'rain 100%', { weather: 'rain', weatherDensity: 1 }],
  ['Weather', 'rain 200%', { weather: 'rain', weatherDensity: 2 }],
  ['Weather', 'snow 100%', { weather: 'snow', weatherDensity: 1 }],
  ['Fog and god rays', 'on', { volumetrics: true }],
  ['Water reflection', 'on', { waterReflection: true }],
];
if (!FULL) {
  const skip = new Set(['Bloom|50%', 'Depth of field|100%', 'Sun shadows, map|low', 'Sun shadows, map|high', 'Sun shadows, voxel|low, softness 0%', 'Sun shadows, voxel|low, softness 40%',
    'Sun shadows, voxel|medium, softness 100%', 'Sun shadows, voxel|high, softness 40%', 'Grass blades|density 5', 'Ambient life|density 200%', 'Weather|rain 200%', 'Weather|snow 100%']);
  for (let i = CASES.length - 1; i >= 0; i--) if (skip.has(CASES[i][0] + '|' + CASES[i][1])) CASES.splice(i, 1);
}
if (process.env.ONLY) { // e.g. ONLY=preset: just those groups (and the baseline), in the normal view and the sun sweep
  const only = new RegExp(process.env.ONLY);
  for (let i = CASES.length - 1; i >= 0; i--) if (CASES[i][0] !== 'baseline' && !only.test(CASES[i][0]) && !only.test(CASES[i][1])) CASES.splice(i, 1);
}
// The quality presets of the menu (presets.js), so the report shows what each one costs.
{
  const win = {};
  new Function('window', readFileSync(new URL('../presets.js', import.meta.url), 'utf8'))(win);
  for (const p of win.wurfelPresets) CASES.push(['preset', p.name, p.settings]);
}
// What is on the screen changes what some effects cost (depth of field blurs more where the picture has
// more depth, the shadows and the grass work on more blocks when zoomed out), so every case is timed in
// several views. The position stays where the player spawned.
const SCENES = [['far view (zoom 25%)', { zoom: 0.25 }], ['normal view (zoom 50%)', { zoom: 0.5 }], ['close view (zoom 100%)', { zoom: 1 }]];

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const browser = await puppeteer.launch({
  executablePath: CHROME, headless: false,
  args: ['--enable-unsafe-webgpu', '--use-angle=metal', '--disable-frame-rate-limit', '--disable-gpu-vsync',
    '--ignore-gpu-blocklist', '--enable-webgpu-developer-features', '--disable-renderer-backgrounding', '--disable-backgrounding-occluded-windows', '--window-size=1920,1180'],
});
const page = await browser.newPage();
await page.setViewport({ width: 1920, height: 1080, deviceScaleFactor: DSF });
await page.evaluateOnNewDocument((m) => localStorage.setItem('wurfel.settings.v1', JSON.stringify({ ...m, fpsLimit: 0, playerName: 'Bench' })), MINIMAL);
await page.goto(URL_, { waitUntil: 'load' });
await sleep(3000);
await page.evaluate((server) => window.dispatchEvent(new CustomEvent('wurfel:play', { detail: { name: 'Bench', color: '#e64d4d', server, generator: 'island', seed: 1, create: false } })), SERVER);
await sleep(8000);
const status = await page.evaluate(() => window.wurfelStatus);
if (!status || !status.connected) { console.error('The game did not connect:', status); await browser.close(); process.exit(1); }

const apply = (patch) => page.evaluate((s) => { Object.assign(window.wurfelSettings, s); window.dispatchEvent(new CustomEvent('wurfel:settings')); }, { ...MINIMAL, ...patch, fpsLimit: 0 });
// The game times its own GPU passes (timestamp queries, see src/gputime.rs) and publishes the average
// of the last half second in window.wurfelStatus.gpu. Sample that, and count the presented frames.
const measure = (seconds) => page.evaluate((sec) => new Promise((res) => {
  const samples = []; let frames = 0; let last = null; let seen = 0;
  const start = performance.now();
  const raf = () => { frames++; if (performance.now() - start < sec * 1000) requestAnimationFrame(raf); };
  requestAnimationFrame(raf);
  const poll = setInterval(() => {
    const g = window.wurfelStatus && window.wurfelStatus.gpu;
    // The game publishes the average of the last half second every 0.5 s: the first two it publishes after a
    // change still hold frames from before it (or a rebuild), so they are not used.
    if (g && g !== last) { last = g; seen++; if (seen > 2) samples.push(JSON.parse(JSON.stringify(g))); }
    if (performance.now() - start >= sec * 1000) { clearInterval(poll); res({ samples, fps: frames / sec }); }
  }, 100);
}), seconds);
// A rebuild (new shadow grid, shaders) can outlast the window: measure again until samples arrive.
const measureSome = async (seconds) => {
  for (let i = 0; i < 4; i++) { const m = await measure(seconds); if (m.samples.length) return m; await sleep(1500); }
  return { samples: [], fps: 0 };
};
const median = (a) => { const s = [...a].sort((x, y) => x - y); return s.length ? s[Math.floor(s.length / 2)] : 0; };
const stats = ({ samples, fps }) => {
  const names = [...new Set(samples.flatMap((s) => Object.keys(s.stages)))];
  const stages = Object.fromEntries(names.map((n) => [n, median(samples.map((s) => s.stages[n] ?? 0))]));
  return { busy: median(samples.map((s) => s.busy)), span: median(samples.map((s) => s.span)), worst: Math.max(0, ...samples.map((s) => s.busy)), fps, samples: samples.length, stages };
};

const sun = (az) => page.evaluate((a) => window.wurfelNet.sun(a), az);
const NOON = 90;
// Every 45 degrees round the whole day (90 is noon with the default spin, 270 midnight).
const SUN_AZIMUTHS = [0, 45, 90, 135, 180, 225, 270, 315];
const SWEEP_GROUPS = ['Sun shadows', 'Fog and god rays', 'Cloud shadows', 'Ambient life', 'Water reflection', 'Sprite shadows'];
const SWEEP_SECONDS = 3;
const results = [];
await sun(NOON); // the clock stops here, so the views below are compared at the same time of day
for (const [scene, view] of SCENES) {
  // The first time a setting is on, its shaders compile and its buffers are built: switch each on once.
  for (const [, , patch] of CASES) { await apply({ ...patch, ...view }); await sleep(results.length ? 400 : 1500); }
  for (const [group, label, patch] of CASES) {
    await apply({ ...patch, ...view });
    await sleep(FULL ? 2500 : 1500);
    let best = null;
    for (let r = 0; r < ROUNDS; r++) { const s = stats(await measureSome(SECONDS)); if (!best || s.busy < best.busy) best = s; }
    results.push({ scene, group, label, patch, ...best });
    console.log(`${scene.slice(0, 12).padEnd(12)} ${group.padEnd(24)} ${label.padEnd(26)} ${best.busy.toFixed(2)} ms`);
  }
}

// The sun sweep: the cost of a setting depends on where the sun is (long shadows at dawn and dusk, nothing to
// shadow at night, god rays towards the sun, fireflies at dusk), so what matters is the worst position, not the
// noon value. The clock is stopped and the sun put at each position in turn, in the normal view.
const sweep = [];
{
  await apply({ zoom: 0.5 });
  const wanted = (g, l) => SWEEP_GROUPS.some((x) => g.startsWith(x)) || g === 'preset';
  const cases = CASES.filter(([g, l]) => g === 'baseline' || wanted(g, l));
  for (const [group, label, patch] of cases) {
    await apply({ ...patch, zoom: 0.5 });
    const row = [];
    for (const az of SUN_AZIMUTHS) {
      await sun(az);
      await sleep(FULL ? 1500 : 600);
      const s = stats(await measureSome(FULL ? 3 : SWEEP_SECONDS));
      row.push({ az, busy: s.busy, worst: s.worst });
    }
    sweep.push({ group, label, patch, row });
    const v = row.map((r) => r.busy);
    console.log(`sweep ${group.padEnd(22)} ${label.padEnd(26)} min ${Math.min(...v).toFixed(2)} max ${Math.max(...v).toFixed(2)} ms`);
  }
  await sun(-1);
}
const info = await page.evaluate(() => ({ ua: navigator.userAgent, status: window.wurfelStatus }));
await browser.close();

const fmt = (x, w, d = 2) => x.toFixed(d).padStart(w);
const stageNames = [...new Set(results.flatMap((r) => Object.keys(r.stages)))];
const lines = [];
lines.push('WURFEL ENGINE GRAPHICS BENCHMARK');
lines.push(`date      ${new Date().toISOString()}`);
lines.push(`machine   ${os.cpus()[0].model}, ${Math.round(os.totalmem() / 2 ** 30)} GB, ${os.platform()} ${os.release()}`);
lines.push(`browser   ${info.ua}`);
lines.push(`backend   ${info.status.backend}, map "${info.status.map}", surface ${1920 * DSF}x${1080 * DSF} (viewport 1920x1080, scale ${DSF})`);
lines.push('sound     off (master, music and effects volume 0)');
lines.push(`method    the game's own GPU timestamps (src/gputime.rs): time the GPU spends on a frame, from the start of its`);
lines.push(`          first render pass to the end of its last. ${SECONDS} s per sample after ${FULL ? 2.5 : 1.5} s settling, median of the`);
lines.push(`          half-second averages, best of ${ROUNDS} rounds, frame cap off. Stationary player, live clouds and wind.`);
lines.push('          The timestamps may be rounded to 100 us by the browser; the half-second averages are finer.');
lines.push('          GPU ms per frame at this surface size; scale by the pixel count for other screens (most passes are per pixel).');
lines.push('');
lines.push('Columns: gpu = GPU ms per frame, +gpu = over the minimal baseline of the same view, worst = the largest');
lines.push('half-second average, fps = frames the page presented per second without a cap.');
for (const [scene] of SCENES) {
  const rs = results.filter((r) => r.scene === scene);
  const base = rs[0].busy;
  lines.push('');
  lines.push(`=== ${scene} ===`);
  lines.push(`${'setting'.padEnd(24)} ${'level'.padEnd(26)} ${'gpu'.padStart(7)} ${'+gpu'.padStart(7)} ${'worst'.padStart(7)} ${'fps'.padStart(6)}   patch`);
  for (const r of rs) lines.push(`${r.group.padEnd(24)} ${r.label.padEnd(26)} ${fmt(r.busy, 7)} ${fmt(r.busy - base, 7)} ${fmt(r.worst, 7)} ${fmt(r.fps, 6, 0)}   ${JSON.stringify(r.patch)}`);
  lines.push('');
  lines.push('GPU ms by stage (where the time goes):');
  lines.push(`${'setting'.padEnd(24)} ${'level'.padEnd(26)} ${stageNames.map((n) => n.slice(0, 14).padStart(14)).join(' ')}`);
  for (const r of rs) lines.push(`${r.group.padEnd(24)} ${r.label.padEnd(26)} ${stageNames.map((n) => fmt(r.stages[n] ?? 0, 14)).join(' ')}`);
}
lines.push('');
lines.push('=== how much the view changes a setting (+gpu over the same view\'s baseline) ===');
lines.push(`${'setting'.padEnd(24)} ${'level'.padEnd(26)} ${SCENES.map(([s]) => s.slice(0, 12).padStart(13)).join(' ')}`);
for (const [group, label] of CASES) {
  const cells = SCENES.map(([scene]) => { const rs = results.filter((r) => r.scene === scene); const r = rs.find((x) => x.group === group && x.label === label); return fmt(r.busy - rs[0].busy, 13); });
  lines.push(`${group.padEnd(24)} ${label.padEnd(26)} ${cells.join(' ')}`);
}
lines.push('');
lines.push('');
lines.push('=== sun sweep: GPU ms per frame with the sun at each azimuth (normal view; 90 = noon, 270 = midnight) ===');
lines.push(`${'setting'.padEnd(24)} ${'level'.padEnd(26)} ${SUN_AZIMUTHS.map((a) => String(a).padStart(6)).join('')}  ${'min'.padStart(6)} ${'median'.padStart(6)} ${'max'.padStart(6)}  jump`);
for (const r of sweep) {
  const v = r.row.map((x) => x.busy); const s = [...v].sort((a, b) => a - b); const med = s[Math.floor(s.length / 2)];
  const jump = s[s.length - 1] - med;
  lines.push(`${r.group.padEnd(24)} ${r.label.padEnd(26)} ${v.map((x) => fmt(x, 6, 1)).join('')}  ${fmt(s[0], 6, 1)} ${fmt(med, 6, 1)} ${fmt(s[s.length - 1], 6, 1)}  ${jump > 1.5 && jump > med * 0.3 ? '<< STEP +' + jump.toFixed(1) + ' ms' : ''}`);
}
lines.push('A STEP is a sun position where the frame costs much more than at most others: shadows exist only while the sun is up, so the frame cost jumps when the sun rises and falls when it sets.');
lines.push('');
lines.push('Stages: ' + stageNames.join(' | '));
lines.push('Costs of different settings add up only roughly (they share bandwidth and overdraw); the preset rows are measured together.');
writeFileSync(OUT, lines.join('\n') + '\n');
writeFileSync(OUT.replace(/\.txt$/, '') + '.json', JSON.stringify({ scenes: SCENES.map(([s]) => s), results, sunAzimuths: SUN_AZIMUTHS, sweep }, null, 1));
console.log('written', OUT);

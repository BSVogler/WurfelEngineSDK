# wurfel-web

Browser client for the Wurfel Engine prototype: a small multiplayer block world rendered with wgpu
(WebGPU, WebGL2 as fallback). Game rules live in [`../wurfel-sim`](../wurfel-sim) and run both here
(prediction) and in [`../wurfel-server`](../wurfel-server) (authority).

## Run it

| | |
|---|---|
| `./dev.sh` | Starts the game server (port 3000) and the client dev server (http://127.0.0.1:8080). A change to `wurfel-web` or `wurfel-sim` rebuilds and reloads the page in about 1 s. Open the URL in two tabs to see two players. `NO_OPEN=1 ./dev.sh` skips opening a browser. |
| `./build.sh` | Runs all tests, then builds an optimised client into `dist/`. |
| `cargo run --release -p wurfel-server -- --static wurfel-web/dist` | Serves the built client and the game from one port (default 3000). This is the thing to put on a VPS. |
| `cargo test -p wurfel-sim -p wurfel-web -p wurfel-server` | Native tests, no browser needed. |
| `node ../wurfel-server/smoke.mjs ws://127.0.0.1:3000/ws` | Protocol check against a running server. |

One-time setup: `rustup target add wasm32-unknown-unknown && cargo install trunk --locked`.

## Controls

WASD / arrows walk, left click places the selected block, right click breaks, `1`-`4` choose the
block, wheel or Q/E zoom. Without a server the page shows a read-only preview of the island.

## Where things are

- `src/web.rs`: canvas, wgpu setup, input, WebSocket, prediction/interpolation, frame loop. GPU errors are logged to the console.
- `src/mesh.rs`: world to triangles; only camera-facing faces next to air. Also the box used for players.
- `src/pick.rs`: screen position to block (for placing/breaking). Pure, unit tested.
- `src/shader.wgsl`: the isometric projection with the Java engine's constants. Depth is exact, so the depth buffer sorts everything.
- To try another map, implement `wurfel_sim::Generator` and construct the world with it in `web.rs` and in the server's `Game::new`.

The grid is the Java engine's staggered one (odd `y` rows shifted half a block), see `wurfel-sim/src/grid.rs`.

## Known limits of the prototype

- A player's height is the top block of their column, so there are no bridges, caves or jumping yet.
- The whole world mesh is rebuilt on every block change (fine for 3x3 chunks, per-chunk meshing is the next step).
- Prediction has no input sequence numbers: small corrections are blended, large ones snap.
- Edits live in server memory only; restarting the server resets the world. No authentication or rate limiting.
- The protocol is JSON for readability while it still changes.

## Menu (`index.html`, `menu.css`, `menu.js`)

Plain HTML/CSS/JS, no framework. The game canvas keeps running behind the menu. Screens: main (Play, Join server, Options, Controls, Credits), Worlds (list and create), Options (audio, graphics, interface, reset), Controls (rebindable keys, gamepad), Credits, and a pause overlay on Esc. Arrow keys/W/S and Enter/Space navigate, Esc goes back, a gamepad works too (D-pad/stick, A, B, Start). Settings are saved in `localStorage` (and the menu still works when it is blocked).

The contract with the game side (the full version is the comment at the top of `menu.js`):

- `window.wurfelSettings`: live object with `playerName`, `serverUrl` (empty = this page's server), `masterVolume`/`musicVolume`/`effectsVolume` (0..1), `renderScale` (0.5..1), `zoom` (0.2..2), `limitFps`, `ambientOcclusion`, `showFps`, `showHelp`, `world`, `generator`, `seed`, and `keys`: `{ up, down, left, right, jump, place, break, zoomIn, zoomOut }`, each `[primary, alternate]` as lowercased `KeyboardEvent.key` or `"mouse0"`/`"mouse1"`/`"mouse2"`; an empty string means unbound.
- Events on `window`: `wurfel:play` with `{ name, server, world, generator, seed, create }` (`server` is a full `ws(s)://` URL, `create` is true for a new world), `wurfel:pause`, `wurfel:resume`, `wurfel:leave`, and `wurfel:settings` (detail is the settings object; fired at startup and after every change).
- `window.wurfelMenuOpen` is true while a menu is open; the game must ignore gameplay input then. The menu also swallows keyboard, mouse and wheel events in the capture phase and dispatches `blur` when it opens so held keys are released.
- Optional, set by the game: `window.wurfelStatus = { connected, players, fps, backend }` (shown in the menu and HUD) and `window.wurfelGenerators = [{ id, name, description, uses_seed }]` (replaces the built-in generator list: island, air, blocktest, fullmap, arena, caveland).
- The Worlds screen fetches `GET <server origin>/api/worlds`, expecting `[{ name, generator, seed, players }]`. A failed, slow (4 s timeout) or malformed answer is shown as "couldn't reach the server" and the create form stays usable.

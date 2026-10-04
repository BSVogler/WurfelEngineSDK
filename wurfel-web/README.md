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

Plain HTML/CSS/JS, no framework. The game canvas keeps running behind the menu. Screens: main (**Connect**, a small Server field, Host a map, Options, Controls, Credits), **Host a map** (the running map, and every map on the server with its generator, seed and saves), Create map, Options, Controls, Credits and a pause overlay on Esc. A **Player panel** (name, colour swatches, custom colour, preview) sits to the right of the main panel on desktop and collapses below it on phones. Arrow keys/W/S and Enter/Space navigate, Esc goes back, a gamepad works too. Settings are saved in `localStorage` (the menu still works when it is blocked).

**Players** just press **Connect**: it joins the map the server is running. The small Server field (default `localhost`, remembered servers as suggestions, up to 8) overrides the address: empty = the server this page came from; a bare host gets the game server's port 3000 (`localhost` -> `ws://localhost:3000/ws`, `myserver.example.org` -> `ws://myserver.example.org:3000/ws`, or `wss://myserver.example.org/ws` on an https page); `host:port`, `ws(s)://` and `http(s)://` URLs are used as given. A **host** opens *Host a map*, picks a save or a new save of any map on that server (or creates a map), and the server switches to it while nobody is connected. Map, world and save files are one thing; the menu says "map". Nothing in the menu starts a server process.

**Lobby.** There is no REST API. Connect and Host a map each open its own short-lived WebSocket to the server (`ws(s)://host/ws`, JSON text frames, 4 s to connect and get a first answer; Refresh reconnects). It closes when the player leaves the Host screens or joins; the game then opens its own socket (the menu never sends `Join`).

- server to menu: `Lobby { world: { map, map_id, slot, generator, seed, players }, generators }`, `Maps { maps: [{ id, name, description, generator, seed, saves: [{ slot, modified }] }] }`, `WorldChanged { world }`, `MapCreated { map }`, `Failed { request, message }`
- menu to server: `ListMaps`, `GetWorld`, `LoadMap { map, slot: n | "new" }`, `CreateMap { id, name, description, generator, seed }` (frames stay under 1 KB)

The contract with the game side (the full version is the comment at the top of `menu.js`):

- `window.wurfelSettings`: live object with `playerName`, `playerColor` (`#rrggbb`), `serverUrl`, `servers`, `masterVolume`/`musicVolume`/`effectsVolume` (0..1), `renderScale` (0.5..1), `zoom` (0.2..2), `limitFps`, `ambientOcclusion`, `showFps`, `showHelp`, `generator`/`seed` (last used in the Create map form), and `keys`: `{ up, down, left, right, jump, place, break, zoomIn, zoomOut }`, each `[primary, alternate]` as lowercased `KeyboardEvent.key` or `"mouse0"`/`"mouse1"`/`"mouse2"`; an empty string means unbound.
- Events on `window`: `wurfel:play` with `{ name, color, server, generator, seed, create }` (`server` is a full `ws(s)://` URL without a query string; `generator`/`seed` describe the world the server is running; `create` is true when a new save slot was created), `wurfel:pause`, `wurfel:resume`, `wurfel:leave`, `wurfel:settings` (detail is the settings object; fired at startup and after every change), and `wurfel:error` (game to menu, `{ message }`: shows the message on the main menu).
- `window.wurfelPlayRequest` is the detail of the last `wurfel:play` (null after leaving), for a game that was not listening yet.
- `window.wurfelMenuOpen` is true while a menu is open; the game must ignore gameplay input then. The menu also swallows keyboard, mouse and wheel events in the capture phase and dispatches `blur` when it opens so held keys are released.
- Optional, set by the game: `window.wurfelStatus = { connected, players, fps, backend }` (shown in the menu and HUD) and `window.wurfelGenerators = [{ id, name, description, uses_seed }]` (fallback when the lobby sends none or cannot be reached; otherwise a built-in list: island, air, blocktest, fullmap, arena, caveland).

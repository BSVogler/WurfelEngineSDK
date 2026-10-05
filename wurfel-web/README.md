# wurfel-web

Browser client for the Wurfel Engine prototype: a small multiplayer block world rendered with wgpu
(WebGPU, WebGL2 as fallback). Game rules live in [`../wurfel-sim`](../wurfel-sim) and run both here
(prediction) and in [`../wurfel-server`](../wurfel-server) (authority).

## Run it

| | |
|---|---|
| `./dev.sh` | Starts the game server (port 3000) and the client dev server (http://127.0.0.1:8080). A change to `wurfel-web` or `wurfel-sim` rebuilds and reloads the page in about 1 s. A change to `wurfel-server`, `wurfel-sim` or `caveland-sim` rebuilds and restarts the server (SIGTERM, so it saves first); open pages show "Server updating, reconnecting…" and rejoin by themselves. Open the URL in two tabs to see two players. `NO_OPEN=1 ./dev.sh` skips opening a browser. |
| `./build.sh` | Runs all tests, then builds an optimised client into `dist/`. |
| `cargo run --release -p wurfel-server -- --static wurfel-web/dist` | Serves the built client and the game from one port (default 3000). This is the thing to put on a VPS. |
| `cargo test -p wurfel-sim -p wurfel-web -p wurfel-server` | Native tests, no browser needed. |
| `node ../wurfel-server/smoke.mjs ws://127.0.0.1:3000/ws` | Protocol check against a running server. |

One-time setup: `rustup target add wasm32-unknown-unknown && cargo install trunk --locked`.

## Controls

WASD / arrows walk, Space jumps, wheel or Q/E zoom. Normal play never edits blocks with the mouse.
`F2` (or the console command `editor`, `editor on|off`) switches the map editor on and off, like the
Java engine's editor: a red EDITOR badge and a toolbar appear, and only then does the mouse edit
blocks. In the editor the left button uses the toolbar's tool (draw: place next to the clicked block,
replace, erase, pick), the right button erases, the middle button picks the clicked block's kind,
`1`-`4` choose stone/dirt/grass/sand, and the bottom-left line shows the cursor's position and block.
The editor needs a joined world (not the offline preview) and is not available in Caveland maps. The
server only applies block edits from players who are in the editor (`ClientMsg::Editor`).

## Where things are

- `src/web.rs`: canvas, wgpu setup, input, WebSocket, prediction/interpolation, frame loop. GPU errors are logged to the console.
- `src/mesh.rs`: world to triangles; only camera-facing faces next to air. Also the box used for players.
- `src/editor.rs`: the editor mode (tools, palette, what a click does). Pure, unit tested; `editor.js`/`editor.css` are its toolbar.
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

## Server updates

A WebSocket cannot move between processes, so an update restarts the server and the pages rejoin.

- **Build id.** `Lobby` and `Welcome` carry `build` (`"0.1.0+1a2b3c4d"`: crate version plus the git commit `wurfel-sim` was built from, or just the version outside a git checkout; empty from older servers). The client compares it with its own (`wurfel_sim::protocol::build_id()`); on a mismatch it calls `wurfelUpdate.show(serverBuild)` (`menu.js`), a small "Update available – Reload" notice that does not block input. Once per server build; no notice when either id is empty.
- **Shutdown.** On Ctrl-C or SIGTERM the server saves the world, sends `ServerRestarting` (no fields) to every player, closes all sockets with a normal close frame and exits (after at most 5 s even if a client does not answer).
- **Reconnect.** When the game socket closes while playing (after `ServerRestarting`, a crash or a network drop) the client keeps the world, camera and HUD on screen, freezes input, shows "Server updating, reconnecting…" and opens a new socket after 0.5 s, 1 s, 2 s, 4 s, then every 5 s, sending `Join` with the same name and colour. The new `Welcome` replaces the old state (world, remote players, prediction history, interpolation clock, mode state). After about 60 s without success it gives up with the usual `wurfel:error`. Leaving through the menu (`wurfel:leave`) and a first connection that never got a `Welcome` never reconnect. The bookkeeping is `src/reconnect.rs` (native tests); the sockets and timers are in `src/web.rs`.

## Menu (`index.html`, `menu.css`, `menu.js`)

Plain HTML/CSS/JS, no framework. The game canvas keeps running behind the menu. Screens: main (**Connect**, a small Server field, Host a map, Options, Controls, Credits), **Host a map** (the running map, and every map on the server with its generator, seed and saves), Create map, Options, Controls, Credits and a pause overlay on Esc. A **Player panel** (name, colour swatches, custom colour, preview) sits to the right of the main panel on desktop and collapses below it on phones. Arrow keys/W/S and Enter/Space navigate, Esc goes back, a gamepad works too. Settings are saved in `localStorage` (the menu still works when it is blocked).

**Players** just press **Connect**: it joins the map the server is running. The small Server field (default `localhost`, remembered servers as suggestions, up to 8) overrides the address: empty = the server this page came from; a bare host gets the game server's port 3000 (`localhost` -> `ws://localhost:3000/ws`, `myserver.example.org` -> `ws://myserver.example.org:3000/ws`, or `wss://myserver.example.org/ws` on an https page); `host:port`, `ws(s)://` and `http(s)://` URLs are used as given. A **host** opens *Host a map*, picks a save or a new save of any map on that server (or creates a map), and the server switches to it while nobody is connected. Map, world and save files are one thing; the menu says "map". Nothing in the menu starts a server process.

**Lobby.** There is no REST API. Connect and Host a map each open its own short-lived WebSocket to the server (`ws(s)://host/ws`, JSON text frames, 4 s to connect and get a first answer; Refresh reconnects). It closes when the player leaves the Host screens or joins; the game then opens its own socket (the menu never sends `Join`).

- server to menu: `Lobby { world: { map, map_id, slot, generator, seed, players }, generators, build }`, `Maps { maps: [{ id, name, description, generator, seed, saves: [{ slot, modified }] }] }`, `WorldChanged { world }`, `MapCreated { map }`, `Failed { request, message }`
- menu to server: `ListMaps`, `GetWorld`, `LoadMap { map, slot: n | "new" }`, `CreateMap { id, name, description, generator, seed }` (frames stay under 1 KB)

The contract with the game side (the full version is the comment at the top of `menu.js`):

- `window.wurfelSettings`: live object with `playerName`, `playerColor` (`#rrggbb`), `serverUrl`, `servers`, `masterVolume`/`musicVolume`/`effectsVolume` (0..1), `renderScale` (0.5..1), `zoom` (0.2..2), `limitFps`, `ambientOcclusion`, `showFps`, `showHelp`, `generator`/`seed` (last used in the Create map form), and `keys`: `{ up, down, left, right, jump, zoomIn, zoomOut }`, each `[primary, alternate]` as lowercased `KeyboardEvent.key` or `"mouse0"`/`"mouse1"`/`"mouse2"`; an empty string means unbound.
- Events on `window`: `wurfel:play` with `{ name, color, server, generator, seed, create }` (`server` is a full `ws(s)://` URL without a query string; `generator`/`seed` describe the world the server is running; `create` is true when a new save slot was created), `wurfel:pause`, `wurfel:resume`, `wurfel:leave`, `wurfel:settings` (detail is the settings object; fired at startup and after every change), and `wurfel:error` (game to menu, `{ message }`: shows the message on the main menu; a lost connection is only reported after the reconnect attempts, see Server updates).
- `window.wurfelPlayRequest` is the detail of the last `wurfel:play` (null after leaving), for a game that was not listening yet.
- `window.wurfelMenuOpen` is true while a menu is open; the game must ignore gameplay input then. The menu also swallows keyboard, mouse and wheel events in the capture phase and dispatches `blur` when it opens so held keys are released.
- Optional, set by the game: `window.wurfelStatus = { connected, players, fps, backend }` (shown in the menu and HUD) and `window.wurfelGenerators = [{ id, name, description, uses_seed }]` (fallback when the lobby sends none or cannot be reached; otherwise a built-in list: island, air, blocktest, fullmap, arena, caveland).

## Caveland HUD

`hud.js` draws the in-game HUD of Caveland worlds. Crafting is a popup like in the Java game: `C` opens it (and closes it again), `W`/`S` or the arrow keys choose a recipe (one is shown at a time, craftable ones first), `Enter`, `Space` or `N` craft it, `Esc`, `M`, a right click or a click beside the popup close it. The server owns the recipes: the `state` message lists them in a fixed order as `[name, can_craft, [ingredient, ...]]`, the client orders them for display and sends `craft` with the recipe's fixed index. `wurfelHud.update` takes `"recipes": [{index, name, can, ingredients: [{name, have}]}]` already in menu order. While the popup is open `window.wurfelDialogOpen` is true, so the game ignores gameplay keys.

## Sprites

The client draws the Java game's art: block faces, entities and the player come from a sprite atlas in
`assets/sprites/` (see `assets/README.md`, which also records that the art licence is not cleared).
`src/atlas.rs` reads the libGDX atlas format, `src/sprites.rs` decides which sprite shows what and builds
the geometry, `src/actors.rs` animates players and things, `src/texture.rs` decodes and uploads the pages
(a texture array, bind group 1 of `shader.wgsl`). While the atlas loads, or if it cannot be loaded, and
for blocks without art, the old flat colours are shown; `?flat=1` in the address forces them. To rebuild
the atlas from the Java sheets run `python3 tools/build_atlas.py`.

### Player animation and the Caveland actions

Animation is presentation: the client owns it and the server neither holds nor sends any animation state
(`PlayerState` has no pose). `src/animation.rs` is the port of `Ejira.playAnimation`/`updateSprite` (swing `h`,
charge/loaded stance `l`, power attack `i`, throw `t`, jump `j`, overlays `s`/`o`) plus a `Performer` that mirrors
just enough of the rules to animate, with the same constants as the server's `Tuning` (0.3 s charge threshold,
1.0 s full charge, 0.15 s swing, 0.6 s drop).

* Local player: the animation starts from the local input at once (`send_action` in `web.rs` feeds `Actors` and
  sends the unchanged `Action` message), without waiting for the server.
* Other players: the server announces each of their moves as a one-shot happening in the `events` Rules message,
  `{"t": "action", "player": id, "name": "attack"|"release_attack"|"prepare_throw"|"throw"|"drop", "ok": bool}`
  (`ok` is false when the rules refused: no swing began, nothing prepared, nothing thrown). An accepted one
  starts that player's animation; timings run on the client from the same constants.
* Reconciling: an `ok: false` event about the local player takes its animation back to standing; an accepted one
  changes nothing.
* Jumping and walking come from the movement seen in the snapshots (leaving the ground upwards faster than 1.5
  blocks per second is a jump, horizontal speed starts and ends the walk), for the local player and others alike.
* Facing is the one derived from movement (the character does not move while aiming, so it keeps where it last
  walked).

Keys (`caveland_client::key_action`, `mouse_action`): left mouse button or `F` swing, hold to charge, release to
fire a charged power attack; right mouse button or `M` hold to wind up a throw, release to throw, hold 0.6 s
(`playerItemDropTime`) to drop the item instead; `G` use the item in hand (torch, explosives, kits), `R`
interact, `X` drop, `Z`/`V` switch items.

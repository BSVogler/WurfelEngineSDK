# wurfel-web

Browser client for the Wurfel Engine prototype: a small multiplayer block world rendered with wgpu
(WebGPU, WebGL2 as fallback). Game rules live in [`../wurfel-sim`](../wurfel-sim) and run both here
(prediction) and in [`../wurfel-server`](../wurfel-server) (authority).

## Run it

| | |
|---|---|
| `./dev.sh` | Starts the game server (port 3000) and the client dev server (http://127.0.0.1:8080). A change to `wurfel-web` or `wurfel-sim` rebuilds and reloads the page in about 1 s. A change to `wurfel-server`, `wurfel-sim` or `caveland-sim` rebuilds and restarts the server (SIGTERM, so it saves first); open pages show "Server updating, reconnecting…" and rejoin by themselves. Open the URL in two tabs to see two players. `NO_OPEN=1 ./dev.sh` skips opening a browser. |
| `./bluegreen.sh` | Blue/green for local testing. `run` keeps the last good build running at http://127.0.0.1:4000 (client and server on one port); `promote` copies the working folder, runs the tests, builds server and client, smoke-tests the new server, and only then switches blue to it, so a broken working folder never reaches the page you are testing; `rollback` goes back one build; `status` shows what runs. `./dev.sh` stays the hot-reload "green" side on its own ports. Blue keeps its own maps in `~/.wurfel-bluegreen/maps`. |
| `./build.sh` | Runs all tests, then builds an optimised client into `dist/`. |
| `cargo run --release -p wurfel-server -- --static wurfel-web/dist` | Serves the built client and the game from one port (default 3000). This is the thing to put on a VPS. |
| `cargo test -p wurfel-sim -p wurfel-web -p wurfel-server` | Native tests, no browser needed. |
| `node ../wurfel-server/smoke.mjs ws://127.0.0.1:3000/ws` | Protocol check against a running server. |

One-time setup: `rustup target add wasm32-unknown-unknown && cargo install trunk --locked`.

## Controls

WASD / arrows walk, Space jumps, wheel zoom. Normal play never edits blocks with the mouse.
`F2` (or the console command `editor`, `editor on|off`) switches the map editor on and off, like the
Java engine's editor: a red EDITOR badge and a toolbar appear, and only then does the mouse edit
blocks. The toolbar has the Java tools in the Java order: draw (place next to the clicked block),
bucket (fill the rectangle between press and release on the layer of the first block), replace,
select, spawn, erase, plus a pick (eyedropper) the Java editor lacks. The tools wear the Java icons
(`assets/editor/`, from `sprites/skin`), the palette shows the blocks' own pictures (`src/cursor.rs`
`palette_previews`), the block under the pointer carries the Java cursor cube (entity sprite 8) and
draw and replace show a see-through ghost of the block they would build. There are no number keys for
the palette: `1` and `2` turn the camera. The left button uses the
tool (draw and replace keep painting while held), the right button erases, the middle button (or
Alt + left) picks the clicked block's kind and value, Ctrl/Cmd+Z undoes and Ctrl/Cmd+Shift+Z redoes
(also the Undo/Redo buttons; the history is dropped when you leave). The bottom-left line shows the
cursor's position, block and value.

- **Things (Java entities).** The spawn tool puts the kind chosen in the toolbar (`Wood`, `Coal`,
  `Ironore`, `Torch`, `Rails`, `money`, `flag`, `bird`: the kinds the client has art for, instead of
  Java's list of registered entity classes) in the empty cell where a block would go. The select
  tool selects the thing under the pointer (a cyan frame), drags it along its layer and Delete (or
  Backspace) removes it. Not ported: selecting several things with a rectangle, and undo for things
  (the server hands out their ids). Things do not move or collide; they are sent to everybody as
  `Things` (right after a change and about once a second) and saved with the world in the save
  slot's `editor-things.json` (this Rust server's own file; not the Java map format).
- **Block values.** The Java `values` field is the block's variant. Nothing in the simulation reads
  it (physics, light and storage only carry it, `.wec` files keep it); it picks the block's
  picture, so only blocks with more than one picture have more than one value: stone has two
  (`editor_block_values` in `wurfel-sim`). `+`/`-` or the toolbar change it, choosing a block
  starts at 0 as in Java, the pick tool takes it over, and the server refuses a value a block has no
  picture for. The flat-colour look (`?flat=1`) cannot show it.
- **Layers.** The wheel limits how many layers are drawn (Java: the Z rendering limit; wheel down
  hides the top layer, wheel up brings layers back, past the top shows everything), also the
  Up/Down toolbar buttons. In the editor the wheel no longer zooms: Ctrl/Cmd + wheel (and a
  trackpad pinch) does. The meshes leave the hidden layers out (`RenderStorage::set_layer_limit`),
  the top shown layer shows its top faces, and picking ignores the hidden layers (`pick`'s `top`);
  a block drawn on the top shown layer lands in the hidden layer above it. Leaving the editor shows
  everything again.
- **Camera.** In the editor WASD / arrows pan the camera away from the player (Shift is three times
  as fast) instead of walking, up to 24 ground units (`PAN_LIMIT`) from the player, and the time of
  day stops (Java: `timespeed` 0) so the light does not change while editing. The existing free
  camera (F8) is a different thing, a yaw turn about the player that picking cannot follow, so it
  stays exclusive with the editor. A stopped simulation clock has no meaning here: the plain engine
  has no time-driven world to stop, only the local light.
- **Save.** The Save button sends `SaveWorld`; the server writes the changed chunks and the things
  now and answers everybody with `Saved` (a banner). Only from the editor, in every game mode too.
- **Keys 1-5.** Java picks tools with them. Here `1`-`4` already choose the block (the palette),
  so the tools are only on the toolbar.

The editor needs a joined world (not the offline preview). It edits blocks in every game mode (Caveland too); things only in the plain engine, because a mode sends its own. The
server only applies edits from players who are in the editor (`ClientMsg::Editor`). Their reach is
`EDITOR_REACH` (48 ground units around the body, `wurfel-server`'s `game.rs`; it used to be 12) so
that the panned camera can edit: only editors can edit at all, so nobody else is exempt from
anything. It applies to blocks, the cells of a bucket fill (one `FillBlocks`, at most 400 columns,
answered by one `BlocksSet`) and things (`SpawnThing`, `MoveThing` for both the old and the new
place, `DeleteThing`), which are also checked for kind, a finite position inside the world's
height, and at most 500 things.

## Where things are

- `src/web.rs`: canvas, wgpu setup, input, WebSocket, prediction/interpolation, frame loop. GPU errors are logged to the console.
- **Camera turn.** In the normal (fixed) camera `1` turns the map a quarter to the left and `2` a quarter to the right, easing round the player; the editor turns with them (its picking and panning follow the view). A turned map is meshed with all sides, like the free camera. The free camera (F8, mouse turns it freely) stays as an experimental mode.
- `src/mesh.rs`: world to triangles; only camera-facing faces next to air. Also the box used for players.
- `src/editor.rs`: the editor mode (tools, palette, things, values, layer limit, camera pan, what a click does). Pure, unit tested; `editor.js`/`editor.css` are its toolbar.
- `src/pick.rs`: screen position to block (for placing/breaking, below the layer limit) and to thing. Pure, unit tested.
- `src/grass.rs`: grass blades on grass blocks (sprite `e7-0`, small stones `e7-1`), a port of the Java `TopologicalSort.drawGrass`. The maths (wind, placement, bending) is `wurfel-sim/src/grass.rs`. Blades are generated around the local player (20 blocks, fewer with distance, at most 3000 per frame), swing with a travelling wind (the sway reaches a blade later the further downwind it stands, at `WIND_SPEED` in `wurfel-sim/src/grass.rs`, with slower gusts across it) and bend away from every player. Every blade has its own softness (0.6 to 1.4 times the sway and bend) and catches the wave up to 0.4 s early or late, so neighbours do not move in step. Settings: Graphics menu (Grass blades, Grass density), or `?grass=0` / `?grass=1` and `?grassdensity=N` in the page address (the address wins). Blades above the editor's layer limit are left out.
  One wind direction serves the grass, the ground plants, the cloud shadows and the particles: the Graphics menu's Wind direction (degrees in the game space, default 37, above the cloud settings) or `?winddirection=N` (the address wins). `LightingController::wind_degrees` holds it; the shader reads its ground-frame angle from `sun_dir.w`, and the clouds' drift is summed up on the CPU (`sun_back.zw`) so turning the wind does not make the shadows jump.
- `src/atmosphere.rs`, `src/atmosphere.wgsl`: atmosphere and life. Everything is instanced quads with no vertex buffer: the vertex shader derives each particle's place, size and brightness from its instance number, a time uniform and the viewer's position, so the CPU only writes two small uniforms and draws a few counted instances per kind (at most about 12 000 quads in all; caps per kind in `wurfel-sim/src/atmosphere.rs`). The pass draws into the blended HDR picture after the scene and before the post-process passes, so bloom picks up the fireflies. It reads the nearest depth of every pixel (a particle behind the ground is hidden, fog and beams fade where they cut it) and a column map (`wurfel_sim::atmosphere::column`: height and kind of the top block of every column around the viewer, 96 x 96, rebuilt a few rows per frame when the viewer walks 16 columns or the terrain changes), so rain lands on roofs and not in rooms, fireflies and pollen stay over grass, leaves start at trees and mist hangs over water. Wind is the grass's: the same direction and the same slow gusts (`wurfel_sim::atmosphere::{wind_direction, gust, drift}`), as a closed-form integral so drifting things move smoothly.
  - Ambient: pollen (day, over grass), fireflies (dusk and night, over grass and water, bright enough to bloom), dust motes (low sun), drifting leaves (from tree tops, tumbling downwind), mist (over water, thicker at dawn, dusk and night). How strong each is follows the sun's height (`Strengths::at`).
  - Weather: rain streaks that slant with the wind and splash rings that spread on the ground (small) and on water (larger, double ring), or snow that flutters and drifts. Rain takes pollen, fireflies and god rays away and thickens the mist.
  - Fog and god rays: large soft fog banks over the ground and slanting beams along the sun's direction, in world space, faded by distance (the window's edge) and by the sun's angle (beams only with a low to middle sun, fog thicker at low sun).
  - Depth of field: the post pass blurs by the depth of the surface behind a pixel, which does not know a particle in front of it. So a particle grows by the circle of confusion `tonemap.wgsl` would give its own depth and gets fainter by the area it gained. A particle in focus in front of a blurred background is still blurred by the post pass.
  - Settings: Graphics menu (Ambient life, Ambient density, Weather, Weather density, Fog and god rays), or `?atmosphere=0|1`, `?atmospheredensity=0..2`, `?weather=clear|rain|snow`, `?weatherdensity=0..2`, `?volumetrics=0|1` in the page address (the address wins). Density 1 draws half of each kind's cap, 2 all of it, 0 none.
- `src/detail.rs`: the stones of the ground cover, part of the grass system: `Grass` owns a `Stones`, which gets the same viewer, terrain, layer limit and density as the blades and writes into the same buffer, so the grass switch and the grass density are the only settings (`?grass=`, `?grassdensity=`). Stones are placed within 22 blocks (at most 4000 per frame, nearest first, fewer with distance). What lies where is `wurfel-sim/src/detail.rs`, deterministic from the block's position: a rockiness noise (patches of about 12 blocks), a lushness noise (8 blocks) and a clump noise per species decide how much there is, and each block type has its own table. Species: **pebbles** (`e7-1`, on stone, dirt and sand) and **rocks** (`e44-0` to `e44-2`, mostly on rocky patches of stone, and along open borders of grass blocks). About half of each block's stones are moved to a border where no block stands beside it, so they hide the cube's edge. Every stone has its own brightness, hue, size and lean, and its colour also follows the patch noise. They do not move. The list is made again only when the viewer changes column, the terrain, the layer limit or the density changes. **Not done, no art:** the atlas has no flowers, mushrooms, ferns, fallen leaves, twigs or clover, and art is not invented, so those are missing until someone draws them. Earlier there were separate detail tufts and reeds made from the grass sprite; they were a second grass mechanism and are gone.
- `src/spriteshadow.rs`: soft contact shadows under the players and things. A blob is two triangle fans just above the ground, unlit and see-through per vertex so the opacity fades to the rim without a texture: an ellipse pushed away from the sun and as long as the sprite's height times the cotangent of the sun's elevation asks for (at most 2.5 heights; round and fainter at night), and a small round contact core under the feet that is always there. The opacity follows the height above the ground, and across a ledge the blob stays under the feet instead of hovering. At most 48 per frame. Settings: Graphics menu (Sprite shadows) or `?spriteshadows=0|1`. What sprites cast onto the terrain is a separate thing, see "Sprite shadows on the terrain" in `../GAME_DESIGN.md`.
- `src/gputime.rs`: how long the GPU spends on a frame, measured by the GPU itself (WebGPU timestamp queries on the render passes; nothing on WebGL2 or where the browser lacks `timestamp-query`). Every pass takes its `timestamp_writes` from `GpuTimer::writes(timer, "stage")`; the results are read back a few frames later without waiting. A stage gets the time from the end of the pass before it to the end of its own (passes overlap on the GPU, so their own lengths add up to far more than the frame), so the stages add up to the frame. The half-second average is `window.wurfelStatus.gpu` = `{ busy, span, frames, stages: { name: ms } }`. Chrome rounds timestamps to 100 µs unless started with `--enable-webgpu-developer-features`.
- `tools/graphics_bench.mjs`: times every graphics setting with that, driving the real game in Chrome (`puppeteer-core`, see the file's header for how to run it). The report is `benchmarks/graphics_bench.txt` (+ `.json`): each setting alone over an all-off baseline in three views (what is on the screen changes what depth of field and the shadows cost), the cost per stage, and a **sun sweep**: the same settings with the sun at 8 positions round the day (`window.wurfelNet.sun(azimuth)` stops the clock and sets it, 90 is noon, 270 midnight, a negative value lets the day run again), because shadows cost nothing at night and a setting that jumps by many ms when the sun rises is a frame drop the player would notice. Default run about 8 minutes, `FULL=1` for every level, `ONLY=<regex>` for some groups and the presets. Sound is off in it.
- Quality presets (`presets.js`, the Quality slider at the top of the Graphics menu): Lowest to Ultra, each a full list of the settings it controls. The menu shows the preset whose values all match the current settings and "Manual" as soon as one differs (nothing else is stored). High equals the defaults. The benchmark times every preset, with the aim of GPU ms per frame at 4K with the sun up written at the top of the file. Settings that are not a cost (wind direction, cloud speed, the weather type, linear blending) are not in a preset.
- Depth pre-pass (`draw_scene` in `web.rs`, `fs_depth` in `shader.wgsl`): the scene is drawn twice per layer, first only the depth (the same discards as `fs_main`, no colour), then the colour with depth test `LessEqual` and no depth write, so the expensive shadow code runs once per pixel instead of once per overlapping face (the faces come in any order). This cut the sun shadows' cost to a third.
- Water reflection: the Graphics menu's Water reflection or `?waterreflection=0|1` (default on). Off, the water mirrors only the sky and the mirror pass (`reflection.rs`) is not drawn.
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

Plain HTML/CSS/JS, no framework. The game canvas keeps running behind the menu. Screens: main (**Connect**, a small Server field, Maps, Options, Controls, Credits), **Maps** (the running map, and every map on the server with its generator, seed and saves), Create map, Options, Controls, Credits and a pause overlay on Esc. A **Player panel** (name, colour swatches, custom colour, preview) sits to the right of the main panel on desktop and collapses below it on phones. Arrow keys/W/S and Enter/Space navigate, Esc goes back, a gamepad works too. Settings are saved in `localStorage` (the menu still works when it is blocked).

**Players** just press **Connect**: it joins the map the server is running. The small Server field (default `localhost`, remembered servers as suggestions, up to 8) overrides the address: empty = the server this page came from; a bare host gets the game server's port 3000 (`localhost` -> `ws://localhost:3000/ws`, `myserver.example.org` -> `ws://myserver.example.org:3000/ws`, or `wss://myserver.example.org/ws` on an https page); `host:port`, `ws(s)://` and `http(s)://` URLs are used as given. A **host** opens *Maps*, picks a save or a new save of any map on that server (or creates a map), and the server switches to it while nobody is connected. Map, world and save files are one thing; the menu says "map". Nothing in the menu starts a server process.

**Lobby.** There is no REST API. Connect and Maps each open its own short-lived WebSocket to the server (`ws(s)://host/ws`, JSON text frames, 4 s to connect and get a first answer; Reconnect re-opens it after a failure and never touches the running world). It closes when the player leaves the Maps screens or joins; the game then opens its own socket (the menu never sends `Join`).

- server to menu: `Lobby { world: { map, map_id, slot, generator, seed, players }, generators, build }`, `Maps { maps: [{ id, name, description, generator, seed, saves: [{ slot, modified }] }] }`, `WorldChanged { world }`, `MapCreated { map }`, `Failed { request, message }`
- menu to server: `ListMaps`, `GetWorld`, `LoadMap { map, slot: n | "new" }`, `CreateMap { id, name, description, generator, seed }` (frames stay under 1 KB)

The contract with the game side (the full version is the comment at the top of `menu.js`):

- `window.wurfelSettings`: live object with `playerName`, `playerColor` (`#rrggbb`), `serverUrl`, `servers`, `masterVolume`/`musicVolume`/`effectsVolume` (0..1), `zoom` (0.2..2), `fpsLimit` (integer, 0 = unlimited, default 60), `ambientOcclusion`, `grass` (bool, default true), `grassDensity` (integer 0..20, default 10), `atmosphere` (bool, default true), `atmosphereDensity` (0..2, default 1), `weather` (`clear`, `rain` or `snow`, default `clear`), `weatherDensity` (0..2, default 1) (bool, default true), `volumetrics` (bool, default true), `spriteShadows` (bool, default true), `waterReflection` (bool, default true), `showFps`, `showHelp`, `generator`/`seed` (last used in the Create map form), and `keys`: `{ up, down, left, right, jump }`, each `[primary, alternate]` as lowercased `KeyboardEvent.key` or `"mouse0"`/`"mouse1"`/`"mouse2"`; an empty string means unbound.
- Events on `window`: `wurfel:play` with `{ name, color, server, generator, seed, create }` (`server` is a full `ws(s)://` URL without a query string; `generator`/`seed` describe the world the server is running; `create` is true when a new save slot was created), `wurfel:pause`, `wurfel:resume`, `wurfel:leave`, `wurfel:settings` (detail is the settings object; fired at startup and after every change), and `wurfel:error` (game to menu, `{ message }`: shows the message on the main menu; a lost connection is only reported after the reconnect attempts, see Server updates).
- `window.wurfelPlayRequest` is the detail of the last `wurfel:play` (null after leaving), for a game that was not listening yet.
- `window.wurfelMenuOpen` is true while a menu is open; the game must ignore gameplay input then. The menu also swallows keyboard, mouse and wheel events in the capture phase and dispatches `blur` when it opens so held keys are released.
- Optional, set by the game: `window.wurfelStatus = { connected, players, fps, backend }` (shown in the menu and HUD) and `window.wurfelGenerators = [{ id, name, description, uses_seed }]` (fallback when the lobby sends none or cannot be reached; otherwise a built-in list: island, air, blocktest, fullmap, arena, caveland).

## Caveland HUD

`hud.js` draws the in-game HUD of Caveland worlds. Crafting is a popup like in the Java game: `C` opens it (and closes it again), `W`/`S` or the arrow keys choose a recipe (one is shown at a time, craftable ones first), `Enter` or `Space` craft it, `Esc`, `C`, a right click or a click beside the popup close it. The server owns the recipes: the `state` message lists them in a fixed order as `[name, can_craft, [ingredient, ...]]`, the client orders them for display and sends `craft` with the recipe's fixed index. `wurfelHud.update` takes `"recipes": [{index, name, can, ingredients: [{name, have}]}]` already in menu order. While the popup is open `window.wurfelDialogOpen` is true, so the game ignores gameplay keys.

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
fire a charged power attack; right mouse button or `T` hold to wind up a throw, release to throw, hold 0.6 s
(`playerItemDropTime`) to drop the item instead; `E` use the item in hand (torch, explosives, kits), `R`
interact, `X` drop, `Z`/`V` switch items.

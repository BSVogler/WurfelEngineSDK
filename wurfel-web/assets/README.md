# Assets

## `sprites/`: the sprite atlas

`sprites0.png` to `sprites4.png` and `sprites.atlas` are generated, do not edit them. They repack the
art of the Java game with `python3 tools/build_atlas.py` (needs Pillow), from
`Caveland/src/main/resources/com/bombinggames/caveland/`:

* `Spritesheet.png/.txt`: the blocks `b<id>-<value>-<side>` (side 0 left, 1 top, 2 right) and
  `b<id>-<value>` (a block that is one picture), the entities `e<id>-<value>` and the interface sprites
  `i<id>-<value>`.
* `playerSheet.png/.txt`: the diffuse frames `diff/<action>/<n>` of the player: `w` walking, `h` swing,
  `l` loaded swing (the charge pose), `i` power attack, `j` jump (64 each: 8 steps in 8 directions), `t`
  throw (48: 6 steps), and the overlays `s` (charge, 64) and `o` (power attack glow, 48). Which frame
  shows when is `src/animation.rs` of the client.

The result is 857 sprites on five 2048x2048 pages (WebGL2 only guarantees 2048 pixel textures), 14 MB.
The normal maps of the Java game are not used. The engine's own `spritesheet.png` (80 pixel blocks
of the old demo art) is not used either: its scale differs from the 200 pixel blocks of Caveland.

Add `?flat=1` to the page address to see the old look (solid colours, no sprites).

## `audio/`

Sounds and music of the Java game.

## Licence: not cleared

The art and audio come from the Java Caveland project. Its credits name **Frederic Brueckner** for the
art and "SteinImBrett" (Felix von Dohlen, Marcel Gohsen) for music and sound. There is no licence file
for them in the repository, so whether they may be redistributed (for example by hosting this page
publicly) is **unknown**. Check with them before publishing; until then keep these assets in a private
repository or on a private server.

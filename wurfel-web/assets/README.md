# Assets

## `sprites/`: the sprite atlas

`sprites0.png` to `sprites2.png` and `sprites.atlas` are generated, do not edit them. They repack the
art of the Java game with `python3 tools/build_atlas.py` (needs Pillow), from
`Caveland/src/main/resources/com/bombinggames/caveland/`:

* `Spritesheet.png/.txt`: the blocks `b<id>-<value>-<side>` (side 0 left, 1 top, 2 right) and
  `b<id>-<value>` (a block that is one picture), the entities `e<id>-<value>` and the interface sprites
  `i<id>-<value>`.
* `playerSheet.png/.txt`: only the walking frames `diff/w/1` to `diff/w/64` of the player (8 steps in
  each of 8 directions). The other player animations (jump, hit, throw...) are not packed yet.

The result is 457 sprites on three 2048x2048 pages (WebGL2 only guarantees 2048 pixel textures), 5.5 MB.
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

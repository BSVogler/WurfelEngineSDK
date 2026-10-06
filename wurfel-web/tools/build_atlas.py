#!/usr/bin/env python3
"""Repacks the Caveland sprite sheets into the small atlas the browser client loads.

Reads the libGDX TexturePacker output of the Java game (Spritesheet.png/.txt with the blocks `b*`,
entities `e*` and interface sprites `i*`, and playerSheet.png/.txt of which only the walking frames
`diff/<action>/*`, the player's frames, are kept) and writes `assets/sprites/sprites<N>.png` plus `assets/sprites/sprites.atlas`,
in the same libGDX atlas text format, so `src/atlas.rs` parses the original files and these alike.

The 4096x4096 sheets hold much empty space and the browser's WebGL2 baseline only guarantees
2048x2048 textures, so the sprites are packed again into 2048x2048 pages (a texture array). Every
sprite gets a border of its own edge pixels so linear filtering never reads a neighbour.

Usage: python3 tools/build_atlas.py [path/to/caveland/resources/com/bombinggames/caveland]
Needs Pillow. Normal maps are not used by the client and are not converted.
"""
import os
import sys
from PIL import Image

PAGE = 2048
PAD = 2  # pixels of edge extrusion around every sprite

DEFAULT_SRC = os.path.join(
    os.path.dirname(__file__), "..", "..", "..", "Caveland", "src", "main", "resources", "com", "bombinggames", "caveland"
)
OUT = os.path.join(os.path.dirname(__file__), "..", "assets", "sprites")

PLAYER_ACTIONS = ("w", "h", "l", "i", "o", "s", "t", "j")

# (sheet name, predicate on the region name)
SOURCES = [
    ("Spritesheet", lambda name: name != "error"),
    # Ejira's frames: w walking, h hit, l loaded hit, i power attack, o its overlay, s the charge
    # overlay, t throw, j jump. Everything of the diffuse sheet; the normal maps are not converted.
    ("playerSheet", lambda name: name.startswith("diff/") and name.split("/")[1] in PLAYER_ACTIONS),
]


def parse_atlas(path):
    """Regions of a libGDX atlas file: name -> dict with xy, size, orig, offset (ints)."""
    regions, current = {}, None
    for line in open(path, encoding="utf-8"):
        line = line.rstrip("\n")
        if not line.strip():
            current = None
            continue
        if not line.startswith((" ", "\t")) and ":" not in line:
            # a page file name or a region name
            current = {}
            regions[line.strip()] = current
        elif current is not None and ":" in line:
            key, value = line.strip().split(":", 1)
            current[key] = value.strip()
    out = {}
    for name, r in regions.items():
        if "xy" not in r:
            continue  # page header
        if r.get("rotate", "false") != "false":
            raise SystemExit(f"{name}: rotated regions are not supported")
        pair = lambda key: tuple(int(v) for v in r[key].split(","))
        out[name] = {"xy": pair("xy"), "size": pair("size"), "orig": pair("orig"), "offset": pair("offset")}
    return out


def pack(sprites):
    """Shelf packing, tallest first. Returns (placements, page count); placement = (page, x, y)."""
    order = sorted(range(len(sprites)), key=lambda i: (-sprites[i][1].height, -sprites[i][1].width))
    placements = [None] * len(sprites)
    page, x, y, shelf = 0, 0, 0, 0
    for i in order:
        w, h = sprites[i][1].size
        w, h = w + 2 * PAD, h + 2 * PAD
        if w > PAGE or h > PAGE:
            raise SystemExit(f"{sprites[i][0]} does not fit on a page")
        if x + w > PAGE:
            x, y, shelf = 0, y + shelf, 0
        if y + h > PAGE:
            page, x, y, shelf = page + 1, 0, 0, 0
        placements[i] = (page, x + PAD, y + PAD)
        x += w
        shelf = max(shelf, h)
    return placements, page + 1


def extruded(image):
    w, h = image.size
    out = Image.new("RGBA", (w + 2 * PAD, h + 2 * PAD))
    out.paste(image, (PAD, PAD))
    out.paste(image.crop((0, 0, w, 1)).resize((w, PAD), Image.NEAREST), (PAD, 0))
    out.paste(image.crop((0, h - 1, w, h)).resize((w, PAD), Image.NEAREST), (PAD, h + PAD))
    out.paste(out.crop((PAD, 0, PAD + 1, h + 2 * PAD)).resize((PAD, h + 2 * PAD), Image.NEAREST), (0, 0))
    out.paste(out.crop((w + PAD - 1, 0, w + PAD, h + 2 * PAD)).resize((PAD, h + 2 * PAD), Image.NEAREST), (w + PAD, 0))
    return out


def main():
    src = os.path.abspath(sys.argv[1] if len(sys.argv) > 1 else DEFAULT_SRC)
    sprites = []  # (name, cropped image, meta)
    for sheet, keep in SOURCES:
        image = Image.open(os.path.join(src, sheet + ".png")).convert("RGBA")
        for name, r in parse_atlas(os.path.join(src, sheet + ".txt")).items():
            if keep(name):
                x, y = r["xy"]
                w, h = r["size"]
                sprites.append((name, image.crop((x, y, x + w, y + h)), r))
    placements, pages = pack(sprites)

    os.makedirs(OUT, exist_ok=True)
    images = [Image.new("RGBA", (PAGE, PAGE)) for _ in range(pages)]
    for (name, crop, meta), (page, x, y) in zip(sprites, placements):
        images[page].paste(extruded(crop), (x - PAD, y - PAD))
    for old in os.listdir(OUT):
        if old.startswith("sprites") and old.endswith(".png"):
            os.remove(os.path.join(OUT, old))
    for i, image in enumerate(images):
        image.save(os.path.join(OUT, f"sprites{i}.png"), optimize=True)

    lines = []
    for page in range(pages):
        lines += [f"sprites{page}.png", f"size: {PAGE}, {PAGE}", "format: RGBA8888", "filter: Linear,Linear", "repeat: none"]
        for (name, crop, meta), (p, x, y) in zip(sprites, placements):
            if p != page:
                continue
            lines += [
                name,
                "  rotate: false",
                f"  xy: {x}, {y}",
                f"  size: {meta['size'][0]}, {meta['size'][1]}",
                f"  orig: {meta['orig'][0]}, {meta['orig'][1]}",
                f"  offset: {meta['offset'][0]}, {meta['offset'][1]}",
                "  index: -1",
            ]
        lines.append("")
    with open(os.path.join(OUT, "sprites.atlas"), "w", encoding="utf-8") as f:
        f.write("\n".join(lines))
    area = sum(c.width * c.height for _, c, _ in sprites)
    print(f"{len(sprites)} sprites, {area / 1e6:.1f} Mpx on {pages} pages of {PAGE}x{PAGE}")


if __name__ == "__main__":
    main()

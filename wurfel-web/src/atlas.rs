//! Reader for libGDX `TexturePacker` atlas files (the text format of the Java engine's
//! `Spritesheet.txt`), mapping sprite names to rectangles in the page images.
//!
//! Pure data, no graphics dependency. Atlas pixel coordinates have their origin at the top left of a
//! page. `offset_y` is measured from the *bottom* of the original image, like libGDX does: a sprite
//! that was trimmed sits `orig_h - offset_y - h` pixels below the top of its original box.

use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq)]
pub struct Page {
    pub file: String,
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Region {
    pub name: String,
    /// Index into [`Atlas::pages`].
    pub page: usize,
    /// Top left corner of the (trimmed) sprite on its page, in pixels.
    pub x: u32,
    pub y: u32,
    /// Size of the (trimmed) sprite.
    pub w: u32,
    pub h: u32,
    /// Size of the sprite before trimming transparent borders.
    pub orig_w: u32,
    pub orig_h: u32,
    /// Where the trimmed sprite sits in the original: pixels from the left and from the bottom.
    pub offset_x: i32,
    pub offset_y: i32,
}

impl Region {
    /// Pixels between the top of the original box and the top of the trimmed sprite.
    pub fn top_in_orig(&self) -> i32 {
        self.orig_h as i32 - self.offset_y - self.h as i32
    }

    /// Atlas texture coordinates of a point given in pixels of the *original* box (origin top
    /// left). Points in the trimmed-away border land outside the sprite on the page; the packer
    /// leaves transparent edge pixels there, and callers only ask for points inside the art.
    pub fn orig_point_uv(&self, atlas: &Atlas, px: f32, py: f32) -> [f32; 2] {
        let page = &atlas.pages[self.page];
        [
            (self.x as f32 + px - self.offset_x as f32) / page.width as f32,
            (self.y as f32 + py - self.top_in_orig() as f32) / page.height as f32,
        ]
    }
}

#[derive(Debug, Clone, Default)]
pub struct Atlas {
    pub pages: Vec<Page>,
    regions: Vec<Region>,
    by_name: HashMap<String, usize>,
}

impl Atlas {
    /// Parse the text of an atlas file. Several pages and the libGDX "old" layout (page header
    /// of `size`, `format`, `filter`, `repeat`) are supported; rotated regions are an error because
    /// the meshing code could not place them.
    pub fn parse(text: &str) -> Result<Atlas, String> {
        let mut atlas = Atlas::default();
        let mut expect_page = true;
        // The region being read, and whether the current page header is still open.
        let mut region: Option<Region> = None;
        let mut header_open = false;

        let finish = |atlas: &mut Atlas, region: &mut Option<Region>| -> Result<(), String> {
            if let Some(r) = region.take() {
                if r.w == 0 || r.h == 0 {
                    return Err(format!("region '{}' has no size", r.name));
                }
                atlas.by_name.insert(r.name.clone(), atlas.regions.len());
                atlas.regions.push(r);
            }
            Ok(())
        };

        for (number, raw) in text.lines().enumerate() {
            let line = raw.trim_end();
            let at = |msg: String| format!("line {}: {msg}", number + 1);
            if line.trim().is_empty() {
                finish(&mut atlas, &mut region)?;
                expect_page = true;
                header_open = false;
                continue;
            }
            let indented = line.starts_with(' ') || line.starts_with('\t');
            if let Some((key, value)) = line.trim().split_once(':').filter(|_| indented || header_open) {
                let (key, value) = (key.trim(), value.trim());
                if header_open && !indented {
                    let page = atlas.pages.last_mut().expect("a header belongs to a page");
                    if key == "size" {
                        let (w, h) = pair(value).map_err(&at)?;
                        page.width = w as u32;
                        page.height = h as u32;
                    }
                    continue;
                }
                let current = region.as_mut().ok_or_else(|| at(format!("'{key}' outside of a region")))?;
                match key {
                    "rotate" if value != "false" => return Err(at(format!("'{}' is rotated, which is not supported", current.name))),
                    "xy" => {
                        let (x, y) = pair(value).map_err(&at)?;
                        current.x = x as u32;
                        current.y = y as u32;
                    }
                    "size" => {
                        let (w, h) = pair(value).map_err(&at)?;
                        current.w = w as u32;
                        current.h = h as u32;
                    }
                    "orig" => {
                        let (w, h) = pair(value).map_err(&at)?;
                        current.orig_w = w as u32;
                        current.orig_h = h as u32;
                    }
                    "offset" => {
                        let (x, y) = pair(value).map_err(&at)?;
                        current.offset_x = x;
                        current.offset_y = y;
                    }
                    _ => {} // index, split, pad...: not needed
                }
                continue;
            }
            if expect_page {
                finish(&mut atlas, &mut region)?;
                atlas.pages.push(Page { file: line.trim().to_string(), width: 0, height: 0 });
                expect_page = false;
                header_open = true;
            } else {
                finish(&mut atlas, &mut region)?;
                header_open = false;
                let page = atlas.pages.len().checked_sub(1).ok_or_else(|| at("region before any page".into()))?;
                region = Some(Region {
                    name: line.trim().to_string(),
                    page,
                    x: 0,
                    y: 0,
                    w: 0,
                    h: 0,
                    orig_w: 0,
                    orig_h: 0,
                    offset_x: 0,
                    offset_y: 0,
                });
            }
        }
        finish(&mut atlas, &mut region)?;
        // Regions without an `orig` line are untrimmed.
        for r in &mut atlas.regions {
            if r.orig_w == 0 {
                r.orig_w = r.w;
                r.orig_h = r.h;
            }
        }
        if let Some(page) = atlas.pages.iter().find(|p| p.width == 0 || p.height == 0) {
            return Err(format!("page '{}' has no size", page.file));
        }
        Ok(atlas)
    }

    pub fn region(&self, name: &str) -> Option<&Region> {
        self.by_name.get(name).map(|&i| &self.regions[i])
    }

    pub fn len(&self) -> usize {
        self.regions.len()
    }

    /// All regions, in file order; an index into this slice identifies a region.
    pub fn regions(&self) -> &[Region] {
        &self.regions
    }

    pub fn at(&self, index: usize) -> &Region {
        &self.regions[index]
    }
}

fn pair(value: &str) -> Result<(i32, i32), String> {
    let (a, b) = value.split_once(',').ok_or_else(|| format!("expected 'a, b', got '{value}'"))?;
    let number = |s: &str| s.trim().parse::<i32>().map_err(|_| format!("'{}' is not a whole number", s.trim()));
    Ok((number(a)?, number(b)?))
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "\nsheet0.png\nsize: 256, 128\nformat: RGBA8888\nfilter: Linear,Linear\nrepeat: none\n\
b1-0-0\n  rotate: false\n  xy: 2, 4\n  size: 100, 173\n  orig: 100, 173\n  offset: 0, 0\n  index: -1\n\
e5-0\n  rotate: false\n  xy: 110, 4\n  size: 30, 40\n  orig: 50, 60\n  offset: 5, 8\n  index: -1\n\
\nsheet1.png\nsize: 64, 64\nformat: RGBA8888\nfilter: Nearest,Nearest\nrepeat: none\n\
diff/w/1\n  rotate: false\n  xy: 0, 0\n  size: 10, 10\n  orig: 10, 10\n  offset: 0, 0\n  index: -1\n";

    #[test]
    fn pages_regions_and_their_pages_are_read() {
        let atlas = Atlas::parse(SAMPLE).unwrap();
        assert_eq!(atlas.pages, vec![
            Page { file: "sheet0.png".into(), width: 256, height: 128 },
            Page { file: "sheet1.png".into(), width: 64, height: 64 },
        ]);
        assert_eq!(atlas.len(), 3);
        let grass = atlas.region("b1-0-0").unwrap();
        assert_eq!((grass.page, grass.x, grass.y, grass.w, grass.h), (0, 2, 4, 100, 173));
        assert_eq!(atlas.region("diff/w/1").unwrap().page, 1, "a region belongs to the page above it");
        assert!(atlas.region("nope").is_none());
    }

    #[test]
    fn a_trimmed_sprite_sits_below_the_top_of_its_original_box_by_what_the_bottom_offset_leaves() {
        let atlas = Atlas::parse(SAMPLE).unwrap();
        let e = atlas.region("e5-0").unwrap();
        // orig 50x60, sprite 30x40, 5 from the left, 8 from the bottom: 60 - 8 - 40 = 12 from the top
        assert_eq!(e.top_in_orig(), 12);
        let uv = e.orig_point_uv(&atlas, 5.0, 12.0);
        assert_eq!(uv, [110.0 / 256.0, 4.0 / 128.0], "the top left of the art is the region's corner");
        let uv = e.orig_point_uv(&atlas, 35.0, 52.0);
        assert_eq!(uv, [140.0 / 256.0, 44.0 / 128.0], "and its bottom right is the opposite corner");
    }

    #[test]
    fn rotated_regions_and_garbage_are_errors_not_silently_wrong_sprites() {
        let rotated = SAMPLE.replace("rotate: false\n  xy: 2, 4", "rotate: true\n  xy: 2, 4");
        assert!(Atlas::parse(&rotated).unwrap_err().contains("rotated"));
        let bad = SAMPLE.replace("xy: 2, 4", "xy: two, 4");
        assert!(Atlas::parse(&bad).unwrap_err().contains("not a whole number"));
        let no_size = "sheet.png\nformat: RGBA8888\n\n";
        assert!(Atlas::parse(no_size).unwrap_err().contains("no size"));
    }

    #[test]
    fn a_region_without_orig_is_untrimmed() {
        let text = "s.png\nsize: 16, 16\nthing\n  xy: 1, 1\n  size: 7, 9\n";
        let r = Atlas::parse(text).unwrap();
        let region = r.region("thing").unwrap();
        assert_eq!((region.orig_w, region.orig_h, region.top_in_orig()), (7, 9, 0));
    }

    #[test]
    fn the_shipped_atlas_parses_and_has_the_sprites_the_game_looks_up() {
        let text = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/assets/sprites/sprites.atlas")).unwrap();
        let atlas = Atlas::parse(&text).unwrap();
        assert!(!atlas.pages.is_empty());
        for page in &atlas.pages {
            assert!(
                std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/sprites").join(&page.file).exists(),
                "page image {} is missing",
                page.file
            );
            assert!(page.width <= 2048 && page.height <= 2048, "WebGL2 only guarantees 2048 pixel textures");
        }
        for name in ["b1-0-0", "b1-0-1", "b1-0-2", "b11-0", "e46-0", "diff/w/1", "diff/w/64"] {
            assert!(atlas.region(name).is_some(), "{name}");
        }
        for r in atlas.regions() {
            let page = &atlas.pages[r.page];
            assert!(r.x + r.w <= page.width && r.y + r.h <= page.height, "{} leaves its page", r.name);
        }
    }
}

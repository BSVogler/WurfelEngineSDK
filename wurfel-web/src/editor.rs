//! The map editor mode (the browser counterpart of the Java engine's `editor` package).
//!
//! Outside the editor the mouse never changes blocks. Inside it, the left mouse button uses the
//! tool picked in the toolbar (Java: `Toolbar.selectionLeft`, starts as DRAW), the right button
//! always erases (Java: `selectionRight = ERASE`) and the middle button (or Alt + left) picks the
//! block under the cursor. The number keys choose the block to place (Java: `BlockTable`). Holding
//! the button while drawing or replacing paints along the way, the bucket fills a rectangle between
//! press and release, and every change can be undone and redone (Java: `Controller.undoCommand`).
//!
//! This file is pure logic with no browser types, so it is unit tested natively. `web.rs` feeds it
//! the pointer and sends the resulting edits to the server.

use wurfel_sim::block::id;
use wurfel_sim::protocol::MAX_FILL_CELLS;
use wurfel_sim::Block;

use crate::pick::Pick;

/// Blocks the editor can place, in number-key order. The server accepts exactly these (and air).
pub const PALETTE: [(u8, &str); 4] =
    [(id::STONE, "stone"), (id::DIRT, "dirt"), (id::GRASS, "grass"), (id::SAND, "sand")];

/// The Java `Tool`s that make sense for blocks without entities: DRAW, BUCKET, REPLACE and ERASE,
/// plus an eyedropper (`Pick`) that Java does not have. (SELECT and SPAWN work on entities.)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tool {
    /// Put the selected block in the empty cell in front of the hit block.
    Draw,
    /// Fill the rectangle between where the button went down and where it came up, on the layer of
    /// the block first hit.
    Bucket,
    /// Overwrite the hit block with the selected one.
    Replace,
    /// Remove the hit block.
    Erase,
    /// Select the kind of the hit block for drawing.
    Pick,
}

impl Tool {
    pub const ALL: [Tool; 5] = [Tool::Draw, Tool::Bucket, Tool::Replace, Tool::Erase, Tool::Pick];

    pub fn name(self) -> &'static str {
        match self {
            Tool::Draw => "draw",
            Tool::Bucket => "bucket",
            Tool::Replace => "replace",
            Tool::Erase => "erase",
            Tool::Pick => "pick",
        }
    }

    pub fn parse(name: &str) -> Option<Tool> {
        Tool::ALL.into_iter().find(|t| t.name() == name)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Button {
    Left,
    Right,
    Middle,
}

impl Button {
    /// From `MouseEvent.button`.
    pub fn from_dom(button: i16) -> Option<Button> {
        match button {
            0 => Some(Button::Left),
            1 => Some(Button::Middle),
            2 => Some(Button::Right),
            _ => None,
        }
    }
}

/// A block change the editor wants the server to make.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Edit {
    pub x: i32,
    pub y: i32,
    pub z: i32,
    /// Raw block (`0` removes).
    pub block: u16,
}

/// A rectangle of columns on one layer to fill (`ClientMsg::FillBlocks`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Fill {
    pub from: (i32, i32),
    pub to: (i32, i32),
    pub z: i32,
    pub block: u16,
}

/// What the editor remembers of how many changes, oldest dropped first.
const HISTORY: usize = 100;

/// One undoable step: the cells it changed with the block each had before.
type Step = Vec<(Edit, u16)>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Editor {
    active: bool,
    left: Tool,
    selected: usize,
    /// The cell the left button last painted, so a drag does not repeat itself. `Some` while it is
    /// held with a painting tool.
    painting: Option<(i32, i32, i32)>,
    /// Where the bucket went down.
    bucket_from: Option<(i32, i32, i32)>,
    undo: Vec<Step>,
    redo: Vec<Step>,
}

impl Default for Editor {
    fn default() -> Self {
        Editor { active: false, left: Tool::Draw, selected: 0, painting: None, bucket_from: None, undo: Vec::new(), redo: Vec::new() }
    }
}

impl Editor {
    pub fn active(&self) -> bool {
        self.active
    }

    #[cfg(test)]
    pub fn selected(&self) -> usize {
        self.selected
    }

    pub fn selected_block(&self) -> u8 {
        PALETTE[self.selected].0
    }

    /// Enter or leave. Returns the new state.
    #[cfg(test)]
    pub fn toggle(&mut self) -> bool {
        self.active = !self.active;
        self.active
    }

    pub fn set_active(&mut self, active: bool) {
        self.active = active;
        // A started stroke dies with the mode; the history belongs to one visit (the world may
        // have been changed by others in between).
        self.painting = None;
        self.bucket_from = None;
        if !active {
            self.undo.clear();
            self.redo.clear();
        }
    }

    pub fn select_tool(&mut self, tool: Tool) {
        self.left = tool;
    }

    /// Choose a palette entry; an index outside the palette is ignored. Only works in the editor.
    pub fn select_block(&mut self, index: usize) -> bool {
        if self.active && index < PALETTE.len() {
            self.selected = index;
            true
        } else {
            false
        }
    }

    /// Number key `1`..`4` as the palette index it chooses.
    pub fn index_for_key(key: &str) -> Option<usize> {
        key.parse::<usize>().ok().filter(|n| (1..=PALETTE.len()).contains(n)).map(|n| n - 1)
    }

    fn tool_for(&self, button: Button) -> Tool {
        match button {
            Button::Left => self.left,
            Button::Right => Tool::Erase,
            Button::Middle => Tool::Pick,
        }
    }

    /// A mouse button went down over `target` (`None` when it is over nothing). `block_at` reads a
    /// cell. Returns the change to ask the server for. Outside the editor this does nothing at all.
    pub fn click(&mut self, button: Button, target: Option<Pick>, block_at: impl Fn((i32, i32, i32)) -> Block) -> Option<Edit> {
        if !self.active {
            return None;
        }
        let target = target?;
        let tool = self.tool_for(button);
        match tool {
            Tool::Draw => {
                self.painting = (button == Button::Left).then_some(target.place);
                self.set(target.place, self.selected_raw(), &block_at)
            }
            Tool::Replace => {
                self.painting = (button == Button::Left).then_some(target.hit);
                self.set(target.hit, self.selected_raw(), &block_at)
            }
            Tool::Erase => self.set(target.hit, 0, &block_at),
            Tool::Bucket => {
                self.bucket_from = Some(target.hit);
                None
            }
            Tool::Pick => {
                let picked = block_at(target.hit).id();
                if let Some(index) = PALETTE.iter().position(|(id, _)| *id == picked) {
                    self.selected = index;
                }
                None
            }
        }
    }

    /// The pointer moved over `target` while the left button is held. Painting tools continue their
    /// stroke (Java: dragging with DRAW or REPLACE) and never touch the same cell twice in a row.
    pub fn drag(&mut self, target: Option<Pick>, block_at: impl Fn((i32, i32, i32)) -> Block) -> Option<Edit> {
        let last = self.painting?;
        let target = target?;
        let cell = if self.left == Tool::Draw { target.place } else { target.hit };
        if cell == last || self.left == Tool::Bucket {
            return None;
        }
        self.painting = Some(cell);
        self.set(cell, self.selected_raw(), &block_at)
    }

    /// The left button went up over `target`: ends a stroke, and for the bucket yields the fill
    /// from where it went down. A rectangle the server would refuse for its size gives nothing.
    pub fn release(&mut self, target: Option<Pick>, block_at: impl Fn((i32, i32, i32)) -> Block) -> Option<Fill> {
        self.painting = None;
        let from = self.bucket_from.take()?;
        let to = target?.hit;
        let cells = (from.0.abs_diff(to.0) as usize + 1) * (from.1.abs_diff(to.1) as usize + 1);
        if !self.active || cells > MAX_FILL_CELLS {
            return None;
        }
        let block = self.selected_raw();
        let (lo, hi) = ((from.0.min(to.0), from.1.min(to.1)), (from.0.max(to.0), from.1.max(to.1)));
        let step: Step = (lo.1..=hi.1)
            .flat_map(|y| (lo.0..=hi.0).map(move |x| (x, y, from.2)))
            .filter_map(|(x, y, z)| {
                let before = block_at((x, y, z)).raw();
                (before != block).then_some((Edit { x, y, z, block }, before))
            })
            .collect();
        self.remember(step);
        Some(Fill { from: (from.0, from.1), to: (to.0, to.1), z: from.2, block })
    }

    /// Take back the last change: the edits that restore the cells it touched.
    pub fn undo(&mut self) -> Vec<Edit> {
        self.step_back(true)
    }

    /// Do the last undone change again.
    pub fn redo(&mut self) -> Vec<Edit> {
        self.step_back(false)
    }

    fn step_back(&mut self, undo: bool) -> Vec<Edit> {
        if !self.active {
            return Vec::new();
        }
        let (from, to) = if undo { (&mut self.undo, &mut self.redo) } else { (&mut self.redo, &mut self.undo) };
        let Some(step) = from.pop() else { return Vec::new() };
        // Reverse order so overlapping cells end where they started. The opposite step stores what
        // is being overwritten, which is what this step wrote.
        let edits: Vec<Edit> = step.iter().rev().map(|&(edit, previous)| Edit { block: previous, ..edit }).collect();
        to.push(step.iter().map(|&(edit, previous)| (Edit { block: previous, ..edit }, edit.block)).collect());
        edits
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    fn selected_raw(&self) -> u16 {
        Block::new(self.selected_block(), 0).raw()
    }

    /// One edit of one cell (nothing when it is already that), remembered for undo.
    fn set(&mut self, (x, y, z): (i32, i32, i32), block: u16, block_at: &impl Fn((i32, i32, i32)) -> Block) -> Option<Edit> {
        let before = block_at((x, y, z)).raw();
        if before == block {
            return None;
        }
        let edit = Edit { x, y, z, block };
        self.remember(vec![(edit, before)]);
        Some(edit)
    }

    fn remember(&mut self, step: Step) {
        if step.is_empty() {
            return;
        }
        self.redo.clear();
        self.undo.push(step);
        if self.undo.len() > HISTORY {
            self.undo.remove(0);
        }
    }

    /// The state for the page's toolbar (`wurfelEditor.update`), with the cursor line.
    pub fn ui_json(&self, cursor: &str) -> String {
        serde_json::json!({
            "active": self.active,
            "tool": self.left.name(),
            "selected": self.selected,
            "tools": Tool::ALL.map(Tool::name),
            "undo": self.can_undo(),
            "redo": self.can_redo(),
            "blocks": PALETTE.map(|(_, name)| name),
            "cursor": cursor,
        })
        .to_string()
    }
}

/// The Java `CursorInfo` line: where the cursor is and what is there.
pub fn cursor_text(target: Option<Pick>, hit_id: u8) -> String {
    match target {
        None => String::new(),
        Some(Pick { hit: (x, y, z), .. }) => {
            let name = PALETTE.iter().find(|(id, _)| *id == hit_id).map_or("block", |(_, n)| n);
            format!("{x}, {y}, {z} · {name} (id {hit_id})")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TARGET: Pick = Pick { hit: (4, 5, 6), place: (4, 5, 7) };
    const STONE: u16 = id::STONE as u16;

    /// A world where the target block is stone and everything else is air.
    fn world((x, y, z): (i32, i32, i32)) -> Block {
        if (x, y, z) == TARGET.hit { Block::new(id::STONE, 0) } else { Block::AIR }
    }

    fn active() -> Editor {
        let mut e = Editor::default();
        e.toggle();
        e
    }

    #[test]
    fn the_mouse_edits_nothing_outside_the_editor() {
        let mut e = Editor::default();
        for button in [Button::Left, Button::Right, Button::Middle] {
            assert_eq!(e.click(button, Some(TARGET), world), None);
        }
        assert_eq!(e.drag(Some(TARGET), world), None);
        assert_eq!(e.release(Some(TARGET), world), None);
        assert!(!e.select_block(2), "number keys do not choose blocks outside the editor");
        assert_eq!(e.selected(), 0);
        assert!(e.toggle());
        assert!(!e.toggle());
    }

    #[test]
    fn left_draws_in_the_empty_cell_and_right_erases_the_hit_block() {
        let mut e = active();
        assert_eq!(e.click(Button::Left, Some(TARGET), world), Some(Edit { x: 4, y: 5, z: 7, block: STONE }));
        assert_eq!(e.click(Button::Right, Some(TARGET), world), Some(Edit { x: 4, y: 5, z: 6, block: 0 }));
        assert_eq!(e.click(Button::Left, None, world), None, "nothing under the cursor");
    }

    #[test]
    fn the_toolbar_changes_what_the_left_button_does() {
        let mut e = active();
        assert!(e.select_block(3));
        let sand = id::SAND as u16;
        e.select_tool(Tool::Replace);
        assert_eq!(e.click(Button::Left, Some(TARGET), world), Some(Edit { x: 4, y: 5, z: 6, block: sand }));
        e.select_tool(Tool::Erase);
        assert_eq!(e.click(Button::Left, Some(TARGET), world), Some(Edit { x: 4, y: 5, z: 6, block: 0 }));
        e.select_tool(Tool::Draw);
        assert_eq!(e.click(Button::Left, Some(TARGET), world), Some(Edit { x: 4, y: 5, z: 7, block: sand }));
        assert_eq!(e.click(Button::Right, Some(TARGET), world).unwrap().block, 0, "right stays erase");
    }

    #[test]
    fn a_change_that_changes_nothing_is_not_sent() {
        let mut e = active();
        e.select_tool(Tool::Replace); // the selected block is stone and so is the target
        assert_eq!(e.click(Button::Left, Some(TARGET), world), None);
        assert!(!e.can_undo());
    }

    #[test]
    fn middle_click_picks_the_block_kind_without_editing() {
        let mut e = active();
        assert_eq!(e.click(Button::Middle, Some(TARGET), |_| Block::new(id::GRASS, 0)), None);
        assert_eq!(e.selected_block(), id::GRASS);
        // Something that cannot be placed (water, say) leaves the selection alone.
        e.click(Button::Middle, Some(TARGET), |_| Block::new(id::WATER, 0));
        assert_eq!(e.selected_block(), id::GRASS);
        assert!(!e.can_undo(), "picking is not a change");
    }

    #[test]
    fn dragging_paints_each_new_cell_once_while_the_button_is_held() {
        let mut e = active();
        e.select_tool(Tool::Draw);
        let at = |place: (i32, i32, i32)| Some(Pick { hit: (place.0, place.1, place.2 - 1), place });
        assert!(e.drag(at((1, 1, 1)), world).is_none(), "no stroke without a press");
        assert!(e.click(Button::Left, at((1, 1, 1)), world).is_some());
        assert!(e.drag(at((1, 1, 1)), world).is_none(), "same cell");
        assert_eq!(e.drag(at((2, 1, 1)), world), Some(Edit { x: 2, y: 1, z: 1, block: STONE }));
        assert!(e.drag(None, world).is_none());
        assert!(e.drag(at((2, 1, 1)), world).is_none());
        e.release(at((2, 1, 1)), world);
        assert!(e.drag(at((3, 1, 1)), world).is_none(), "the stroke ended");
        // Replace paints over the hit blocks instead.
        e.select_tool(Tool::Replace);
        e.select_block(1);
        e.click(Button::Left, at((1, 1, 1)), world);
        assert_eq!(e.drag(at((2, 1, 1)), world).map(|d| (d.x, d.y, d.z)), Some((2, 1, 0)));
        // The right button erases single blocks and never strokes.
        e.release(None, world);
        e.click(Button::Right, Some(TARGET), world);
        assert!(e.drag(at((5, 5, 5)), world).is_none());
    }

    #[test]
    fn the_bucket_fills_the_rectangle_from_press_to_release() {
        let mut e = active();
        e.select_tool(Tool::Bucket);
        e.select_block(2); // grass
        let grass = id::GRASS as u16;
        let hit = |x, y| Some(Pick { hit: (x, y, 3), place: (x, y, 4) });
        assert_eq!(e.click(Button::Left, hit(5, 5), world), None, "nothing happens before the release");
        let fill = e.release(hit(3, 6), world).expect("a fill");
        assert_eq!(fill, Fill { from: (5, 5), to: (3, 6), z: 3, block: grass });
        assert!(e.can_undo());
        // Release without a press, or onto nothing, fills nothing.
        assert_eq!(e.release(hit(1, 1), world), None);
        e.click(Button::Left, hit(0, 0), world);
        assert_eq!(e.release(None, world), None);
        // Too large for the server: dropped here as well.
        e.click(Button::Left, hit(0, 0), world);
        assert_eq!(e.release(hit(MAX_FILL_CELLS as i32, 1), world), None);
        // The right button never starts a fill.
        e.click(Button::Right, hit(0, 0), world);
        assert_eq!(e.release(hit(2, 2), world), None);
    }

    #[test]
    fn undo_and_redo_walk_through_the_changes() {
        let mut e = active();
        assert!(e.undo().is_empty() && e.redo().is_empty());
        let drawn = e.click(Button::Left, Some(TARGET), world).unwrap();
        let erased = e.click(Button::Right, Some(TARGET), world).unwrap();
        assert_eq!(e.undo(), vec![Edit { block: STONE, ..erased }], "the erased stone comes back");
        assert_eq!(e.undo(), vec![Edit { block: 0, ..drawn }], "the drawn block goes away");
        assert!(!e.can_undo() && e.can_redo());
        assert_eq!(e.redo(), vec![drawn]);
        assert_eq!(e.redo(), vec![erased]);
        assert!(e.redo().is_empty());
        // A new change forgets what was undone.
        e.undo();
        e.click(Button::Left, Some(TARGET), world);
        assert!(!e.can_redo());
        // Leaving the editor drops the history.
        e.set_active(false);
        assert!(!e.can_undo());
    }

    #[test]
    fn undoing_a_fill_restores_every_cell_it_changed() {
        let mut e = active();
        e.select_tool(Tool::Bucket);
        let hit = |x, y| Some(Pick { hit: (x, y, 2), place: (x, y, 3) });
        e.click(Button::Left, hit(0, 0), world);
        e.release(hit(1, 1), world);
        let mut back = e.undo();
        back.sort_by_key(|b| (b.x, b.y));
        assert_eq!(back.len(), 4);
        assert!(back.iter().all(|b| b.z == 2 && b.block == 0));
        // The step it makes for redo is the fill again.
        let again = e.redo();
        assert_eq!(again.len(), 4);
        assert!(again.iter().all(|b| b.block == STONE));
    }

    #[test]
    fn block_keys_and_tool_names_parse_strictly() {
        assert_eq!(Editor::index_for_key("1"), Some(0));
        assert_eq!(Editor::index_for_key("4"), Some(3));
        for bad in ["0", "5", "a", "", "-1"] {
            assert_eq!(Editor::index_for_key(bad), None, "{bad:?}");
        }
        for tool in Tool::ALL {
            assert_eq!(Tool::parse(tool.name()), Some(tool));
        }
        assert_eq!(Tool::parse("lasso"), None);
        assert_eq!(Button::from_dom(1), Some(Button::Middle));
        assert_eq!(Button::from_dom(3), None);
    }

    #[test]
    fn cursor_info_and_toolbar_state() {
        assert_eq!(cursor_text(None, 0), "");
        assert_eq!(cursor_text(Some(TARGET), id::DIRT), "4, 5, 6 · dirt (id 2)");
        let mut e = active();
        let json: serde_json::Value = serde_json::from_str(&e.ui_json("x")).unwrap();
        assert_eq!(json["active"], true);
        assert_eq!(json["tool"], "draw");
        assert_eq!(json["blocks"].as_array().unwrap().len(), PALETTE.len());
        assert_eq!((&json["undo"], &json["redo"]), (&false.into(), &false.into()));
        e.click(Button::Left, Some(TARGET), world);
        assert_eq!(serde_json::from_str::<serde_json::Value>(&e.ui_json("")).unwrap()["undo"], true);
    }
}

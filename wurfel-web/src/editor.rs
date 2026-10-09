//! The map editor mode (the browser counterpart of the Java engine's `editor` package).
//!
//! Outside the editor the mouse never changes blocks. Inside it, the left mouse button uses the
//! tool picked in the toolbar (Java: `Toolbar.selectionLeft`, starts as DRAW), the right button
//! always erases (Java: `selectionRight = ERASE`) and the middle button (or Alt + left) picks the
//! block under the cursor. The number keys choose the block to place (Java: `BlockTable`). Holding
//! the button while drawing or replacing paints along the way, the bucket fills a rectangle between
//! press and release, and every change can be undone and redone (Java: `Controller.undoCommand`).
//! The select tool picks a thing (Java: an entity) and drags it, Delete removes it, the spawn tool
//! puts the thing chosen in the toolbar where the block would go (Java: `EntityTable`); `+`/`-`
//! change the block's value; the wheel limits how many layers are drawn (Java: the Z rendering
//! limit) and WASD pans the camera away from the player (Java: `EditorView`'s camera).
//!
//! This file is pure logic with no browser types, so it is unit tested natively. `web.rs` feeds it
//! the pointer and sends the resulting edits to the server.

use glam::Vec2;
use wurfel_sim::block::id;
use wurfel_sim::entity::screen_to_iso;
use wurfel_sim::grid::to_iso;
use wurfel_sim::protocol::{editor_block_values, ThingState, EDITOR_THING_KINDS, MAX_FILL_CELLS};
use wurfel_sim::{Block, CHUNK_SIZE_Z};

use crate::pick::{ground_at_view, Pick};
use crate::view::View;

/// Panning the editor camera, in ground units per second (the player walks at about this speed),
/// and how many times faster with Shift (Java: `setCameraSpeed`, Shift for fast).
pub const PAN_SPEED: f32 = 12.0;
pub const PAN_FAST: f32 = 3.0;
/// How far the camera may be panned from the player, in ground units. The server only reaches
/// `EDITOR_REACH` (twice this, in `wurfel-server`'s `game.rs`) around the player and only sends the chunks
/// around the player, so a camera further away would show nothing that can be edited.
pub const PAN_LIMIT: f32 = 24.0;
/// A thing is dragged in steps of this many ground units, so a drag is not a message per pixel.
const DRAG_STEP: f32 = 0.1;
/// Wheel units (a notch of a mouse wheel is about 100) per layer.
const WHEEL_PER_LAYER: f32 = 100.0;

/// Blocks the editor can place, in number-key order. The server accepts exactly these (and air).
pub const PALETTE: [(u8, &str); 4] =
    [(id::STONE, "stone"), (id::DIRT, "dirt"), (id::GRASS, "grass"), (id::SAND, "sand")];

/// The Java `Tool`s in the Java order (DRAW, BUCKET, REPLACE, SELECT, SPAWN, ERASE), plus an
/// eyedropper (`Pick`) that Java does not have. SELECT and SPAWN work on things, the others on blocks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tool {
    /// Put the selected block in the empty cell in front of the hit block.
    Draw,
    /// Fill the rectangle between where the button went down and where it came up, on the layer of
    /// the block first hit.
    Bucket,
    /// Overwrite the hit block with the selected one.
    Replace,
    /// Select the thing under the pointer and drag it to move it (Delete removes it).
    Select,
    /// Put the chosen kind of thing in the empty cell in front of the hit block.
    Spawn,
    /// Remove the hit block.
    Erase,
    /// Select the kind (and value) of the hit block for drawing.
    Pick,
}

impl Tool {
    pub const ALL: [Tool; 7] = [Tool::Draw, Tool::Bucket, Tool::Replace, Tool::Select, Tool::Spawn, Tool::Erase, Tool::Pick];

    pub fn name(self) -> &'static str {
        match self {
            Tool::Draw => "draw",
            Tool::Bucket => "bucket",
            Tool::Replace => "replace",
            Tool::Select => "select",
            Tool::Spawn => "spawn",
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

/// A change of the things the editor wants the server to make (`SpawnThing`, `MoveThing`,
/// `DeleteThing`). Not part of the undo history: a spawned thing only gets its id from the server.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ThingAction {
    Spawn { kind: &'static str, pos: [f32; 3] },
    Move { id: u32, pos: [f32; 3] },
    Delete { id: u32 },
}

/// What the editor remembers of how many changes, oldest dropped first.
const HISTORY: usize = 100;

/// One undoable step: the cells it changed with the block each had before.
type Step = Vec<(Edit, u16)>;

#[derive(Debug, Clone, PartialEq)]
pub struct Editor {
    active: bool,
    left: Tool,
    selected: usize,
    /// The value (variant) of the block to place, below `editor_block_values` of the selected one.
    value: u8,
    /// Which of `EDITOR_THING_KINDS` the spawn tool puts down.
    thing_kind: usize,
    /// The thing the select tool grabbed, and whether the button is still held on it.
    selected_thing: Option<u32>,
    moving: bool,
    /// Where the thing being dragged was last sent to.
    moved_to: Option<[f32; 3]>,
    /// The highest layer drawn; `None` draws everything (Java: the Z rendering limit).
    layer: Option<i32>,
    /// Wheel movement that has not made a whole layer yet.
    wheel: f32,
    /// How far the camera is panned from the player, in ground units.
    pan: (f32, f32),
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
        Editor {
            active: false,
            left: Tool::Draw,
            selected: 0,
            value: 0,
            thing_kind: 0,
            selected_thing: None,
            moving: false,
            moved_to: None,
            layer: None,
            wheel: 0.0,
            pan: (0.0, 0.0),
            painting: None,
            bucket_from: None,
            undo: Vec::new(),
            redo: Vec::new(),
        }
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
        self.moving = false;
        // The camera and the layer limit are for looking around while editing: the game starts
        // again on the player with every layer.
        self.pan = (0.0, 0.0);
        self.layer = None;
        self.wheel = 0.0;
        if !active {
            self.undo.clear();
            self.redo.clear();
            self.selected_thing = None;
        }
    }

    pub fn select_tool(&mut self, tool: Tool) {
        self.left = tool;
    }

    /// Choose a palette entry; an index outside the palette is ignored. Only works in the editor.
    pub fn select_block(&mut self, index: usize) -> bool {
        if self.active && index < PALETTE.len() {
            self.selected = index;
            self.value = 0; // Java: `selectItem` starts at value 0
            true
        } else {
            false
        }
    }

    /// The block the draw and replace tools put down.
    pub fn brush(&self) -> Block {
        Block::new(self.selected_block(), self.value)
    }

    /// What the left tool would put down with the pointer over `target`, and where: the preview of
    /// the build. Only the draw and replace tools build at a single cell.
    pub fn ghost(&self, target: Option<Pick>) -> Option<(Block, (i32, i32, i32))> {
        if !self.active {
            return None;
        }
        let target = target?;
        let cell = match self.left {
            Tool::Draw => target.place,
            Tool::Replace => target.hit,
            _ => return None,
        };
        Some((self.brush(), cell))
    }

    #[cfg(test)]
    pub fn value(&self) -> u8 {
        self.value
    }

    /// Next (`+1`) or previous (`-1`) value of the selected block, wrapping around. Only in the editor.
    pub fn step_value(&mut self, step: i32) {
        let count = editor_block_values(self.selected_block()) as i32;
        if self.active && count > 1 {
            self.value = (self.value as i32 + step).rem_euclid(count) as u8;
        }
    }

    pub fn select_thing_kind(&mut self, index: usize) {
        if index < EDITOR_THING_KINDS.len() {
            self.thing_kind = index;
        }
    }

    pub fn selected_thing(&self) -> Option<u32> {
        self.selected_thing
    }

    pub fn layer(&self) -> Option<i32> {
        self.layer
    }

    /// Wheel movement (`delta_y` as the page reports it: down is positive). A notch down lowers the
    /// highest drawn layer by one, a notch up raises it, past the top draws everything again.
    /// Returns whether the layer changed. Only in the editor.
    pub fn scroll(&mut self, delta_y: f32) -> bool {
        if !self.active {
            return false;
        }
        self.wheel += delta_y;
        let steps = (self.wheel / WHEEL_PER_LAYER).trunc();
        self.wheel -= steps * WHEEL_PER_LAYER;
        self.step_layer(-(steps as i32))
    }

    /// Show `steps` layers more (positive) or fewer. Returns whether the layer changed.
    pub fn step_layer(&mut self, steps: i32) -> bool {
        if !self.active || steps == 0 {
            return false;
        }
        let top = CHUNK_SIZE_Z - 1;
        let before = self.layer;
        // From "all layers" the first step down shows all but the top one (Java: height - 100).
        let current = match self.layer {
            Some(layer) => layer,
            None if steps < 0 => top,
            None => return false,
        };
        let next = (current + steps).clamp(0, top);
        self.layer = (next < top).then_some(next);
        self.layer != before
    }

    /// Move the camera with the keys: `dir` is the direction on the screen (x right, y down, each
    /// -1, 0 or 1), `dt` the seconds passed. Only in the editor; the camera stays within
    /// [`PAN_LIMIT`] of the player.
    pub fn pan_by(&mut self, dir: (f32, f32), yaw: f32, fast: bool, dt: f32) {
        if !self.active || dir == (0.0, 0.0) {
            return;
        }
        // The keys are directions on the screen: with a turned camera the ground direction is turned back.
        let ground = Vec2::from_angle(-yaw).rotate(screen_to_iso(Vec2::new(dir.0, dir.1).normalize())) * PAN_SPEED * if fast { PAN_FAST } else { 1.0 } * dt;
        let moved = Vec2::new(self.pan.0 + ground.x, self.pan.1 + ground.y).clamp_length_max(PAN_LIMIT);
        self.pan = (moved.x, moved.y);
    }

    /// Where the camera is relative to the player, in ground units.
    pub fn pan(&self) -> (f32, f32) {
        self.pan
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
            // Things have their own handling, see `click_thing`.
            Tool::Select | Tool::Spawn => None,
            Tool::Bucket => {
                self.bucket_from = Some(target.hit);
                None
            }
            Tool::Pick => {
                let picked = block_at(target.hit);
                if let Some(index) = PALETTE.iter().position(|(id, _)| *id == picked.id()) {
                    self.selected = index;
                    // Java: `select(id, value)`. A value the block has no picture for is not kept.
                    self.value = if picked.value() < editor_block_values(picked.id()) { picked.value() } else { 0 };
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
        if !matches!(self.left, Tool::Draw | Tool::Replace) {
            return None;
        }
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
        self.moving = false;
        self.moved_to = None;
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

    /// The left button went down: the spawn tool puts a thing in the cell in front of the block it
    /// hit, the select tool grabs the thing under the pointer (`under`, nothing deselects).
    pub fn click_thing(&mut self, button: Button, target: Option<Pick>, under: Option<u32>) -> Option<ThingAction> {
        if !self.active || button != Button::Left {
            return None;
        }
        match self.left {
            Tool::Spawn => {
                let (x, y, z) = target?.place;
                let (gx, gy) = to_iso(x, y);
                Some(ThingAction::Spawn { kind: EDITOR_THING_KINDS[self.thing_kind], pos: [gx, gy, z as f32] })
            }
            Tool::Select => {
                self.selected_thing = under;
                self.moving = under.is_some();
                self.moved_to = None;
                None
            }
            _ => None,
        }
    }

    /// The pointer moved at screen position `screen` while the left button is held on a thing:
    /// the thing follows along its layer. Not more than one message per [`DRAG_STEP`] of movement.
    pub fn drag_thing(&mut self, screen: (f32, f32), view: &View, things: &[ThingState]) -> Option<ThingAction> {
        if !self.moving || self.left != Tool::Select {
            return None;
        }
        let id = self.selected_thing?;
        let thing = things.iter().find(|t| t.id == id)?;
        let z = thing.pos[2];
        // The middle of the picture follows the pointer, as it is what `pick_thing` measures to.
        let (gx, gy) = ground_at_view(view, screen.0, screen.1, z + 0.5);
        let snap = |v: f32| (v / DRAG_STEP).round() * DRAG_STEP;
        let pos = [snap(gx), snap(gy), z];
        let from = self.moved_to.unwrap_or(thing.pos);
        (pos != from).then(|| {
            self.moved_to = Some(pos);
            ThingAction::Move { id, pos }
        })
    }

    /// Delete (or Backspace): remove the selected thing.
    pub fn delete_selected(&mut self) -> Option<ThingAction> {
        if !self.active {
            return None;
        }
        self.moving = false;
        self.selected_thing.take().map(|id| ThingAction::Delete { id })
    }

    /// Forget the selection when its thing is gone (somebody else deleted it).
    pub fn keep_selection_in(&mut self, things: &[ThingState]) {
        if self.selected_thing.is_some_and(|id| !things.iter().any(|t| t.id == id)) {
            self.selected_thing = None;
            self.moving = false;
        }
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
        Block::new(self.selected_block(), self.value).raw()
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
    pub fn ui_json(&self, cursor: &str, previews: &serde_json::Value) -> String {
        serde_json::json!({
            "active": self.active,
            "tool": self.left.name(),
            "selected": self.selected,
            "tools": Tool::ALL.map(Tool::name),
            "undo": self.can_undo(),
            "redo": self.can_redo(),
            "blocks": PALETTE.map(|(_, name)| name),
            "value": self.value,
            "values": editor_block_values(self.selected_block()),
            "things": EDITOR_THING_KINDS,
            "thing": self.thing_kind,
            "layer": self.layer,
            "cursor": cursor,
            "previews": previews,
        })
        .to_string()
    }
}

/// The Java `CursorInfo` line: where the cursor is and what is there.
pub fn cursor_text(target: Option<Pick>, hit: Block) -> String {
    match target {
        None => String::new(),
        Some(Pick { hit: (x, y, z), .. }) => {
            let name = PALETTE.iter().find(|(id, _)| *id == hit.id()).map_or("block", |(_, n)| n);
            format!("{x}, {y}, {z} · {name} (id {}, value {})", hit.id(), hit.value())
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
    fn tool_names_parse_strictly() {
        for tool in Tool::ALL {
            assert_eq!(Tool::parse(tool.name()), Some(tool));
        }
        assert_eq!(Tool::parse("lasso"), None);
        assert_eq!(Button::from_dom(1), Some(Button::Middle));
        assert_eq!(Button::from_dom(3), None);
    }

    #[test]
    fn cursor_info_and_toolbar_state() {
        assert_eq!(cursor_text(None, Block::AIR), "");
        assert_eq!(cursor_text(Some(TARGET), Block::new(id::DIRT, 0)), "4, 5, 6 · dirt (id 2, value 0)");
        assert_eq!(cursor_text(Some(TARGET), Block::new(id::STONE, 1)), "4, 5, 6 · stone (id 3, value 1)");
        let mut e = active();
        let json: serde_json::Value = serde_json::from_str(&e.ui_json("x", &serde_json::Value::Null)).unwrap();
        assert_eq!(json["active"], true);
        assert_eq!(json["tool"], "draw");
        assert_eq!(json["blocks"].as_array().unwrap().len(), PALETTE.len());
        assert_eq!(json["things"].as_array().unwrap().len(), EDITOR_THING_KINDS.len());
        assert_eq!((&json["value"], &json["values"], &json["layer"]), (&0.into(), &2.into(), &serde_json::Value::Null));
        assert_eq!((&json["undo"], &json["redo"]), (&false.into(), &false.into()));
        e.click(Button::Left, Some(TARGET), world);
        assert_eq!(serde_json::from_str::<serde_json::Value>(&e.ui_json("", &serde_json::Value::Null)).unwrap()["undo"], true);
    }

    #[test]
    fn a_block_value_is_placed_picked_and_limited_to_the_pictures_a_block_has() {
        let mut e = active();
        let stone_1 = Block::new(id::STONE, 1);
        e.step_value(1);
        assert_eq!(e.value(), 1);
        assert_eq!(e.click(Button::Left, Some(TARGET), world), Some(Edit { x: 4, y: 5, z: 7, block: stone_1.raw() }));
        e.step_value(1);
        assert_eq!(e.value(), 0, "wraps around");
        e.step_value(-1);
        assert_eq!(e.value(), 1, "and back");
        // Other blocks have one value only: nothing to step, and choosing a block starts at 0.
        e.select_block(2);
        assert_eq!(e.value(), 0);
        e.step_value(1);
        assert_eq!(e.value(), 0);
        // The eyedropper takes the value too, unless the block has no picture for it.
        e.click(Button::Middle, Some(TARGET), |_| stone_1);
        assert_eq!((e.selected_block(), e.value()), (id::STONE, 1));
        e.click(Button::Middle, Some(TARGET), |_| Block::new(id::STONE, 9));
        assert_eq!((e.selected_block(), e.value()), (id::STONE, 0));
        // Undo puts back what was there, value included.
        e.select_tool(Tool::Replace);
        let variant = |_: (i32, i32, i32)| stone_1;
        e.click(Button::Left, Some(TARGET), variant);
        assert_eq!(e.undo()[0].block, stone_1.raw());
        // Outside the editor the keys do nothing.
        let mut idle = Editor::default();
        idle.step_value(1);
        assert_eq!(idle.value(), 0);
    }

    fn thing(id: u32, pos: [f32; 3]) -> ThingState {
        ThingState { id, kind: "Wood".into(), pos }
    }

    #[test]
    fn the_spawn_tool_puts_the_chosen_thing_where_a_block_would_go() {
        let mut e = active();
        e.select_tool(Tool::Spawn);
        e.select_thing_kind(3);
        e.select_thing_kind(99); // ignored
        let (gx, gy) = to_iso(4, 5);
        assert_eq!(
            e.click_thing(Button::Left, Some(TARGET), None),
            Some(ThingAction::Spawn { kind: EDITOR_THING_KINDS[3], pos: [gx, gy, 7.0] })
        );
        assert_eq!(e.click_thing(Button::Left, None, None), None, "nothing under the cursor");
        assert_eq!(e.click_thing(Button::Right, Some(TARGET), None), None);
        assert_eq!(e.click(Button::Left, Some(TARGET), world), None, "no block is placed");
        assert!(!e.can_undo(), "things are not in the undo history");
        let mut idle = Editor::default();
        idle.select_tool(Tool::Spawn);
        assert_eq!(idle.click_thing(Button::Left, Some(TARGET), None), None, "not outside the editor");
    }

    #[test]
    fn the_select_tool_grabs_drags_and_deletes_a_thing() {
        let mut e = active();
        e.select_tool(Tool::Select);
        let things = [thing(7, [5.0, 5.0, 2.0])];
        assert_eq!(e.click(Button::Left, Some(TARGET), world), None, "no block changes");
        assert_eq!(e.drag_thing((0.0, 0.0), &View::default(), &things), None, "nothing grabbed");
        assert_eq!(e.click_thing(Button::Left, Some(TARGET), Some(7)), None);
        assert_eq!(e.selected_thing(), Some(7));

        // The pointer is over the ground point (5.5, 5.0) at the thing's mid height.
        let (sx, sy) = crate::pick::screen_of(5.5, 5.0, 2.5);
        let Some(ThingAction::Move { id, pos }) = e.drag_thing((sx, sy), &View::default(), &things) else { panic!("no move") };
        assert_eq!(id, 7);
        assert!((pos[0] - 5.5).abs() < 0.06 && (pos[1] - 5.0).abs() < 0.06 && pos[2] == 2.0, "{pos:?}");
        assert_eq!(e.drag_thing((sx, sy), &View::default(), &things), None, "not again for the same place");
        assert_eq!(e.drag_thing((sx + 0.5, sy), &View::default(), &things), None, "nor for less than a step");
        // Letting go ends the drag; the thing stays selected.
        e.release(None, world);
        assert_eq!(e.drag_thing((sx + 100.0, sy), &View::default(), &things), None);
        assert_eq!(e.selected_thing(), Some(7));

        assert_eq!(e.delete_selected(), Some(ThingAction::Delete { id: 7 }));
        assert_eq!(e.delete_selected(), None);
        // Clicking on nothing deselects; a selection that vanished is forgotten.
        e.click_thing(Button::Left, None, Some(7));
        e.click_thing(Button::Left, None, None);
        assert_eq!(e.selected_thing(), None);
        e.click_thing(Button::Left, None, Some(7));
        e.keep_selection_in(&things);
        assert_eq!(e.selected_thing(), Some(7));
        e.keep_selection_in(&[]);
        assert_eq!(e.selected_thing(), None);
        // Leaving the editor drops the selection.
        e.click_thing(Button::Left, None, Some(7));
        e.set_active(false);
        assert_eq!(e.selected_thing(), None);
        assert_eq!(e.delete_selected(), None);
    }

    #[test]
    fn the_wheel_sets_how_many_layers_are_drawn() {
        let top = CHUNK_SIZE_Z - 1;
        let mut idle = Editor::default();
        assert!(!idle.scroll(100.0));
        let mut e = active();
        assert_eq!(e.layer(), None);
        assert!(!e.scroll(-100.0), "up from all layers changes nothing");
        assert!(e.scroll(100.0), "a notch down");
        assert_eq!(e.layer(), Some(top - 1), "all but the top layer");
        assert!(e.scroll(300.0));
        assert_eq!(e.layer(), Some(top - 4));
        assert!(e.scroll(-100.0));
        assert_eq!(e.layer(), Some(top - 3));
        // Small steps (a trackpad) add up.
        assert!(!e.scroll(40.0) && !e.scroll(40.0));
        assert!(e.scroll(40.0));
        assert_eq!(e.layer(), Some(top - 4));
        // The bottom layer is the lowest, going past the top shows everything.
        e.scroll(5000.0);
        assert_eq!(e.layer(), Some(0));
        assert!(!e.scroll(100.0));
        e.scroll(-100_000.0);
        assert_eq!(e.layer(), None);
        // The toolbar's buttons do the same by whole layers.
        assert!(e.step_layer(-2));
        assert_eq!(e.layer(), Some(top - 2));
        assert!(!e.step_layer(0));
        // Leaving the editor draws everything again.
        e.set_active(false);
        assert_eq!(e.layer(), None);
    }

    #[test]
    fn the_keys_pan_the_camera_within_a_limit_and_only_in_the_editor() {
        let mut idle = Editor::default();
        idle.pan_by((1.0, 0.0), 0.0, false, 1.0);
        assert_eq!(idle.pan(), (0.0, 0.0));

        let mut e = active();
        // W moves up the screen: on the ground that is towards -x and -y.
        e.pan_by((0.0, -1.0), 0.0, false, 0.5);
        let (x, y) = e.pan();
        assert!(x < 0.0 && y < 0.0 && (x - y).abs() < 1e-5, "{x} {y}");
        let slow = Vec2::new(x, y).length();
        assert!((slow - PAN_SPEED * 0.5).abs() < 1e-4, "{slow}");
        // Shift is faster.
        let mut fast = active();
        fast.pan_by((0.0, -1.0), 0.0, true, 0.5);
        assert!((Vec2::new(fast.pan().0, fast.pan().1).length() - slow * PAN_FAST).abs() < 1e-4);
        // Diagonals are not faster, no keys is no movement, and the camera cannot leave the player.
        let mut diagonal = active();
        diagonal.pan_by((1.0, 1.0), 0.0, false, 0.5);
        assert!((Vec2::new(diagonal.pan().0, diagonal.pan().1).length() - slow).abs() < 1e-4);
        e.pan_by((0.0, 0.0), 0.0, false, 10.0);
        assert_eq!(e.pan(), (x, y));
        for _ in 0..100 {
            e.pan_by((1.0, -1.0), 0.0, true, 1.0);
        }
        assert!((Vec2::new(e.pan().0, e.pan().1).length() - PAN_LIMIT).abs() < 1e-3);
        // Leaving puts the camera back on the player.
        e.set_active(false);
        assert_eq!(e.pan(), (0.0, 0.0));
    }

    #[test]
    fn the_ghost_shows_what_draw_and_replace_would_build() {
        let mut e = active();
        let brush = Block::new(id::STONE, 0);
        assert_eq!(e.ghost(Some(TARGET)), Some((brush, TARGET.place)), "draw builds in front of the hit block");
        assert_eq!(e.ghost(None), None);
        e.select_tool(Tool::Replace);
        assert_eq!(e.ghost(Some(TARGET)), Some((brush, TARGET.hit)));
        for tool in [Tool::Bucket, Tool::Select, Tool::Spawn, Tool::Erase, Tool::Pick] {
            e.select_tool(tool);
            assert_eq!(e.ghost(Some(TARGET)), None, "{tool:?}");
        }
        assert_eq!(Editor::default().ghost(Some(TARGET)), None, "not outside the editor");
    }

    #[test]
    fn panning_follows_the_turned_camera() {
        // Up the screen is towards -x and -y; a quarter turn of the world makes that another way.
        let mut straight = active();
        straight.pan_by((0.0, -1.0), 0.0, false, 1.0);
        let mut turned = active();
        turned.pan_by((0.0, -1.0), std::f32::consts::FRAC_PI_2, false, 1.0);
        let (a, b) = (Vec2::from(straight.pan()), Vec2::from(turned.pan()));
        assert!((a.length() - b.length()).abs() < 1e-4 && a.dot(b).abs() < 1e-3, "{a} {b}");
    }

    #[test]
    fn every_tool_has_a_distinct_name_in_java_order() {
        let names: Vec<_> = Tool::ALL.iter().map(|t| t.name()).collect();
        assert_eq!(names, ["draw", "bucket", "replace", "select", "spawn", "erase", "pick"]);
    }
}

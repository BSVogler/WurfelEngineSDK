//! The map editor mode (the browser counterpart of the Java engine's `editor` package).
//!
//! Outside the editor the mouse never changes blocks. Inside it, the left mouse button uses the
//! tool picked in the toolbar (Java: `Toolbar.selectionLeft`, starts as DRAW), the right button
//! always erases (Java: `selectionRight = ERASE`) and the middle button picks the block under the
//! cursor. The number keys choose the block to place (Java: `BlockTable`).
//!
//! This file is pure logic with no browser types, so it is unit tested natively. `web.rs` feeds it
//! the pointer and sends the resulting edits to the server.

use wurfel_sim::block::id;
use wurfel_sim::Block;

use crate::pick::Pick;

/// Blocks the editor can place, in number-key order. The server accepts exactly these (and air).
pub const PALETTE: [(u8, &str); 4] =
    [(id::STONE, "stone"), (id::DIRT, "dirt"), (id::GRASS, "grass"), (id::SAND, "sand")];

/// The Java `Tool`s that make sense for blocks without entities: DRAW, REPLACE and ERASE, plus an
/// eyedropper (`Pick`) that Java does not have.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tool {
    /// Put the selected block in the empty cell in front of the hit block.
    Draw,
    /// Overwrite the hit block with the selected one.
    Replace,
    /// Remove the hit block.
    Erase,
    /// Select the kind of the hit block for drawing.
    Pick,
}

impl Tool {
    pub const ALL: [Tool; 4] = [Tool::Draw, Tool::Replace, Tool::Erase, Tool::Pick];

    pub fn name(self) -> &'static str {
        match self {
            Tool::Draw => "draw",
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Editor {
    active: bool,
    left: Tool,
    selected: usize,
}

impl Default for Editor {
    fn default() -> Self {
        Editor { active: false, left: Tool::Draw, selected: 0 }
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

    /// A mouse button went down over `target` (`None` when it is over nothing). `block_id_at` reads
    /// the id of a cell. Returns the change to ask the server for. Outside the editor this does
    /// nothing at all.
    pub fn click(&mut self, button: Button, target: Option<Pick>, block_id_at: impl Fn((i32, i32, i32)) -> u8) -> Option<Edit> {
        if !self.active {
            return None;
        }
        let target = target?;
        let at = |(x, y, z): (i32, i32, i32), block: u16| Edit { x, y, z, block };
        match self.tool_for(button) {
            Tool::Draw => Some(at(target.place, Block::new(self.selected_block(), 0).raw())),
            Tool::Replace => Some(at(target.hit, Block::new(self.selected_block(), 0).raw())),
            Tool::Erase => Some(at(target.hit, 0)),
            Tool::Pick => {
                let picked = block_id_at(target.hit);
                if let Some(index) = PALETTE.iter().position(|(id, _)| *id == picked) {
                    self.selected = index;
                }
                None
            }
        }
    }

    /// The state for the page's toolbar (`wurfelEditor.update`), with the cursor line.
    pub fn ui_json(&self, cursor: &str) -> String {
        serde_json::json!({
            "active": self.active,
            "tool": self.left.name(),
            "selected": self.selected,
            "tools": Tool::ALL.map(Tool::name),
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

    fn active() -> Editor {
        let mut e = Editor::default();
        e.toggle();
        e
    }

    #[test]
    fn the_mouse_edits_nothing_outside_the_editor() {
        let mut e = Editor::default();
        for button in [Button::Left, Button::Right, Button::Middle] {
            assert_eq!(e.click(button, Some(TARGET), |_| id::SAND), None);
        }
        assert!(!e.select_block(2), "number keys do not choose blocks outside the editor");
        assert_eq!(e.selected(), 0);
        assert!(e.toggle());
        assert!(!e.toggle());
    }

    #[test]
    fn left_draws_in_the_empty_cell_and_right_erases_the_hit_block() {
        let mut e = active();
        assert_eq!(e.click(Button::Left, Some(TARGET), |_| 0), Some(Edit { x: 4, y: 5, z: 7, block: STONE }));
        assert_eq!(e.click(Button::Right, Some(TARGET), |_| 0), Some(Edit { x: 4, y: 5, z: 6, block: 0 }));
        assert_eq!(e.click(Button::Left, None, |_| 0), None, "nothing under the cursor");
    }

    #[test]
    fn the_toolbar_changes_what_the_left_button_does() {
        let mut e = active();
        assert!(e.select_block(3));
        let sand = id::SAND as u16;
        e.select_tool(Tool::Replace);
        assert_eq!(e.click(Button::Left, Some(TARGET), |_| 0), Some(Edit { x: 4, y: 5, z: 6, block: sand }));
        e.select_tool(Tool::Erase);
        assert_eq!(e.click(Button::Left, Some(TARGET), |_| 0), Some(Edit { x: 4, y: 5, z: 6, block: 0 }));
        e.select_tool(Tool::Draw);
        assert_eq!(e.click(Button::Left, Some(TARGET), |_| 0), Some(Edit { x: 4, y: 5, z: 7, block: sand }));
        assert_eq!(e.click(Button::Right, Some(TARGET), |_| 0).unwrap().block, 0, "right stays erase");
    }

    #[test]
    fn middle_click_picks_the_block_kind_without_editing() {
        let mut e = active();
        assert_eq!(e.click(Button::Middle, Some(TARGET), |_| id::GRASS), None);
        assert_eq!(e.selected_block(), id::GRASS);
        // Something that cannot be placed (water, say) leaves the selection alone.
        e.click(Button::Middle, Some(TARGET), |_| id::WATER);
        assert_eq!(e.selected_block(), id::GRASS);
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
        assert_eq!(Tool::parse("bucket"), None);
        assert_eq!(Button::from_dom(1), Some(Button::Middle));
        assert_eq!(Button::from_dom(3), None);
    }

    #[test]
    fn cursor_info_and_toolbar_state() {
        assert_eq!(cursor_text(None, 0), "");
        assert_eq!(cursor_text(Some(TARGET), id::DIRT), "4, 5, 6 · dirt (id 2)");
        let json: serde_json::Value = serde_json::from_str(&active().ui_json("x")).unwrap();
        assert_eq!(json["active"], true);
        assert_eq!(json["tool"], "draw");
        assert_eq!(json["blocks"].as_array().unwrap().len(), PALETTE.len());
    }
}

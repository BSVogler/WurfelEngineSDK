//! Dialogs: what Java opened as an `ActionBox` on the player's screen (an NPC's line, a construction
//! site's menu, the shop). The rules run on the server, so a dialog is plain data the client shows;
//! the player answers with [`Action::Choose`](crate::Action::Choose) or
//! [`Action::Cancel`](crate::Action::Cancel).

use crate::game::Cell;
use wurfel_sim::entity::EntityId;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DialogMode {
    /// Text and "next" (`BoxModes.SIMPLE`): choose option 1 to go on.
    Simple,
    /// A question (`BoxModes.BOOLEAN`): option 1 is yes, 0 is no.
    Boolean,
    /// A list of options (`BoxModes.SELECTION`).
    Selection,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DialogOption {
    pub id: u8,
    pub label: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dialog {
    pub title: String,
    pub text: String,
    pub mode: DialogMode,
    pub options: Vec<DialogOption>,
}

impl Dialog {
    pub fn simple(title: &str, text: &str) -> Self {
        Dialog { title: title.to_string(), text: text.to_string(), mode: DialogMode::Simple, options: Vec::new() }
    }

    pub fn boolean(title: &str, text: &str) -> Self {
        Dialog { title: title.to_string(), text: text.to_string(), mode: DialogMode::Boolean, options: Vec::new() }
    }

    pub fn selection(title: &str, text: &str, options: Vec<DialogOption>) -> Self {
        Dialog { title: title.to_string(), text: text.to_string(), mode: DialogMode::Selection, options }
    }

    /// Is `id` something the player may answer with?
    pub fn accepts(&self, id: u8) -> bool {
        match self.mode {
            DialogMode::Simple | DialogMode::Boolean => id <= 1,
            DialogMode::Selection => self.options.iter().any(|o| o.id == id),
        }
    }
}

pub fn option(id: u8, label: impl Into<String>) -> DialogOption {
    DialogOption { id, label: label.into() }
}

/// Who a dialog belongs to, so the answer reaches the right place.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Source {
    Vanya(EntityId),
    Shop(EntityId),
    Site(Cell),
    Factory(Cell),
    /// A turret's menu: who it shoots at.
    Turret(Cell),
    /// A catapult's or cannon's menu: aim, load and fire.
    Launcher(Cell),
    /// The toolkit in hand: choose what to build where the player stands.
    Toolkit,
    /// A rails or cable kit in hand: choose the piece to lay where the player stands.
    LineKit,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct OpenDialog {
    pub dialog: Dialog,
    pub source: Source,
}

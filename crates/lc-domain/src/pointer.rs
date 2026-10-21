//! Pointer input in domain terms: never a coordinate, since the domain owns no
//! layout. `lc-inbound` resolves the panel and row.

use crate::export::Button;
use crate::model::Key;
use crate::panel::Panel;

/// What a button answers as. The export form's two buttons share `Enter`, so
/// they are named rather than keyed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Press {
    Key(Key),
    Export(Button),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pointer {
    Click(Click),
    /// Lights the button; only a release over it answers.
    Hold(Press),
    Drag(Option<Press>),
    /// Over the held button, answered as that button's press.
    Release(Option<Press>),
    /// A slip inside the question does not answer it.
    Modal,
    /// Takes effect on press, as a text field does.
    Field(crate::export::Field),
    /// An index into the previous frame's list; selects and answers `⏎`.
    Entry(usize),
    /// Scrolling is a glance, clicking a commitment.
    Wheel {
        panel: Panel,
        direction: Wheel,
    },
}

/// Dragging off unlights it and cancels the press, as in a GUI.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Held {
    pub press: Press,
    pub over: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Click {
    /// `column` is `None` over a gutter and for list bodies.
    Row {
        panel: Panel,
        row: usize,
        column: Option<usize>,
    },
    Column {
        panel: Panel,
        column: usize,
    },
    /// The `s` or `f` binding, by pointer.
    Mark {
        panel: Panel,
        column: usize,
        mark: Mark,
    },
    /// An index into the previous frame's strip; a vanished tab drops the
    /// press.
    Tab {
        panel: Panel,
        tab: usize,
    },
    /// A border, space past the last row, or an empty body.
    Panel(Panel),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mark {
    Sort,
    Filter,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Wheel {
    Up,
    Down,
}

impl Click {
    #[must_use]
    pub const fn panel(self) -> Panel {
        match self {
            Self::Row { panel, .. }
            | Self::Column { panel, .. }
            | Self::Mark { panel, .. }
            | Self::Tab { panel, .. }
            | Self::Panel(panel) => panel,
        }
    }
}

//! Pointer input, in domain terms.
//!
//! Deliberately not a coordinate: the domain owns no layout and cannot say
//! which panel a column and row fall in (architecture.md §2). `lc-inbound`
//! resolves the press against the frame it happened in and reports the panel
//! and the row, the same way it reports a [`crate::model::KeyPress`] rather
//! than a crossterm event.

use crate::model::Key;
use crate::panel::Panel;

/// A pointer gesture, already resolved against the frame it happened in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pointer {
    Click(Click),
    /// The button went down on a modal's button. Lights it and answers
    /// nothing: the press is only a press once it is released on the same
    /// button.
    Hold(Key),
    /// The pointer moved with the button held, and is over `Some` modal
    /// button or none.
    Drag(Option<Key>),
    /// The button came up over `Some` modal button or none. Over the one that
    /// was held, it is that button's key, answered as if typed — the rule
    /// every gesture follows
    /// (docs/superpowers/specs/2026-08-09-mouse-input-design.md §1).
    Release(Option<Key>),
    /// A press on a modal's own block, off its buttons. Spent on nothing, so
    /// a slip of the pointer inside the question does not answer it.
    Modal,
    /// A press on a form's field or radio, which takes effect on the press
    /// as a text field's does.
    Field(crate::export::Field),
    /// A press on a row of a modal's list, by its index in the previous
    /// frame's list. Selects it and answers `⏎`, as a click on a tree row
    /// does (design §1).
    Entry(usize),
    /// Scrolls `panel` without focusing it: scrolling is a glance where
    /// clicking is a commitment
    /// (docs/superpowers/specs/2026-08-09-mouse-input-design.md §6).
    Wheel {
        panel: Panel,
        direction: Wheel,
    },
}

/// A modal button the pointer went down on, and whether it is still over it.
///
/// Leaving the button unlights it and returning relights it, so a press
/// dragged away before release is cancelled the way a GUI's is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Held {
    pub key: Key,
    pub over: bool,
}

/// Where a press landed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Click {
    /// A row of a panel that has rows, and the column of it where the body has
    /// columns — `None` over a grid's gutter, and for every body that is a
    /// list rather than a grid.
    Row {
        panel: Panel,
        row: usize,
        column: Option<usize>,
    },
    /// A grid's heading cell: the column it names, and no row.
    Column { panel: Panel, column: usize },
    /// One of the two marks a grid heading carries, and the column it belongs
    /// to. A press here is the `s` or `f` binding, reached by pointer — the
    /// rule every gesture follows (design §1,
    /// docs/superpowers/specs/2026-08-31-grid-sort-filter-design.md §5).
    Mark {
        panel: Panel,
        column: usize,
        mark: Mark,
    },
    /// A tab on a panel's top border, by its place in the strip that was
    /// drawn.
    ///
    /// A tab has no key of its own to name it by, so this is an index into
    /// the *previous* frame's strip: one that no longer exists is dropped
    /// rather than clamped onto whichever tab took its position — the rule a
    /// click on a vanished row already follows (design §6).
    Tab { panel: Panel, tab: usize },
    /// Chrome: a border, blank space past the last row, or a body with no rows
    /// at all.
    Panel(Panel),
}

/// Which of a heading's two marks was pressed.
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
    /// The panel the press landed in, row or not.
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

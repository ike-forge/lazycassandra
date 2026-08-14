//! Pointer input, in domain terms.
//!
//! Deliberately not a coordinate: the domain owns no layout and cannot say
//! which panel a column and row fall in (architecture.md §2). `lc-inbound`
//! resolves the press against the frame it happened in and reports the panel
//! and the row, the same way it reports a [`crate::model::KeyPress`] rather
//! than a crossterm event.

use crate::state::Panel;

/// A pointer gesture, already resolved against the frame it happened in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pointer {
    Click(Click),
    /// Scrolls `panel` without focusing it: scrolling is a glance where
    /// clicking is a commitment
    /// (docs/superpowers/specs/2026-08-09-mouse-input-design.md §6).
    Wheel {
        panel: Panel,
        direction: Wheel,
    },
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
    /// A tab on a panel's top border, named by the key that reaches it.
    ///
    /// The key rather than the tab's position in the strip: the map a press is
    /// resolved against describes the previous frame, and an index into a strip
    /// that has since changed names whichever tab took that position. A
    /// shortcut names the same place in every frame, and it is what makes this
    /// gesture the `d`/`c` binding rather than a second route to it (design
    /// §1).
    Tab { panel: Panel, shortcut: char },
    /// Chrome: a border, blank space past the last row, or a body with no rows
    /// at all.
    Panel(Panel),
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
            | Self::Tab { panel, .. }
            | Self::Panel(panel) => panel,
        }
    }
}

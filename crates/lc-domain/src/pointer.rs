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
    /// A row of a panel that has rows.
    Row { panel: Panel, row: usize },
    /// Chrome: a border, a title, a grid's header, blank space past the last
    /// row, or a body with no rows at all.
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
            Self::Row { panel, .. } | Self::Panel(panel) => panel,
        }
    }
}

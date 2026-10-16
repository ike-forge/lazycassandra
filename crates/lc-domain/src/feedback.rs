//! Transient, tick-counted feedback: the loader, the yank flash, toasts.

use crate::panel::Panel;
use crate::selection::Ranges;
use crate::style::Severity;

/// Resets when idle, so an idle frame is always the same frame.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Loader {
    frame: usize,
}

impl Loader {
    /// `false` when nothing moved: an idle tick must not cost a repaint.
    pub const fn advance(&mut self, working: bool) -> bool {
        let next = if working {
            self.frame.wrapping_add(1)
        } else {
            0
        };
        let moved = next != self.frame;
        self.frame = next;
        moved
    }

    /// Unbounded: the widget owns its cycle length.
    #[must_use]
    pub const fn frame(self) -> usize {
        self.frame
    }
}

/// A yank spends the selection, so this carries the rectangle it took. Empty on
/// an axis means all of it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Flash {
    panel: Panel,
    rows: Ranges,
    columns: Ranges,
    remaining: usize,
}

/// Twelve 40 ms ticks: just under half a second.
pub const FLASH_TICKS: usize = 12;

impl Flash {
    #[must_use]
    pub fn lit(panel: Panel, rows: Ranges, columns: Ranges) -> Self {
        Self {
            panel,
            rows,
            columns,
            remaining: FLASH_TICKS,
        }
    }

    /// `true` only on the tick that ends it, so a flash costs two repaints.
    pub const fn spend(&mut self) -> bool {
        self.remaining = self.remaining.saturating_sub(1);
        self.remaining == 0
    }

    #[must_use]
    pub const fn panel(&self) -> Panel {
        self.panel
    }

    #[must_use]
    pub const fn rows(&self) -> &Ranges {
        &self.rows
    }

    #[must_use]
    pub const fn columns(&self) -> &Ranges {
        &self.columns
    }
}

/// For a finished action, or a bound chord with nothing to do, where silence
/// would read as a dead key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Toast {
    severity: Severity,
    message: String,
    remaining: usize,
}

/// Three seconds at 40 ms ticks.
pub const TOAST_TICKS: usize = 75;

impl Toast {
    #[must_use]
    pub fn new(severity: Severity, message: impl Into<String>) -> Self {
        Self {
            severity,
            message: message.into(),
            remaining: TOAST_TICKS,
        }
    }

    /// `true` only on the tick that ends it.
    pub const fn spend(&mut self) -> bool {
        self.remaining = self.remaining.saturating_sub(1);
        self.remaining == 0
    }

    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }

    #[must_use]
    pub const fn severity(&self) -> Severity {
        self.severity
    }
}

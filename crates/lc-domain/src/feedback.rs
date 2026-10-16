//! Transient, tick-counted feedback: the loader, the yank flash, toasts.

use crate::panel::Panel;
use crate::selection::Ranges;
use crate::style::Severity;

/// The loader's phase.
///
/// Advances only while something is outstanding and resets when nothing is, so
/// an idle frame is always the same frame rather than wherever the animation
/// happened to stop.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Loader {
    frame: usize,
}

impl Loader {
    /// Advance one frame, or fall back to rest. Reports whether the phase
    /// moved — an idle tick that changes nothing must not cost a repaint
    /// (docs/superpowers/specs/2026-08-09-visual-polish-design.md §4.2).
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

    /// Unbounded: the renderer folds it into the widget's own cycle, whose
    /// length is the widget's business rather than the domain's.
    #[must_use]
    pub const fn frame(self) -> usize {
        self.frame
    }
}

/// The afterglow a `y` leaves on what it copied
/// (docs/superpowers/specs/2026-08-14-visual-mode-design.md §6.1).
///
/// A yank spends the selection, so without this the only sign it happened
/// would be a band disappearing. The flash holds what was taken, brightly,
/// for long enough to read and no longer.
///
/// It carries the rectangle rather than reading the selection back, because by
/// the time it is drawn there is no selection left to read. `rows` and
/// `columns` follow the view model's convention: empty on an axis is the
/// whole of it, which is what row mode and column mode take.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Flash {
    panel: Panel,
    rows: Ranges,
    columns: Ranges,
    remaining: usize,
}

/// How long the afterglow lasts, counted in [`crate::Msg::Tick`]s.
///
/// Elapsed time reaches the domain only as ticks, so a duration is a count
/// here: twelve of `lc-app`'s forty-millisecond ticks, a little under half a
/// second. Long enough to catch the eye, short enough that it is over before
/// the next key.
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

    /// Burns one tick. Reports whether the frame changed — which only the tick
    /// that puts the flash out does, so a flash costs two repaints rather than
    /// one per tick ([`Loader::advance`]'s rule).
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

/// A line of feedback for an action that finished, or a chord that is bound
/// but has nothing to do, where silence would read as a dead key. Not a modal: it asks nothing and leaves
/// on its own, counted in ticks as [`Flash`] is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Toast {
    severity: Severity,
    message: String,
    remaining: usize,
}

/// Three seconds of [`crate::Msg::Tick`]s at `lc-app`'s forty milliseconds.
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

    /// As [`Flash::spend`]: true only on the tick that ends it.
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

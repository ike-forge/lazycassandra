//! Mouse capture, and its release.
//!
//! `ratatui::restore` disables raw mode and leaves the alternate screen; it
//! knows nothing about capture, and the panic hook `ratatui::init` installs
//! only calls `restore`. A leaked capture leaves the terminal reporting every
//! pointer movement as an escape sequence until the user runs `reset`
//! (doc/specs/2026-08-09-mouse-input-design.md §8).

use std::io::stdout;

use anyhow::Result;
use crossterm::event::{DisableMouseCapture, EnableMouseCapture};
use crossterm::execute;

/// Enables mouse capture and guarantees its release — on return, on `?`, and
/// on panic.
///
/// The private field is what stops one being constructed without going through
/// [`MouseCapture::enable`], so capture cannot be held without holding the
/// thing that releases it (conventions.md §1).
pub(crate) struct MouseCapture(());

impl MouseCapture {
    /// Construct *after* `ratatui::init`, so the hook this chains onto is the
    /// one that restores the terminal.
    ///
    /// # Errors
    ///
    /// If the escape sequence cannot be written, in which case capture is not
    /// on and there is nothing to release.
    pub(crate) fn enable() -> Result<Self> {
        // Installed before capture goes on, so a failed `execute!` below never
        // leaves a hook writing "capture off" to a terminal that was never
        // capturing — harmless, since the sequence is idempotent, but there is
        // no reason to open the window at all.
        let restore = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            disable();
            restore(info);
        }));

        execute!(stdout(), EnableMouseCapture)?;

        Ok(Self(()))
    }
}

impl Drop for MouseCapture {
    fn drop(&mut self) {
        disable();
    }
}

/// Ignores its error: this runs while unwinding and on the way out, and there
/// is nobody left to report to. Running twice is harmless — the sequence says
/// "off", not "toggle".
fn disable() {
    let _released = execute!(stdout(), DisableMouseCapture);
}

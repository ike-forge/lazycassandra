//! Mouse capture, and its release.
//!
//! `ratatui::restore` disables raw mode and leaves the alternate screen; it
//! knows nothing about capture, and the panic hook `ratatui::init` installs
//! only calls `restore`. A leaked capture leaves the terminal reporting every
//! pointer movement as an escape sequence until the user runs `reset`
//! (docs/superpowers/specs/2026-08-09-mouse-input-design.md §8).

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
    /// If the escape sequence cannot be written, in which case capture never
    /// came on. The hook is already installed by then and stays installed; it
    /// writes "capture off" to a terminal that was never capturing, which is a
    /// no-op.
    pub(crate) fn enable() -> Result<Self> {
        // Installed before capture goes on, so there is no window in which
        // capture is on with nothing yet able to release it.
        let restore = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            disable();
            restore(info);
        }));

        execute!(stdout(), EnableMouseCapture)?;

        Ok(Self(()))
    }

    /// Turns capture off for as long as `body` runs, and back on after.
    ///
    /// The guard is not given up and re-taken: `enable` installs a panic hook,
    /// and re-taking it per edit would chain a new hook onto the last one every
    /// time (external editor design §4).
    ///
    /// # Errors
    ///
    /// If capture cannot be turned back on, which leaves the terminal usable
    /// but with the pointer inert.
    #[allow(
        clippy::unused_self,
        reason = "the receiver is the proof a MouseCapture exists to suspend; \
                  dropping it would let this be called with none enabled"
    )]
    pub(crate) fn suspended<T>(&self, body: impl FnOnce() -> T) -> Result<T> {
        disable();

        let value = body();

        execute!(stdout(), EnableMouseCapture)?;

        Ok(value)
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

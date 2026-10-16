//! Mouse capture and its release. `ratatui::restore` and its panic hook know
//! nothing of capture, and a leaked capture prints escape sequences until
//! `reset`.

use std::io::stdout;

use anyhow::Result;
use crossterm::event::{DisableMouseCapture, EnableMouseCapture};
use crossterm::execute;

/// Released on return, `?` and panic. The private field means capture cannot
/// exist without its release.
pub(crate) struct MouseCapture(());

impl MouseCapture {
    /// Construct after `ratatui::init`, so the chained hook restores the
    /// terminal.
    ///
    /// # Errors
    ///
    /// If the sequence cannot be written; capture never came on, and the hook's
    /// "off" is a no-op.
    pub(crate) fn enable() -> Result<Self> {
        // Installed first, so capture is never on without a release.
        let restore = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            disable();
            restore(info);
        }));

        execute!(stdout(), EnableMouseCapture)?;

        Ok(Self(()))
    }

    /// The guard is kept: re-enabling would chain a new panic hook per edit.
    ///
    /// # Errors
    ///
    /// If capture cannot be turned back on; the pointer is then inert.
    #[expect(
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

/// Errors are ignored: this runs while unwinding. Idempotent.
fn disable() {
    let _released = execute!(stdout(), DisableMouseCapture);
}

//! Terminal input, forwarded as messages.
//!
//! crossterm reads on a dedicated blocking thread so the main loop never
//! blocks on a keystroke (architecture.md §3).

use crossterm::event::{self, Event, MouseEvent};
use lc_domain::Msg;
use tokio::sync::mpsc::UnboundedSender;

/// What the main loop receives.
///
/// A pointer event only means something against the frame it happened in, and
/// this thread has no frame — so it forwards the event and the main loop, which
/// holds the map, resolves it
/// (doc/specs/2026-08-09-mouse-input-design.md §4).
#[derive(Debug)]
pub(crate) enum Incoming {
    Msg(Msg),
    Mouse(MouseEvent),
}

impl From<Msg> for Incoming {
    fn from(msg: Msg) -> Self {
        Self::Msg(msg)
    }
}

pub(crate) fn spawn_reader(tx: UnboundedSender<Incoming>) {
    std::thread::spawn(move || {
        loop {
            // A read error means the terminal is gone; there is nobody left to
            // report it to, so stop reading and let the loop end when the
            // channel closes.
            let Ok(event) = event::read() else { return };

            let incoming = match event {
                Event::Key(key) => lc_inbound::input::to_key_press(key)
                    .map(Msg::Input)
                    .map(Incoming::Msg),
                Event::Mouse(mouse) => Some(Incoming::Mouse(mouse)),
                Event::Resize(..) => Some(Incoming::Msg(Msg::Resized)),
                _ => None,
            };

            if let Some(incoming) = incoming
                && tx.send(incoming).is_err()
            {
                return;
            }
        }
    });
}

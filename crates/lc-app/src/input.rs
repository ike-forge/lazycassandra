//! Terminal input, forwarded as messages.
//!
//! crossterm reads on a dedicated blocking thread so the main loop never
//! blocks on a keystroke (architecture.md §3).

use crossterm::event::{self, Event};
use lc_domain::Msg;
use tokio::sync::mpsc::UnboundedSender;

pub(crate) fn spawn_reader(tx: UnboundedSender<Msg>) {
    std::thread::spawn(move || {
        loop {
            // A read error means the terminal is gone; there is nobody left to
            // report it to, so stop reading and let the loop end when the
            // channel closes.
            let Ok(event) = event::read() else { return };

            let msg = match event {
                Event::Key(key) => lc_inbound::input::to_key_press(key).map(Msg::Input),
                _ => None,
            };

            if let Some(msg) = msg
                && tx.send(msg).is_err()
            {
                return;
            }
        }
    });
}

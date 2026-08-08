//! Composition root and the main loop.
//!
//! Deliberately thin: wiring, not logic (architecture.md §2). It is the only
//! crate that sees all three layers.

mod effects;
mod input;

use anyhow::Result;
use lc_domain::model::{Key, KeyPress};
use lc_domain::{Msg, State, update, view_model};
use lc_inbound::Theme;
use ratatui::DefaultTerminal;
use tokio::sync::mpsc;

#[tokio::main]
async fn main() -> Result<()> {
    // Installs a panic hook that restores the terminal before unwinding, so a
    // panic does not leave the user in raw mode. It does not cover panics on
    // tokio workers — architecture.md §11 has that still open.
    let terminal = ratatui::init();
    let result = run(terminal).await;
    ratatui::restore();
    result
}

async fn run(mut terminal: DefaultTerminal) -> Result<()> {
    let (tx, mut rx) = mpsc::unbounded_channel();
    input::spawn_reader(tx.clone());

    let mut state = State::default();
    let theme = Theme::default();

    terminal.draw(|frame| lc_inbound::render(frame, &view_model(&state), theme))?;

    while let Some(msg) = rx.recv().await {
        // Bindings live in the domain from Phase 0 onward; until the focus
        // model exists there is one, and it is here so the scaffold is
        // quittable.
        let msg = match msg {
            Msg::Input(KeyPress {
                key: Key::Char('q'),
                ..
            }) => Msg::Quit,
            other => other,
        };

        for effect in update(&mut state, msg) {
            effects::run(&tx, &effect);
        }

        if state.should_quit {
            break;
        }

        if state.dirty {
            terminal.draw(|frame| lc_inbound::render(frame, &view_model(&state), theme))?;
            state.dirty = false;
        }
    }

    Ok(())
}

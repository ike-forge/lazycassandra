//! Composition root and the main loop.
//!
//! Deliberately thin: wiring, not logic (architecture.md §2). It is the only
//! crate that sees all three layers.

mod effects;
mod input;

use std::sync::Arc;

use anyhow::Result;
use lc_domain::model::ConnId;
use lc_domain::ports::Cluster;
use lc_domain::posture::Posture;
use lc_domain::{update, view_model};
use lc_inbound::Theme;
use lc_outbound::cassandra::CassandraCluster;
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

/// The fixture's convention (docker/compose.yaml). Scaffolding until the
/// config layering of vision.md §6 lands — not a configuration mechanism.
fn contact_point() -> String {
    let host = std::env::var("LC_CASSANDRA_HOST").unwrap_or_else(|_| "127.0.0.1".to_owned());
    let port = std::env::var("LC_CASSANDRA_PORT").unwrap_or_else(|_| "9042".to_owned());

    format!("{host}:{port}")
}

async fn run(mut terminal: DefaultTerminal) -> Result<()> {
    let (tx, mut rx) = mpsc::unbounded_channel();
    input::spawn_reader(tx.clone());

    let conn = ConnId::next();
    let cluster: Arc<dyn Cluster> = Arc::new(CassandraCluster::new(
        conn,
        contact_point(),
        Posture::ReadOnly,
    ));

    let (mut state, startup) = lc_domain::init(conn);
    let theme = Theme::default();

    for effect in &startup {
        effects::run(&tx, &cluster, effect);
    }

    terminal.draw(|frame| lc_inbound::render(frame, &view_model(&state), theme))?;
    state.dirty = false;

    while let Some(msg) = rx.recv().await {
        for effect in update(&mut state, msg) {
            effects::run(&tx, &cluster, &effect);
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

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
use lc_domain::state::Geometry;
use lc_domain::{Msg, State, update, view_model};
use lc_inbound::Theme;
use lc_outbound::cassandra::CassandraCluster;
use ratatui::DefaultTerminal;
use tokio::sync::mpsc;
use tokio::time::{Duration, interval};

/// The interval `ratatui_opentui_loader` is built around. Real time enters the
/// program here and nowhere else — the domain sees only `Msg::Tick`, and
/// discards one that changes nothing (design §4.2).
const TICK: Duration = Duration::from_millis(40);

/// Feeds the loader's phase. Sends unconditionally; `update` is what decides
/// whether a tick is worth a repaint, because it is what knows whether
/// anything is outstanding.
fn spawn_ticker(tx: mpsc::UnboundedSender<Msg>) {
    tokio::spawn(async move {
        let mut ticks = interval(TICK);

        loop {
            ticks.tick().await;
            if tx.send(Msg::Tick).is_err() {
                // The main loop has ended.
                return;
            }
        }
    });
}

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

/// Draws a frame and reports what it turned out to be.
fn draw(terminal: &mut DefaultTerminal, state: &State, theme: Theme) -> Result<Geometry> {
    let mut geometry = Geometry::default();

    terminal.draw(|frame| geometry = lc_inbound::render(frame, &view_model(state), theme))?;

    Ok(geometry)
}

/// Tells the domain what the frame turned out to be, but only when it
/// changed.
///
/// Sending unconditionally would redraw, re-measure and redraw forever
/// (doc/specs/2026-08-09-panel-scrolling-design.md §3).
fn report(tx: &mpsc::UnboundedSender<Msg>, last: &mut Option<Geometry>, geometry: Geometry) {
    if *last == Some(geometry) {
        return;
    }

    *last = Some(geometry);
    // A closed channel means the main loop has already ended.
    let _sent = tx.send(Msg::Measured(geometry));
}

async fn run(mut terminal: DefaultTerminal) -> Result<()> {
    let (tx, mut rx) = mpsc::unbounded_channel();
    input::spawn_reader(tx.clone());
    spawn_ticker(tx.clone());

    let conn = ConnId::next();
    let cluster: Arc<dyn Cluster> = Arc::new(CassandraCluster::new(
        conn,
        contact_point(),
        Posture::ReadOnly,
    ));

    let (mut state, startup) = lc_domain::init(conn);
    let theme = Theme::default();
    let mut measured: Option<Geometry> = None;

    for effect in &startup {
        effects::run(&tx, &cluster, effect);
    }

    report(&tx, &mut measured, draw(&mut terminal, &state, theme)?);
    state.dirty = false;

    while let Some(msg) = rx.recv().await {
        for effect in update(&mut state, msg) {
            effects::run(&tx, &cluster, &effect);
        }

        if state.should_quit {
            break;
        }

        if state.dirty {
            report(&tx, &mut measured, draw(&mut terminal, &state, theme)?);
            state.dirty = false;
        }
    }

    Ok(())
}

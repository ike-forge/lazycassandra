//! Composition root and the main loop.
//!
//! Deliberately thin: wiring, not logic (architecture.md §2). It is the only
//! crate that sees all three layers.

mod clipboard;
mod editor;
mod effects;
mod export;
mod input;
mod mouse;

use std::sync::Arc;

use anyhow::Result;
use input::Incoming;
use lc_domain::model::ConnId;
use lc_domain::ports::{Cluster, ConfigSource, Writer};
use lc_domain::posture::Posture;
use lc_domain::state::Geometry;
use lc_domain::{Effect, Msg, State, update, view_model};
use lc_inbound::{FrameMap, Rendered, Themes};
use lc_outbound::cassandra::CassandraCluster;
use ratatui::DefaultTerminal;
use tokio::sync::mpsc;
use tokio::time::{Duration, MissedTickBehavior, interval};

/// The interval `ratatui_opentui_loader` is built around. Real time enters the
/// program here and nowhere else — the domain sees only `Msg::Tick`, and
/// discards one that changes nothing (design §4.2).
const TICK: Duration = Duration::from_millis(40);

/// Feeds the loader's phase. Sends unconditionally; `update` is what decides
/// whether a tick is worth a repaint, because it is what knows whether
/// anything is outstanding.
fn spawn_ticker(tx: mpsc::UnboundedSender<Incoming>) {
    tokio::spawn(async move {
        let mut ticks = interval(TICK);
        // A blocking editing session lets ticks queue up behind it; the
        // default `Burst` behaviour would replay every missed one on return,
        // driving the loader and marking the frame dirty thousands of times
        // before the edited buffer is even applied. A tick is a phase pulse,
        // not an event with a payload — one catch-up tick means as much as
        // any other (external editor design §4).
        ticks.set_missed_tick_behavior(MissedTickBehavior::Skip);

        loop {
            ticks.tick().await;
            if tx
                .send(Msg::Tick(lc_outbound::clock::now()).into())
                .is_err()
            {
                // The main loop has ended.
                return;
            }
        }
    });
}

#[tokio::main]
async fn main() -> Result<()> {
    // Installs a panic hook that restores the terminal before unwinding, so a
    // panic does not leave the user in raw mode. A panic on a tokio worker
    // fires the hook without ending the process, so the terminal is restored
    // under a program that is still drawing — architecture.md §11 has that
    // still open.
    let terminal = ratatui::init();
    let result = captured(terminal).await;
    ratatui::restore();
    result
}

/// Holds mouse capture for exactly as long as the loop runs, so it is released
/// while the terminal is still ours.
async fn captured(terminal: DefaultTerminal) -> Result<()> {
    let capture = mouse::MouseCapture::enable()?;

    run(terminal, &capture).await
}

/// The fixture's convention (docker/compose.yaml). Scaffolding until the
/// config layering of vision.md §6 lands — not a configuration mechanism.
fn contact_point() -> String {
    let host = std::env::var("LC_CASSANDRA_HOST").unwrap_or_else(|_| "127.0.0.1".to_owned());
    let port = std::env::var("LC_CASSANDRA_PORT").unwrap_or_else(|_| "9042".to_owned());

    format!("{host}:{port}")
}

/// Draws a frame and reports what it turned out to be.
fn draw(terminal: &mut DefaultTerminal, state: &State, themes: &Themes) -> Result<Rendered> {
    let mut rendered = Rendered::default();
    let view = view_model(state);
    let theme = themes.get(&view.theme);

    terminal.draw(|frame| rendered = lc_inbound::render(frame, &view, theme))?;

    Ok(rendered)
}

/// Tells the domain what the frame turned out to be, but only when it
/// changed.
///
/// Sending unconditionally would redraw, re-measure and redraw forever
/// (docs/superpowers/specs/2026-08-09-panel-scrolling-design.md §3).
fn report(tx: &mpsc::UnboundedSender<Incoming>, last: &mut Option<Geometry>, geometry: Geometry) {
    if *last == Some(geometry) {
        return;
    }

    *last = Some(geometry);
    // A closed channel means the main loop has already ended.
    let _sent = tx.send(Msg::Measured(geometry).into());
}

/// A pointer event only means something against the frame it happened in, so
/// it is resolved here rather than on the reader thread, which has none.
///
/// `None` is a gesture that landed on nothing, or one with no binding behind
/// it: no message, and so no redraw.
fn resolve(incoming: Incoming, map: &FrameMap) -> Option<Msg> {
    match incoming {
        Incoming::Msg(msg) => Some(msg),
        Incoming::Mouse(event) => lc_inbound::input::to_pointer(event, map).map(Msg::Pointer),
    }
}

/// The next message already waiting, where the frame this batch will draw may
/// be the one that shows it — architecture.md §3's *redraw on state change,
/// not per event*.
///
/// A held key repeats faster than a frame is painted, so a loop that draws
/// once per press falls behind and keeps moving after the key is released: the
/// events are queued, and nothing about being late makes them cheaper. Applied
/// as a batch they cost one frame between them, and the cursor is where the
/// user let go.
///
/// A gesture is the one thing that cannot join a batch. It is resolved against
/// [`FrameMap`], which describes the frame it happened in and not the one the
/// messages ahead of it have earned, so it is carried to the loop's next turn
/// — after the draw that makes the map true again
/// (docs/superpowers/specs/2026-08-09-mouse-input-design.md §4).
fn queued(
    rx: &mut mpsc::UnboundedReceiver<Incoming>,
    carried: &mut Option<Incoming>,
) -> Option<Incoming> {
    match rx.try_recv() {
        Ok(incoming @ Incoming::Msg(_)) => Some(incoming),
        Ok(gesture) => {
            *carried = Some(gesture);
            None
        }
        Err(_) => None,
    }
}

async fn run(mut terminal: DefaultTerminal, capture: &mouse::MouseCapture) -> Result<()> {
    let (tx, mut rx) = mpsc::unbounded_channel();
    let suspension = input::Suspension::default();
    input::spawn_reader(tx.clone(), suspension.clone());
    spawn_ticker(tx.clone());

    let conn = ConnId::next();
    let cluster: Arc<dyn Cluster> = Arc::new(CassandraCluster::new(
        conn,
        contact_point(),
        Posture::ReadOnly,
    ));
    let writer: Arc<dyn Writer> = Arc::new(lc_outbound::file::FileWriter);

    lc_outbound::config::write_example_config_from_env();
    let source = lc_outbound::config::FileConfigSource::from_env();
    let (config, diagnostics) = source.load().await;
    let themes = Themes::new(&config.themes, config.background);

    let (mut state, startup) =
        lc_domain::init(conn, config, diagnostics, lc_outbound::clock::now());
    state.cwd = std::env::current_dir().unwrap_or_default();
    let mut measured: Option<Geometry> = None;

    for effect in &startup {
        effects::run(&tx, &cluster, &writer, effect);
    }

    let rendered = draw(&mut terminal, &state, &themes)?;
    let mut map = rendered.map;
    report(&tx, &mut measured, rendered.geometry);
    state.dirty = false;

    // What the last batch took off the channel and could not apply against the
    // frame it was drawing. `None` on every turn but the one after a gesture
    // arrived mid-batch.
    let mut carried: Option<Incoming> = None;

    loop {
        let incoming = if let Some(incoming) = carried.take() {
            incoming
        } else if let Some(incoming) = rx.recv().await {
            incoming
        } else {
            break;
        };

        let mut batch = Some(incoming);
        while let Some(incoming) = batch {
            if let Some(msg) = resolve(incoming, &map) {
                for effect in update(&mut state, msg) {
                    // The one effect that is not a task: the child owns the
                    // terminal, so nothing else may draw while it holds it.
                    // Its outcome comes back through the same channel every
                    // other effect returns through (external editor design
                    // §4).
                    if let Effect::OpenEditor { buffer } = &effect {
                        let outcome = editor::open(&mut terminal, capture, &suspension, buffer)?;
                        let _sent = tx.send(outcome.into());
                        continue;
                    }

                    effects::run(&tx, &cluster, &writer, &effect);
                }
            }

            if state.should_quit {
                break;
            }

            batch = queued(&mut rx, &mut carried);
        }

        if state.should_quit {
            break;
        }

        if state.dirty {
            let rendered = draw(&mut terminal, &state, &themes)?;
            map = rendered.map;
            report(&tx, &mut measured, rendered.geometry);
            state.dirty = false;
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use crossterm::event::{KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
    use lc_domain::clock::Stamp;

    use super::*;

    fn gesture() -> Incoming {
        Incoming::Mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 0,
            row: 0,
            modifiers: KeyModifiers::NONE,
        })
    }

    #[test]
    fn a_batch_takes_every_message_the_channel_is_already_holding() {
        let (tx, mut rx) = mpsc::unbounded_channel();
        for _ in 0..3 {
            let _sent = tx.send(Msg::Tick(Stamp::default()).into());
        }
        let mut carried = None;

        for _ in 0..3 {
            assert!(
                matches!(
                    queued(&mut rx, &mut carried),
                    Some(Incoming::Msg(Msg::Tick(_)))
                ),
                "a queued message is applied before the frame, not after it"
            );
        }

        assert!(
            queued(&mut rx, &mut carried).is_none(),
            "and then the batch ends"
        );
        assert!(carried.is_none());
    }

    /// The frame a gesture is resolved against is the one it happened in. A
    /// batch that applied it behind three keystrokes would resolve it against
    /// a map those keystrokes have already made false.
    #[test]
    fn a_gesture_ends_the_batch_and_waits_for_the_frame_it_earned() {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let _sent = tx.send(gesture());
        let _also_sent = tx.send(Msg::Tick(Stamp::default()).into());
        let mut carried = None;

        assert!(
            queued(&mut rx, &mut carried).is_none(),
            "the batch stops here"
        );
        assert!(
            matches!(carried, Some(Incoming::Mouse(_))),
            "and the gesture is held for the next turn rather than dropped"
        );
    }

    #[test]
    fn an_empty_channel_ends_the_batch_without_carrying_anything() {
        let (_tx, mut rx) = mpsc::unbounded_channel::<Incoming>();
        let mut carried = None;

        assert!(queued(&mut rx, &mut carried).is_none());
        assert!(carried.is_none());
    }
}

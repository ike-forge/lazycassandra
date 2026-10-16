//! Composition root and the main loop: wiring, not logic. The only crate that
//! sees all three layers.

mod cli;
mod clipboard;
mod editor;
mod effects;
mod export;
mod input;
mod mouse;

use std::sync::Arc;

use anyhow::Result;
use input::Incoming;
use lc_domain::geometry::Geometry;
use lc_domain::ports::{Cluster, ConfigSink, ConfigSource, Store, Writer};
use lc_domain::{Effect, Msg, State, update, view_model};
use lc_inbound::{FrameMap, Rendered, Themes};
use lc_outbound::cassandra::CassandraCluster;
use ratatui::DefaultTerminal;
use tokio::sync::mpsc;
use tokio::time::{Duration, MissedTickBehavior, interval};

/// Real time enters here only; the domain sees `Msg::Tick` and ignores one that
/// changes nothing.
const TICK: Duration = Duration::from_millis(40);

/// Sends unconditionally: only `update` knows whether a tick is worth a
/// repaint.
fn spawn_ticker(tx: mpsc::UnboundedSender<Incoming>) {
    tokio::spawn(async move {
        let mut ticks = interval(TICK);
        // `Skip`, not `Burst`: after a blocking editor session, replaying every
        // missed tick would mark thousands of frames dirty.
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
    // Before the terminal is touched, so usage errors and `--help` land in the
    // shell.
    let options = match cli::parse(std::env::args_os().skip(1)) {
        Ok(cli::Invocation::Run(options)) => options,
        Ok(cli::Invocation::Print(text)) => {
            println!("{text}");
            return Ok(());
        }
        Err(message) => {
            eprintln!("{message}");
            std::process::exit(2);
        }
    };
    if let Some(path) = &options.config
        && !path.is_file()
    {
        eprintln!("`--config {}`: no such file", path.display());
        std::process::exit(2);
    }

    // The panic hook restores the terminal. A tokio worker panic fires it
    // without ending the process (architecture.md §11, open).
    let terminal = ratatui::init();
    let result = captured(terminal, &options).await;
    ratatui::restore();
    result
}

/// Released while the terminal is still ours.
async fn captured(terminal: DefaultTerminal, options: &cli::Options) -> Result<()> {
    let capture = mouse::MouseCapture::enable()?;

    run(terminal, &capture, options).await
}

fn draw(terminal: &mut DefaultTerminal, state: &State, themes: &Themes) -> Result<Rendered> {
    let mut rendered = Rendered::default();
    let view = view_model(state);
    let theme = themes.get(&view.theme);

    terminal.draw(|frame| rendered = lc_inbound::render(frame, &view, theme))?;

    Ok(rendered)
}

/// Unconditional sends would redraw and re-measure forever.
fn report(tx: &mpsc::UnboundedSender<Incoming>, last: &mut Option<Geometry>, geometry: Geometry) {
    if *last == Some(geometry) {
        return;
    }

    *last = Some(geometry);
    // A closed channel means the main loop has ended.
    let _sent = tx.send(Msg::Measured(geometry).into());
}

/// Resolved here: only the main loop has the frame map. `None` sends nothing
/// and draws nothing.
fn resolve(incoming: Incoming, map: &FrameMap) -> Option<Msg> {
    match incoming {
        Incoming::Msg(msg) => Some(msg),
        Incoming::Mouse(event) => lc_inbound::input::to_pointer(event, map).map(Msg::Pointer),
    }
}

/// Messages already queued are applied as one batch and one frame, so a held
/// key stops where it was released. A gesture ends the batch: it must be
/// resolved against the frame it happened in.
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

async fn run(
    mut terminal: DefaultTerminal,
    capture: &mouse::MouseCapture,
    options: &cli::Options,
) -> Result<()> {
    let (tx, mut rx) = mpsc::unbounded_channel();
    let suspension = input::Suspension::default();
    input::spawn_reader(tx.clone(), suspension.clone());
    spawn_ticker(tx.clone());

    // Built empty: the target is chosen from the config default or the picker.
    let cluster: Arc<dyn Cluster> = Arc::new(CassandraCluster::new());
    let writer: Arc<dyn Writer> = Arc::new(lc_outbound::file::FileWriter);

    lc_outbound::config::write_example_config_from_env();
    let mut source = lc_outbound::config::FileConfigSource::from_env();
    if let Some(path) = &options.config {
        source = source.named(path);
    }
    if let Some(name) = &options.connection {
        source = source.opening(name);
    }
    let source = Arc::new(source);
    let (config, mut diagnostics) = source.load().await;
    let sink: Arc<dyn ConfigSink> = source;

    // Loaded here, not by an effect: `init` runs before the loop can answer
    // one.
    let store: Arc<dyn Store> = Arc::new(lc_outbound::history::FileStore::from_env());
    let (histories, history_diagnostics) = store.load(config.history.limit).await;
    diagnostics.extend(history_diagnostics);
    let themes = Themes::new(&config.themes, config.background);

    let config_home = lc_outbound::config::global_path();
    let (mut state, startup) = lc_domain::init(config, diagnostics, lc_outbound::clock::now());
    state.histories = histories;
    state.cwd = std::env::current_dir().unwrap_or_default();
    state.config_home = config_home;
    let mut measured: Option<Geometry> = None;

    for effect in &startup {
        effects::run(&tx, &cluster, &writer, &sink, &store, effect);
    }

    let rendered = draw(&mut terminal, &state, &themes)?;
    let mut map = rendered.map;
    report(&tx, &mut measured, rendered.geometry);
    state.dirty = false;

    // A gesture carried from the last batch, to resolve after the draw.
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
                    // Not a task: the editor owns the terminal while it runs.
                    if let Effect::OpenEditor { buffer } = &effect {
                        let outcome = editor::open(&mut terminal, capture, &suspension, buffer)?;
                        let _sent = tx.send(outcome.into());
                        continue;
                    }

                    effects::run(&tx, &cluster, &writer, &sink, &store, &effect);
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

//! Interprets the effects `update` returned.
//!
//! This is the only place that performs I/O on the domain's behalf. Effects
//! become tokio tasks; their results re-enter through the same channel as a
//! keystroke (architecture.md §3).

use std::sync::Arc;

use lc_domain::ports::Cluster;
use lc_domain::{Effect, Msg};
use tokio::sync::mpsc::UnboundedSender;

use crate::input::Incoming;

/// The effects that still have no port behind them are inert. `OpenEditor`
/// is the one exception the main loop runs itself, before anything reaches
/// here — the child process takes the terminal away from us (external editor
/// design §4). `Yank` is not like that: it is an OSC 52 sequence written to
/// the same stream every frame is, so it is handled below like the rest.
/// `Quit` needs no work here — `update` has already set
/// `State::should_quit`.
pub(crate) fn run(tx: &UnboundedSender<Incoming>, cluster: &Arc<dyn Cluster>, effect: &Effect) {
    match effect {
        Effect::Connect(conn) => {
            let (tx, cluster, conn) = (tx.clone(), Arc::clone(cluster), *conn);
            tokio::spawn(async move {
                let msg = match cluster.connect(conn).await {
                    Ok(posture) => Msg::Connected { conn, posture },
                    Err(error) => Msg::ConnectFailed { conn, error },
                };
                // A closed channel means the main loop has already ended.
                let _sent = tx.send(msg.into());
            });
        }

        Effect::LoadSchema(conn) => {
            let (tx, cluster, conn) = (tx.clone(), Arc::clone(cluster), *conn);
            tokio::spawn(async move {
                let msg = match cluster.describe(conn).await {
                    Ok(schema) => Msg::SchemaLoaded {
                        conn,
                        schema: Box::new(schema),
                    },
                    Err(error) => Msg::SchemaFailed { conn, error },
                };
                let _sent = tx.send(msg.into());
            });
        }

        Effect::Execute { conn, query, stmt } => {
            let (tx, cluster, conn, query, stmt) = (
                tx.clone(),
                Arc::clone(cluster),
                *conn,
                *query,
                stmt.as_ref().clone(),
            );
            tokio::spawn(async move {
                let msg = match cluster.execute(conn, &stmt).await {
                    Ok(page) => Msg::PageReady {
                        query,
                        page: Box::new(page),
                    },
                    Err(error) => Msg::Failed { query, error },
                };
                let _sent = tx.send(msg.into());
            });
        }

        // OSC 52 asks the terminal to set its clipboard, and the terminal
        // does not answer: a terminal that ignores the sequence is
        // indistinguishable from one that honoured it, so there is nothing
        // to report and nothing to send back (yank design §3, §4). Unlike
        // `OpenEditor` this does not take the terminal away from us — it is
        // written to the same stream every frame is written to.
        Effect::Yank(line) => {
            let _written = crossterm::execute!(
                std::io::stdout(),
                crossterm::clipboard::CopyToClipboard::to_clipboard_from(line)
            );
        }

        Effect::FetchNextPage { .. }
        | Effect::Cancel(_)
        | Effect::RecordHistory(_)
        | Effect::OpenEditor { .. }
        | Effect::Quit => {}
    }
}

//! Interprets the effects `update` returned.
//!
//! This is the only place that performs I/O on the domain's behalf. Effects
//! become tokio tasks; their results re-enter through the same channel as a
//! keystroke (architecture.md §3).

use std::sync::Arc;

use lc_domain::config::{ConnectionConfig, DriverConfig};
use lc_domain::model::{ConnId, DbError, Page, QueryId};
use lc_domain::ports::{Cluster, ConfigSink, Writer};
use lc_domain::{Effect, Msg};
use tokio::sync::mpsc::UnboundedSender;

use crate::input::Incoming;

/// The effects that still have no port behind them are inert. `OpenEditor`
/// is the one exception the main loop runs itself, before anything reaches
/// here — the child process takes the terminal away from us (external editor
/// design §4). `Yank` is not like that: its OSC 52 sequence goes to the same
/// stream every frame is written to, so it is handled below like the rest.
/// `Quit` needs no work here — `update` has already set
/// `State::should_quit`.
pub(crate) fn run(
    tx: &UnboundedSender<Incoming>,
    cluster: &Arc<dyn Cluster>,
    writer: &Arc<dyn Writer>,
    sink: &Arc<dyn ConfigSink>,
    effect: &Effect,
) {
    match effect {
        Effect::Connect {
            conn,
            target,
            driver,
        } => connect(tx, cluster, *conn, target.as_ref().clone(), *driver),

        Effect::LoadSchema(conn) => {
            let (tx, cluster, conn) = (tx.clone(), Arc::clone(cluster), *conn);
            tokio::spawn(async move {
                let started = std::time::Instant::now();
                let result = cluster.describe(conn).await;
                let took = started.elapsed();
                let msg = match result {
                    Ok(schema) => Msg::SchemaLoaded {
                        conn,
                        schema: Box::new(schema),
                        took,
                    },
                    Err(error) => Msg::SchemaFailed { conn, error, took },
                };
                let _sent = tx.send(msg.into());
            });
        }

        Effect::Execute { conn, query, stmt } => {
            let (cluster, conn, stmt) = (Arc::clone(cluster), *conn, stmt.as_ref().clone());
            answer(
                tx,
                *query,
                async move { cluster.execute(conn, &stmt).await },
            );
        }

        Effect::ExecutePaged {
            conn,
            query,
            stmt,
            page_size,
            cursor,
        } => {
            let (cluster, conn, stmt, page_size, cursor) = (
                Arc::clone(cluster),
                *conn,
                stmt.as_ref().clone(),
                *page_size,
                cursor.clone(),
            );
            answer(tx, *query, async move {
                cluster
                    .execute_paged(conn, &stmt, page_size, cursor.as_ref())
                    .await
            });
        }

        // Both routes, every time. OSC 52 asks the terminal, and the terminal
        // does not answer: one that ignores the sequence is indistinguishable
        // from one that honoured it (yank design §3, §4) — so there is no
        // failure to detect and nothing to fall back from. A multiplexer that
        // declines to forward the sequence is exactly that case, and it is why
        // the helper is asked as well. Both are told the same line, so the
        // clipboard holds the same thing whichever of them the platform
        // listens to.
        //
        // The OSC 52 write stays here on the main loop: unlike `OpenEditor` it
        // does not take the terminal away from us, but it does go to the
        // stream every frame goes to, and a write racing a draw would
        // interleave with an escape sequence mid-flight. The helper touches no
        // terminal, so it is a task.
        Effect::Yank(line) => {
            let _written = crossterm::execute!(
                std::io::stdout(),
                crossterm::clipboard::CopyToClipboard::to_clipboard_from(line)
            );
            crate::clipboard::hand_off(line);
        }

        Effect::Export {
            conn,
            job,
            source,
            destination,
            format,
            overwrite,
            page_size,
        } => crate::export::spawn(
            tx,
            cluster,
            writer,
            crate::export::Job {
                conn: *conn,
                id: *job,
                source: source.clone(),
                destination: destination.clone(),
                format: *format,
                overwrite: *overwrite,
                page_size: *page_size,
            },
        ),

        Effect::SaveTheme(theme) => {
            tokio::spawn(save(tx.clone(), Arc::clone(sink), theme.clone()));
        }

        Effect::Cancel(_) | Effect::RecordHistory(_) | Effect::OpenEditor { .. } | Effect::Quit => {
        }
    }
}

fn connect(
    tx: &UnboundedSender<Incoming>,
    cluster: &Arc<dyn Cluster>,
    conn: ConnId,
    target: ConnectionConfig,
    driver: DriverConfig,
) {
    let (tx, cluster) = (tx.clone(), Arc::clone(cluster));
    tokio::spawn(async move {
        let started = std::time::Instant::now();
        let result = cluster.connect(conn, &target, &driver).await;
        let took = started.elapsed();
        let msg = match result {
            Ok(posture) => Msg::Connected {
                conn,
                posture,
                took,
            },
            Err(error) => Msg::ConnectFailed { conn, error, took },
        };
        // A closed channel means the main loop has already ended.
        let _sent = tx.send(msg.into());
    });
}

/// Runs `call` as a task and sends back the page or the failure, timed.
fn answer(
    tx: &UnboundedSender<Incoming>,
    query: QueryId,
    call: impl Future<Output = Result<Page, DbError>> + Send + 'static,
) {
    let tx = tx.clone();
    tokio::spawn(async move {
        let started = std::time::Instant::now();
        let result = call.await;
        let took = started.elapsed();
        let msg = match result {
            Ok(page) => Msg::PageReady {
                query,
                page: Box::new(page),
                took,
            },
            Err(error) => Msg::Failed { query, error, took },
        };
        // A closed channel means the main loop has already ended.
        let _sent = tx.send(msg.into());
    });
}

/// Silent on success, by design: the theme on screen is the confirmation, and
/// only a failure is worth a line (theme persistence design §6).
///
/// Spawned by [`run`], and written as its own `async fn` so the two outcomes
/// can be tested without a runtime racing the assertion.
async fn save(
    tx: UnboundedSender<Incoming>,
    sink: Arc<dyn ConfigSink>,
    theme: lc_domain::theme::ThemeChoice,
) {
    if let Err(error) = sink.save_theme(&theme).await {
        // A closed channel means the main loop has already ended.
        let _sent = tx.send(
            Msg::ThemeSaveFailed {
                reason: error.to_string(),
            }
            .into(),
        );
    }
}

#[cfg(test)]
mod tests {
    use lc_domain::ports::{ConfigSink, ConfigWriteError};
    use lc_domain::theme::{Ours, ThemeChoice};

    use super::*;

    struct Sink(Result<(), ConfigWriteError>);

    #[async_trait::async_trait]
    impl ConfigSink for Sink {
        async fn save_theme(&self, _theme: &ThemeChoice) -> Result<(), ConfigWriteError> {
            self.0.clone()
        }
    }

    fn theme() -> ThemeChoice {
        ThemeChoice::Ours(Ours::QuillDark)
    }

    #[tokio::test]
    async fn a_successful_save_says_nothing() {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();

        save(tx, Arc::new(Sink(Ok(()))), theme()).await;

        assert!(
            rx.try_recv().is_err(),
            "the theme on screen is the confirmation"
        );
    }

    #[tokio::test]
    async fn a_failed_save_reports_which_file_and_why() {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let error = ConfigWriteError::Io("/home/x/config.toml: permission denied".to_owned());

        save(tx, Arc::new(Sink(Err(error))), theme()).await;

        let Some(Incoming::Msg(Msg::ThemeSaveFailed { reason })) = rx.recv().await else {
            panic!("a save the user asked for and did not get is worth one line");
        };
        assert!(reason.contains("permission denied"), "{reason}");
        assert!(reason.contains("/home/x/config.toml"), "{reason}");
    }
}

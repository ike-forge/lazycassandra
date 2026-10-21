//! Performs the effects `update` returned, as tokio tasks whose results
//! re-enter through the keystroke channel.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, PoisonError};

use lc_domain::config::{ConnectionConfig, ConnectionName, DriverConfig};
use lc_domain::contents::TableRef;
use lc_domain::model::{ConnId, DbError, Page, QueryId};
use lc_domain::ports::{Cluster, ConfigSink, Store, Writer};
use lc_domain::{Effect, Msg};
use tokio::sync::mpsc::UnboundedSender;
use tokio::task::AbortHandle;

use crate::input::Incoming;

/// The queries whose tasks can still be aborted.
#[derive(Clone, Default)]
pub(crate) struct Running(Arc<Mutex<HashMap<QueryId, AbortHandle>>>);

impl Running {
    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<QueryId, AbortHandle>> {
        // Poisoned only by a panic between an insert and a remove; the map is
        // still sound.
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn abort(&self, query: QueryId) {
        if let Some(handle) = self.lock().remove(&query) {
            handle.abort();
        }
    }
}

/// `OpenEditor` is run by the main loop. `Quit` needs nothing: `update` set
/// `should_quit`.
pub(crate) fn run(
    tx: &UnboundedSender<Incoming>,
    running: &Running,
    cluster: &Arc<dyn Cluster>,
    writer: &Arc<dyn Writer>,
    sink: &Arc<dyn ConfigSink>,
    store: &Arc<dyn Store>,
    effect: &Effect,
) {
    match effect {
        Effect::Connect {
            conn,
            target,
            driver,
        } => connect(tx, cluster, *conn, target.as_ref().clone(), *driver),

        Effect::LoadSchema(conn) => load_schema(tx, cluster, *conn),

        Effect::Execute { conn, query, stmt } => {
            let (cluster, conn, stmt) = (Arc::clone(cluster), *conn, stmt.as_ref().clone());
            answer(tx, running, *query, async move {
                cluster.execute(conn, &stmt).await
            });
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
            answer(tx, running, *query, async move {
                cluster
                    .execute_paged(conn, &stmt, page_size, cursor.as_ref())
                    .await
            });
        }

        // Both routes every time: OSC 52 gets no answer, so failure is
        // undetectable, and a multiplexer may drop it. The OSC write stays on
        // the main loop so it cannot interleave with a frame; the helper is a
        // task.
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
                page_size: *page_size,
            },
        ),

        Effect::SaveTheme(theme) => {
            tokio::spawn(save(tx.clone(), Arc::clone(sink), theme.clone()));
        }

        Effect::SaveHistory {
            connection,
            entries,
        } => {
            tokio::spawn(save_history(
                tx.clone(),
                Arc::clone(store),
                connection.clone(),
                entries.clone(),
            ));
        }

        Effect::LoadHealth {
            conn,
            request,
            table,
        } => load_health(tx, cluster, *conn, *request, table.clone()),

        Effect::Cancel(query) => running.abort(*query),

        Effect::RecordHistory(_) | Effect::OpenEditor { .. } | Effect::Quit => {}
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
        // A closed channel means the main loop has ended.
        let _sent = tx.send(msg.into());
    });
}

fn load_schema(tx: &UnboundedSender<Incoming>, cluster: &Arc<dyn Cluster>, conn: ConnId) {
    let (tx, cluster) = (tx.clone(), Arc::clone(cluster));
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

fn load_health(
    tx: &UnboundedSender<Incoming>,
    cluster: &Arc<dyn Cluster>,
    conn: ConnId,
    request: QueryId,
    table: TableRef,
) {
    let (tx, cluster) = (tx.clone(), Arc::clone(cluster));
    tokio::spawn(async move {
        let started = std::time::Instant::now();
        let health = cluster.table_health(conn, &table).await;
        let took = started.elapsed();
        // A closed channel means the main loop has ended.
        let _sent = tx.send(
            Msg::HealthLoaded {
                request,
                health,
                took,
            }
            .into(),
        );
    });
}

/// Aborting drops the call, so nothing is sent: `update` already forgot the id.
fn answer(
    tx: &UnboundedSender<Incoming>,
    running: &Running,
    query: QueryId,
    call: impl Future<Output = Result<Page, DbError>> + Send + 'static,
) {
    let (tx, registry) = (tx.clone(), running.clone());
    // Held across the spawn so a task that finishes at once removes an entry
    // that is already there.
    let mut handles = running.lock();
    let task = tokio::spawn(async move {
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
        registry.lock().remove(&query);
        // A closed channel means the main loop has ended.
        let _sent = tx.send(msg.into());
    });
    handles.insert(query, task.abort_handle());
}

/// Silent on success. Its own `async fn` so both outcomes test without a racing
/// runtime.
async fn save(
    tx: UnboundedSender<Incoming>,
    sink: Arc<dyn ConfigSink>,
    theme: lc_domain::theme::ThemeChoice,
) {
    if let Err(error) = sink.save_theme(&theme).await {
        // A closed channel means the main loop has ended.
        let _sent = tx.send(
            Msg::ThemeSaveFailed {
                reason: error.to_string(),
            }
            .into(),
        );
    }
}

/// A failure is a trail line, not a modal: it repeats every run while the
/// directory is unwritable.
async fn save_history(
    tx: UnboundedSender<Incoming>,
    store: Arc<dyn Store>,
    connection: ConnectionName,
    entries: Vec<String>,
) {
    if let Err(error) = store.save(&connection, &entries).await {
        // A closed channel means the main loop has ended.
        let _sent = tx.send(
            Msg::HistorySaveFailed {
                reason: error.to_string(),
            }
            .into(),
        );
    }
}

#[cfg(test)]
mod tests {
    use lc_domain::config::{Diagnostic, HistoryLimit};
    use lc_domain::history::Histories;
    use lc_domain::ports::{ConfigSink, ConfigWriteError, StoreError};
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
    async fn a_cancelled_query_never_answers() {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let running = Running::default();
        let query = QueryId::next();
        let (started, hold) = tokio::sync::oneshot::channel::<()>();
        answer(&tx, &running, query, async move {
            let _held = started;
            std::future::pending().await
        });

        running.abort(query);

        // The aborted future drops its sender, which is the only observable
        // sign it was torn down.
        assert!(hold.await.is_err());
        assert!(rx.try_recv().is_err());
        assert!(running.lock().is_empty());
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

    struct Disk(Result<(), StoreError>);

    #[async_trait::async_trait]
    impl Store for Disk {
        async fn load(&self, _limit: HistoryLimit) -> (Histories, Vec<Diagnostic>) {
            (Histories::default(), Vec::new())
        }

        async fn save(
            &self,
            _connection: &ConnectionName,
            _entries: &[String],
        ) -> Result<(), StoreError> {
            self.0.clone()
        }
    }

    fn name() -> ConnectionName {
        ConnectionName::try_new("dev").expect("a valid name")
    }

    #[tokio::test]
    async fn a_saved_history_says_nothing() {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();

        save_history(
            tx,
            Arc::new(Disk(Ok(()))),
            name(),
            vec!["SELECT 1;".to_owned()],
        )
        .await;

        assert!(rx.try_recv().is_err());
    }

    #[tokio::test]
    async fn a_history_that_could_not_be_saved_reports_why() {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let error = StoreError::Unavailable("/home/x/history.toml: permission denied".to_owned());

        save_history(tx, Arc::new(Disk(Err(error))), name(), Vec::new()).await;

        let Some(Incoming::Msg(Msg::HistorySaveFailed { reason })) = rx.recv().await else {
            panic!("a lost save is worth one line");
        };
        assert!(reason.contains("permission denied"), "{reason}");
    }
}

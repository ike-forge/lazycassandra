//! An in-memory `Cluster` with a scripted schema.
//!
//! Not test-only: scenario tests (architecture.md §10) drive whole sessions
//! through it from the `lc-domain` side, so it ships in the crate rather than
//! behind `#[cfg(test)]`. Deterministic by construction — no clock, no socket
//! (conventions.md §4).

use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use async_trait::async_trait;
use lc_domain::model::{ConnId, DbError, Page, PagingState, Schema, Statement};
use lc_domain::ports::{Cluster, ExportError, Sink, Writer};
use lc_domain::posture::Posture;

#[derive(Debug, Default)]
pub struct FakeCluster {
    schema: Schema,
    posture: Posture,
    /// What `execute` hands back, whatever the statement.
    page: Page,
    /// What `connect` returns instead of succeeding.
    connect_error: Option<DbError>,
    /// What `describe` returns instead of succeeding.
    describe_error: Option<DbError>,
    /// What `execute` returns instead of a page.
    execute_error: Option<DbError>,
    /// The statements `execute` was asked for, in order. Behind a lock
    /// because the port takes `&self`; a scenario asserts on it afterwards.
    executed: Mutex<Vec<Statement>>,
    /// Pages `execute_paged` walks, in order. Empty means one page — whatever
    /// `execute` would return — and no cursor.
    pages: Vec<Page>,
    /// The page index at which `execute_paged` fails instead of handing back
    /// the scripted page, and what it fails with. Every page before it is
    /// still delivered, so a scenario can prove a mid-export abort rather
    /// than a first-page one.
    failing_page: Option<(usize, DbError)>,
}

impl FakeCluster {
    #[must_use]
    pub fn new(schema: Schema, posture: Posture) -> Self {
        Self {
            schema,
            posture,
            ..Self::default()
        }
    }

    /// The rows `execute` will hand back.
    #[must_use]
    pub fn returning(self, page: Page) -> Self {
        Self { page, ..self }
    }

    /// A cluster that connects and describes itself but refuses to read.
    #[must_use]
    pub fn refusing_to_execute(self, error: DbError) -> Self {
        Self {
            execute_error: Some(error),
            ..self
        }
    }

    /// The pages `execute_paged` walks through, in order.
    ///
    /// The cursor is the index of the page that follows, so a scripted run
    /// needs no driver and no clock.
    #[must_use]
    pub fn paging(self, pages: Vec<Page>) -> Self {
        Self { pages, ..self }
    }

    /// Fails `execute_paged` when the cursor names `index` rather than
    /// handing back the scripted page there.
    ///
    /// Two things to know before reaching for this: it is consulted only
    /// after `pages` is non-empty (an unscripted cluster returns its one
    /// `execute` page before `index` is ever checked), and `execute_error`
    /// from [`Self::refusing_to_execute`] is checked first and, if set,
    /// fails every call regardless of `index`.
    #[must_use]
    pub fn failing_page(self, index: usize, error: DbError) -> Self {
        Self {
            failing_page: Some((index, error)),
            ..self
        }
    }

    /// Every statement `execute` has been handed, in order.
    ///
    /// A poisoned lock is recovered from rather than propagated: the panic
    /// that poisoned it is the failure worth reporting, not this.
    #[must_use]
    pub fn executed(&self) -> Vec<Statement> {
        self.executed
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// A cluster that cannot be reached at all.
    #[must_use]
    pub fn failing(error: DbError) -> Self {
        Self {
            connect_error: Some(error),
            ..Self::default()
        }
    }

    /// A cluster that connects and then refuses to describe itself — the shape
    /// of a connection without the permissions to read `system_schema`.
    #[must_use]
    pub fn failing_to_describe(error: DbError) -> Self {
        Self {
            describe_error: Some(error),
            ..Self::default()
        }
    }
}

#[async_trait]
impl Cluster for FakeCluster {
    async fn connect(&self, _conn: ConnId) -> Result<Posture, DbError> {
        self.connect_error.clone().map_or(Ok(self.posture), Err)
    }

    async fn describe(&self, _conn: ConnId) -> Result<Schema, DbError> {
        self.describe_error
            .clone()
            .map_or_else(|| Ok(self.schema.clone()), Err)
    }

    async fn execute(&self, _conn: ConnId, stmt: &Statement) -> Result<Page, DbError> {
        self.executed
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(stmt.clone());

        self.execute_error
            .clone()
            .map_or_else(|| Ok(self.page.clone()), Err)
    }

    async fn execute_paged(
        &self,
        _conn: ConnId,
        stmt: &Statement,
        cursor: Option<&PagingState>,
    ) -> Result<Page, DbError> {
        self.executed
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(stmt.clone());

        if let Some(error) = self.execute_error.clone() {
            return Err(error);
        }

        if self.pages.is_empty() {
            return Ok(Page {
                next: None,
                ..self.page.clone()
            });
        }

        let index = cursor
            .and_then(|PagingState(bytes)| bytes.first().copied())
            .map_or(0, usize::from);

        if let Some((fails_at, error)) = &self.failing_page
            && index == *fails_at
        {
            return Err(error.clone());
        }

        Ok(self.pages.get(index).cloned().unwrap_or_default())
    }
}

/// A [`Writer`] that keeps what it was given in memory.
///
/// Ships in the crate rather than behind `#[cfg(test)]` for the reason
/// [`FakeCluster`] does: the scenario tier drives whole exports through it.
#[derive(Debug, Default, Clone)]
pub struct FakeWriter {
    written: Arc<Mutex<String>>,
    aborted: Arc<AtomicBool>,
    finished: Arc<AtomicBool>,
    /// Whether `open` refuses every path as already taken.
    exists: bool,
}

impl FakeWriter {
    /// Everything every chunk was handed, concatenated.
    #[must_use]
    pub fn written(&self) -> String {
        self.written
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// A writer that refuses every path as already taken.
    #[must_use]
    pub fn refusing_because_it_exists() -> Self {
        Self {
            exists: true,
            ..Self::default()
        }
    }

    /// Whether `abort` was called.
    #[must_use]
    pub fn aborted(&self) -> bool {
        self.aborted.load(Ordering::Relaxed)
    }

    /// Whether `finish` was called. A run that aborts must never also finish
    /// (design §4).
    #[must_use]
    pub fn finished(&self) -> bool {
        self.finished.load(Ordering::Relaxed)
    }
}

#[async_trait]
impl Writer for FakeWriter {
    async fn open(&self, _path: &Path, _overwrite: bool) -> Result<Box<dyn Sink>, ExportError> {
        if self.exists {
            return Err(ExportError::Exists);
        }

        Ok(Box::new(FakeSink {
            written: Arc::clone(&self.written),
            aborted: Arc::clone(&self.aborted),
            finished: Arc::clone(&self.finished),
        }))
    }
}

/// The open half of a [`FakeWriter`], sharing its state so a scenario can
/// still observe it after `open` has handed the sink away.
struct FakeSink {
    written: Arc<Mutex<String>>,
    aborted: Arc<AtomicBool>,
    finished: Arc<AtomicBool>,
}

#[async_trait]
impl Sink for FakeSink {
    async fn write(&mut self, chunk: &str) -> Result<(), ExportError> {
        self.written
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push_str(chunk);
        Ok(())
    }

    async fn finish(&mut self) -> Result<(), ExportError> {
        self.finished.store(true, Ordering::Relaxed);
        Ok(())
    }

    async fn abort(&mut self) {
        self.aborted.store(true, Ordering::Relaxed);
        self.written
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lc_domain::model::{CqlValue, Row};

    fn page(id: i64, next: Option<u8>) -> Page {
        Page {
            columns: vec!["id".to_owned()],
            rows: vec![Row {
                values: vec![CqlValue::Int(id)],
            }],
            next: next.map(|byte| PagingState(vec![byte])),
        }
    }

    #[tokio::test]
    async fn a_scripted_cluster_hands_back_its_pages_in_order() {
        let cluster =
            FakeCluster::default().paging(vec![page(1, Some(1)), page(2, Some(2)), page(3, None)]);
        let conn = ConnId::next();
        let stmt = Statement::typed("SELECT * FROM shop.orders");

        let first = cluster
            .execute_paged(conn, &stmt, None)
            .await
            .expect("a page");
        assert_eq!(first, page(1, Some(1)));

        let cursor = first.next.clone().expect("a cursor");
        let second = cluster
            .execute_paged(conn, &stmt, Some(&cursor))
            .await
            .expect("a page");
        assert_eq!(second, page(2, Some(2)));

        let cursor = second.next.clone().expect("a cursor");
        let third = cluster
            .execute_paged(conn, &stmt, Some(&cursor))
            .await
            .expect("a page");
        assert_eq!(third.next, None, "the last page hands back no cursor");
    }

    #[tokio::test]
    async fn an_unscripted_cluster_pages_once_and_stops() {
        let cluster = FakeCluster::default();

        let page = cluster
            .execute_paged(ConnId::next(), &Statement::typed("SELECT 1"), None)
            .await
            .expect("a page");

        assert_eq!(page.next, None);
    }
}

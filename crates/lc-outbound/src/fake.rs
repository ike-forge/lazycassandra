//! An in-memory `Cluster` with a scripted schema. Shipped, not test-only:
//! scenario tests drive sessions through it.

use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use async_trait::async_trait;
use lc_domain::config::PageSize;
use lc_domain::contents::TableRef;
use lc_domain::health::Health;
use lc_domain::model::{ConnId, DbError, Page, PagingState, Schema, Statement};
use lc_domain::ports::{Cluster, ExportError, Sink, Writer};
use lc_domain::posture::Posture;

#[derive(Debug, Default)]
pub struct FakeCluster {
    schema: Schema,
    posture: Posture,
    page: Page,
    connect_error: Option<DbError>,
    describe_error: Option<DbError>,
    /// From the second call on, for sessions that change the schema and re-read
    /// it. `None` keeps one shape.
    re_described: Option<Schema>,
    /// Atomic: the port takes `&self`.
    describes: AtomicUsize,
    execute_error: Option<DbError>,
    /// Locked: the port takes `&self`.
    executed: Mutex<Vec<Statement>>,
    page_sizes: Mutex<Vec<PageSize>>,
    /// Empty means one page, as `execute` would return, with no cursor.
    pages: Vec<Page>,
    /// Earlier pages are still delivered, so a mid-export abort is testable.
    failing_page: Option<(usize, DbError)>,
    health: Health,
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

    #[must_use]
    pub fn returning(self, page: Page) -> Self {
        Self { page, ..self }
    }

    #[must_use]
    pub fn re_describing(self, schema: Schema) -> Self {
        Self {
            re_described: Some(schema),
            ..self
        }
    }

    #[must_use]
    pub fn refusing_to_execute(self, error: DbError) -> Self {
        Self {
            execute_error: Some(error),
            ..self
        }
    }

    /// The cursor is the next page's index.
    #[must_use]
    pub fn paging(self, pages: Vec<Page>) -> Self {
        Self { pages, ..self }
    }

    /// Only consulted once `pages` is set; `refusing_to_execute` takes
    /// precedence.
    #[must_use]
    pub fn failing_page(self, index: usize, error: DbError) -> Self {
        Self {
            failing_page: Some((index, error)),
            ..self
        }
    }

    #[must_use]
    pub fn reporting(self, health: Health) -> Self {
        Self { health, ..self }
    }

    /// A poisoned lock is recovered: the panic that poisoned it is the real
    /// failure.
    #[must_use]
    pub fn executed(&self) -> Vec<Statement> {
        self.executed
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    #[must_use]
    pub fn page_sizes(&self) -> Vec<PageSize> {
        self.page_sizes
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    #[must_use]
    pub fn failing(error: DbError) -> Self {
        Self {
            connect_error: Some(error),
            ..Self::default()
        }
    }

    /// A connection without permission to read `system_schema`.
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
    async fn connect(
        &self,
        _conn: ConnId,
        _target: &lc_domain::config::ConnectionConfig,
        _driver: &lc_domain::config::DriverConfig,
    ) -> Result<Posture, DbError> {
        self.connect_error.clone().map_or(Ok(self.posture), Err)
    }

    async fn describe(&self, _conn: ConnId) -> Result<Schema, DbError> {
        if let Some(error) = self.describe_error.clone() {
            return Err(error);
        }

        let first = self.describes.fetch_add(1, Ordering::Relaxed) == 0;

        Ok(match &self.re_described {
            Some(changed) if !first => changed.clone(),
            _ => self.schema.clone(),
        })
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
        page_size: PageSize,
        cursor: Option<&PagingState>,
    ) -> Result<Page, DbError> {
        self.executed
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(stmt.clone());
        self.page_sizes
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(page_size);

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

    async fn table_health(&self, _conn: ConnId, _table: &TableRef) -> Result<Health, DbError> {
        Ok(self.health.clone())
    }
}

/// Shipped for the scenario tier, like [`FakeCluster`].
#[derive(Debug, Default, Clone)]
pub struct FakeWriter {
    written: Arc<Mutex<String>>,
    aborted: Arc<AtomicBool>,
    finished: Arc<AtomicBool>,
    exists: bool,
    /// Disk full at close, where a partial file is likeliest.
    unflushable: bool,
}

impl FakeWriter {
    #[must_use]
    pub fn written(&self) -> String {
        self.written
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    #[must_use]
    pub fn refusing_because_it_exists() -> Self {
        Self {
            exists: true,
            ..Self::default()
        }
    }

    /// `finish` is the flush, so this leaves a truncated file.
    #[must_use]
    pub fn failing_to_finish() -> Self {
        Self {
            unflushable: true,
            ..Self::default()
        }
    }

    #[must_use]
    pub fn aborted(&self) -> bool {
        self.aborted.load(Ordering::Relaxed)
    }

    /// An aborted run must never also finish.
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
            unflushable: self.unflushable,
        }))
    }
}

/// Shares state with its writer so a scenario can observe it after `open`.
struct FakeSink {
    written: Arc<Mutex<String>>,
    aborted: Arc<AtomicBool>,
    finished: Arc<AtomicBool>,
    unflushable: bool,
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

        if self.unflushable {
            return Err(ExportError::Io("no space left on device".to_owned()));
        }

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
            .execute_paged(conn, &stmt, PageSize::default(), None)
            .await
            .expect("a page");
        assert_eq!(first, page(1, Some(1)));

        let cursor = first.next.clone().expect("a cursor");
        let second = cluster
            .execute_paged(conn, &stmt, PageSize::default(), Some(&cursor))
            .await
            .expect("a page");
        assert_eq!(second, page(2, Some(2)));

        let cursor = second.next.clone().expect("a cursor");
        let third = cluster
            .execute_paged(conn, &stmt, PageSize::default(), Some(&cursor))
            .await
            .expect("a page");
        assert_eq!(third.next, None, "the last page hands back no cursor");
    }

    #[tokio::test]
    async fn an_unscripted_cluster_pages_once_and_stops() {
        let cluster = FakeCluster::default();

        let page = cluster
            .execute_paged(
                ConnId::next(),
                &Statement::typed("SELECT 1"),
                PageSize::default(),
                None,
            )
            .await
            .expect("a page");

        assert_eq!(page.next, None);
    }
}

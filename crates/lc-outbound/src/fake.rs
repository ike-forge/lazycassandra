//! An in-memory `Cluster` with a scripted schema.
//!
//! Not test-only: scenario tests (architecture.md §10) drive whole sessions
//! through it from the `lc-domain` side, so it ships in the crate rather than
//! behind `#[cfg(test)]`. Deterministic by construction — no clock, no socket
//! (conventions.md §4).

use async_trait::async_trait;
use lc_domain::model::{ConnId, DbError, Page, PagingState, Schema, Statement};
use lc_domain::ports::Cluster;
use lc_domain::posture::Posture;

#[derive(Debug, Clone, Default)]
pub struct FakeCluster {
    schema: Schema,
    posture: Posture,
    /// What `connect` returns instead of succeeding.
    connect_error: Option<DbError>,
    /// What `describe` returns instead of succeeding.
    describe_error: Option<DbError>,
}

impl FakeCluster {
    #[must_use]
    pub const fn new(schema: Schema, posture: Posture) -> Self {
        Self {
            schema,
            posture,
            connect_error: None,
            describe_error: None,
        }
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

    async fn execute(&self, _conn: ConnId, _stmt: &Statement) -> Result<Page, DbError> {
        Ok(Page {
            columns: Vec::new(),
            rows: Vec::new(),
            next: None,
        })
    }

    async fn fetch_page(&self, _conn: ConnId, _cursor: &PagingState) -> Result<Page, DbError> {
        Ok(Page {
            columns: Vec::new(),
            rows: Vec::new(),
            next: None,
        })
    }
}

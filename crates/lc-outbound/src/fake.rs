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
}

impl FakeCluster {
    #[must_use]
    pub const fn new(schema: Schema, posture: Posture) -> Self {
        Self { schema, posture }
    }
}

#[async_trait]
impl Cluster for FakeCluster {
    async fn connect(&self, _conn: ConnId) -> Result<Posture, DbError> {
        Ok(self.posture)
    }

    async fn describe(&self, _conn: ConnId) -> Result<Schema, DbError> {
        Ok(self.schema.clone())
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

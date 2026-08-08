//! An in-memory `Cluster` with a scripted schema.
//!
//! Not test-only: scenario tests (architecture.md §10) drive whole sessions
//! through it from the `lc-domain` side, so it ships in the crate rather than
//! behind `#[cfg(test)]`. Deterministic by construction — no clock, no socket
//! (conventions.md §4).

use std::sync::{Mutex, PoisonError};

use async_trait::async_trait;
use lc_domain::model::{ConnId, DbError, Page, PagingState, Schema, Statement};
use lc_domain::ports::Cluster;
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

    async fn fetch_page(&self, _conn: ConnId, _cursor: &PagingState) -> Result<Page, DbError> {
        Ok(self.page.clone())
    }
}

//! Writes in flight: an export, a drop, a commit, one of each at a time.

use std::path::{Path, PathBuf};

use crate::contents::TableRef;
use crate::drop::Target;
use crate::edit::CommitScope;
use crate::export::ExportForm;
use crate::inspect::Inspector;
use crate::model::{JobId, QueryId};
use crate::update::Source;

#[derive(Debug, Clone, PartialEq)]
pub struct ExportJob {
    id: JobId,
    rows: usize,
    destination: PathBuf,
    /// So `ExportError::Exists` can reopen the form as it was.
    form: ExportForm,
    /// The panel stays live during an export, so a retry must re-emit what was
    /// attempted.
    source: Source,
}

impl ExportJob {
    #[must_use]
    pub const fn started(
        id: JobId,
        destination: PathBuf,
        form: ExportForm,
        source: Source,
    ) -> Self {
        Self {
            id,
            rows: 0,
            destination,
            form,
            source,
        }
    }

    #[must_use]
    pub const fn id(&self) -> JobId {
        self.id
    }

    #[must_use]
    pub const fn rows(&self) -> usize {
        self.rows
    }

    #[must_use]
    pub fn destination(&self) -> &Path {
        &self.destination
    }

    pub fn progressed(&mut self, job: JobId, rows: usize) {
        if self.id == job {
            self.rows = rows;
        }
    }

    /// Consumed, so nobody holds the job and its parts at once.
    #[must_use]
    pub fn into_parts(self) -> (ExportForm, Source) {
        (self.form, self.source)
    }
}

/// Carries the target: the tree is live during the round trip.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dropping {
    query: QueryId,
    target: Target,
}

impl Dropping {
    #[must_use]
    pub const fn started(query: QueryId, target: Target) -> Self {
        Self { query, target }
    }

    #[must_use]
    pub const fn query(&self) -> QueryId {
        self.query
    }

    #[must_use]
    pub const fn target(&self) -> &Target {
        &self.target
    }
}

/// Carries the table: the page is live during the round trip.
#[derive(Debug, Clone, PartialEq)]
pub struct Committing {
    query: QueryId,
    table: TableRef,
    /// Given back on refusal.
    form: Option<Box<Inspector>>,
    scope: CommitScope,
}

impl Committing {
    #[must_use]
    pub const fn started(query: QueryId, table: TableRef, scope: CommitScope) -> Self {
        Self {
            query,
            table,
            scope,
            form: None,
        }
    }

    /// Lands in no page row, so the grid is re-read.
    #[must_use]
    pub fn inserted(query: QueryId, table: TableRef, form: Inspector) -> Self {
        Self {
            query,
            table,
            scope: CommitScope::All,
            form: Some(Box::new(form)),
        }
    }

    #[must_use]
    pub const fn is_insert(&self) -> bool {
        self.form.is_some()
    }

    /// `None` for an update.
    pub fn take_form(&mut self) -> Option<Inspector> {
        self.form.take().map(|form| *form)
    }

    #[must_use]
    pub const fn scope(&self) -> CommitScope {
        self.scope
    }

    #[must_use]
    pub const fn query(&self) -> QueryId {
        self.query
    }

    #[must_use]
    pub const fn table(&self) -> &TableRef {
        &self.table
    }
}

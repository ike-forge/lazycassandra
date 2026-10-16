//! Writes in flight: an export, a drop, a commit — one of each at a time.

use std::path::{Path, PathBuf};

use crate::contents::TableRef;
use crate::drop::Target;
use crate::edit::CommitScope;
use crate::export::ExportForm;
use crate::inspect::Inspector;
use crate::model::{JobId, QueryId};
use crate::update::Source;

/// A running export.
#[derive(Debug, Clone, PartialEq)]
pub struct ExportJob {
    id: JobId,
    rows: usize,
    destination: PathBuf,
    /// Kept so `ExportError::Exists` can put the form back exactly as it was
    /// (export design §7). Dropped on any other outcome.
    form: ExportForm,
    /// Kept for the same round trip, and for the same reason the form is: what
    /// the confirmation re-emits has to be what was attempted, not what the
    /// panel happens to hold by the time the answer arrives — the panel stays
    /// live for the whole of an export (final review I3).
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

    /// Rows written so far, as the hint bar reports it.
    #[must_use]
    pub const fn rows(&self) -> usize {
        self.rows
    }

    #[must_use]
    pub fn destination(&self) -> &Path {
        &self.destination
    }

    /// Applies a progress report, if it belongs to this job.
    pub fn progressed(&mut self, job: JobId, rows: usize) {
        if self.id == job {
            self.rows = rows;
        }
    }

    /// The form the job was started from and the source it was started with,
    /// consumed so a caller cannot hold both the job and its parts at once.
    #[must_use]
    pub fn into_parts(self) -> (ExportForm, Source) {
        (self.form, self.source)
    }
}

/// A drop that has reached the cluster and not yet come back.
///
/// The [`QueryId`] alone would not do: the tree the target was read from is
/// live for the whole round trip, so what the message names when the answer
/// arrives is carried here rather than read again (the argument
/// [`ExportJob::source`] already makes).
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

/// A commit in flight: the query, and the table it is writing to.
///
/// [`Dropping`]'s shape and its reason: the page the edits were read from is
/// live for the whole round trip, so what the message names when the answer
/// arrives is carried here rather than read again
/// (docs/superpowers/specs/2026-09-18-cell-editing-design.md §5).
#[derive(Debug, Clone, PartialEq)]
pub struct Committing {
    query: QueryId,
    table: TableRef,
    /// The insert form being written, kept so a refusal can give it back
    /// (insert design §5).
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

    /// An `INSERT`, which lands in no row of the page: the grid is re-read
    /// rather than patched (docs/superpowers/specs/2026-10-12-row-insert-design.md §5).
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

    /// The form that was being written, `None` for an update.
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

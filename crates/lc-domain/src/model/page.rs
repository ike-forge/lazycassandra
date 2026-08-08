//! One page of a result set.
//!
//! Paging is incremental by construction: a [`Page`] carries the cursor needed
//! to ask for the next one, and nothing here assumes the whole result set fits
//! in memory (vision.md §5.1).

use crate::model::value::CqlValue;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Page {
    pub columns: Vec<String>,
    pub rows: Vec<Row>,
    /// `None` when the cluster reported no further pages.
    pub next: Option<PagingState>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Row {
    pub values: Vec<CqlValue>,
}

/// The cluster's opaque cursor. Never interpreted, only handed back.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PagingState(pub Vec<u8>);

impl Page {
    #[must_use]
    pub const fn has_more(&self) -> bool {
        self.next.is_some()
    }
}

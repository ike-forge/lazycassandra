//! One page of a result set, carrying the cursor for the next: nothing assumes
//! the whole result fits in memory.

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

/// Opaque: never interpreted, only handed back.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PagingState(pub Vec<u8>);

impl Page {
    #[must_use]
    pub const fn has_more(&self) -> bool {
        self.next.is_some()
    }
}

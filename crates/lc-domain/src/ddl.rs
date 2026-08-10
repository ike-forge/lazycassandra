//! The Table DDL panel's state: a viewport over the selected node's
//! description, and the filter narrowing it.
//!
//! A module of its own for the reason [`crate::tree`] and [`crate::contents`]
//! are ones — per-panel state with its own vocabulary rather than another
//! field on `State` (architecture.md §11).

use crate::search::Filter;
use crate::state::Scroll;

/// The Table DDL panel's own state.
///
/// It scrolls a viewport rather than a selection: it has no row to act on, so
/// there is nothing for a cursor to mean.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DdlPanel {
    pub scroll: Scroll,
    pub filter: Option<Filter>,
}

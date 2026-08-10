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
    /// Private for the reason [`crate::tree::TablesPanel`]'s filter is:
    /// setting it is going to owe a repair, and a public field would let a
    /// caller skip it (design §2.1, §2.2).
    filter: Option<Filter>,
}

impl DdlPanel {
    /// The filter narrowing the panel's body, if any.
    #[must_use]
    pub fn filter(&self) -> Option<&Filter> {
        self.filter.as_ref()
    }

    /// Sets the filter. Not public: [`State::apply_filter`](crate::state::State::apply_filter)
    /// is the one caller, and the repair a change here owes is its job.
    pub(crate) fn set_filter(&mut self, filter: Option<Filter>) {
        self.filter = filter;
    }
}

//! The Contents panel's state: one table's rows, and how far along asking for
//! them we are.
//!
//! A module of its own for the reason [`crate::tree`] is one — per-panel state
//! with its own vocabulary rather than another field on `State`
//! (architecture.md §11).

use crate::model::{DbError, Page, QueryId};
use crate::search::Filter;
use crate::state::{Cursor, Scroll};

/// The Contents panel's own state.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ContentsPanel {
    pub cursor: Cursor,
    /// The first row drawn (doc/specs/2026-08-09-list-viewport-design.md §1).
    pub scroll: Scroll,
    /// Private for the reason [`crate::tree::TablesPanel`]'s unfold set is:
    /// the only ways in are the transitions below.
    state: ContentsState,
    /// Flags the cells the applied filter matches; the grid keeps every row
    /// (design §4.3). Private for uniformity with the other two panels'
    /// filters, not because setting it here owes a repair — Contents filters
    /// nothing, so there is no clamp to skip.
    filter: Option<Filter>,
}

/// How far along the panel's one request is.
///
/// The [`QueryId`] sits inside `Loading` rather than beside it, so "waiting on
/// nothing in particular" cannot be represented.
#[derive(Debug, Clone, Default, PartialEq)]
pub enum ContentsState {
    #[default]
    Empty,
    Loading {
        query: QueryId,
        table: TableRef,
    },
    Loaded {
        table: TableRef,
        page: Page,
    },
    Failed {
        table: TableRef,
        error: DbError,
    },
}

/// Which table a request was about.
///
/// Owned rather than borrowed from the `Schema`: a result outlives the reload
/// that dropped its table, and a page that says which table it came from is
/// merely stale where a cleared panel would lose the user's place.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TableRef {
    pub keyspace: String,
    pub table: String,
}

impl TableRef {
    #[must_use]
    pub fn new(keyspace: &str, table: &str) -> Self {
        Self {
            keyspace: keyspace.to_owned(),
            table: table.to_owned(),
        }
    }

    /// `keyspace.table`, for a panel title.
    #[must_use]
    pub fn qualified(&self) -> String {
        format!("{}.{}", self.keyspace, self.table)
    }
}

impl ContentsPanel {
    #[must_use]
    pub const fn state(&self) -> &ContentsState {
        &self.state
    }

    /// The filter flagging matching cells, if any. Narrows nothing: every row
    /// the cluster returned is still drawn (design §4.3).
    #[must_use]
    pub fn filter(&self) -> Option<&Filter> {
        self.filter.as_ref()
    }

    /// Sets the filter. Not public, for uniformity with
    /// [`crate::tree::TablesPanel::set_filter`]: [`State::apply_filter`](crate::state::State::apply_filter)
    /// is the one caller, and there is no repair this owes — Contents has no
    /// clamp to keep in step with the filter the way Tables' cursor does.
    pub(crate) fn set_filter(&mut self, filter: Option<Filter>) {
        self.filter = filter;
    }

    /// The query this panel is waiting on, if any.
    #[must_use]
    pub const fn outstanding(&self) -> Option<QueryId> {
        match self.state {
            ContentsState::Loading { query, .. } => Some(query),
            _ => None,
        }
    }

    /// Starts waiting on `query`, discarding whatever was showing.
    pub fn requested(&mut self, query: QueryId, table: TableRef) {
        self.state = ContentsState::Loading { query, table };
        self.cursor = Cursor::default();
        self.scroll = Scroll::default();
    }

    /// Applies a page, if it is the one being waited on.
    pub fn page_ready(&mut self, query: QueryId, page: Page) {
        let Some(table) = self.awaited(query) else {
            return;
        };

        self.state = ContentsState::Loaded { table, page };
    }

    /// Records a request that never became a query, because the domain
    /// refused it before it could.
    pub fn refused(&mut self, table: TableRef, error: DbError) {
        self.state = ContentsState::Failed { table, error };
        self.cursor = Cursor::default();
        self.scroll = Scroll::default();
    }

    /// Records a failure, if it belongs to the query being waited on.
    pub fn failed(&mut self, query: QueryId, error: DbError) {
        let Some(table) = self.awaited(query) else {
            return;
        };

        self.state = ContentsState::Failed { table, error };
    }

    /// The rows on screen, for a cursor that must not run past them.
    #[must_use]
    pub fn row_count(&self) -> usize {
        match &self.state {
            ContentsState::Loaded { page, .. } => page.rows.len(),
            _ => 0,
        }
    }

    /// The table `query` was about, when it is the outstanding one.
    fn awaited(&self, query: QueryId) -> Option<TableRef> {
        match &self.state {
            ContentsState::Loading {
                query: outstanding,
                table,
            } if *outstanding == query => Some(table.clone()),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{CqlValue, Row};

    fn page(rows: i64) -> Page {
        Page {
            columns: vec!["id".to_owned()],
            rows: (0..rows)
                .map(|i| Row {
                    values: vec![CqlValue::Int(i)],
                })
                .collect(),
            next: None,
        }
    }

    fn loading() -> (ContentsPanel, QueryId) {
        let mut panel = ContentsPanel::default();
        let query = QueryId::next();
        panel.requested(query, TableRef::new("shop", "orders"));
        (panel, query)
    }

    #[test]
    fn a_fresh_panel_is_waiting_on_nothing() {
        let panel = ContentsPanel::default();

        assert_eq!(*panel.state(), ContentsState::Empty);
        assert_eq!(panel.outstanding(), None);
        assert_eq!(panel.row_count(), 0);
    }

    #[test]
    fn a_request_records_the_query_it_is_waiting_on() {
        let (panel, query) = loading();

        assert_eq!(panel.outstanding(), Some(query));
    }

    #[test]
    fn the_page_being_waited_on_is_applied() {
        let (mut panel, query) = loading();

        panel.page_ready(query, page(3));

        assert_eq!(panel.row_count(), 3);
        assert_eq!(panel.outstanding(), None, "the wait is over");
    }

    #[test]
    fn a_page_for_another_query_is_not_applied() {
        let (mut panel, query) = loading();

        panel.page_ready(QueryId::next(), page(3));

        assert_eq!(
            panel.outstanding(),
            Some(query),
            "a page belonging to a superseded request must not land here"
        );
        assert_eq!(panel.row_count(), 0);
    }

    #[test]
    fn a_page_arriving_after_the_panel_moved_on_is_not_applied() {
        let (mut panel, first) = loading();
        panel.requested(QueryId::next(), TableRef::new("shop", "users"));

        panel.page_ready(first, page(3));

        assert_eq!(panel.row_count(), 0);
        let ContentsState::Loading { table, .. } = panel.state() else {
            panic!("still waiting on the second request");
        };
        assert_eq!(table.table, "users");
    }

    #[test]
    fn a_failure_for_the_outstanding_query_is_recorded_against_its_table() {
        let (mut panel, query) = loading();

        panel.failed(query, DbError::Timeout);

        let ContentsState::Failed { table, error } = panel.state() else {
            panic!("the failure is showing");
        };
        assert_eq!(table.qualified(), "shop.orders");
        assert_eq!(*error, DbError::Timeout);
    }

    #[test]
    fn a_failure_for_another_query_is_ignored() {
        let (mut panel, query) = loading();

        panel.failed(QueryId::next(), DbError::Timeout);

        assert_eq!(panel.outstanding(), Some(query));
    }

    #[test]
    fn a_new_request_starts_the_cursor_at_the_top() {
        let (mut panel, query) = loading();
        panel.page_ready(query, page(5));
        panel.cursor.down(5);

        let next = QueryId::next();
        panel.requested(next, TableRef::new("shop", "users"));

        assert_eq!(
            panel.cursor.index(),
            0,
            "a different table's first row is not the fourth row of the last one"
        );
    }

    #[test]
    fn a_panel_with_no_page_has_no_row_to_move_onto() {
        let (mut panel, _) = loading();

        panel.cursor.down(panel.row_count());

        assert_eq!(panel.cursor.index(), 0, "a loading panel has no rows");
    }

    #[test]
    fn a_refused_request_shows_against_the_table_it_was_about() {
        let mut panel = ContentsPanel::default();

        panel.refused(TableRef::new("shop", "orders"), DbError::ReadOnly);

        let ContentsState::Failed { table, error } = panel.state() else {
            panic!("the refusal is showing");
        };
        assert_eq!(table.qualified(), "shop.orders");
        assert_eq!(*error, DbError::ReadOnly);
        assert_eq!(
            panel.outstanding(),
            None,
            "nothing was ever asked of the cluster"
        );
    }

    #[test]
    fn a_qualified_name_reads_the_way_cql_writes_it() {
        assert_eq!(TableRef::new("shop", "orders").qualified(), "shop.orders");
    }

    #[test]
    fn a_new_request_opens_at_the_top_of_the_grid() {
        let mut panel = ContentsPanel::default();
        panel.scroll.follow(40, 5);
        panel.cursor.go_to(40, 60);

        panel.requested(QueryId::next(), TableRef::new("shop", "orders"));

        assert_eq!(panel.scroll.offset(), 0, "a second table opens at its top");
        assert_eq!(panel.cursor.index(), 0);
    }

    #[test]
    fn a_refused_request_opens_at_the_top_too() {
        let mut panel = ContentsPanel::default();
        panel.scroll.follow(40, 5);
        panel.cursor.go_to(40, 60);

        panel.refused(TableRef::new("shop", "orders"), DbError::ReadOnly);

        assert_eq!(panel.scroll.offset(), 0);
        assert_eq!(panel.cursor.index(), 0);
    }
}

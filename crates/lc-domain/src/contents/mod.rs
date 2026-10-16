//! The Contents panel's state: one table's rows, and how far along fetching
//! them is.

mod filter;
mod grid_page;
mod paging;
mod subject;

pub use filter::Filter;
pub use grid_page::{GridPage, Order, Sort};
pub use paging::{Continue, Failure, Fetch, Landed, MEMORY_STEP, Paging, ROW_STEP, Stopped, Stops};
pub use subject::{Subject, TableRef};

use crate::grid::ColumnScroll;
use crate::inspect::Inspector;
use crate::model::{DbError, QueryId, Statement};
use crate::search::{Direction, Search};
use crate::selection::Selection;
use crate::viewport::{Cursor, Scroll};

#[derive(Debug, Clone, Default, PartialEq)]
pub struct ContentsPanel {
    pub cursor: Cursor,
    pub column: Cursor,
    pub mode: SelectionMode,
    pub scroll: Scroll,
    pub column_scroll: ColumnScroll,
    pub rows: Selection,
    pub columns: Selection,
    /// Private: the transitions below are the only ways in.
    state: ContentsState,
    search: Option<Search>,
    /// The insert form of a write the cluster refused, for `i` to give back.
    draft: Option<Inspector>,
}

/// Changes only the highlight: both cursors move on the same keys in every
/// mode.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum SelectionMode {
    #[default]
    Row,
    Cell,
    Column,
}

impl SelectionMode {
    #[must_use]
    pub const fn next(self) -> Self {
        match self {
            Self::Cell => Self::Row,
            Self::Row => Self::Column,
            Self::Column => Self::Cell,
        }
    }
}

/// The [`QueryId`] sits inside `Loading`, so "waiting on nothing" is
/// unrepresentable.
#[derive(Debug, Clone, Default, PartialEq)]
pub enum ContentsState {
    #[default]
    Empty,
    Loading {
        query: QueryId,
        subject: Subject,
        /// Kept to resume from: a paging cursor alone names nothing the driver
        /// can continue.
        stmt: Statement,
    },
    Loaded {
        subject: Subject,
        page: Box<GridPage>,
        paging: Paging,
    },
    Failed {
        subject: Subject,
        /// Drawn in the panel: the fix belongs where the failure is.
        error: String,
    },
}

impl ContentsPanel {
    #[must_use]
    pub const fn state(&self) -> &ContentsState {
        &self.state
    }

    /// Flags matching cells; every row is still drawn.
    #[must_use]
    pub fn search(&self) -> Option<&Search> {
        self.search.as_ref()
    }

    pub(crate) fn set_search(&mut self, search: Option<Search>) {
        self.search = search;
    }

    /// Row-major. Both cursors move, so the cell cursor lands on the match.
    #[must_use]
    pub fn next_match(&self, from: (usize, usize), direction: Direction) -> Option<(usize, usize)> {
        let search = self.search.as_ref()?;
        let page = self.loaded()?;
        let columns = page.page().columns.len();
        // Over the arrangement: a row a filter dropped is not a match.
        let len = page.len().checked_mul(columns)?;

        if len == 0 {
            return None;
        }

        let from = from.0 * columns + from.1;

        (0..len)
            .scan(from, |at, _| {
                *at = direction.step(*at, len);
                Some(*at)
            })
            .find(|at| {
                let (row, column) = (at / columns, at % columns);
                page.value(row, column)
                    .is_some_and(|value| search.matches(&crate::cell::text(value)))
            })
            .map(|at| (at / columns, at % columns))
    }

    pub fn requested(&mut self, query: QueryId, subject: Subject, stmt: Statement) {
        // What was typed addressed the old content's rows.
        self.draft = None;
        self.state = ContentsState::Loading {
            query,
            subject,
            stmt,
        };
        self.go_home();
    }

    fn go_home(&mut self) {
        self.cursor = Cursor::default();
        self.column = Cursor::default();
        self.scroll = Scroll::default();
        self.column_scroll = ColumnScroll::default();
        self.rows = Selection::default();
        self.columns = Selection::default();
    }

    pub fn keep_draft(&mut self, form: Inspector) {
        self.draft = Some(form);
    }

    /// Whoever opens it puts it back if the write fails again.
    pub fn take_draft(&mut self) -> Option<Inspector> {
        self.draft.take()
    }

    /// A request the domain refused before it became a query.
    pub fn refused(&mut self, subject: Subject, error: &DbError) {
        self.state = ContentsState::Failed {
            subject,
            error: error.to_string(),
        };
        self.go_home();
    }

    pub fn failed(&mut self, query: QueryId, error: &DbError) -> Option<Failure> {
        if self.outstanding() != Some(query) {
            return None;
        }

        match &mut self.state {
            ContentsState::Loading { subject, .. } => {
                self.state = ContentsState::Failed {
                    subject: subject.clone(),
                    error: error.to_string(),
                };
                Some(Failure::Read)
            }
            ContentsState::Loaded {
                paging: Paging::More { fetch, .. },
                ..
            } => {
                *fetch = Fetch::Idle;
                Some(Failure::Page)
            }
            _ => None,
        }
    }

    /// What the arrangement left, not what the cluster returned.
    #[must_use]
    pub fn row_count(&self) -> usize {
        self.loaded().map_or(0, GridPage::len)
    }

    #[must_use]
    pub const fn loaded(&self) -> Option<&GridPage> {
        match &self.state {
            ContentsState::Loaded { page, .. } => Some(page),
            _ => None,
        }
    }

    pub const fn loaded_mut(&mut self) -> Option<&mut GridPage> {
        match &mut self.state {
            ContentsState::Loaded { page, .. } => Some(page),
            _ => None,
        }
    }

    /// Drops `v`'s ranges: they name rows by drawn position.
    pub fn cycle_sort(&mut self, column: usize) {
        if let Some(page) = self.loaded_mut() {
            page.cycle_sort(column);
            self.rows.clear();
        }
    }

    /// Drops `v`'s ranges, as [`ContentsPanel::cycle_sort`] does.
    pub fn set_filter(&mut self, column: usize, filter: Option<Search>) {
        if let Some(page) = self.loaded_mut() {
            page.set_filter(column, filter);
            self.rows.clear();
        }
    }

    #[must_use]
    pub fn filter(&self, column: usize) -> Option<&Search> {
        self.loaded()?.filter(column)
    }

    #[must_use]
    pub fn column_count(&self) -> usize {
        self.loaded().map_or(0, |page| page.page().columns.len())
    }
}

#[cfg(test)]
pub(crate) mod tests_support {
    use super::*;
    use crate::config::MaxColumnWidth;
    use crate::model::{CqlValue, Page, PagingState, Row};

    pub(crate) fn rejection() -> DbError {
        DbError::Invalid("undefined column name total".to_owned())
    }

    pub(crate) fn page(rows: i64) -> Page {
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

    pub(crate) fn more(rows: i64) -> Page {
        Page {
            next: Some(PagingState(vec![1])),
            ..page(rows)
        }
    }

    pub(crate) fn loading() -> (ContentsPanel, QueryId) {
        let mut panel = ContentsPanel::default();
        let query = QueryId::next();
        panel.requested(
            query,
            Subject::Table(TableRef::new("shop", "orders")),
            Statement::typed("SELECT * FROM shop.orders"),
        );
        (panel, query)
    }

    pub(crate) fn pageable(rows: i64) -> ContentsPanel {
        let (mut panel, query) = loading();
        panel.page_ready(query, more(rows), MaxColumnWidth::default(), QueryId::next);
        panel
    }

    pub(crate) fn land(panel: &mut ContentsPanel, page: Page) -> Landed {
        let query = panel.outstanding().expect("a page is outstanding");
        panel.page_ready(query, page, MaxColumnWidth::default(), QueryId::next)
    }
}

#[cfg(test)]
mod tests {
    use super::tests_support::*;
    use super::*;
    use crate::config::MaxColumnWidth;
    use crate::model::{CqlValue, Page, Row};

    fn wide_page(columns: usize) -> Page {
        Page {
            columns: (0..columns).map(|i| format!("c{i}")).collect(),
            rows: vec![Row {
                values: (0..columns)
                    .map(|i| CqlValue::Int(i64::try_from(i).expect("the fixture is small")))
                    .collect(),
            }],
            next: None,
        }
    }

    /// `(0, 0)` does not match `"ali"`, so searches from it and from the last
    /// match compare equal.
    fn loaded_panel() -> ContentsPanel {
        let (mut panel, query) = loading();
        panel.page_ready(
            query,
            Page {
                columns: vec!["a".to_owned(), "b".to_owned()],
                rows: vec![
                    Row {
                        values: vec![
                            CqlValue::Text("Bob".to_owned()),
                            CqlValue::Text("Ali".to_owned()),
                        ],
                    },
                    Row {
                        values: vec![
                            CqlValue::Text("Carl".to_owned()),
                            CqlValue::Text("Dana".to_owned()),
                        ],
                    },
                    Row {
                        values: vec![
                            CqlValue::Text("Eve".to_owned()),
                            CqlValue::Text("Alison".to_owned()),
                        ],
                    },
                ],
                next: None,
            },
            MaxColumnWidth::default(),
            QueryId::next,
        );
        panel
    }

    #[test]
    fn the_next_match_is_the_next_cell_row_major() {
        let mut panel = loaded_panel();
        panel.set_search(Search::new("ali"));

        assert_eq!(
            panel.next_match((0, 0), Direction::Forwards),
            Some((0, 1)),
            "across the row before down it"
        );
    }

    #[test]
    fn the_next_match_wraps_past_the_last_row() {
        let mut panel = loaded_panel();
        panel.set_search(Search::new("ali"));
        let last = panel
            .next_match((0, 0), Direction::Backwards)
            .expect("a match");

        assert_eq!(
            panel.next_match(last, Direction::Forwards),
            panel.next_match((0, 0), Direction::Forwards),
            "forward from the last match is the first one again"
        );
    }

    #[test]
    fn no_search_applied_is_none() {
        let panel = loaded_panel();

        assert_eq!(panel.next_match((0, 0), Direction::Forwards), None);
    }

    #[test]
    fn a_search_matching_nothing_is_none() {
        let mut panel = loaded_panel();
        panel.set_search(Search::new("zzz"));

        assert_eq!(panel.next_match((0, 0), Direction::Forwards), None);
    }

    #[test]
    fn an_empty_panel_has_no_match_to_reach() {
        let mut panel = ContentsPanel::default();
        panel.set_search(Search::new("ali"));

        assert_eq!(panel.next_match((0, 0), Direction::Forwards), None);
    }

    #[test]
    fn a_loading_panel_has_no_page_to_search() {
        let (mut panel, _) = loading();
        panel.set_search(Search::new("ali"));

        assert_eq!(panel.next_match((0, 0), Direction::Forwards), None);
    }

    #[test]
    fn a_failed_panel_has_no_page_to_search() {
        let (mut panel, query) = loading();
        panel.failed(query, &rejection());
        panel.set_search(Search::new("ali"));

        assert_eq!(panel.next_match((0, 0), Direction::Forwards), None);
    }

    #[test]
    fn a_page_with_no_rows_has_no_match_to_reach() {
        let (mut panel, query) = loading();
        panel.page_ready(query, page(0), MaxColumnWidth::default(), QueryId::next);
        panel.set_search(Search::new("ali"));

        assert_eq!(panel.next_match((0, 0), Direction::Forwards), None);
    }

    #[test]
    fn a_page_with_no_columns_has_no_match_to_reach() {
        let (mut panel, query) = loading();
        panel.page_ready(
            query,
            Page {
                columns: Vec::new(),
                rows: vec![Row { values: Vec::new() }],
                next: None,
            },
            MaxColumnWidth::default(),
            QueryId::next,
        );
        panel.set_search(Search::new("ali"));

        assert_eq!(panel.next_match((0, 0), Direction::Forwards), None);
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

        panel.page_ready(query, page(3), MaxColumnWidth::default(), QueryId::next);

        assert_eq!(panel.row_count(), 3);
        assert_eq!(panel.outstanding(), None, "the wait is over");
    }

    #[test]
    fn a_page_for_another_query_is_not_applied() {
        let (mut panel, query) = loading();

        panel.page_ready(
            QueryId::next(),
            page(3),
            MaxColumnWidth::default(),
            QueryId::next,
        );

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
        panel.requested(
            QueryId::next(),
            Subject::Table(TableRef::new("shop", "users")),
            Statement::typed("SELECT * FROM shop.orders"),
        );

        panel.page_ready(first, page(3), MaxColumnWidth::default(), QueryId::next);

        assert_eq!(panel.row_count(), 0);
        let ContentsState::Loading { subject, .. } = panel.state() else {
            panic!("still waiting on the second request");
        };
        assert_eq!(subject.table(), Some(&TableRef::new("shop", "users")));
    }

    #[test]
    fn a_failure_for_the_outstanding_query_is_recorded_against_its_table() {
        let (mut panel, query) = loading();

        panel.failed(query, &rejection());

        let ContentsState::Failed { subject, error } = panel.state() else {
            panic!("the failure is showing");
        };
        assert_eq!(subject.label(), "shop.orders");
        assert_eq!(error, &rejection().to_string());
    }

    #[test]
    fn a_failure_for_another_query_is_ignored() {
        let (mut panel, query) = loading();

        panel.failed(QueryId::next(), &rejection());

        assert_eq!(panel.outstanding(), Some(query));
    }

    #[test]
    fn a_new_request_starts_the_cursor_at_the_top() {
        let (mut panel, query) = loading();
        panel.page_ready(query, page(5), MaxColumnWidth::default(), QueryId::next);
        panel.cursor.down(5);

        let next = QueryId::next();
        panel.requested(
            next,
            Subject::Table(TableRef::new("shop", "users")),
            Statement::typed("SELECT * FROM shop.orders"),
        );

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

        panel.refused(
            Subject::Table(TableRef::new("shop", "orders")),
            &rejection(),
        );

        let ContentsState::Failed { subject, error } = panel.state() else {
            panic!("the refusal is showing");
        };
        assert_eq!(subject.label(), "shop.orders");
        assert_eq!(error, &rejection().to_string());
        assert_eq!(
            panel.outstanding(),
            None,
            "nothing was ever asked of the cluster"
        );
    }

    #[test]
    fn a_new_request_opens_at_the_top_of_the_grid() {
        let mut panel = ContentsPanel::default();
        panel.scroll.follow(40, 5);
        panel.cursor.go_to(40, 60);

        panel.requested(
            QueryId::next(),
            Subject::Table(TableRef::new("shop", "orders")),
            Statement::typed("SELECT * FROM shop.orders"),
        );

        assert_eq!(panel.scroll.offset(), 0, "a second table opens at its top");
        assert_eq!(panel.cursor.index(), 0);
    }

    #[test]
    fn a_refused_request_opens_at_the_top_too() {
        let mut panel = ContentsPanel::default();
        panel.scroll.follow(40, 5);
        panel.cursor.go_to(40, 60);

        panel.refused(
            Subject::Table(TableRef::new("shop", "orders")),
            &rejection(),
        );

        assert_eq!(panel.scroll.offset(), 0);
        assert_eq!(panel.cursor.index(), 0);
    }

    #[test]
    fn a_typed_subject_travels_the_same_road_a_table_does() {
        let mut panel = ContentsPanel::default();
        let query = QueryId::next();
        panel.requested(
            query,
            Subject::Typed("SELECT 1".to_owned()),
            Statement::typed("SELECT * FROM shop.orders"),
        );

        panel.page_ready(query, page(3), MaxColumnWidth::default(), QueryId::next);

        assert_eq!(panel.row_count(), 3);
        assert_eq!(panel.outstanding(), None, "the wait is over");
    }

    #[test]
    fn a_fresh_panel_has_no_columns_to_move_over() {
        let panel = ContentsPanel::default();

        assert_eq!(panel.column_count(), 0);
        assert_eq!(panel.column.index(), 0);
        assert_eq!(panel.column_scroll.first(), 0);
    }

    #[test]
    fn a_page_arrives_with_its_column_widths_already_measured() {
        let (mut panel, query) = loading();

        panel.page_ready(
            query,
            wide_page(3),
            MaxColumnWidth::default(),
            QueryId::next,
        );

        let ContentsState::Loaded { page, .. } = panel.state() else {
            panic!("the page landed");
        };
        assert_eq!(
            page.widths(),
            crate::grid::widths(page.page(), MaxColumnWidth::default()),
            "measured once beside the page, not once per tick"
        );
    }

    #[test]
    fn a_loaded_page_reports_its_columns() {
        let (mut panel, query) = loading();

        panel.page_ready(
            query,
            wide_page(4),
            MaxColumnWidth::default(),
            QueryId::next,
        );

        assert_eq!(panel.column_count(), 4);
    }

    #[test]
    fn a_request_returns_the_cell_cursor_to_the_first_cell() {
        let (mut panel, query) = loading();
        panel.page_ready(
            query,
            wide_page(4),
            MaxColumnWidth::default(),
            QueryId::next,
        );
        panel.cursor.go_to(0, 1);
        panel.column.go_to(3, 4);
        panel.column_scroll.follow(3, &[1, 1, 1, 1], 4);

        panel.requested(
            QueryId::next(),
            Subject::Table(TableRef::new("shop", "carts")),
            Statement::typed("SELECT * FROM shop.orders"),
        );

        assert_eq!(panel.column.index(), 0);
        assert_eq!(panel.column_scroll.first(), 0);
    }

    #[test]
    fn a_refusal_returns_the_cell_cursor_to_the_first_cell() {
        let (mut panel, query) = loading();
        panel.page_ready(
            query,
            wide_page(4),
            MaxColumnWidth::default(),
            QueryId::next,
        );
        panel.column.go_to(3, 4);

        panel.refused(Subject::Table(TableRef::new("shop", "carts")), &rejection());

        assert_eq!(panel.column.index(), 0);
        assert_eq!(panel.column_scroll.first(), 0);
    }

    #[test]
    fn a_fresh_panel_selects_a_row() {
        assert_eq!(ContentsPanel::default().mode, SelectionMode::Row);
    }

    #[test]
    fn the_mode_cycles_row_column_cell_and_wraps() {
        let mut mode = SelectionMode::Row;

        mode = mode.next();
        assert_eq!(mode, SelectionMode::Column);
        mode = mode.next();
        assert_eq!(mode, SelectionMode::Cell);
        mode = mode.next();
        assert_eq!(mode, SelectionMode::Row, "the third press comes home");
    }

    #[test]
    fn a_new_request_keeps_the_mode_and_clears_the_cursors() {
        let mut panel = ContentsPanel {
            mode: SelectionMode::Column,
            ..Default::default()
        };
        panel.cursor.go_to(3, 10);
        panel.column.go_to(2, 5);

        panel.requested(
            QueryId::next(),
            Subject::Table(TableRef::new("shop", "orders")),
            Statement::typed("SELECT * FROM shop.orders"),
        );

        assert_eq!(panel.mode, SelectionMode::Column);
        assert_eq!(panel.cursor.index(), 0);
        assert_eq!(panel.column.index(), 0);
    }

    #[test]
    fn a_new_request_clears_both_ranges() {
        let mut panel = ContentsPanel::default();
        panel.rows.start(0);
        panel.rows.stop(3);
        panel.columns.start(1);

        panel.requested(
            QueryId::next(),
            Subject::Typed("select 1".to_owned()),
            Statement::typed("SELECT * FROM shop.orders"),
        );

        assert!(!panel.rows.is_active());
        assert!(!panel.columns.is_active());
    }
}

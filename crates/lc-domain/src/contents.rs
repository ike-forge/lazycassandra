//! The Contents panel's state: one table's rows, and how far along asking for
//! them we are.
//!
//! A module of its own for the reason [`crate::tree`] is one — per-panel state
//! with its own vocabulary rather than another field on `State`
//! (architecture.md §11).

use crate::grid::ColumnScroll;
use crate::model::{KeyspaceKind, Page, QueryId, Row};
use crate::search::{Direction, Search};
use crate::state::{Cursor, Scroll, Selection};
use crate::style::{Segment, Token};

/// The Contents panel's own state.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ContentsPanel {
    pub cursor: Cursor,
    /// The selected column. With [`ContentsPanel::cursor`] it names one cell,
    /// which is what a future delete or edit acts on
    /// (docs/superpowers/specs/2026-08-10-table-display-design.md §2).
    pub column: Cursor,
    /// How the two cursors above are drawn
    /// (docs/superpowers/specs/2026-08-13-selection-modes-design.md §2).
    pub mode: SelectionMode,
    /// The first row drawn (docs/superpowers/specs/2026-08-09-list-viewport-design.md §1).
    pub scroll: Scroll,
    /// The first column drawn (docs/superpowers/specs/2026-08-10-table-display-design.md §5).
    pub column_scroll: ColumnScroll,
    /// The rows `v` has selected, and the columns — which of the two `v`
    /// extends is what [`SelectionMode`] answers
    /// (docs/superpowers/specs/2026-08-14-visual-mode-design.md §3).
    pub rows: Selection,
    pub columns: Selection,
    /// Private for the reason [`crate::tree::TablesPanel`]'s unfold set is:
    /// the only ways in are the transitions below.
    state: ContentsState,
    /// Flags the cells the applied search matches; the grid keeps every row
    /// (design §4.3). Private for uniformity with the other two panels'
    /// searches, not because setting it here owes a repair — Contents narrows
    /// nothing, so there is no clamp to skip.
    search: Option<Search>,
}

/// Which of the grid's two cursors is painted, and how.
///
/// Changes nothing but the highlight: both cursors move on the same keys in
/// every mode, so no key changes meaning and docs/shortcuts.md §2 stands
/// (design §3).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum SelectionMode {
    /// The whole row. The default: reading a table is reading rows, and a cell
    /// cursor is what you cycle to once you have a cell in mind (design §2).
    #[default]
    Row,
    /// The cell, over a faint wash across its row and its column.
    Cell,
    Column,
}

impl SelectionMode {
    /// The next mode `m` reaches, wrapping.
    #[must_use]
    pub const fn next(self) -> Self {
        match self {
            Self::Cell => Self::Row,
            Self::Row => Self::Column,
            Self::Column => Self::Cell,
        }
    }
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
        subject: Subject,
    },
    Loaded {
        subject: Subject,
        page: GridPage,
    },
    Failed {
        subject: Subject,
    },
}

/// Which way a sorted column is read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Order {
    Descending,
    Ascending,
}

/// The column a grid is sorted on, and which way.
///
/// One column at a time: sorting a second replaces the first rather than
/// adding a tiebreak, so what the arrow over a heading claims is the whole
/// order and not a term in it
/// (docs/superpowers/specs/2026-08-31-grid-sort-filter-design.md §2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Sort {
    pub column: usize,
    pub order: Order,
}

impl Sort {
    /// The state `s` on `column` reaches when the sort key is pressed there:
    /// unsorted, descending, ascending, unsorted (design §2). Landing on
    /// another column starts that column's own cycle from the top.
    #[must_use]
    fn next(sort: Option<Self>, column: usize) -> Option<Self> {
        let order = match sort {
            Some(Self { column: at, order }) if at == column => match order {
                Order::Descending => Order::Ascending,
                Order::Ascending => return None,
            },
            _ => Order::Descending,
        };

        Some(Self { column, order })
    }
}

/// A page, the column widths measured from it, and the arrangement the user
/// is reading it through.
///
/// One type rather than two fields because measuring is expensive and the
/// answer never changes: [`crate::grid::widths`] runs [`crate::cell::text`]
/// over every cell, which hex-expands a whole blob to 2n characters purely to
/// count them. `update`'s epilogue and every drawn frame each wanted that
/// answer, so an idle tick over a page of blobs allocated megabytes to reach
/// the conclusion that nothing had moved — the cost
/// [`crate::state::Loader`]'s repaint rule exists to avoid.
///
/// The widths are a pure function of the page and are built with it, so the
/// cache has no way to go stale. That is why there is no public constructor
/// taking both. They are measured over the whole page and not over
/// [`GridPage::order`]: a filter that narrowed the columns it left standing
/// would resize the grid under every keystroke of the prompt that typed it
/// (design §5).
///
/// The sort and the filters live here rather than on [`ContentsPanel`] so
/// that they cannot outlive the page they address: a filter on column 3 says
/// nothing about the next statement's third column, and a new page replaces
/// this whole value (design §3).
#[derive(Debug, Clone, PartialEq)]
pub struct GridPage {
    page: Page,
    widths: Vec<u16>,
    sort: Option<Sort>,
    /// One slot per column of the page, so a filter is addressed by the
    /// column index the grid already counts in and no lookup can name a
    /// column the page does not have.
    filters: Vec<Option<Search>>,
    /// Which rows of [`GridPage::page`] are drawn, in the order they are
    /// drawn. The one place the arrangement is resolved: every reader counts
    /// rows in this, so a cursor, a yank and an export cannot disagree about
    /// which row is the third one (design §4).
    order: Vec<usize>,
}

impl GridPage {
    fn new(page: Page) -> Self {
        let widths = crate::grid::widths(&page);
        let filters = vec![None; page.columns.len()];
        let order = (0..page.rows.len()).collect();

        Self {
            page,
            widths,
            sort: None,
            filters,
            order,
        }
    }

    #[must_use]
    pub const fn page(&self) -> &Page {
        &self.page
    }

    /// Each column's natural width, measured once when the page landed.
    #[must_use]
    pub fn widths(&self) -> &[u16] {
        &self.widths
    }

    /// The column the grid is sorted on, if any.
    #[must_use]
    pub const fn sort(&self) -> Option<Sort> {
        self.sort
    }

    /// The filter standing on `column`, if any.
    #[must_use]
    pub fn filter(&self, column: usize) -> Option<&Search> {
        self.filters.get(column)?.as_ref()
    }

    /// How many rows the arrangement leaves — what a cursor is bounded by,
    /// and what the title counts.
    #[must_use]
    pub fn len(&self) -> usize {
        self.order.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.order.is_empty()
    }

    /// Whether anything was filtered out, which is what lets the title say
    /// how much of the page it is showing.
    #[must_use]
    pub fn is_filtered(&self) -> bool {
        self.filters.iter().any(Option::is_some)
    }

    /// The rows the arrangement leaves, in the order they are drawn.
    #[must_use]
    pub fn rows(&self) -> impl ExactSizeIterator<Item = &Row> {
        self.order.iter().map(|at| &self.page.rows[*at])
    }

    /// The `index`th drawn row, counted the way the cursor counts.
    #[must_use]
    pub fn row(&self, index: usize) -> Option<&Row> {
        self.page.rows.get(*self.order.get(index)?)
    }

    /// Steps `column` through the sort cycle (design §2).
    pub fn cycle_sort(&mut self, column: usize) {
        if column >= self.page.columns.len() {
            return;
        }

        self.sort = Sort::next(self.sort, column);
        self.arrange();
    }

    /// Applies `filter` to `column`, `None` clearing it.
    pub fn set_filter(&mut self, column: usize, filter: Option<Search>) {
        let Some(slot) = self.filters.get_mut(column) else {
            return;
        };

        *slot = filter;
        self.arrange();
    }

    /// Rebuilds [`GridPage::order`]: every row that satisfies every filter,
    /// in the sorted order.
    ///
    /// Sorted by the text the grid draws rather than by the value's own type,
    /// which is what "lexicographic" means here: a column holds one CQL type,
    /// so an order over the drawn text is an order the user can read off the
    /// screen. The sort is stable, so rows the key cannot separate keep the
    /// order the cluster returned them in (design §2).
    fn arrange(&mut self) {
        let mut order: Vec<usize> = (0..self.page.rows.len())
            .filter(|at| self.kept(*at))
            .collect();

        if let Some(sort) = self.sort {
            let keys: Vec<String> = order
                .iter()
                .map(|at| {
                    self.page.rows[*at]
                        .values
                        .get(sort.column)
                        .map(crate::cell::text)
                        .unwrap_or_default()
                })
                .collect();
            // Sorted alongside its keys rather than by a comparator that
            // re-renders a cell on every comparison: `cell::text` allocates,
            // and a blob column would hex-expand once per comparison.
            let mut paired: Vec<(String, usize)> = keys.into_iter().zip(order).collect();
            paired.sort_by(|(left, _), (right, _)| match sort.order {
                Order::Ascending => left.cmp(right),
                Order::Descending => right.cmp(left),
            });
            order = paired.into_iter().map(|(_, at)| at).collect();
        }

        self.order = order;
    }

    /// Whether row `at` satisfies every filter standing. Several filters are
    /// conjunctive: each one narrows what the ones before it left (design §3).
    fn kept(&self, at: usize) -> bool {
        let row = &self.page.rows[at];

        self.filters
            .iter()
            .enumerate()
            .filter_map(|(column, filter)| Some((column, filter.as_ref()?)))
            .all(|(column, filter)| {
                row.values
                    .get(column)
                    .is_some_and(|value| filter.matches(&crate::cell::text(value)))
            })
    }
}

/// An open filter prompt: the column it will narrow, and the term typed so
/// far.
///
/// A modal of its own rather than a second [`crate::search::Prompt`]: a
/// search flags cells and keeps every row, and this drops the rows it does
/// not match — the two mean different things and must not share a key or a
/// type (design §3). The term is applied when the prompt closes rather than
/// on each keystroke, because a hard filter re-seats the cursor and rebuilds
/// the grid under the hands typing it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Filter {
    column: usize,
    /// The column's name, for the modal's own title. Captured at open: the
    /// prompt claims every key, so the page underneath cannot change while it
    /// is up.
    name: String,
    query: String,
}

impl Filter {
    /// Opens over `column`, seeded with the filter already standing there —
    /// which is what makes deleting the text the way to clear one (design §3).
    #[must_use]
    pub fn opened(column: usize, name: &str, standing: Option<&Search>) -> Self {
        Self {
            column,
            name: name.to_owned(),
            query: standing.map(Search::as_str).unwrap_or_default().to_owned(),
        }
    }

    #[must_use]
    pub const fn column(&self) -> usize {
        self.column
    }

    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    #[must_use]
    pub fn query(&self) -> &str {
        &self.query
    }

    /// What the query now means: `None` for an empty one, which clears the
    /// filter rather than matching everything — [`Search::new`]'s own
    /// invariant, and the reason this reuses it.
    #[must_use]
    pub fn search(&self) -> Option<Search> {
        Search::new(&self.query)
    }

    pub fn push(&mut self, character: char) {
        self.query.push(character);
    }

    pub fn pop(&mut self) {
        self.query.pop();
    }
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

    /// [`TableRef::qualified`] in the colours the tree spends on the two
    /// names, with the dot faint between them. A system keyspace is faint on
    /// both halves, exactly as its rows are (`view::tables`) — a title is one
    /// more place a schema name is written, and it reads as one there too
    /// (vision.md §2).
    #[must_use]
    pub fn segments(&self) -> Vec<Segment> {
        let (keyspace, table) = match KeyspaceKind::of(&self.keyspace) {
            KeyspaceKind::System => (Token::Faint, Token::Faint),
            KeyspaceKind::User => (Token::Keyspace, Token::Table),
        };

        vec![
            Segment::styled(self.keyspace.clone(), keyspace),
            Segment::styled(".", Token::Faint),
            Segment::styled(self.table.clone(), table),
        ]
    }
}

/// What a page in the Contents panel is about.
///
/// The panel shows one grid whichever way the request was made — the tree's
/// `Enter`, or `ctrl-r` over the editor
/// (docs/superpowers/specs/2026-08-10-query-execution-design.md §6).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Subject {
    Table(TableRef),
    Typed(String),
}

impl Subject {
    /// What the panel's title says it is showing.
    ///
    /// Every run of whitespace in a typed statement collapses to one space,
    /// newlines included, so a buffer written over four lines still titles the
    /// panel in one. Clipping to the frame is the renderer's job.
    #[must_use]
    pub fn label(&self) -> String {
        match self {
            Self::Table(table) => table.qualified(),
            Self::Typed(cql) => cql.split_whitespace().collect::<Vec<_>>().join(" "),
        }
    }

    /// The table whose layout a page's columns can be checked against —
    /// `None` for a typed statement, whose columns belong to no one table.
    #[must_use]
    pub const fn table(&self) -> Option<&TableRef> {
        match self {
            Self::Table(table) => Some(table),
            Self::Typed(_) => None,
        }
    }
}

impl ContentsPanel {
    #[must_use]
    pub const fn state(&self) -> &ContentsState {
        &self.state
    }

    /// The search flagging matching cells, if any. Narrows nothing: every row
    /// the cluster returned is still drawn (design §4.3).
    #[must_use]
    pub fn search(&self) -> Option<&Search> {
        self.search.as_ref()
    }

    /// Sets the search. Not public, for uniformity with
    /// [`crate::tree::TablesPanel::set_search`]: [`State::apply_search`](crate::state::State::apply_search)
    /// is the one caller, and there is no repair this owes — Contents has no
    /// clamp to keep in step with the search the way Tables' cursor does.
    pub(crate) fn set_search(&mut self, search: Option<Search>) {
        self.search = search;
    }

    /// The cell `n` reaches from `from`, as `(row, column)`, wrapping.
    ///
    /// Row-major: across the row first, then down. Both cursors move, so the
    /// cell cursor lands on the cell that matched — which is the reason the
    /// panel has two (design §5.1).
    #[must_use]
    pub fn next_match(&self, from: (usize, usize), direction: Direction) -> Option<(usize, usize)> {
        let search = self.search.as_ref()?;
        let ContentsState::Loaded { page, .. } = &self.state else {
            return None;
        };
        let columns = page.page().columns.len();
        // Over the arrangement rather than the page: `n` walks the grid as it
        // is drawn, so a row a filter dropped is not a match to land on
        // (grid sort and filter design §4).
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
                page.row(row)
                    .and_then(|row| row.values.get(column))
                    .is_some_and(|value| search.matches(&crate::cell::text(value)))
            })
            .map(|at| (at / columns, at % columns))
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
    pub fn requested(&mut self, query: QueryId, subject: Subject) {
        self.state = ContentsState::Loading { query, subject };
        self.cursor = Cursor::default();
        self.column = Cursor::default();
        self.scroll = Scroll::default();
        self.column_scroll = ColumnScroll::default();
        self.rows = Selection::default();
        self.columns = Selection::default();
    }

    /// Applies a page, if it is the one being waited on.
    pub fn page_ready(&mut self, query: QueryId, page: Page) {
        let Some(subject) = self.awaited(query) else {
            return;
        };

        self.state = ContentsState::Loaded {
            subject,
            page: GridPage::new(page),
        };
    }

    /// Records a request that never became a query, because the domain
    /// refused it before it could.
    pub fn refused(&mut self, subject: Subject) {
        self.state = ContentsState::Failed { subject };
        self.cursor = Cursor::default();
        self.column = Cursor::default();
        self.scroll = Scroll::default();
        self.column_scroll = ColumnScroll::default();
        self.rows = Selection::default();
        self.columns = Selection::default();
    }

    /// Records a failure, if it belongs to the query being waited on.
    pub fn failed(&mut self, query: QueryId) {
        let Some(subject) = self.awaited(query) else {
            return;
        };

        self.state = ContentsState::Failed { subject };
    }

    /// The rows on screen, for a cursor that must not run past them — what
    /// the arrangement left, not what the cluster returned
    /// (grid sort and filter design §4).
    #[must_use]
    pub fn row_count(&self) -> usize {
        match &self.state {
            ContentsState::Loaded { page, .. } => page.len(),
            _ => 0,
        }
    }

    /// The loaded page, for the readers that count rows in the order it is
    /// drawn: the projection, a yank, an export.
    #[must_use]
    pub const fn loaded(&self) -> Option<&GridPage> {
        match &self.state {
            ContentsState::Loaded { page, .. } => Some(page),
            _ => None,
        }
    }

    /// Steps `column` through the sort cycle, dropping the ranges `v` held.
    ///
    /// A range names rows by where they are drawn, so carrying one across a
    /// re-ordering would address rows the user never chose — the argument `m`
    /// already makes for the ranges it drops
    /// (grid sort and filter design §4).
    pub fn cycle_sort(&mut self, column: usize) {
        if let ContentsState::Loaded { page, .. } = &mut self.state {
            page.cycle_sort(column);
            self.rows.clear();
        }
    }

    /// Applies `filter` to `column`, `None` clearing it. Drops the ranges `v`
    /// held, for [`ContentsPanel::cycle_sort`]'s reason.
    pub fn set_filter(&mut self, column: usize, filter: Option<Search>) {
        if let ContentsState::Loaded { page, .. } = &mut self.state {
            page.set_filter(column, filter);
            self.rows.clear();
        }
    }

    /// The filter standing on `column`, if any.
    #[must_use]
    pub fn filter(&self, column: usize) -> Option<&Search> {
        self.loaded()?.filter(column)
    }

    /// The columns on screen, for a cursor that must not run past them.
    #[must_use]
    pub fn column_count(&self) -> usize {
        match &self.state {
            ContentsState::Loaded { page, .. } => page.page().columns.len(),
            _ => 0,
        }
    }

    /// What `query` was about, when it is the outstanding one.
    fn awaited(&self, query: QueryId) -> Option<Subject> {
        match &self.state {
            ContentsState::Loading {
                query: outstanding,
                subject,
            } if *outstanding == query => Some(subject.clone()),
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

    fn loading() -> (ContentsPanel, QueryId) {
        let mut panel = ContentsPanel::default();
        let query = QueryId::next();
        panel.requested(query, Subject::Table(TableRef::new("shop", "orders")));
        (panel, query)
    }

    /// Two columns, three rows, with matches for `"ali"` at `(0, 1)` and
    /// `(2, 1)` only — `(0, 0)` deliberately does not match, so a forward
    /// search from it and a forward search from the last match can be
    /// compared for equality.
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
        panel.failed(query);
        panel.set_search(Search::new("ali"));

        assert_eq!(panel.next_match((0, 0), Direction::Forwards), None);
    }

    #[test]
    fn a_page_with_no_rows_has_no_match_to_reach() {
        let (mut panel, query) = loading();
        panel.page_ready(query, page(0));
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
        panel.requested(
            QueryId::next(),
            Subject::Table(TableRef::new("shop", "users")),
        );

        panel.page_ready(first, page(3));

        assert_eq!(panel.row_count(), 0);
        let ContentsState::Loading { subject, .. } = panel.state() else {
            panic!("still waiting on the second request");
        };
        assert_eq!(subject.table(), Some(&TableRef::new("shop", "users")));
    }

    #[test]
    fn a_failure_for_the_outstanding_query_is_recorded_against_its_table() {
        let (mut panel, query) = loading();

        panel.failed(query);

        let ContentsState::Failed { subject } = panel.state() else {
            panic!("the failure is showing");
        };
        assert_eq!(subject.label(), "shop.orders");
    }

    #[test]
    fn a_failure_for_another_query_is_ignored() {
        let (mut panel, query) = loading();

        panel.failed(QueryId::next());

        assert_eq!(panel.outstanding(), Some(query));
    }

    #[test]
    fn a_new_request_starts_the_cursor_at_the_top() {
        let (mut panel, query) = loading();
        panel.page_ready(query, page(5));
        panel.cursor.down(5);

        let next = QueryId::next();
        panel.requested(next, Subject::Table(TableRef::new("shop", "users")));

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

        panel.refused(Subject::Table(TableRef::new("shop", "orders")));

        let ContentsState::Failed { subject } = panel.state() else {
            panic!("the refusal is showing");
        };
        assert_eq!(subject.label(), "shop.orders");
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

        panel.requested(
            QueryId::next(),
            Subject::Table(TableRef::new("shop", "orders")),
        );

        assert_eq!(panel.scroll.offset(), 0, "a second table opens at its top");
        assert_eq!(panel.cursor.index(), 0);
    }

    #[test]
    fn a_refused_request_opens_at_the_top_too() {
        let mut panel = ContentsPanel::default();
        panel.scroll.follow(40, 5);
        panel.cursor.go_to(40, 60);

        panel.refused(Subject::Table(TableRef::new("shop", "orders")));

        assert_eq!(panel.scroll.offset(), 0);
        assert_eq!(panel.cursor.index(), 0);
    }

    #[test]
    fn a_table_subject_is_labelled_by_its_qualified_name() {
        assert_eq!(
            Subject::Table(TableRef::new("shop", "orders")).label(),
            "shop.orders"
        );
    }

    #[test]
    fn a_typed_subject_collapses_to_one_line() {
        assert_eq!(
            Subject::Typed("SELECT *\n  FROM shop.orders\nLIMIT 10".to_owned()).label(),
            "SELECT * FROM shop.orders LIMIT 10"
        );
    }

    #[test]
    fn only_a_table_subject_names_a_table() {
        assert_eq!(
            Subject::Table(TableRef::new("shop", "orders")).table(),
            Some(&TableRef::new("shop", "orders"))
        );
        assert_eq!(Subject::Typed("SELECT 1".to_owned()).table(), None);
    }

    #[test]
    fn a_typed_subject_travels_the_same_road_a_table_does() {
        let mut panel = ContentsPanel::default();
        let query = QueryId::next();
        panel.requested(query, Subject::Typed("SELECT 1".to_owned()));

        panel.page_ready(query, page(3));

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

        panel.page_ready(query, wide_page(3));

        let ContentsState::Loaded { page, .. } = panel.state() else {
            panic!("the page landed");
        };
        assert_eq!(
            page.widths(),
            crate::grid::widths(page.page()),
            "measured once beside the page, not once per tick"
        );
    }

    #[test]
    fn a_loaded_page_reports_its_columns() {
        let (mut panel, query) = loading();

        panel.page_ready(query, wide_page(4));

        assert_eq!(panel.column_count(), 4);
    }

    #[test]
    fn a_request_returns_the_cell_cursor_to_the_first_cell() {
        let (mut panel, query) = loading();
        panel.page_ready(query, wide_page(4));
        panel.cursor.go_to(0, 1);
        panel.column.go_to(3, 4);
        panel.column_scroll.follow(3, &[1, 1, 1, 1], 4);

        panel.requested(
            QueryId::next(),
            Subject::Table(TableRef::new("shop", "carts")),
        );

        assert_eq!(panel.column.index(), 0);
        assert_eq!(panel.column_scroll.first(), 0);
    }

    #[test]
    fn a_refusal_returns_the_cell_cursor_to_the_first_cell() {
        let (mut panel, query) = loading();
        panel.page_ready(query, wide_page(4));
        panel.column.go_to(3, 4);

        panel.refused(Subject::Table(TableRef::new("shop", "carts")));

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

    /// The mode is how the user reads a grid, not something about this page.
    /// `requested` clears the cursors because a new page has different rows to
    /// point at; it has no reason to un-choose the way they are drawn.
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

        panel.requested(QueryId::next(), Subject::Typed("select 1".to_owned()));

        assert!(!panel.rows.is_active());
        assert!(!panel.columns.is_active());
    }

    /// Three rows over two columns, with a `name` column whose lexicographic
    /// order is not the order the cluster returned and a `city` column two of
    /// the three share — so a sort and a filter each have something to do.
    fn arranged() -> ContentsPanel {
        let (mut panel, query) = loading();
        panel.page_ready(
            query,
            Page {
                columns: vec!["name".to_owned(), "city".to_owned()],
                rows: ["Bob", "Ada", "Cleo"]
                    .into_iter()
                    .zip(["Berlin", "Aarhus", "Berlin"])
                    .map(|(name, city)| Row {
                        values: vec![
                            CqlValue::Text(name.to_owned()),
                            CqlValue::Text(city.to_owned()),
                        ],
                    })
                    .collect(),
                next: None,
            },
        );
        panel
    }

    fn drawn(panel: &ContentsPanel, column: usize) -> Vec<String> {
        panel
            .loaded()
            .expect("a page is loaded")
            .rows()
            .map(|row| crate::cell::text(&row.values[column]))
            .collect()
    }

    #[test]
    fn sorting_a_column_walks_descending_then_ascending_then_back_to_the_page() {
        let mut panel = arranged();

        panel.cycle_sort(0);
        assert_eq!(drawn(&panel, 0), ["Cleo", "Bob", "Ada"], "descending first");

        panel.cycle_sort(0);
        assert_eq!(drawn(&panel, 0), ["Ada", "Bob", "Cleo"], "then ascending");

        panel.cycle_sort(0);
        assert_eq!(
            drawn(&panel, 0),
            ["Bob", "Ada", "Cleo"],
            "and then the order the cluster returned"
        );
        assert_eq!(panel.loaded().and_then(GridPage::sort), None);
    }

    #[test]
    fn sorting_a_second_column_starts_that_columns_own_cycle() {
        let mut panel = arranged();
        panel.cycle_sort(0);
        panel.cycle_sort(0);

        panel.cycle_sort(1);

        assert_eq!(
            panel.loaded().and_then(GridPage::sort),
            Some(Sort {
                column: 1,
                order: Order::Descending
            }),
            "one column is sorted at a time, and a new one starts descending"
        );
    }

    #[test]
    fn rows_a_sort_cannot_separate_keep_the_order_they_arrived_in() {
        let mut panel = arranged();

        panel.cycle_sort(1);

        assert_eq!(
            drawn(&panel, 0),
            ["Bob", "Cleo", "Ada"],
            "the two Berliners keep the page's own order between them"
        );
    }

    #[test]
    fn a_filter_drops_the_rows_it_does_not_match() {
        let mut panel = arranged();

        panel.set_filter(1, Search::new("berlin"));

        assert_eq!(
            drawn(&panel, 0),
            ["Bob", "Cleo"],
            "matching is the search's own, case folded and anywhere in the cell"
        );
        assert_eq!(
            panel.row_count(),
            2,
            "the cursor is bounded by what is left"
        );
    }

    #[test]
    fn filters_on_two_columns_narrow_each_other() {
        let mut panel = arranged();

        panel.set_filter(1, Search::new("berlin"));
        panel.set_filter(0, Search::new("cl"));

        assert_eq!(drawn(&panel, 0), ["Cleo"]);
    }

    #[test]
    fn clearing_a_filter_puts_its_rows_back() {
        let mut panel = arranged();
        panel.set_filter(1, Search::new("aarhus"));

        panel.set_filter(1, None);

        assert_eq!(drawn(&panel, 0), ["Bob", "Ada", "Cleo"]);
        assert!(!panel.loaded().expect("a page").is_filtered());
    }

    #[test]
    fn a_filter_and_a_sort_compose() {
        let mut panel = arranged();

        panel.set_filter(1, Search::new("berlin"));
        panel.cycle_sort(0);

        assert_eq!(
            drawn(&panel, 0),
            ["Cleo", "Bob"],
            "the sort orders what the filter left"
        );
    }

    #[test]
    fn a_filter_matching_nothing_leaves_a_page_with_no_rows_to_draw() {
        let mut panel = arranged();

        panel.set_filter(0, Search::new("zzz"));

        assert_eq!(panel.row_count(), 0);
        assert!(panel.loaded().expect("a page").is_empty());
        assert!(
            panel.loaded().expect("a page").is_filtered(),
            "which is what tells an emptied grid apart from an empty page"
        );
    }

    #[test]
    fn arranging_drops_the_range_v_was_holding() {
        let mut panel = arranged();
        panel.rows.start(0);
        panel.rows.stop(2);

        panel.cycle_sort(0);

        assert!(
            !panel.rows.is_active(),
            "a range names rows by where they are drawn, and they have moved"
        );
    }

    #[test]
    fn a_search_walks_the_rows_the_filter_left_and_not_the_ones_it_dropped() {
        let mut panel = arranged();
        panel.set_filter(1, Search::new("berlin"));
        panel.set_search(Search::new("cleo"));

        assert_eq!(
            panel.next_match((0, 0), Direction::Forwards),
            Some((1, 0)),
            "Cleo is the third row of the page and the second of the grid"
        );
    }

    #[test]
    fn an_arrangement_of_a_page_with_no_columns_addresses_nothing() {
        let (mut panel, query) = loading();
        panel.page_ready(
            query,
            Page {
                columns: Vec::new(),
                rows: Vec::new(),
                next: None,
            },
        );

        panel.cycle_sort(0);
        panel.set_filter(0, Search::new("x"));

        assert_eq!(panel.loaded().and_then(GridPage::sort), None);
        assert_eq!(panel.filter(0), None);
    }

    #[test]
    fn a_filter_prompt_opens_on_the_term_already_standing() {
        let filter = Filter::opened(1, "city", Search::new("berlin").as_ref());

        assert_eq!(filter.query(), "berlin");
        assert_eq!(filter.name(), "city");
        assert_eq!(filter.column(), 1);
    }

    #[test]
    fn emptying_a_filter_prompt_is_what_clears_the_filter() {
        let mut filter = Filter::opened(0, "name", Search::new("ada").as_ref());

        for _ in 0..3 {
            filter.pop();
        }

        assert_eq!(filter.search(), None, "an empty term is no filter at all");
    }
}

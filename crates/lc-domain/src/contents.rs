//! The Contents panel's state: one table's rows, and how far along fetching
//! them is.

use std::borrow::Cow;
use std::cmp::Ordering;

use crate::bytes::Bytes;
use crate::config::MaxColumnWidth;
use crate::edit::{CommitScope, Edited, Edits};
use crate::grid::ColumnScroll;
use crate::inspect::Inspector;
use crate::model::{CqlValue, DbError, KeyspaceKind, Page, PagingState, QueryId, Row, Statement};
use crate::search::{Direction, Search};
use crate::selection::Selection;
use crate::style::{Segment, Token};
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

#[derive(Debug, Clone, PartialEq)]
pub enum Paging {
    Ended,
    More {
        stmt: Statement,
        cursor: PagingState,
        fetch: Fetch,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fetch {
    Idle,
    Stopped(Stopped),
    One(QueryId),
    All { query: QueryId, stops: Stops },
}

/// The next multiple of each step above where `P` was pressed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Stops {
    bytes: Bytes,
    rows: usize,
}

pub const MEMORY_STEP: Bytes = Bytes::MB.times(100);

pub const ROW_STEP: usize = 1_000_000;

impl Stops {
    const fn above(bytes: Bytes, rows: usize) -> Self {
        Self {
            bytes: MEMORY_STEP.times(bytes.get() / MEMORY_STEP.get() + 1),
            rows: (rows / ROW_STEP + 1) * ROW_STEP,
        }
    }

    const fn reached(self, bytes: Bytes, rows: usize) -> Option<Stopped> {
        if bytes.get() >= self.bytes.get() {
            Some(Stopped::Memory(self.bytes))
        } else if rows >= self.rows {
            Some(Stopped::Rows(self.rows))
        } else {
            None
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stopped {
    Memory(Bytes),
    Rows(usize),
}

impl Bytes {
    /// Never recomputed: a page's estimate is added as it lands.
    #[must_use]
    pub fn of_row(row: &Row) -> Self {
        Self::new(ROW_OVERHEAD + row.values.iter().map(value_bytes).sum::<u64>())
    }
}

const ROW_OVERHEAD: u64 = size_of::<Row>() as u64;
const VALUE_OVERHEAD: u64 = size_of::<CqlValue>() as u64;

fn value_bytes(value: &CqlValue) -> u64 {
    let payload = match value {
        CqlValue::Null
        | CqlValue::Boolean(_)
        | CqlValue::Int(_)
        | CqlValue::Float(_)
        | CqlValue::Timestamp(_) => 0,
        CqlValue::Text(text) | CqlValue::Uuid(text) => text.len() as u64,
        CqlValue::Blob(bytes) => bytes.len() as u64,
        CqlValue::List(values) | CqlValue::Set(values) => values.iter().map(value_bytes).sum(),
        CqlValue::Map(entries) => entries
            .iter()
            .map(|(key, value)| key.len() as u64 + value_bytes(value))
            .sum(),
    };

    VALUE_OVERHEAD + payload
}

#[derive(Debug, Clone, PartialEq)]
pub struct Continue {
    pub query: QueryId,
    pub stmt: Statement,
    pub cursor: PagingState,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Landed {
    Elsewhere,
    Read,
    /// Followed under a new query rather than shown.
    Skipped(Continue),
    Appended(Option<Continue>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Failure {
    Read,
    Page,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Order {
    Descending,
    Ascending,
}

impl Order {
    fn compare(self, left: &str, right: &str) -> Ordering {
        match self {
            Self::Ascending => left.cmp(right),
            Self::Descending => right.cmp(left),
        }
    }
}

/// One column at a time, so the heading arrow states the whole order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Sort {
    pub column: usize,
    pub order: Order,
}

impl Sort {
    /// Unsorted, descending, ascending, unsorted; another column starts its own
    /// cycle.
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

/// Widths are cached with the page: measuring hex-expands every blob, and every
/// frame wanted it. Measured over the whole page, not the filtered order, so
/// typing a filter does not resize the grid. Sort and filters live here so a
/// new page discards them.
#[derive(Debug, Clone, PartialEq)]
pub struct GridPage {
    page: Page,
    widths: Vec<u16>,
    /// Uncapped, for sharing out a panel's spare room.
    natural: Vec<u16>,
    sort: Option<Sort>,
    /// One slot per page column, so no filter can name a column the page lacks.
    filters: Vec<Option<Search>>,
    /// The one place the arrangement is resolved: cursor, yank and export all
    /// count rows in this.
    order: Vec<usize>,
    bytes: Bytes,
    /// Keyed by page row, not drawn position; a re-read discards them.
    edits: Edits,
}

impl GridPage {
    fn new(page: Page, cap: MaxColumnWidth) -> Self {
        let natural = crate::grid::natural_widths(&page);
        let widths = crate::grid::capped(&natural, cap);
        let filters = vec![None; page.columns.len()];
        let order = (0..page.rows.len()).collect();
        let bytes = page.rows.iter().map(Bytes::of_row).sum();

        Self {
            page: Page { next: None, ..page },
            widths,
            natural,
            sort: None,
            filters,
            order,
            bytes,
            edits: Edits::default(),
        }
    }

    /// Every row fetched, filtered or not.
    #[must_use]
    pub fn fetched(&self) -> usize {
        self.page.rows.len()
    }

    #[must_use]
    pub const fn bytes(&self) -> Bytes {
        self.bytes
    }

    fn append(&mut self, rows: Vec<Row>, cap: MaxColumnWidth) {
        let added = Page {
            columns: self.page.columns.clone(),
            rows,
            next: None,
        };
        let natural = crate::grid::natural_widths(&added);
        for (width, wider) in self
            .widths
            .iter_mut()
            .zip(crate::grid::capped(&natural, cap))
        {
            *width = (*width).max(wider);
        }
        for (width, wider) in self.natural.iter_mut().zip(natural) {
            *width = (*width).max(wider);
        }

        let start = self.page.rows.len();
        self.bytes = self.bytes + added.rows.iter().map(Bytes::of_row).sum();
        self.page.rows.extend(added.rows);

        let kept: Vec<usize> = (start..self.page.rows.len())
            .filter(|at| self.kept(*at))
            .collect();
        match self.sort {
            None => self.order.extend(kept),
            Some(sort) => self.merge(sort, kept),
        }
    }

    /// Each lands after every row it ties with, where a stable sort of the
    /// whole grid would put it.
    fn merge(&mut self, sort: Sort, added: Vec<usize>) {
        let key = |at: usize| {
            self.page.rows[at]
                .values
                .get(sort.column)
                .map(crate::cell::text)
                .unwrap_or_default()
        };
        let mut added: Vec<(String, usize)> = added.into_iter().map(|at| (key(at), at)).collect();
        added.sort_by(|(left, _), (right, _)| sort.order.compare(left, right));

        let mut merged = Vec::with_capacity(self.order.len() + added.len());
        let mut from = 0;
        for (new, at) in added {
            let before = from
                + self.order[from..].partition_point(|old| {
                    sort.order.compare(&key(*old), &new) != Ordering::Greater
                });
            merged.extend_from_slice(&self.order[from..before]);
            merged.push(at);
            from = before;
        }
        merged.extend_from_slice(&self.order[from..]);

        self.order = merged;
    }

    #[must_use]
    pub const fn page(&self) -> &Page {
        &self.page
    }

    #[must_use]
    pub const fn edits(&self) -> &Edits {
        &self.edits
    }

    /// The edit where there is one. Every reader goes through here so they
    /// agree on what a cell says.
    #[must_use]
    pub fn value(&self, index: usize, column: usize) -> Option<&CqlValue> {
        let at = *self.order.get(index)?;

        Some(match self.edits.get(at, column) {
            Some(edited) => edited.value(),
            None => self.page.rows.get(at)?.values.get(column)?,
        })
    }

    #[must_use]
    pub fn is_edited(&self, index: usize, column: usize) -> bool {
        self.order
            .get(index)
            .is_some_and(|at| self.edits.get(*at, column).is_some())
    }

    #[must_use]
    pub fn drawn_rows(&self) -> Vec<Row> {
        (0..self.order.len())
            .map(|index| Row {
                values: (0..self.page.columns.len())
                    .map(|column| self.value(index, column).cloned().unwrap_or(CqlValue::Null))
                    .collect(),
            })
            .collect()
    }

    /// An edit back to the original value clears the mark.
    pub fn set_edit(&mut self, index: usize, column: usize, edited: Edited) {
        let Some(&at) = self.order.get(index) else {
            return;
        };
        let original = self
            .page
            .rows
            .get(at)
            .and_then(|row| row.values.get(column));

        let kept = (original != Some(edited.value())).then_some(edited);
        self.edits.set(at, column, kept);
    }

    /// No re-read and no re-sort: either would move the row from under the
    /// cursor that edited it.
    pub fn commit_edits(&mut self, scope: CommitScope) {
        for ((row, column), edited) in self.edits.drain_in(scope) {
            if let Some(cell) = self
                .page
                .rows
                .get_mut(row)
                .and_then(|row| row.values.get_mut(column))
            {
                *cell = edited.value().clone();
            }
        }
    }

    #[must_use]
    pub fn widths(&self) -> &[u16] {
        &self.widths
    }

    /// The one answer the column follow, projection and renderer share; two
    /// would drift apart.
    #[must_use]
    pub fn drawn(&self, grid_width: usize) -> (Cow<'_, [u16]>, u16) {
        let width = u16::try_from(grid_width).unwrap_or(u16::MAX);
        let available = crate::grid::available(width, crate::grid::gutter(self.len()));

        (
            crate::grid::spread(&self.widths, &self.natural, available),
            available,
        )
    }

    #[must_use]
    pub const fn sort(&self) -> Option<Sort> {
        self.sort
    }

    #[must_use]
    pub fn filter(&self, column: usize) -> Option<&Search> {
        self.filters.get(column)?.as_ref()
    }

    /// Rows the arrangement leaves.
    #[must_use]
    pub fn len(&self) -> usize {
        self.order.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.order.is_empty()
    }

    #[must_use]
    pub fn is_filtered(&self) -> bool {
        self.filters.iter().any(Option::is_some)
    }

    #[must_use]
    pub fn rows(&self) -> impl ExactSizeIterator<Item = &Row> {
        self.order.iter().map(|at| &self.page.rows[*at])
    }

    /// The page row the `index`th drawn row is; edits are keyed by it.
    #[must_use]
    pub fn page_row(&self, index: usize) -> Option<usize> {
        self.order.get(index).copied()
    }

    #[must_use]
    pub fn row(&self, index: usize) -> Option<&Row> {
        self.page.rows.get(*self.order.get(index)?)
    }

    pub fn cycle_sort(&mut self, column: usize) {
        if column >= self.page.columns.len() {
            return;
        }

        self.sort = Sort::next(self.sort, column);
        self.arrange();
    }

    pub fn set_filter(&mut self, column: usize, filter: Option<Search>) {
        let Some(slot) = self.filters.get_mut(column) else {
            return;
        };

        *slot = filter;
        self.arrange();
    }

    /// Sorted by drawn text: a column holds one CQL type, so that order is
    /// readable off the screen. Stable, so ties keep the cluster's order.
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
            // Keys are rendered once: `cell::text` allocates and hex-expands
            // blobs.
            let mut paired: Vec<(String, usize)> = keys.into_iter().zip(order).collect();
            paired.sort_by(|(left, _), (right, _)| sort.order.compare(left, right));
            order = paired.into_iter().map(|(_, at)| at).collect();
        }

        self.order = order;
    }

    /// Filters are conjunctive.
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

/// Not a [`crate::search::Prompt`]: a search flags cells and keeps rows, a
/// filter drops rows. Applied on close, since a hard filter rebuilds the grid
/// under the typing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Filter {
    column: usize,
    /// Captured at open: the prompt claims every key, so the page cannot change
    /// under it.
    name: String,
    query: String,
}

impl Filter {
    /// Seeded with the standing filter, so deleting the text clears it.
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

    /// `None` for an empty query, which clears the filter.
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

/// Owned: a result outlives the schema reload that dropped its table.
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

    #[must_use]
    pub fn qualified(&self) -> String {
        format!("{}.{}", self.keyspace, self.table)
    }

    /// A system keyspace is faint on both halves, as in the tree.
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Subject {
    Table(TableRef),
    Typed(String),
}

impl Subject {
    /// Whitespace runs collapse, so a multi-line buffer titles in one line.
    #[must_use]
    pub fn label(&self) -> String {
        match self {
            Self::Table(table) => table.qualified(),
            Self::Typed(cql) => cql.split_whitespace().collect::<Vec<_>>().join(" "),
        }
    }

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
        let ContentsState::Loaded { page, .. } = &self.state else {
            return None;
        };
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

    #[must_use]
    pub const fn outstanding(&self) -> Option<QueryId> {
        match &self.state {
            ContentsState::Loading { query, .. }
            | ContentsState::Loaded {
                paging:
                    Paging::More {
                        fetch: Fetch::One(query) | Fetch::All { query, .. },
                        ..
                    },
                ..
            } => Some(*query),
            ContentsState::Empty
            | ContentsState::Failed { .. }
            | ContentsState::Loaded {
                paging:
                    Paging::Ended
                    | Paging::More {
                        fetch: Fetch::Idle | Fetch::Stopped(_),
                        ..
                    },
                ..
            } => None,
        }
    }

    #[must_use]
    pub const fn paging(&self) -> Option<&Paging> {
        match &self.state {
            ContentsState::Loaded { paging, .. } => Some(paging),
            _ => None,
        }
    }

    #[must_use]
    pub const fn has_more(&self) -> bool {
        matches!(self.paging(), Some(Paging::More { .. }))
    }

    #[must_use]
    pub const fn can_fetch(&self) -> bool {
        matches!(
            self.paging(),
            Some(Paging::More {
                fetch: Fetch::Idle | Fetch::Stopped(_),
                ..
            })
        )
    }

    #[must_use]
    pub const fn fetching_all(&self) -> bool {
        matches!(
            self.paging(),
            Some(Paging::More {
                fetch: Fetch::All { .. },
                ..
            })
        )
    }

    pub fn fetch_one(&mut self, query: impl FnOnce() -> QueryId) -> Option<Continue> {
        self.fetch(|_| Fetch::One(query()))
    }

    pub fn fetch_all(&mut self, query: impl FnOnce() -> QueryId) -> Option<Continue> {
        self.fetch(|page| Fetch::All {
            query: query(),
            stops: Stops::above(page.bytes, page.fetched()),
        })
    }

    fn fetch(&mut self, started: impl FnOnce(&GridPage) -> Fetch) -> Option<Continue> {
        let ContentsState::Loaded {
            page,
            paging:
                Paging::More {
                    stmt,
                    cursor,
                    fetch: fetch @ (Fetch::Idle | Fetch::Stopped(_)),
                },
            ..
        } = &mut self.state
        else {
            return None;
        };

        *fetch = started(page);
        let (Fetch::One(query) | Fetch::All { query, .. }) = *fetch else {
            return None;
        };

        Some(Continue {
            query,
            stmt: stmt.clone(),
            cursor: cursor.clone(),
        })
    }

    /// The page in flight still lands; nothing follows it.
    pub fn stop_fetching(&mut self) -> bool {
        let Some(Paging::More { fetch, .. }) = self.paging_mut() else {
            return false;
        };
        let Fetch::All { query, .. } = *fetch else {
            return false;
        };

        *fetch = Fetch::One(query);
        true
    }

    /// A paging cursor is only good on the session that issued it.
    pub fn end_paging(&mut self) -> Option<QueryId> {
        let paging = self.paging_mut()?;
        let flying = match paging {
            Paging::More {
                fetch: Fetch::One(query) | Fetch::All { query, .. },
                ..
            } => Some(*query),
            Paging::Ended | Paging::More { .. } => None,
        };

        *paging = Paging::Ended;
        flying
    }

    /// Fakes an estimate no fast test can afford to fetch.
    #[cfg(test)]
    pub(crate) fn pretend(&mut self, bytes: Bytes, to: Fetch) {
        if let ContentsState::Loaded {
            page,
            paging: Paging::More { fetch, .. },
            ..
        } = &mut self.state
        {
            page.bytes = bytes;
            *fetch = to;
        }
    }

    fn paging_mut(&mut self) -> Option<&mut Paging> {
        match &mut self.state {
            ContentsState::Loaded { paging, .. } => Some(paging),
            _ => None,
        }
    }

    pub fn requested(&mut self, query: QueryId, subject: Subject, stmt: Statement) {
        // What was typed addressed the old content's rows.
        self.draft = None;
        self.state = ContentsState::Loading {
            query,
            subject,
            stmt,
        };
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

    pub fn page_ready(
        &mut self,
        landed: QueryId,
        page: Page,
        cap: MaxColumnWidth,
        query: impl FnOnce() -> QueryId,
    ) -> Landed {
        if self.outstanding() != Some(landed) {
            return Landed::Elsewhere;
        }

        match self.state {
            ContentsState::Loading { .. } => self.first_page(page, cap, query),
            ContentsState::Loaded { .. } => self.later_page(page, cap, query),
            ContentsState::Empty | ContentsState::Failed { .. } => Landed::Elsewhere,
        }
    }

    fn first_page(
        &mut self,
        page: Page,
        cap: MaxColumnWidth,
        query: impl FnOnce() -> QueryId,
    ) -> Landed {
        match &mut self.state {
            ContentsState::Loading {
                query: outstanding,
                subject,
                stmt,
            } => {
                let Page {
                    columns,
                    rows,
                    next,
                } = page;
                // An empty page can still have a successor: Cassandra ends a
                // page on what it scanned, not what matched or survived
                // tombstones.
                let paging = match next {
                    Some(cursor) if rows.is_empty() => {
                        *outstanding = query();
                        return Landed::Skipped(Continue {
                            query: *outstanding,
                            stmt: stmt.clone(),
                            cursor,
                        });
                    }
                    Some(cursor) => Paging::More {
                        stmt: stmt.clone(),
                        cursor,
                        fetch: Fetch::Idle,
                    },
                    None => Paging::Ended,
                };
                self.state = ContentsState::Loaded {
                    subject: subject.clone(),
                    page: Box::new(GridPage::new(
                        Page {
                            columns,
                            rows,
                            next: None,
                        },
                        cap,
                    )),
                    paging,
                };
                Landed::Read
            }
            _ => Landed::Elsewhere,
        }
    }

    fn later_page(
        &mut self,
        page: Page,
        cap: MaxColumnWidth,
        query: impl FnOnce() -> QueryId,
    ) -> Landed {
        match &mut self.state {
            ContentsState::Loaded {
                page: grid, paging, ..
            } => {
                let empty = page.rows.is_empty();
                let row = grid.order.get(self.cursor.index()).copied();
                grid.append(page.rows, cap);

                // Unsorted, rows only arrive past the end, so cursor and ranges
                // stay put; sorted, they move.
                if grid.sort.is_some() {
                    if let Some(at) =
                        row.and_then(|row| grid.order.iter().position(|at| *at == row))
                    {
                        self.cursor.go_to(at, grid.len());
                    }
                    self.rows.clear();
                    if self.mode == SelectionMode::Cell {
                        self.columns.clear();
                    }
                }

                let (
                    Some(next),
                    Paging::More {
                        stmt,
                        cursor,
                        fetch,
                    },
                ) = (page.next, &mut *paging)
                else {
                    *paging = Paging::Ended;
                    return Landed::Appended(None);
                };
                *cursor = next;

                let stopped = match *fetch {
                    _ if empty => None,
                    Fetch::All { stops, .. } => stops.reached(grid.bytes, grid.fetched()),
                    Fetch::Idle | Fetch::Stopped(_) | Fetch::One(_) => {
                        *fetch = Fetch::Idle;
                        return Landed::Appended(None);
                    }
                };
                if let Some(stopped) = stopped {
                    *fetch = Fetch::Stopped(stopped);
                    return Landed::Appended(None);
                }

                let following = query();
                if let Fetch::One(outstanding)
                | Fetch::All {
                    query: outstanding, ..
                } = fetch
                {
                    *outstanding = following;
                }
                Landed::Appended(Some(Continue {
                    query: following,
                    stmt: stmt.clone(),
                    cursor: cursor.clone(),
                }))
            }
            _ => Landed::Elsewhere,
        }
    }

    /// A request the domain refused before it became a query.
    pub fn refused(&mut self, subject: Subject, error: &DbError) {
        self.state = ContentsState::Failed {
            subject,
            error: error.to_string(),
        };
        self.cursor = Cursor::default();
        self.column = Cursor::default();
        self.scroll = Scroll::default();
        self.column_scroll = ColumnScroll::default();
        self.rows = Selection::default();
        self.columns = Selection::default();
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
        match &self.state {
            ContentsState::Loaded { page, .. } => page.len(),
            _ => 0,
        }
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
        if let ContentsState::Loaded { page, .. } = &mut self.state {
            page.cycle_sort(column);
            self.rows.clear();
        }
    }

    /// Drops `v`'s ranges, as [`ContentsPanel::cycle_sort`] does.
    pub fn set_filter(&mut self, column: usize, filter: Option<Search>) {
        if let ContentsState::Loaded { page, .. } = &mut self.state {
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
        match &self.state {
            ContentsState::Loaded { page, .. } => page.page().columns.len(),
            _ => 0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{CqlValue, Row};

    fn rejection() -> DbError {
        DbError::Invalid("undefined column name total".to_owned())
    }

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
        panel.requested(
            query,
            Subject::Table(TableRef::new("shop", "orders")),
            crate::model::Statement::typed("SELECT * FROM shop.orders"),
        );
        (panel, query)
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
            crate::model::Statement::typed("SELECT * FROM shop.orders"),
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
            crate::model::Statement::typed("SELECT * FROM shop.orders"),
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
            crate::model::Statement::typed("SELECT * FROM shop.orders"),
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
        panel.requested(
            query,
            Subject::Typed("SELECT 1".to_owned()),
            crate::model::Statement::typed("SELECT * FROM shop.orders"),
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
            crate::model::Statement::typed("SELECT * FROM shop.orders"),
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
            crate::model::Statement::typed("SELECT * FROM shop.orders"),
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
            crate::model::Statement::typed("SELECT * FROM shop.orders"),
        );

        assert!(!panel.rows.is_active());
        assert!(!panel.columns.is_active());
    }

    /// `name` is out of lexicographic order and two rows share a `city`, so
    /// sort and filter both have work.
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
            MaxColumnWidth::default(),
            QueryId::next,
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
            MaxColumnWidth::default(),
            QueryId::next,
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

    fn more(rows: i64) -> Page {
        Page {
            next: Some(PagingState(vec![1])),
            ..page(rows)
        }
    }

    fn pageable(rows: i64) -> ContentsPanel {
        let (mut panel, query) = loading();
        panel.page_ready(query, more(rows), MaxColumnWidth::default(), QueryId::next);
        panel
    }

    fn land(panel: &mut ContentsPanel, page: Page) -> Landed {
        let query = panel.outstanding().expect("a page is outstanding");
        panel.page_ready(query, page, MaxColumnWidth::default(), QueryId::next)
    }

    fn fetch_of(panel: &ContentsPanel) -> Option<Fetch> {
        match panel.paging()? {
            Paging::More { fetch, .. } => Some(*fetch),
            Paging::Ended => None,
        }
    }

    #[test]
    fn a_first_page_with_a_successor_leaves_more_to_fetch() {
        let panel = pageable(3);

        assert_eq!(fetch_of(&panel), Some(Fetch::Idle));
        assert!(panel.can_fetch());
        assert_eq!(panel.outstanding(), None, "nothing is in flight");
    }

    #[test]
    fn the_next_page_is_appended_and_the_last_ends_the_result() {
        let mut panel = pageable(3);

        let next = panel.fetch_one(QueryId::next).expect("a page to ask for");
        assert_eq!(panel.outstanding(), Some(next.query));
        assert_eq!(land(&mut panel, more(2)), Landed::Appended(None));
        assert_eq!(panel.row_count(), 5);
        assert_eq!(fetch_of(&panel), Some(Fetch::Idle));

        panel.fetch_one(QueryId::next).expect("another page");
        land(&mut panel, page(1));

        assert_eq!(panel.row_count(), 6);
        assert_eq!(panel.paging(), Some(&Paging::Ended));
        assert!(!panel.has_more());
        assert_eq!(
            panel.fetch_one(QueryId::next),
            None,
            "an ended result has nothing to fetch"
        );
    }

    #[test]
    fn nothing_is_fetched_twice_at_once() {
        let mut panel = pageable(3);
        panel.fetch_one(QueryId::next).expect("a page");

        assert_eq!(panel.fetch_one(QueryId::next), None);
        assert_eq!(panel.fetch_all(QueryId::next), None);
    }

    #[test]
    fn an_empty_first_page_with_a_successor_is_followed_rather_than_shown() {
        let (mut panel, query) = loading();

        let landed = panel.page_ready(query, more(0), MaxColumnWidth::default(), QueryId::next);

        let Landed::Skipped(next) = landed else {
            panic!("the empty page is skipped: {landed:?}");
        };
        assert_eq!(panel.outstanding(), Some(next.query));
        assert!(matches!(panel.state(), ContentsState::Loading { .. }));
    }

    #[test]
    fn an_empty_later_page_does_not_count_as_the_one_asked_for() {
        let mut panel = pageable(3);
        panel.fetch_one(QueryId::next).expect("a page");

        let Landed::Appended(Some(next)) = land(&mut panel, more(0)) else {
            panic!("the empty page is followed");
        };
        assert_eq!(fetch_of(&panel), Some(Fetch::One(next.query)));

        land(&mut panel, more(2));
        assert_eq!(fetch_of(&panel), Some(Fetch::Idle));
        assert_eq!(panel.row_count(), 5);
    }

    #[test]
    fn every_page_runs_to_the_end() {
        let mut panel = pageable(3);
        panel.fetch_all(QueryId::next).expect("a page");

        assert!(matches!(
            land(&mut panel, more(3)),
            Landed::Appended(Some(_))
        ));
        assert!(panel.fetching_all());
        assert_eq!(land(&mut panel, page(3)), Landed::Appended(None));

        assert_eq!(panel.row_count(), 9);
        assert_eq!(panel.paging(), Some(&Paging::Ended));
    }

    #[test]
    fn stopping_every_page_still_appends_the_one_in_flight() {
        let mut panel = pageable(3);
        panel.fetch_all(QueryId::next).expect("a page");

        assert!(panel.stop_fetching());
        assert!(!panel.stop_fetching(), "there is nothing left to stop");
        assert_eq!(land(&mut panel, more(3)), Landed::Appended(None));

        assert_eq!(panel.row_count(), 6);
        assert_eq!(fetch_of(&panel), Some(Fetch::Idle));
    }

    #[test]
    fn every_page_stops_at_the_next_hundred_megabytes_and_continues_past_it() {
        let mut panel = pageable(3);
        if let ContentsState::Loaded { page, .. } = &mut panel.state {
            page.bytes = Bytes::new(MEMORY_STEP.get() - 1);
        }

        panel.fetch_all(QueryId::next).expect("a page");
        assert_eq!(land(&mut panel, more(1)), Landed::Appended(None));
        assert_eq!(
            fetch_of(&panel),
            Some(Fetch::Stopped(Stopped::Memory(MEMORY_STEP)))
        );

        panel
            .fetch_all(QueryId::next)
            .expect("pressed again, it continues");
        assert!(matches!(
            land(&mut panel, more(1)),
            Landed::Appended(Some(_))
        ));
        if let ContentsState::Loaded { page, .. } = &mut panel.state {
            page.bytes = MEMORY_STEP.times(2);
        }
        land(&mut panel, more(1));
        assert_eq!(
            fetch_of(&panel),
            Some(Fetch::Stopped(Stopped::Memory(MEMORY_STEP.times(2)))),
            "the next stop is the next multiple"
        );
    }

    #[test]
    fn every_page_stops_at_the_next_million_rows_and_continues_past_it() {
        let columnless = |rows: usize, next: bool| Page {
            columns: Vec::new(),
            rows: vec![Row::default(); rows],
            next: next.then(|| PagingState(vec![1])),
        };
        let (mut panel, query) = loading();
        panel.page_ready(
            query,
            columnless(ROW_STEP - 1, true),
            MaxColumnWidth::default(),
            QueryId::next,
        );

        panel.fetch_all(QueryId::next).expect("a page");
        land(&mut panel, columnless(2, true));
        assert_eq!(
            fetch_of(&panel),
            Some(Fetch::Stopped(Stopped::Rows(ROW_STEP)))
        );

        panel
            .fetch_all(QueryId::next)
            .expect("pressed again, it continues");
        assert!(matches!(
            land(&mut panel, columnless(2, true)),
            Landed::Appended(Some(_))
        ));
    }

    #[test]
    fn the_next_page_ignores_a_stop() {
        let mut panel = pageable(3);
        if let ContentsState::Loaded { page, .. } = &mut panel.state {
            page.bytes = Bytes::new(MEMORY_STEP.get() - 1);
        }
        panel.fetch_all(QueryId::next).expect("a page");
        land(&mut panel, more(1));

        assert!(panel.fetch_one(QueryId::next).is_some());
    }

    #[test]
    fn ending_the_result_drops_the_page_in_flight() {
        let mut panel = pageable(3);
        let next = panel.fetch_all(QueryId::next).expect("a page");

        assert_eq!(panel.end_paging(), Some(next.query));

        assert_eq!(panel.paging(), Some(&Paging::Ended));
        assert_eq!(panel.row_count(), 3);
        assert_eq!(panel.outstanding(), None);
    }

    #[test]
    fn a_failed_page_keeps_the_rows_already_loaded() {
        let mut panel = pageable(3);
        let next = panel.fetch_all(QueryId::next).expect("a page");

        assert_eq!(panel.failed(next.query, &rejection()), Some(Failure::Page));

        assert_eq!(panel.row_count(), 3);
        assert!(panel.can_fetch(), "the same page can be asked for again");
    }

    #[test]
    fn a_page_for_a_superseded_fetch_is_not_applied() {
        let mut panel = pageable(3);
        let next = panel.fetch_one(QueryId::next).expect("a page");
        panel.requested(
            QueryId::next(),
            Subject::Table(TableRef::new("shop", "users")),
            crate::model::Statement::typed("SELECT * FROM shop.users"),
        );

        assert_eq!(
            panel.page_ready(
                next.query,
                more(2),
                MaxColumnWidth::default(),
                QueryId::next
            ),
            Landed::Elsewhere
        );
    }

    #[test]
    fn an_append_to_an_unsorted_grid_keeps_the_cursor_and_the_range() {
        let mut panel = pageable(5);
        panel.cursor.go_to(3, 5);
        panel.rows.start(1);
        panel.rows.stop(3);
        panel.fetch_one(QueryId::next).expect("a page");

        land(&mut panel, more(4));

        assert_eq!(panel.cursor.index(), 3);
        assert!(panel.rows.is_active(), "rows only arrived past the end");
    }

    #[test]
    fn an_append_to_a_sorted_grid_follows_the_row_and_drops_the_range() {
        let mut panel = pageable(5);
        panel.cycle_sort(0);
        panel.cursor.go_to(0, 5);
        let under = panel.loaded().and_then(|page| page.row(0)).cloned();
        panel.rows.start(0);
        panel.rows.stop(1);
        panel.fetch_one(QueryId::next).expect("a page");

        land(&mut panel, more(8));

        assert_eq!(
            panel
                .loaded()
                .and_then(|page| page.row(panel.cursor.index()))
                .cloned(),
            under,
            "the cursor stays on the row it was on"
        );
        assert!(!panel.rows.is_active());
    }

    #[test]
    fn an_appended_page_lands_where_a_sort_of_every_row_puts_it() {
        let text = |values: &[&str], next: bool| Page {
            columns: vec!["name".to_owned()],
            rows: values
                .iter()
                .map(|value| Row {
                    values: vec![CqlValue::Text((*value).to_owned())],
                })
                .collect(),
            next: next.then(|| PagingState(vec![1])),
        };
        for order in [Order::Ascending, Order::Descending] {
            let (mut panel, query) = loading();
            panel.page_ready(
                query,
                text(&["b", "d", "a", "b"], true),
                MaxColumnWidth::default(),
                QueryId::next,
            );
            panel.cycle_sort(0);
            if order == Order::Ascending {
                panel.cycle_sort(0);
            }
            panel.set_filter(0, None);
            panel.fetch_one(QueryId::next).expect("a page");

            land(&mut panel, text(&["c", "b", "e", "a"], false));

            let appended = panel.loaded().expect("a page").order.clone();
            let mut arranged = panel.loaded().expect("a page").clone();
            arranged.arrange();
            assert_eq!(appended, arranged.order, "{order:?}");
        }
    }

    #[test]
    fn an_appended_page_is_filtered_as_it_lands() {
        let mut panel = arranged_pageable();
        panel.set_filter(1, Search::new("berlin"));
        panel.fetch_one(QueryId::next).expect("a page");

        land(
            &mut panel,
            Page {
                columns: vec!["name".to_owned(), "city".to_owned()],
                rows: ["Dora", "Eli"]
                    .into_iter()
                    .zip(["Berlin", "Oslo"])
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

        assert_eq!(drawn(&panel, 0), ["Bob", "Cleo", "Dora"]);
    }

    #[test]
    fn a_search_reaches_an_appended_row() {
        let mut panel = pageable(3);
        panel.set_search(Search::new("7"));
        assert_eq!(panel.next_match((0, 0), Direction::Forwards), None);
        panel.fetch_one(QueryId::next).expect("a page");

        land(
            &mut panel,
            Page {
                columns: vec!["id".to_owned()],
                rows: vec![Row {
                    values: vec![CqlValue::Int(7)],
                }],
                next: None,
            },
        );

        assert_eq!(panel.next_match((0, 0), Direction::Forwards), Some((3, 0)));
    }

    #[test]
    fn a_wider_appended_value_widens_its_column() {
        let mut panel = pageable(1);
        panel.fetch_one(QueryId::next).expect("a page");

        land(
            &mut panel,
            Page {
                columns: vec!["id".to_owned()],
                rows: vec![Row {
                    values: vec![CqlValue::Text("x".repeat(20))],
                }],
                next: None,
            },
        );

        assert_eq!(panel.loaded().expect("a page").widths(), [20]);
    }

    #[test]
    fn an_appended_value_past_the_cap_grows_the_width_the_panel_widens_to() {
        let mut panel = pageable(1);
        panel.fetch_one(QueryId::next).expect("a page");
        let grid_width = usize::from(crate::grid::BAR + 1 + crate::grid::COLUMN_SPACING + 100);

        land(
            &mut panel,
            Page {
                columns: vec!["id".to_owned()],
                rows: vec![Row {
                    values: vec![CqlValue::Text("x".repeat(60))],
                }],
                next: None,
            },
        );

        let page = panel.loaded().expect("a page");
        assert_eq!(page.widths(), [30], "capped");
        assert_eq!(page.drawn(grid_width).0.as_ref(), [60], "then widened");
    }

    #[test]
    fn a_rows_estimate_counts_its_text_and_blobs_recursively() {
        let row = Row {
            values: vec![
                CqlValue::Text("abcd".to_owned()),
                CqlValue::Blob(vec![0; 10]),
                CqlValue::List(vec![CqlValue::Uuid("u".repeat(36))]),
                CqlValue::Map([("k".to_owned(), CqlValue::Int(1))].into_iter().collect()),
                CqlValue::Null,
            ],
        };

        assert_eq!(
            Bytes::of_row(&row).get(),
            ROW_OVERHEAD + 7 * VALUE_OVERHEAD + 4 + 10 + 36 + 1
        );
    }

    #[test]
    fn a_stop_is_the_next_multiple_above_where_every_page_was_pressed() {
        let stops = Stops::above(MEMORY_STEP, 0);

        assert_eq!(stops.bytes, MEMORY_STEP.times(2));
        assert_eq!(stops.rows, ROW_STEP);
        assert_eq!(stops.reached(Bytes::new(0), ROW_STEP - 1), None);
        assert_eq!(
            stops.reached(MEMORY_STEP.times(2), ROW_STEP),
            Some(Stopped::Memory(MEMORY_STEP.times(2))),
            "memory first when a page reaches both"
        );
    }

    fn arranged_pageable() -> ContentsPanel {
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
                next: Some(PagingState(vec![1])),
            },
            MaxColumnWidth::default(),
            QueryId::next,
        );
        panel
    }
}

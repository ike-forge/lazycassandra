use std::borrow::Cow;
use std::cmp::Ordering;

use crate::bytes::Bytes;
use crate::config::MaxColumnWidth;
use crate::edit::{CommitScope, Edited, Edits};
use crate::model::{CqlValue, Page, Row};
use crate::search::Search;

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
    pub(super) fn new(page: Page, cap: MaxColumnWidth) -> Self {
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

    pub(super) fn append(&mut self, rows: Vec<Row>, cap: MaxColumnWidth) {
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
        let key = |at: usize| self.read_text(at, sort.column);
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

    pub(super) fn index_of(&self, page_row: usize) -> Option<usize> {
        self.order.iter().position(|at| *at == page_row)
    }

    #[cfg(test)]
    pub(crate) const fn pretend_bytes(&mut self, bytes: Bytes) {
        self.bytes = bytes;
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
            // Keys are rendered once: `cell::text` allocates and hex-expands
            // blobs.
            let mut paired: Vec<(String, usize)> = order
                .into_iter()
                .map(|at| (self.read_text(at, sort.column), at))
                .collect();
            paired.sort_by(|(left, _), (right, _)| sort.order.compare(left, right));
            order = paired.into_iter().map(|(_, at)| at).collect();
        }

        self.order = order;
    }

    /// As read, not as edited.
    fn read_text(&self, at: usize, column: usize) -> String {
        self.page.rows[at]
            .values
            .get(column)
            .map(crate::cell::text)
            .unwrap_or_default()
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

#[cfg(test)]
mod tests {
    use super::super::ContentsPanel;
    use super::super::tests_support::*;
    use super::*;
    use crate::model::{PagingState, QueryId};
    use crate::search::Direction;

    /// `name` is out of lexicographic order and two rows share a `city`, so
    /// sort and filter both have work.
    fn arranged() -> ContentsPanel {
        arranged_page(None)
    }

    fn arranged_page(next: Option<PagingState>) -> ContentsPanel {
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
                next,
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
        let mut panel = arranged_page(Some(PagingState(vec![1])));
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
}

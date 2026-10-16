use crate::bytes::Bytes;
use crate::config::MaxColumnWidth;
use crate::model::{CqlValue, Page, PagingState, QueryId, Row, Statement};

use super::{ContentsPanel, ContentsState, GridPage, SelectionMode};

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

impl Fetch {
    const fn in_flight(self) -> Option<QueryId> {
        match self {
            Self::One(query) | Self::All { query, .. } => Some(query),
            Self::Idle | Self::Stopped(_) => None,
        }
    }
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

impl ContentsPanel {
    #[must_use]
    pub const fn outstanding(&self) -> Option<QueryId> {
        match &self.state {
            ContentsState::Loading { query, .. } => Some(*query),
            ContentsState::Loaded {
                paging: Paging::More { fetch, .. },
                ..
            } => fetch.in_flight(),
            ContentsState::Empty
            | ContentsState::Failed { .. }
            | ContentsState::Loaded {
                paging: Paging::Ended,
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
            stops: Stops::above(page.bytes(), page.fetched()),
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

        Some(Continue {
            query: fetch.in_flight()?,
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
        match std::mem::replace(self.paging_mut()?, Paging::Ended) {
            Paging::More { fetch, .. } => fetch.in_flight(),
            Paging::Ended => None,
        }
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
            page.pretend_bytes(bytes);
            *fetch = to;
        }
    }

    fn paging_mut(&mut self) -> Option<&mut Paging> {
        match &mut self.state {
            ContentsState::Loaded { paging, .. } => Some(paging),
            _ => None,
        }
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
                let row = grid.page_row(self.cursor.index());
                grid.append(page.rows, cap);

                // Unsorted, rows only arrive past the end, so cursor and ranges
                // stay put; sorted, they move.
                if grid.sort().is_some() {
                    if let Some(at) = row.and_then(|row| grid.index_of(row)) {
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
                    Fetch::All { stops, .. } => stops.reached(grid.bytes(), grid.fetched()),
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
}

#[cfg(test)]
mod tests {
    use super::super::tests_support::*;
    use super::super::{Subject, TableRef};
    use super::*;

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
        if let Some(page) = panel.loaded_mut() {
            page.pretend_bytes(Bytes::new(MEMORY_STEP.get() - 1));
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
        if let Some(page) = panel.loaded_mut() {
            page.pretend_bytes(MEMORY_STEP.times(2));
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
        if let Some(page) = panel.loaded_mut() {
            page.pretend_bytes(Bytes::new(MEMORY_STEP.get() - 1));
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
            Statement::typed("SELECT * FROM shop.users"),
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
}

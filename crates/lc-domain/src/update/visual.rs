//! Visual mode: the selection axes `v`, `esc` and `y` act on.

use crate::contents::SelectionMode;
use crate::panel::Panel;
use crate::queries::QueriesTab;
use crate::selection::{Range, Ranges, Selection};
use crate::state::State;
use crate::viewport::Cursor;

use super::row_count;

pub(super) fn spanned(selection: &Selection, cursor: usize) -> Ranges {
    let ranges = selection.ranges(cursor);

    if ranges.is_empty() {
        Ranges::of(Range::new(cursor, cursor))
    } else {
        ranges
    }
}

/// A selection only means a range in company with its cursor, and which cursor
/// depends on the grid's mode.
pub(super) struct Axis<'a> {
    pub(super) selection: &'a mut Selection,
    pub(super) cursor: &'a mut Cursor,
    pub(super) len: usize,
}

/// The one place a selection is matched to its cursor: column mode puts the
/// column cursor on the first axis.
pub(super) fn axes(state: &mut State, panel: Panel) -> [Option<Axis<'_>>; 2] {
    let rows = row_count(state, panel);
    let columns = state.contents.column_count();
    let contents = &mut state.contents;

    match panel {
        Panel::Tables => {
            let (selection, cursor) = state.tables.selection_and_cursor();
            [
                Some(Axis {
                    selection,
                    cursor,
                    len: rows,
                }),
                None,
            ]
        }
        Panel::Ddl => [
            Some(Axis {
                selection: &mut state.ddl.selection,
                cursor: &mut state.ddl.cursor,
                len: rows,
            }),
            None,
        ],
        Panel::Contents => match contents.mode {
            SelectionMode::Row => [
                Some(Axis {
                    selection: &mut contents.rows,
                    cursor: &mut contents.cursor,
                    len: rows,
                }),
                None,
            ],
            SelectionMode::Column => [
                Some(Axis {
                    selection: &mut contents.columns,
                    cursor: &mut contents.column,
                    len: columns,
                }),
                None,
            ],
            SelectionMode::Cell => [
                Some(Axis {
                    selection: &mut contents.rows,
                    cursor: &mut contents.cursor,
                    len: rows,
                }),
                Some(Axis {
                    selection: &mut contents.columns,
                    cursor: &mut contents.column,
                    len: columns,
                }),
            ],
        },
        Panel::Queries => match state.queries.tab() {
            QueriesTab::Cql => [None, None],
            QueriesTab::Logs => [
                Some(Axis {
                    selection: &mut state.log.selection,
                    cursor: &mut state.log.cursor,
                    len: rows,
                }),
                None,
            ],
        },
    }
}

pub(super) fn toggle_visual(state: &mut State) {
    for axis in axes(state, state.focus()).into_iter().flatten() {
        axis.selection.toggle(axis.cursor.index());
    }
}

pub(super) fn stop_extending(state: &mut State) -> bool {
    let mut stopped = false;

    for axis in axes(state, state.focus()).into_iter().flatten() {
        if axis.selection.is_extending() {
            axis.selection.stop(axis.cursor.index());
            stopped = true;
        }
    }

    stopped
}

pub(super) fn clear_selection(state: &mut State) -> bool {
    let mut cleared = false;

    for axis in axes(state, state.focus()).into_iter().flatten() {
        if axis.selection.is_active() {
            axis.selection.clear();
            cleared = true;
        }
    }

    cleared
}

/// The cursor returns to the anchor, not the range's first row: a range grown
/// upwards started at its last.
pub(super) fn collapse_selection(state: &mut State) {
    for axis in axes(state, state.focus()).into_iter().flatten() {
        if let Some(anchor) = axis.selection.anchor() {
            axis.cursor.go_to(anchor, axis.len);
        }
        axis.selection.clear();
    }
}

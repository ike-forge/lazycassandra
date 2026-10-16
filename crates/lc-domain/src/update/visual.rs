//! Visual mode: the selection axes `v`, `esc` and `y` act on.

use crate::contents::SelectionMode;
use crate::panel::Panel;
use crate::queries::QueriesTab;
use crate::selection::{Range, Ranges, Selection};
use crate::state::State;
use crate::viewport::Cursor;

use super::row_count;

/// The groups, or the one row the cursor is on when nothing is selected.
pub(super) fn spanned(selection: &Selection, cursor: usize) -> Ranges {
    let ranges = selection.ranges(cursor);

    if ranges.is_empty() {
        Ranges::of(Range::new(cursor, cursor))
    } else {
        ranges
    }
}

/// One axis a panel selects on: the run selected, and the cursor that is its
/// far end while it is being extended.
///
/// The two travel together because reading one without the other is always a
/// mistake — a `Selection` only means a range in company with its cursor, and
/// which cursor that is depends on the grid's mode.
pub(super) struct Axis<'a> {
    pub(super) selection: &'a mut Selection,
    pub(super) cursor: &'a mut Cursor,
    /// How many rows the axis has, so a cursor sent along it cannot leave it.
    pub(super) len: usize,
}

/// The axes `panel` selects on. Two for the grid in cell mode, which selects a
/// rectangle (design §3); none for Queries' Cql tab, which is an editor; one
/// for its Logs tab, a list like Tables and Ddl (query log design §5.3).
///
/// The one place a selection is matched up with its cursor. Column mode puts
/// the *column* cursor on the first axis, and a second match written out
/// elsewhere would be one edit away from disagreeing about that — silently,
/// since both are `usize`.
pub(super) fn axes(state: &mut State, panel: Panel) -> [Option<Axis<'_>>; 2] {
    let rows = row_count(state, panel);
    let columns = state.contents.column_count();
    let contents = &mut state.contents;

    match panel {
        Panel::Tables => [
            Some(Axis {
                selection: &mut state.tables.selection,
                cursor: &mut state.tables.cursor,
                len: rows,
            }),
            None,
        ],
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

/// `v`: freeze, drop or open a group at the cursor (multi-group selection
/// design §1).
pub(super) fn toggle_visual(state: &mut State) {
    for axis in axes(state, state.focus()).into_iter().flatten() {
        axis.selection.toggle(axis.cursor.index());
    }
}

/// `esc`'s second rung: leave visual mode, keeping the range. Reports whether
/// there was one to leave.
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

/// `esc`'s third rung: drop the range. Reports whether there was one.
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

/// `y`'s second half: spend the selection and send each cursor back to the end
/// it grew from (visual mode design §5.1).
///
/// A yank is one gesture rather than a mode the user is left standing in — the
/// range has been said, so it stops being drawn, and the cursor goes back to
/// where `v` was pressed rather than sitting wherever the range happened to
/// end. The anchor and not the range's first row: a selection grown upwards
/// started at its last one.
pub(super) fn collapse_selection(state: &mut State) {
    for axis in axes(state, state.focus()).into_iter().flatten() {
        // The selection is spent unconditionally (visual-mode design §5.1);
        // only the cursor's move home is conditional on an anchor surviving
        // (multi-group design §3).
        if let Some(anchor) = axis.selection.anchor() {
            axis.cursor.go_to(anchor, axis.len);
        }
        axis.selection.clear();
    }
}

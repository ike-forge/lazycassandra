//! What `y` copies, by panel (yank design).

use crate::contents::{ContentsState, SelectionMode};
use crate::feedback::Flash;
use crate::log::Entry;
use crate::model::Row;
use crate::panel::Panel;
use crate::queries::QueriesTab;
use crate::selection::{Range, Ranges};
use crate::state::State;

use super::{Effect, collapse_selection, spanned};

/// A payload of only delimiters raises no effect: an empty OSC 52 clears the
/// clipboard.
pub(super) fn yank(state: &mut State) -> Vec<Effect> {
    let line = match state.focus() {
        Panel::Contents => contents(state),
        Panel::Tables => tree_nodes(state),
        Panel::Ddl => detail_rows(state),
        Panel::Queries => log_entries(state),
    };

    let Some(line) = line.filter(|line| says_something(line)) else {
        // Spending the selection here would punish a `y` on a null.
        return Vec::new();
    };

    state.flash = Some(lit(state));
    collapse_selection(state);

    vec![Effect::Yank(line)]
}

pub(super) fn says_something(line: &str) -> bool {
    line.chars().any(|c| c != ',' && c != '\n')
}

pub(super) fn copied(payload: Option<String>) -> Vec<Effect> {
    payload
        .filter(|line| says_something(line))
        .map(Effect::Yank)
        .into_iter()
        .collect()
}

pub(super) fn lit(state: &State) -> Flash {
    let (rows, columns) = match state.focus() {
        Panel::Contents => grid_extent(state),
        Panel::Tables => (
            spanned(&state.tables.selection, state.tables.cursor().index()),
            Ranges::default(),
        ),
        Panel::Ddl => (
            spanned(&state.ddl.selection, state.ddl.cursor.index()),
            Ranges::default(),
        ),
        // Only reached over Logs: over Cql `yank` has already returned.
        Panel::Queries => (
            spanned(&state.log.selection, state.log.cursor.index()),
            Ranges::default(),
        ),
    };

    Flash::lit(state.focus(), rows, columns)
}

/// Empty on an axis means all of it. The one place the mode bounds axes, so the
/// copy and the flash agree.
pub(super) fn grid_extent(state: &State) -> (Ranges, Ranges) {
    let rows = spanned(&state.contents.rows, state.contents.cursor.index());
    let columns = spanned(&state.contents.columns, state.contents.column.index());

    match state.contents.mode {
        SelectionMode::Row => (rows, Ranges::default()),
        SelectionMode::Column => (Ranges::default(), columns),
        SelectionMode::Cell => (rows, columns),
    }
}

pub(super) fn contents(state: &State) -> Option<String> {
    let ContentsState::Loaded { page, .. } = state.contents.state() else {
        return None;
    };
    // As drawn, not as returned: filtered-out rows are not copied, and sorted
    // ones copy in drawn order.
    let drawn: Vec<Row> = page.drawn_rows();
    let (rows, columns) = grid_extent(state);
    let whole = |len: usize| Ranges::of(Range::new(0, len.saturating_sub(1)));
    let rows = if rows.is_empty() {
        whole(drawn.len())
    } else {
        rows
    };
    let columns = if columns.is_empty() {
        whole(page.page().columns.len())
    } else {
        columns
    };

    match state.contents.mode {
        SelectionMode::Cell if rows.rows() == 1 && columns.rows() == 1 => {
            let row = drawn.get(rows.first()?.first())?;
            let value = row.values.get(columns.first()?.first())?;

            crate::yank::grid_cell(value)
        }
        SelectionMode::Row | SelectionMode::Cell => {
            Some(crate::yank::grid_block(&drawn, &rows, &columns))
        }
        SelectionMode::Column => Some(crate::yank::grid_columns(&drawn, &columns)),
    }
}

pub(super) fn tree_nodes(state: &State) -> Option<String> {
    let schema = state.schema()?;
    let rows = state.tables.rows(schema);
    let selected = spanned(&state.tables.selection, state.tables.cursor().index());

    Some(crate::yank::lines(
        selected
            .indices()
            .filter_map(|index| rows.get(index))
            .map(crate::yank::node),
    ))
}

pub(super) fn detail_rows(state: &State) -> Option<String> {
    let details = crate::view::details_of(state)?;
    let selected = spanned(&state.ddl.selection, state.ddl.cursor.index());
    let rows: Vec<_> = details.rows().collect();

    Some(crate::yank::lines(
        selected
            .indices()
            .filter_map(|index| rows.get(index))
            .map(|row| crate::yank::detail_row(row)),
    ))
}

/// Each Logs entry yanks as its two rows joined.
pub(super) fn log_entries(state: &State) -> Option<String> {
    if state.queries.tab() != QueriesTab::Logs {
        return None;
    }

    let entries: Vec<&Entry> = state.log.entries().collect();
    let selected = spanned(&state.log.selection, state.log.cursor.index());

    Some(crate::yank::lines(
        selected
            .indices()
            .filter_map(|index| entries.get(index).copied())
            .map(crate::yank::log_entry),
    ))
}

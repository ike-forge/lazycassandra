//! What `y` copies, by panel
//! (docs/superpowers/specs/2026-08-13-yank-design.md).

use crate::contents::{ContentsState, SelectionMode};
use crate::feedback::Flash;
use crate::log::Entry;
use crate::model::Row;
use crate::panel::Panel;
use crate::queries::QueriesTab;
use crate::selection::{Range, Ranges};
use crate::state::State;

use super::{Effect, collapse_selection, spanned};

/// What the focused panel has under its cursor, as one line — which of the
/// panel's contents that is, and whether it is CSV, is the panel's answer.
/// Nothing under the cursor yanks nothing: there is nothing there to say
/// anything about (yank design §1).
///
/// A payload holding nothing but `,` and `\n` raises no effect either.
/// `Effect::Yank` becomes an OSC 52 sequence, and an empty payload asks the
/// terminal to clear its clipboard, so a projection with nothing but
/// delimiters to say must say nothing rather than wipe what the user had
/// copied (selection-modes design §4). Every projection converges here, so no
/// shape can reach the clipboard empty.
pub(super) fn yank(state: &mut State) -> Vec<Effect> {
    let line = match state.focus() {
        Panel::Contents => contents(state),
        Panel::Tables => tree_nodes(state),
        Panel::Ddl => detail_rows(state),
        Panel::Queries => log_entries(state),
    };

    let Some(line) = line.filter(|line| says_something(line)) else {
        // A `y` that said nothing has nothing to show and nothing to reset:
        // spending the selection here would punish the user for pressing `y`
        // on a null (visual mode design §5.1).
        return Vec::new();
    };

    state.flash = Some(lit(state));
    collapse_selection(state);

    vec![Effect::Yank(line)]
}

/// A payload of nothing but delimiters says nothing, and `Effect::Yank`
/// becomes an OSC 52 sequence: an empty one asks the terminal to clear the
/// clipboard (design §5).
pub(super) fn says_something(line: &str) -> bool {
    line.chars().any(|c| c != ',' && c != '\n')
}

/// A yank of what the inspector offers, or nothing where it offers nothing:
/// a null, and a line that says nothing.
pub(super) fn copied(payload: Option<String>) -> Vec<Effect> {
    payload
        .filter(|line| says_something(line))
        .map(Effect::Yank)
        .into_iter()
        .collect()
}

/// What `y` just took, ready to be lit once the selection that named it is
/// gone (visual mode design §6.1).
pub(super) fn lit(state: &State) -> Flash {
    let (rows, columns) = match state.focus() {
        Panel::Contents => grid_extent(state),
        Panel::Tables => (
            spanned(&state.tables.selection, state.tables.cursor.index()),
            Ranges::default(),
        ),
        Panel::Ddl => (
            spanned(&state.ddl.selection, state.ddl.cursor.index()),
            Ranges::default(),
        ),
        // Only reached over the Logs tab: over Cql, `log_entries` yanks
        // nothing and `yank` has already returned by the time this is asked.
        Panel::Queries => (
            spanned(&state.log.selection, state.log.cursor.index()),
            Ranges::default(),
        ),
    };

    Flash::lit(state.focus(), rows, columns)
}

/// The rectangle `y` reads out of the grid, on each of its two axes.
///
/// Empty on an axis is the whole of it — the convention the view model's
/// `rows_ranges` and `columns_ranges` already carry, empty for `None`, and
/// what row mode and column mode take. The one place the mode decides which
/// axes it bounds, so what is copied and what is lit afterwards cannot answer
/// differently.
pub(super) fn grid_extent(state: &State) -> (Ranges, Ranges) {
    let rows = spanned(&state.contents.rows, state.contents.cursor.index());
    let columns = spanned(&state.contents.columns, state.contents.column.index());

    match state.contents.mode {
        SelectionMode::Row => (rows, Ranges::default()),
        SelectionMode::Column => (Ranges::default(), columns),
        SelectionMode::Cell => (rows, columns),
    }
}

/// Which of the grid's two cursors `y` means is what the selection mode
/// answers (docs/superpowers/specs/2026-08-13-yank-selection-modes-design.md §1).
pub(super) fn contents(state: &State) -> Option<String> {
    let ContentsState::Loaded { page, .. } = state.contents.state() else {
        return None;
    };
    // The rows as the grid draws them, not as the cluster returned them: a
    // yank copies what is on screen, so a filtered-out row is not in it and
    // a sorted grid copies in the order it reads
    // (grid sort and filter design §4).
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
    let selected = spanned(&state.tables.selection, state.tables.cursor.index());

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

/// `None` over the Cql tab — the editor claims `y` as text before `yank` is
/// ever called, so this is unreached there, but stated rather than assumed.
/// Over the Logs tab, each selected entry yanks as its two rows joined,
/// which is what makes it paste into a ticket (query log design §5.3).
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

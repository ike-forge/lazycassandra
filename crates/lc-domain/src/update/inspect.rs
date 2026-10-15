//! The cell inspector
//! (docs/superpowers/specs/2026-09-17-cell-inspector-design.md).

use crate::contents::{ContentsState, SelectionMode};
use crate::inspect::{Inspected, Inspector};
use crate::model::{CqlValue, Key};
use crate::state::{Overlay, Panel, State};

use super::{
    Effect, copied, editable_here, escape, field_key, open_insert_commit, quit, start_edit,
};

/// The inspector's wrapped line count and body height, as the last frame
/// measured them.
pub(super) const fn inspector_extent(state: &State) -> (usize, usize) {
    (
        state.geometry.inspector_lines(),
        state.geometry.inspector_rows(),
    )
}

/// What `Enter` over the grid would inspect: the row under the cursor in row
/// mode, the cell under it in cell mode, as the grid draws them.
///
/// `None` in column mode, whose cursor names a column rather than one value;
/// with nothing loaded, on an empty grid, while a range is held —
/// `Enter` names one thing, and a selection is several — and on a frame with
/// no room for the block (cell inspector design §2, §5). Read by the hint bar
/// too, so `⏎ inspect` is named exactly where it opens.
pub(crate) fn inspection(state: &State) -> Option<Inspector> {
    if state.focus() != Panel::Contents
        || state.geometry.inspector_rows() == 0
        || state.contents.rows.is_active()
        || state.contents.columns.is_active()
    {
        return None;
    }

    let ContentsState::Loaded { subject, page, .. } = state.contents.state() else {
        return None;
    };
    let index = state.contents.cursor.index();
    page.row(index)?;
    let columns = &page.page().columns;

    let inspected = match state.contents.mode {
        SelectionMode::Row => Inspected::Row {
            number: index + 1,
            columns: columns.clone(),
            values: (0..columns.len())
                .map(|at| page.value(index, at).cloned().unwrap_or(CqlValue::Null))
                .collect(),
        },
        SelectionMode::Column => return None,
        SelectionMode::Cell => {
            let at = state.contents.column.index();

            Inspected::Cell {
                at,
                column: columns.get(at)?.clone(),
                value: page.value(index, at)?.clone(),
            }
        }
    };

    // The dirty marks the grid is already drawing, carried onto the block:
    // a reopened inspector says the same thing about the same cells.
    let edited = match state.contents.mode {
        SelectionMode::Row => (0..columns.len())
            .filter(|at| page.is_edited(index, *at))
            .collect(),
        SelectionMode::Cell | SelectionMode::Column => {
            let at = state.contents.column.index();
            if page.is_edited(index, at) {
                vec![0]
            } else {
                Vec::new()
            }
        }
    };

    Some(Inspector::opened(index, subject.clone(), inspected).with_edited(edited))
}

pub(super) fn open_inspector(state: &mut State) {
    if let Some(inspector) = inspection(state) {
        state.overlay = Some(Overlay::Inspect(inspector));
    }
}

/// `Enter` then `e`, in one press.
pub(super) fn edit_from_grid(state: &mut State) -> Vec<Effect> {
    open_inspector(state);
    if matches!(state.overlay, Some(Overlay::Inspect(_))) {
        // A row block opens on its first column, which is the partition key
        // nearly everywhere: `e` there would only ever refuse.
        if state.contents.mode == SelectionMode::Row {
            focus_first_editable(state);
        }
        return start_edit(state);
    }
    Vec::new()
}

/// Moves the open inspector's cursor to the first field `e` would open, and
/// leaves it on the first when there is none, so the refusal names that one.
fn focus_first_editable(state: &mut State) {
    let Some(Overlay::Inspect(inspector)) = &state.overlay else {
        return;
    };
    let count = inspector.field_count();

    for at in 0..count {
        if let Some(Overlay::Inspect(inspector)) = &mut state.overlay {
            inspector.focus_field(at);
        }
        if editable_here(state) {
            return;
        }
    }
    if let Some(Overlay::Inspect(inspector)) = &mut state.overlay {
        inspector.focus_field(0);
    }
}

/// Every key the inspector claims (cell inspector design §4). Anything else
/// is inert, so no key means "close" in one frame and something else in the
/// next.
pub(super) fn inspect_key(state: &mut State, key: Key) -> Vec<Effect> {
    // An open field claims every printable key and the editing keys, the same
    // rule the prompt, the form and the typed drop confirmation follow
    // (cell editing design §3).
    if matches!(&state.overlay, Some(Overlay::Inspect(inspector)) if inspector.field().is_some()) {
        return field_key(state, key);
    }

    if matches!(key, Key::Char('e')) {
        return start_edit(state);
    }

    // `⏎` closes the inspector everywhere but over a row that does not exist
    // yet, where it asks to write it (insert design §4).
    if matches!(key, Key::Enter)
        && matches!(&state.overlay, Some(Overlay::Inspect(inspector)) if inspector.is_inserting())
    {
        open_insert_commit(state);
        return Vec::new();
    }

    let (_, height) = inspector_extent(state);
    let Some(Overlay::Inspect(inspector)) = &mut state.overlay else {
        return Vec::new();
    };

    match key {
        Key::Char('j') | Key::Down => inspector.down(),
        Key::Char('k') | Key::Up => inspector.up(),
        Key::PageDown => inspector.page_down(height),
        Key::PageUp => inspector.page_up(height),
        Key::Home => inspector.to_start(),
        Key::End => inspector.to_end(),
        Key::Char('x') if inspector.is_inserting() => inspector.unset(),
        Key::Char('y') => return copied(inspector.yank()),
        Key::Char('Y') => return copied(inspector.line()),
        Key::Enter | Key::Escape => escape(state),
        Key::Char('q') => return quit(state),
        _ => {}
    }

    Vec::new()
}

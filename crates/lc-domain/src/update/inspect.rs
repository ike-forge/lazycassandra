//! The cell inspector (cell inspector design).

use crate::contents::{ContentsState, SelectionMode};
use crate::inspect::{Inspected, Inspector};
use crate::model::{CqlValue, Key};
use crate::overlay::Overlay;
use crate::panel::Panel;
use crate::state::State;

use super::{
    Effect, copied, editable_here, escape, field_key, open_insert_commit, quit, start_edit,
};

pub(super) const fn inspector_extent(state: &State) -> (usize, usize) {
    (
        state.geometry.inspector_lines(),
        state.geometry.inspector_rows(),
    )
}

/// `None` in column mode, over a held range, and where the block does not fit.
/// Read by the hint bar too.
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

    // Carries the grid's dirty marks, so a reopened inspector agrees with the
    // grid.
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

pub(super) fn edit_from_grid(state: &mut State) -> Vec<Effect> {
    open_inspector(state);
    if matches!(state.overlay, Some(Overlay::Inspect(_))) {
        // A row's first column is nearly always the partition key, where `e`
        // only refuses.
        if state.contents.mode == SelectionMode::Row {
            focus_first_editable(state);
        }
        return start_edit(state);
    }
    Vec::new()
}

/// Stays on the first when none is editable, so the refusal names it.
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

/// Unclaimed keys are inert.
pub(super) fn inspect_key(state: &mut State, key: Key) -> Vec<Effect> {
    if matches!(&state.overlay, Some(Overlay::Inspect(inspector)) if inspector.field().is_some()) {
        return field_key(state, key);
    }

    if matches!(key, Key::Char('e')) {
        return start_edit(state);
    }

    // Over a row not yet inserted, `⏎` asks to write it.
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

//! Mouse wheel and clicks (mouse input design).

use crate::model::Key;
use crate::overlay::Overlay;
use crate::panel::Panel;
use crate::pointer::{Click, Held, Mark, Pointer, Wheel};
use crate::state::State;

use super::{
    Effect, activate, axes, escape, health_entry, open_filter, overlay_key, panel_scroll,
    row_count, scroll_len, show_tab,
};

/// Rows per notch, in the panel's own unit. Three is what terminals settled on.
pub(super) const WHEEL_ROWS: usize = 3;

/// Every gesture decomposes into an existing binding.
pub(super) fn handle_pointer(state: &mut State, pointer: Pointer) -> Vec<Effect> {
    state.mark_dirty();

    // Modal: the dismissing click is spent. Through `escape` so a prompt
    // restores `previous`.
    if state.overlay.is_some() {
        match pointer {
            Pointer::Click(_) => {
                state.held = None;
                escape(state);
            }
            Pointer::Hold(key) => state.held = Some(Held { key, over: true }),
            Pointer::Drag(over) => {
                if let Some(held) = &mut state.held {
                    held.over = over == Some(held.key);
                }
            }
            Pointer::Release(over) => {
                if let Some(held) = state.held.take()
                    && over == Some(held.key)
                {
                    if let Some(Overlay::Export(form)) = &mut state.overlay
                        && form.confirm().is_none()
                    {
                        form.focus_button();
                    }
                    return overlay_key(state, held.key).unwrap_or_default();
                }
            }
            Pointer::Field(field) => {
                state.held = None;
                if let Some(Overlay::Export(form)) = &mut state.overlay
                    && form.confirm().is_none()
                {
                    form.press(field);
                }
            }
            Pointer::Modal => state.held = None,
            Pointer::Entry(row) => {
                state.held = None;
                return health_entry(state, row);
            }
            // Lists and documents scroll; questions do not.
            Pointer::Wheel { direction, .. } => {
                if scrolls(state.overlay.as_ref()) {
                    let key = match direction {
                        Wheel::Down => Key::Down,
                        Wheel::Up => Key::Up,
                    };
                    for _ in 0..WHEEL_ROWS {
                        overlay_key(state, key);
                    }
                }
            }
        }
        return Vec::new();
    }

    match pointer {
        Pointer::Click(click) => click_at(state, click),
        // Drawn only with a modal, so one arriving without is a stale frame's.
        Pointer::Hold(_)
        | Pointer::Drag(_)
        | Pointer::Release(_)
        | Pointer::Field(_)
        | Pointer::Modal
        | Pointer::Entry(_) => {
            state.held = None;
            Vec::new()
        }
        Pointer::Wheel { panel, direction } => {
            wheel(state, panel, direction);
            Vec::new()
        }
    }
}

fn scrolls(overlay: Option<&Overlay>) -> bool {
    matches!(
        overlay,
        Some(
            Overlay::Commands(_)
                | Overlay::Diagnostics(_)
                | Overlay::Inspect(_)
                | Overlay::Themes { .. }
                | Overlay::Connections { .. }
                | Overlay::History { .. }
                | Overlay::Health(_)
                | Overlay::Commit(_)
        )
    )
}

/// Neither focuses nor moves the selection: a keypress returns you.
pub(super) fn wheel(state: &mut State, panel: Panel, direction: Wheel) {
    let len = scroll_len(state, panel);
    let height = state.geometry.height(panel);

    let Some(scroll) = panel_scroll(state, panel) else {
        return;
    };

    for _ in 0..WHEEL_ROWS {
        match direction {
            Wheel::Down => scroll.down(len, height),
            Wheel::Up => scroll.up(),
        }
    }
}

pub(super) fn click_at(state: &mut State, click: Click) -> Vec<Effect> {
    state.focus_on(click.panel());

    // A press names one row, so the range would make the next `y` lie.
    for axis in axes(state, click.panel()).into_iter().flatten() {
        axis.selection.clear();
    }

    let (panel, row, column) = match click {
        Click::Row { panel, row, column } => (panel, Some(row), column),
        Click::Column { panel, column } => (panel, None, Some(column)),
        // A mark names its own column, so it acts whatever the grid's mode.
        Click::Mark {
            panel,
            column,
            mark,
        } => {
            if panel == Panel::Contents && column < state.contents.column_count() {
                state
                    .contents
                    .column
                    .go_to(column, state.contents.column_count());

                match mark {
                    Mark::Sort => state.contents.cycle_sort(column),
                    Mark::Filter => open_filter(state),
                }
            }
            return Vec::new();
        }
        Click::Tab { panel, tab } => {
            show_tab(state, panel, tab);
            return Vec::new();
        }
        // A press on the buffer is `4` then `⏎`.
        Click::Panel(Panel::Queries) => {
            state.queries.start_editing();
            return Vec::new();
        }
        Click::Panel(_) => return Vec::new(),
    };

    // Neither cursor changes the selection mode; only `m` does.
    if let Some(column) = column {
        let columns = state.contents.column_count();
        if panel == Panel::Contents && column < columns {
            state.contents.column.go_to(column, columns);
        }
    }

    let Some(row) = row else {
        return Vec::new();
    };
    let len = row_count(state, panel);

    // The map describes the previous frame: a row gone since is dropped, not
    // clamped, or Tables would read a table nobody pointed at.
    if row >= len {
        return Vec::new();
    }

    match panel {
        Panel::Tables => {
            state.move_tree_cursor(row);
            activate(state)
        }
        Panel::Contents => {
            state.contents.cursor.go_to(row, len);
            Vec::new()
        }
        // The viewport follows on `Msg::Measured`: only the renderer knows how
        // the row wrapped.
        Panel::Ddl => {
            state.ddl.cursor.go_to(row, len);
            Vec::new()
        }
        Panel::Queries => Vec::new(),
    }
}

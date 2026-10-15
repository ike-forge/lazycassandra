//! Mouse wheel and clicks
//! (docs/superpowers/specs/2026-08-09-mouse-input-design.md).

use crate::model::Key;
use crate::pointer::{Click, Mark, Pointer, Wheel};
use crate::state::{Overlay, Panel, State};

use super::{
    Effect, activate, axes, escape, open_filter, overlay_key, panel_scroll, row_count, scroll_len,
    show_tab,
};

/// Rows a wheel notch moves, in whatever unit the panel scrolls in. One is
/// sluggish enough that the pointer feels broken; three is what terminals
/// settled on (design §6).
pub(super) const WHEEL_ROWS: usize = 3;

/// The one place a pointer gesture becomes behaviour, as `handle_key` is for
/// keys (docs/shortcuts.md §2).
///
/// Every gesture decomposes into a binding that already exists (design §1): a
/// click on a tree row is a selection followed by the `Enter` arm, and a wheel
/// notch is repeated motions.
pub(super) fn handle_pointer(state: &mut State, pointer: Pointer) -> Vec<Effect> {
    state.mark_dirty();

    // The overlay is modal: it spends the click that dismisses it rather than
    // letting it act on a row it was covering (design §2). Routed through
    // `escape` rather than a bare `state.overlay = None`, so a click on an
    // open prompt restores `previous` instead of dropping it; with any other
    // overlay up, `escape` only dismisses it, so nothing else changes.
    if state.overlay.is_some() {
        match pointer {
            Pointer::Click(_) => escape(state),
            Pointer::Button(key) => return overlay_key(state, key).unwrap_or_default(),
            Pointer::Modal => {}
            // The modals that are a list or a document: wheeling one is
            // reading it, and a notch is the `j`/`k` the overlay already
            // answers (cell inspector design §4, design §1). The rest are
            // questions, not things to scroll.
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
        Pointer::Button(_) | Pointer::Modal => Vec::new(),
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

/// Scrolls `panel`'s viewport without focusing it and without moving its
/// selection: the wheel is a look somewhere else, and a keypress returns you
/// (docs/superpowers/specs/2026-08-09-list-viewport-design.md §1).
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

    // A press names one row, so it spends the range rather than standing
    // beside it and making the next `y` lie (design §4).
    for axis in axes(state, click.panel()).into_iter().flatten() {
        axis.selection.clear();
    }

    let (panel, row, column) = match click {
        Click::Row { panel, row, column } => (panel, Some(row), column),
        Click::Column { panel, column } => (panel, None, Some(column)),
        // A mark is the `s`/`f` binding reached by pointer: it names its own
        // column, so it moves the cursor there and acts, whatever mode the
        // grid is in — a press says which column it means, where a key has
        // only the cursor to go on (design §5).
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
        // A tab is a place, and pressing it is how one is reached: this is
        // the `[`/`]` arm of `handle_key`, reached by pointer (design §1).
        Click::Tab { panel, tab } => {
            show_tab(state, panel, tab);
            return Vec::new();
        }
        // A press on the buffer is `4` then `⏎`: pointing at text says where
        // the next keys go (cql edit mode design §4).
        Click::Panel(Panel::Queries) => {
            state.queries.start_editing();
            return Vec::new();
        }
        Click::Panel(_) => return Vec::new(),
    };

    // Neither cursor changes the selection mode. A press names whatever cells
    // it can, and the mode is what decides which of them the grid draws a band
    // around — `m` is still the only way to change that.
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

    // The map a press was resolved against describes the *previous* frame. A
    // row that has gone since is dropped rather than clamped onto whatever
    // took its place: in Tables, clamping would read a table nobody pointed at
    // (design §6).
    if row >= len {
        return Vec::new();
    }

    match panel {
        Panel::Tables => {
            state.tables.cursor.go_to(row, len);
            activate(state)
        }
        Panel::Contents => {
            state.contents.cursor.go_to(row, len);
            Vec::new()
        }
        // The offset the viewport owes this cursor is `Msg::Measured`'s to
        // compute, exactly as it is after `j`: only the renderer knows which
        // lines the row wrapped to.
        Panel::Ddl => {
            state.ddl.cursor.go_to(row, len);
            Vec::new()
        }
        // Queries is an editor: it has no rows for a press to land on.
        Panel::Queries => Vec::new(),
    }
}

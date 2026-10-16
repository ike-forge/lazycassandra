//! Cursor and viewport motion, and keeping viewports on their cursors
//! across frames.

use crate::config::Config;
use crate::contents::{ContentsState, SelectionMode};
use crate::geometry::Geometry;
use crate::grid::{self, ColumnScroll};
use crate::overlay::Overlay;
use crate::panel::Panel;
use crate::queries::{CqlMode, QueriesTab};
use crate::state::State;
use crate::viewport::{Cursor, Scroll};
use std::borrow::Cow;

use super::Effect;
use super::commit::commit_extent;
use super::connections::offer_connections;
use super::export::cancel_pending_export;
use super::inspect::inspector_extent;
use super::themes::restore_theme;

#[derive(Clone, Copy)]
pub(super) enum Motion {
    Up,
    Down,
    PageUp,
    PageDown,
    Start,
    End,
}

/// Applies `motion` to the focused panel's selection.
///
/// The focused panel and no other. A wheel names the panel under the pointer
/// and moves its viewport directly ([`wheel`]), so nothing but a key reaches
/// here.
///
/// In Ddl the height is counted in wrapped lines and the cursor in rows, so a
/// page overshoots on a run of wrapped values. Accepted rather than corrected:
/// the fix needs a line-to-row map from the renderer, and `home`/`end` are
/// exact regardless (design §4).
pub(super) fn scroll(state: &mut State, motion: Motion) {
    let panel = state.focus();

    // The Cql tab's rows are its statements. Reachable while the buffer is
    // closed alone — an open one claims these keys as motions before
    // `panel_key` is consulted — which is the mode the band is drawn in
    // (cql statement selection design §4).
    if panel == Panel::Queries && state.queries.mode() == Some(CqlMode::Browsing) {
        step_statement(state, motion);
        return;
    }

    let height = state.geometry.height(panel);
    let len = row_count(state, panel);

    let Some(cursor) = panel_cursor(state, panel) else {
        return;
    };

    match motion {
        Motion::Down => cursor.down(len),
        Motion::Up => cursor.up(),
        Motion::PageDown => cursor.page_down(len, height),
        Motion::PageUp => cursor.page_up(height),
        Motion::Start => cursor.to_start(),
        Motion::End => cursor.to_end(len),
    }
}

/// The Cql tab's motions, which walk statements rather than rows: the band
/// names what `ctrl-r` would run, and stepping it is what makes a buffer of
/// several statements navigable (cql statement selection design §4).
///
/// Paging is left inert. The buffer's viewport is derived from the cursor
/// rather than stored (queries editor design §5), so a statement step already
/// brings its own statement on screen and there is no second thing for a page
/// to move.
pub(super) fn step_statement(state: &mut State, motion: Motion) {
    match motion {
        Motion::Down => state.queries.step_statement(true),
        Motion::Up => state.queries.step_statement(false),
        Motion::Start => state.queries.edge_statement(false),
        Motion::End => state.queries.edge_statement(true),
        Motion::PageDown | Motion::PageUp => {}
    }
}

/// How many rows `panel`'s *cursor* moves over — entries in the Logs tab. The
/// Cql tab never arrives here: the editor claims the motion keys first
/// (queries editor design §3).
pub(super) fn row_count(state: &State, panel: Panel) -> usize {
    match panel {
        Panel::Tables => tree_rows(state),
        Panel::Ddl => crate::view::ddl_rows(state),
        Panel::Contents => state.contents.row_count(),
        Panel::Queries => match state.queries.tab() {
            QueriesTab::Cql => 0,
            QueriesTab::Logs => state.log.len(),
        },
    }
}

/// The Cql tab never arrives here — the editor claims the motion keys before
/// this match is reached (design §3) — so that arm is unreachable in
/// practice and kept for exhaustiveness.
pub(super) fn panel_cursor(state: &mut State, panel: Panel) -> Option<&mut Cursor> {
    match panel {
        Panel::Tables => Some(&mut state.tables.cursor),
        Panel::Ddl => Some(&mut state.ddl.cursor),
        Panel::Contents => Some(&mut state.contents.cursor),
        Panel::Queries => match state.queries.tab() {
            QueriesTab::Cql => None,
            QueriesTab::Logs => Some(&mut state.log.cursor),
        },
    }
}

/// Records what the last frame turned out to be, and repairs whatever that
/// invalidates.
///
/// No unconditional `mark_dirty`: this message describes the frame that
/// produced it, so redrawing on every one of them would re-measure and redraw
/// forever (panel scrolling design §3).
pub(super) fn measured(state: &mut State, geometry: Geometry) -> Vec<Effect> {
    let previous = std::mem::replace(&mut state.geometry, geometry);

    follow_viewports(state, previous, geometry);

    // The grid projects only as many rows as its panel is tall.
    if geometry.height(Panel::Contents) != previous.height(Panel::Contents) {
        state.mark_dirty();
    }

    // The bar's `? commands` hint is a function of `command_rows`
    // (view/hints.rs): a resize that flips it between zero and non-zero
    // changes the view model as surely as a viewport clamp does.
    if geometry.command_rows() != previous.command_rows() {
        state.mark_dirty();
    }

    close_unfitting_overlay(state, geometry);
    refit_overlay(state, geometry);

    // Last, so a picker is offered only into a frame the guards above have
    // already emptied. `init` had no frame to size one against, so this
    // measurement is where an unchosen connection gets its offer
    // (connection picker design §4).
    offer_connections(state);

    Vec::new()
}

/// Keeps every panel's viewport on its selection across a change of frame.
pub(super) fn follow_viewports(state: &mut State, previous: Geometry, geometry: Geometry) {
    // A panel that changed height must draw its selection again: the renderer
    // stopped arranging that, and `update`'s epilogue follows only a cursor
    // that *moved* — a resize moves none
    // (docs/superpowers/specs/2026-08-09-list-viewport-design.md §5).
    //
    // Keyed on the height rather than on the message, so a wheel cannot be
    // undone here: a wheel changes no height, and `lc-app` reports no
    // measurement for a frame whose geometry did not change.
    for panel in LISTS {
        let height = geometry.height(panel);

        if height == previous.height(panel) {
            continue;
        }

        let index = cursor_index(state, panel);

        if let Some(scroll) = panel_scroll(state, panel) {
            scroll.follow(index, height);
        }
    }

    // Follows here rather than in `update`'s epilogue because the epilogue's
    // geometry describes the *previous* frame, and so the previously selected
    // row: there is no correct offset for it to compute (design §5.4).
    //
    // Unconditional, and safe from the wheel without a guard: a wheel changes
    // no height, no wrapped-line count and no span, so `lc-app` sends no
    // measurement at all. The same covers ctrl-e and ctrl-y.
    let before = state.ddl.scroll;
    let height = geometry.height(Panel::Ddl);
    state
        .ddl
        .scroll
        .follow_span(geometry.ddl_selection(), height);
    state.ddl.scroll.clamp_to(geometry.ddl_lines(), height);

    if state.ddl.scroll != before {
        state.mark_dirty();
    }

    // The Logs tab's viewport, for the same reason as the Ddl block above:
    // its offset counts wrapped lines rather than entries, so it is followed
    // by span here rather than by index in the `LISTS` loop (design §5.4).
    let log_before = state.log.scroll;
    let log_height = geometry.height(Panel::Queries);
    state
        .log
        .scroll
        .follow_span(geometry.log_selection(), log_height);
    state.log.scroll.clamp_to(geometry.log_lines(), log_height);

    if state.log.scroll != log_before {
        state.mark_dirty();
    }
}

/// Closes an overlay the frame shrank past: nothing would draw it, and it
/// would still be capturing every key (design §5.1). Leaves the way `Esc`
/// would, so the theme picker restores the theme it opened on rather than
/// stranding the preview, and an export confirmation completes its pending
/// trail entry.
pub(super) fn close_unfitting_overlay(state: &mut State, geometry: Geometry) {
    let Some(overlay) = &state.overlay else {
        return;
    };

    if fits(overlay, geometry, &state.config) {
        return;
    }

    restore_theme(state);
    cancel_pending_export(state);
    state.overlay = None;
    state.mark_dirty();
}

/// Whether `geometry` leaves room to draw `overlay`. Each block answers on its
/// own gate: they are different sizes, and one that fits where another does
/// not must not borrow the other's threshold.
pub(super) fn fits(overlay: &Overlay, geometry: Geometry, config: &Config) -> bool {
    match overlay {
        Overlay::Commands(_) | Overlay::Diagnostics(_) => geometry.command_rows() > 0,
        // Config themes can make the picker's list not fit where a palette
        // would (design §9).
        Overlay::Themes { .. } => geometry.theme_rows() > 0,
        // An empty list is guidance, not a list: there are no rows for a frame
        // to be too short for.
        Overlay::Connections { .. } => {
            config.connections.is_empty() || geometry.connection_rows() > 0
        }
        Overlay::History { .. } => geometry.history_rows() > 0,
        Overlay::Health(_) => geometry.health_rows() > 0,
        Overlay::Inspect(_) => geometry.inspector_rows() > 0,
        Overlay::Commit(_) => geometry.commit_rows() > 0,
        Overlay::Export(_) => geometry.export_fits(),
        // A different block from the export form, and fits where it does not
        // (drop design §1).
        Overlay::Drop(_) => geometry.drop_fits(),
        Overlay::Message(_)
        | Overlay::Search(_)
        | Overlay::Filter(_)
        | Overlay::Discard(_)
        | Overlay::Fatal(_) => true,
    }
}

/// Pulls a scrolling overlay's offset back inside its body: a narrower frame
/// can rewrap the body shorter than the offset it was left at. The commit
/// confirmation has no cursor, so only the inspector has a line to follow
/// (cell editing design §5).
pub(super) fn refit_overlay(state: &mut State, geometry: Geometry) {
    let (lines, height) = inspector_extent(state);
    if let Some(Overlay::Inspect(inspector)) = &mut state.overlay {
        let before = inspector.offset();
        inspector.follow(geometry.inspector_selection(), height);
        inspector.clamp_to(lines, height);

        if inspector.offset() != before {
            state.dirty = true;
        }
    }

    let (lines, height) = commit_extent(state);
    if let Some(Overlay::Commit(prompt)) = &mut state.overlay {
        let before = prompt.offset();
        prompt.clamp_to(lines, height);

        if prompt.offset() != before {
            state.dirty = true;
        }
    }
}

/// The panels whose viewport `update` follows by index. Ddl keeps its own: its
/// offset counts wrapped lines rather than rows, so it is followed by span
/// in `measured` instead (design §5.1).
pub(super) const LISTS: [Panel; 2] = [Panel::Tables, Panel::Contents];

/// The viewport `panel` draws through, for the panels that have one.
pub(super) fn panel_scroll(state: &mut State, panel: Panel) -> Option<&mut Scroll> {
    match panel {
        Panel::Tables => Some(&mut state.tables.scroll),
        Panel::Contents => Some(&mut state.contents.scroll),
        Panel::Ddl => Some(&mut state.ddl.scroll),
        Panel::Queries => match state.queries.tab() {
            QueriesTab::Cql => None,
            QueriesTab::Logs => Some(&mut state.log.scroll),
        },
    }
}

/// How long the document `panel` scrolls over is — wrapped lines in Ddl and in
/// the Logs tab, rows elsewhere. The Cql tab has no document to scroll: the
/// editor claims its motion keys first (queries editor design §3).
pub(super) fn scroll_len(state: &State, panel: Panel) -> usize {
    match panel {
        Panel::Ddl => state.geometry.ddl_lines(),
        Panel::Tables | Panel::Contents => row_count(state, panel),
        Panel::Queries => match state.queries.tab() {
            QueriesTab::Cql => 0,
            QueriesTab::Logs => state.geometry.log_lines(),
        },
    }
}

/// `panel`'s selected row, or zero for a panel with no selection.
pub(super) fn cursor_index(state: &State, panel: Panel) -> usize {
    match panel {
        Panel::Tables => state.tables.cursor.index(),
        Panel::Ddl => state.ddl.cursor.index(),
        Panel::Contents => state.contents.cursor.index(),
        Panel::Queries => match state.queries.tab() {
            QueriesTab::Cql => 0,
            QueriesTab::Logs => state.log.cursor.index(),
        },
    }
}

/// Each list panel's selected index, for the comparison in [`update`].
///
/// Built from `LISTS` rather than written out, so the array this returns and
/// the one [`follow_moved_cursors`] walks cannot fall out of order — a
/// mismatch there would follow each panel against the other's index, silently.
pub(super) fn cursor_indices(state: &State) -> [usize; 2] {
    LISTS.map(|panel| cursor_index(state, panel))
}

/// Pulls every list offset back inside its document.
///
/// Unconditional because a shrinking list does not change the geometry —
/// folding a keyspace leaves every panel the same height, so no
/// [`Msg::Measured`] arrives to clamp against (design §3). `clamp_to` only
/// ever reduces an offset, so this cannot fight a wheel bounded by the same
/// arithmetic.
pub(super) fn clamp_lists(state: &mut State) {
    for panel in LISTS {
        let len = row_count(state, panel);
        let height = state.geometry.height(panel);

        if let Some(scroll) = panel_scroll(state, panel) {
            scroll.clamp_to(len, height);
        }
    }

    // Not in the loop above: `LISTS` is the panels whose offsets are followed
    // by index, and Ddl's offset counts lines while its cursor counts rows
    // (design §5.1). The clamp is unconditional for the same reason the loop
    // is — a schema reload that shrinks a table changes no geometry, so no
    // measurement arrives to clamp against.
    state.ddl.cursor.clamp_to(crate::view::ddl_rows(state));

    // Beside the cursor clamps and for the same reason: a schema reload or a
    // fold that shrinks a list changes no geometry, so no measurement arrives
    // to clamp against (design §2).
    state.tables.selection.clamp_to(tree_rows(state));
    state.ddl.selection.clamp_to(crate::view::ddl_rows(state));

    // The row cursor as well as the ranges: a filter shrinks the grid under
    // a cursor no page change reset (grid sort and filter design §4).
    state.contents.cursor.clamp_to(state.contents.row_count());
    state.contents.rows.clamp_to(state.contents.row_count());
    state
        .contents
        .columns
        .clamp_to(state.contents.column_count());
}

/// Pulls the offset of each list whose selection moved, so the selection is
/// drawn.
///
/// Only the ones that moved: a wheel leaves both indices alone, and following
/// it would undo the scroll it just performed (design §3).
pub(super) fn follow_moved_cursors(state: &mut State, before: [usize; 2]) {
    for (panel, before) in LISTS.into_iter().zip(before) {
        let index = cursor_index(state, panel);

        if index == before {
            continue;
        }

        let height = state.geometry.height(panel);

        if let Some(scroll) = panel_scroll(state, panel) {
            scroll.follow(index, height);
        }
    }
}

/// Which edge of the drawn column window a press works from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Side {
    Left,
    Right,
}

/// `h`/`l` on the grid: the column cursor by one, or — in row mode — the
/// window by one.
///
/// The cursor is what moves in both cases, because it is what
/// [`follow_grid_columns`] carries the window with (selection modes design
/// §3). The modes differ only in where it is put: in cell and column mode the
/// cursor is on screen, so moving it one column is a move the user can see,
/// and the window comes along once it reaches an edge. Row mode paints no
/// column, so those same presses are invisible until the cursor has crossed a
/// whole screen of columns — the grid stands still under a key documented to
/// move it, and a wide table reads as one that cannot be scrolled at all.
/// There the cursor lands on the column just outside the window instead, so
/// every press reveals a column, `csvlens`-style.
pub(super) fn step_column(state: &mut State, side: Side) {
    let column = match state.contents.mode {
        SelectionMode::Row => match beyond(state, side) {
            Some(column) => column,
            // The window is against that edge of the page. Left as it is
            // rather than clamped to the edge column: in row mode that would
            // be a cursor move nothing draws, and the modes the user can cycle
            // to would then open somewhere the presses never showed them.
            None => return,
        },
        SelectionMode::Cell | SelectionMode::Column => match side {
            Side::Left => return state.contents.column.up(),
            Side::Right => return state.contents.column.down(state.contents.column_count()),
        },
    };

    state
        .contents
        .column
        .go_to(column, state.contents.column_count());
}

/// The column just outside the drawn window on `side`, or `None` when the
/// window already ends there — including when the whole page fits, where
/// there is nothing to scroll in either direction.
pub(super) fn beyond(state: &State, side: Side) -> Option<usize> {
    let (widths, available) = grid_columns(state)?;
    let first = state.contents.column_scroll.first();

    match side {
        Side::Left => first.checked_sub(1),
        Side::Right => {
            let next = first + grid::fitting(&widths, first, available);

            (next < widths.len()).then_some(next)
        }
    }
}

/// The columns the grid is drawing over, and the width they have.
///
/// `None` when there is no page: nothing to fit, and nothing to follow.
pub(super) fn grid_columns(state: &State) -> Option<(Cow<'_, [u16]>, u16)> {
    let ContentsState::Loaded { page, .. } = state.contents.state() else {
        return None;
    };

    Some(page.drawn(state.geometry.grid_width()))
}

/// Pulls the column viewport over the selected cell, and back inside a page
/// that narrowed or a panel that grew.
///
/// Unconditional, for [`clamp_lists`]'s reason: a new page changes no
/// geometry, so no [`Msg::Measured`] arrives to clamp against.
pub(super) fn follow_grid_columns(state: &mut State) {
    let column = state.contents.column.index();

    let Some((widths, available)) = grid_columns(state) else {
        state.contents.column_scroll = ColumnScroll::default();
        return;
    };

    // Moved out and back so the widths can stay borrowed from the page they
    // were measured with.
    let mut columns = state.contents.column_scroll;
    // Clamp first, follow second, mirroring `clamp_lists` before
    // `follow_moved_cursors` on the row axis: the clamp answers "is the
    // offset too far right for this page and this width", which can leave the
    // cursor off screen, and the follow is what puts it back. The other order
    // would let the clamp undo the follow and hide the selected cell.
    columns.clamp_to(&widths, available);
    columns.follow(column, &widths, available);

    state.contents.column_scroll = columns;
}

pub(super) fn tree_rows(state: &State) -> usize {
    state
        .schema()
        .map_or(0, |schema| state.tables.rows(schema).len())
}

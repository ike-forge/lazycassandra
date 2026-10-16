//! Cursor and viewport motion, and keeping viewports on their cursors across
//! frames.

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

/// In Ddl a page overshoots on wrapped values: height counts lines, the cursor
/// counts rows. Fixing it needs a line-to-row map from the renderer.
pub(super) fn scroll(state: &mut State, motion: Motion) {
    let panel = state.focus();

    // The band walks statements only while the buffer is closed; an open one
    // claims these keys first.
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

/// Paging is inert: the editor's viewport is derived from the cursor, so a
/// statement step already scrolls.
pub(super) fn step_statement(state: &mut State, motion: Motion) {
    match motion {
        Motion::Down => state.queries.step_statement(true),
        Motion::Up => state.queries.step_statement(false),
        Motion::Start => state.queries.edge_statement(false),
        Motion::End => state.queries.edge_statement(true),
        Motion::PageDown | Motion::PageUp => {}
    }
}

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

/// The Cql arm is unreachable: the editor claims motion keys first.
pub(super) fn panel_cursor(state: &mut State, panel: Panel) -> Option<&mut Cursor> {
    match panel {
        Panel::Tables => Some(state.tables.cursor_mut()),
        Panel::Ddl => Some(&mut state.ddl.cursor),
        Panel::Contents => Some(&mut state.contents.cursor),
        Panel::Queries => match state.queries.tab() {
            QueriesTab::Cql => None,
            QueriesTab::Logs => Some(&mut state.log.cursor),
        },
    }
}

/// No unconditional `mark_dirty`: this message describes the frame that
/// produced it, so redrawing on it would loop.
pub(super) fn measured(state: &mut State, geometry: Geometry) -> Vec<Effect> {
    let previous = std::mem::replace(&mut state.geometry, geometry);

    follow_viewports(state, previous, geometry);

    if geometry.height(Panel::Contents) != previous.height(Panel::Contents) {
        state.mark_dirty();
    }

    // The bar's `? commands` hint depends on `command_rows`.
    if geometry.command_rows() != previous.command_rows() {
        state.mark_dirty();
    }

    close_unfitting_overlay(state, geometry);
    refit_overlay(state, geometry);

    // Last, so a picker is offered only into a frame the guards above emptied.
    offer_connections(state);

    Vec::new()
}

pub(super) fn follow_viewports(state: &mut State, previous: Geometry, geometry: Geometry) {
    // A resize moves no cursor, so `update`'s epilogue would not follow it.
    // Keyed on height so a wheel, which changes none, is not undone.
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

    // Here rather than in the epilogue, whose geometry is the previous frame's.
    // A wheel changes no height, so `lc-app` sends no measurement for it.
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

    // Followed by span, like Ddl: the offset counts wrapped lines.
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

/// An overlay nothing draws would still capture every key. Leaves the way `Esc`
/// would, so a theme preview is restored.
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

/// Each block answers on its own gate: the blocks are different sizes.
pub(super) fn fits(overlay: &Overlay, geometry: Geometry, config: &Config) -> bool {
    match overlay {
        Overlay::Commands(_) | Overlay::Diagnostics(_) => geometry.command_rows() > 0,
        Overlay::Themes { .. } => geometry.theme_rows() > 0,
        // An empty list is guidance, with no rows to be too short for.
        Overlay::Connections { .. } => {
            config.connections.is_empty() || geometry.connection_rows() > 0
        }
        Overlay::History { .. } => geometry.history_rows() > 0,
        Overlay::Health(_) => geometry.health_rows() > 0,
        Overlay::Inspect(_) => geometry.inspector_rows() > 0,
        Overlay::Commit(_) => geometry.commit_rows() > 0,
        Overlay::Export(_) => geometry.export_fits(),
        Overlay::Drop(_) => geometry.drop_fits(),
        Overlay::Message(_)
        | Overlay::Search(_)
        | Overlay::Filter(_)
        | Overlay::Discard(_)
        | Overlay::Fatal(_) => true,
    }
}

/// A narrower frame can rewrap the body shorter than its offset.
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

/// Ddl is followed by span in `measured`: its offset counts wrapped lines.
pub(super) const LISTS: [Panel; 2] = [Panel::Tables, Panel::Contents];

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

pub(super) fn cursor_index(state: &State, panel: Panel) -> usize {
    match panel {
        Panel::Tables => state.tables.cursor().index(),
        Panel::Ddl => state.ddl.cursor.index(),
        Panel::Contents => state.contents.cursor.index(),
        Panel::Queries => match state.queries.tab() {
            QueriesTab::Cql => 0,
            QueriesTab::Logs => state.log.cursor.index(),
        },
    }
}

/// Built from `LISTS` so the order matches [`follow_moved_cursors`].
pub(super) fn cursor_indices(state: &State) -> [usize; 2] {
    LISTS.map(|panel| cursor_index(state, panel))
}

/// Unconditional: folding shrinks a list without changing geometry, so no
/// [`Msg::Measured`] arrives to clamp against.
pub(super) fn clamp_lists(state: &mut State) {
    for panel in LISTS {
        let len = row_count(state, panel);
        let height = state.geometry.height(panel);

        if let Some(scroll) = panel_scroll(state, panel) {
            scroll.clamp_to(len, height);
        }
    }

    // Not in `LISTS`: Ddl's offset counts lines while its cursor counts rows.
    state.ddl.cursor.clamp_to(crate::view::ddl_rows(state));

    state.tables.selection.clamp_to(tree_rows(state));
    state.ddl.selection.clamp_to(crate::view::ddl_rows(state));

    // A filter shrinks the grid under a cursor no page change reset.
    state.contents.cursor.clamp_to(state.contents.row_count());
    state.contents.rows.clamp_to(state.contents.row_count());
    state
        .contents
        .columns
        .clamp_to(state.contents.column_count());
}

/// Only the ones that moved: following a wheel would undo it.
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Side {
    Left,
    Right,
}

/// Row mode paints no column, so moving the cursor inside the window would be
/// invisible; there it jumps just outside the window, so every press reveals a
/// column.
pub(super) fn step_column(state: &mut State, side: Side) {
    let column = match state.contents.mode {
        SelectionMode::Row => match beyond(state, side) {
            Some(column) => column,
            // Left in place rather than clamped: in row mode the clamp would be
            // a cursor move nothing draws.
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

pub(super) fn grid_columns(state: &State) -> Option<(Cow<'_, [u16]>, u16)> {
    let ContentsState::Loaded { page, .. } = state.contents.state() else {
        return None;
    };

    Some(page.drawn(state.geometry.grid_width()))
}

/// Unconditional, as in [`clamp_lists`]: a new page changes no geometry.
pub(super) fn follow_grid_columns(state: &mut State) {
    let column = state.contents.column.index();

    let Some((widths, available)) = grid_columns(state) else {
        state.contents.column_scroll = ColumnScroll::default();
        return;
    };

    // Moved out and back so the widths stay borrowed from the page.
    let mut columns = state.contents.column_scroll;
    // Clamp before follow: the other order lets the clamp hide the selected
    // cell.
    columns.clamp_to(&widths, available);
    columns.follow(column, &widths, available);

    state.contents.column_scroll = columns;
}

pub(super) fn tree_rows(state: &State) -> usize {
    state
        .schema()
        .map_or(0, |schema| state.tables.rows(schema).len())
}

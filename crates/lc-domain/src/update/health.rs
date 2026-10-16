//! The table health block (table health design).

use std::time::Duration;

use crate::contents::TableRef;
use crate::health::{Health, HealthPanel};
use crate::log::{Outcome, Trace};
use crate::model::{DbError, Key, QueryId};
use crate::overlay::Overlay;
use crate::panel::Panel;
use crate::state::State;
use crate::style::Severity;
use crate::tree::Node;

use super::{Effect, escape, logged, quit, sentence};

/// Shared with the hint bar.
pub(crate) fn health_target(state: &State) -> Option<TableRef> {
    if state.focus() != Panel::Tables || state.tables.selects_many() {
        return None;
    }

    match state.tables.selected(state.schema()?)? {
        Node::Table { keyspace, table } => Some(TableRef::new(&keyspace.name, &table.name)),
        Node::Keyspace { .. } => None,
    }
}

pub(super) fn open_health(state: &mut State) -> Vec<Effect> {
    if state.focus() != Panel::Tables {
        return Vec::new();
    }
    let Some(table) = health_target(state) else {
        if state.tables.selects_many() {
            state.notify(Severity::Info, "health takes one table, not a selection");
        }
        return Vec::new();
    };
    let Some(conn) = state.conn_id() else {
        return Vec::new();
    };
    if state.geometry.health_rows() == 0 {
        state.notify(Severity::Info, "no room to draw the health");
        return Vec::new();
    }

    let request = QueryId::next();
    logged(
        state,
        sentence(format!("Reading the health of {}", table.qualified())),
        Outcome::Pending {
            trace: Trace::Query(request),
        },
    );
    state.overlay = Some(Overlay::Health(HealthPanel::new(table.clone(), request)));
    state.mark_dirty();

    vec![Effect::LoadHealth {
        conn,
        request,
        table,
    }]
}

/// Completes the trail even if the block was closed: the read happened.
pub(super) fn health_loaded(
    state: &mut State,
    request: QueryId,
    health: Result<Health, DbError>,
    took: Duration,
) -> Vec<Effect> {
    let outcome = match &health {
        Ok(health) => Outcome::Succeeded {
            detail: format!("{} nodes", health.nodes.len()),
            took,
        },
        Err(error) => Outcome::Failed {
            detail: error.to_string(),
        },
    };
    state.log.complete(Trace::Query(request), outcome);

    if let Some(Overlay::Health(panel)) = &mut state.overlay
        && panel.request() == request
    {
        panel.landed(health);
    }
    state.mark_dirty();

    Vec::new()
}

pub(super) fn health_key(state: &mut State, key: Key) -> Vec<Effect> {
    match key {
        Key::Char('q') => return quit(state),
        Key::Escape => {
            escape(state);
            return Vec::new();
        }
        _ => {}
    }

    let height = state.geometry.health_rows();
    let Some(Overlay::Health(panel)) = &mut state.overlay else {
        return Vec::new();
    };

    match key {
        Key::Char('j') | Key::Down => panel.down(),
        Key::Char('k') | Key::Up => panel.up(),
        Key::PageDown => panel.page_down(height),
        Key::PageUp => panel.page_up(height),
        Key::Home => panel.to_start(),
        Key::End => panel.to_end(),
        Key::Char('l') | Key::Right => panel.step_in(),
        Key::Char('h') | Key::Left => panel.step_out(),
        Key::Enter => panel.toggle(),
        _ => return Vec::new(),
    }
    state.mark_dirty();

    Vec::new()
}

/// A row the previous frame drew and this one lacks is dropped, not clamped.
pub(super) fn health_entry(state: &mut State, row: usize) -> Vec<Effect> {
    let Some(Overlay::Health(panel)) = &mut state.overlay else {
        return Vec::new();
    };
    if !panel.select(row) {
        return Vec::new();
    }

    health_key(state, Key::Enter)
}

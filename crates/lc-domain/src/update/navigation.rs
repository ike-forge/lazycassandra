use crate::contents::TableRef;
use crate::ddl::DdlTab;
use crate::panel::Panel;
use crate::queries::QueriesTab;
use crate::state::State;
use crate::tree::Node;

use super::{Effect, Side, read, step_column};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Step {
    Forward,
    Back,
}

/// Queries is the exception: each tab has its own number.
pub(super) fn focus_by_number(state: &mut State, key: char) {
    if let Some(tab) = QueriesTab::from_number(key) {
        state.focus_on(Panel::Queries);
        state.show_queries_tab(tab);
        state.queries.start_editing();
        return;
    }

    let Some(panel) = Panel::from_number(key) else {
        return;
    };

    if state.focus() == panel {
        walk_tab(state, Step::Forward);
    } else {
        state.focus_on(panel);
    }
}

/// A tab the strip does not draw is skipped: a keyspace's DDL panel offers one
/// tab.
pub(super) fn walk_tab(state: &mut State, step: Step) {
    match state.focus() {
        Panel::Ddl => {}
        Panel::Queries => {
            let shown = state.queries.tab();
            state.show_queries_tab(match step {
                Step::Forward => shown.next(),
                Step::Back => shown.previous(),
            });
            return;
        }
        Panel::Tables | Panel::Contents => return,
    }

    let shown = state.ddl.tab();
    let candidate = match step {
        Step::Forward => shown.next(),
        Step::Back => shown.previous(),
    };

    if crate::view::ddl_tabs(state).contains(&candidate) {
        state.ddl.show(candidate);
    }
}

/// Every `(panel, tab)` pair a strip draws is a stop, so `Tab` alone reaches
/// what the numbers do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Landing {
    Tables,
    Ddl(DdlTab),
    Contents,
    Queries(QueriesTab),
}

impl Landing {
    /// Built fresh: DDL's strip depends on what the Tables cursor is on.
    pub(super) fn all(state: &State) -> Vec<Self> {
        std::iter::once(Self::Tables)
            .chain(crate::view::ddl_tabs(state).iter().copied().map(Self::Ddl))
            .chain(std::iter::once(Self::Contents))
            .chain(QueriesTab::ALL.into_iter().map(Self::Queries))
            .collect()
    }

    pub(super) fn current(state: &State) -> Self {
        match state.focus() {
            Panel::Tables => Self::Tables,
            Panel::Ddl => Self::Ddl(state.ddl.tab()),
            Panel::Contents => Self::Contents,
            Panel::Queries => Self::Queries(state.queries.tab()),
        }
    }

    /// Both, even with focus already there: [`State::focus_on`] reverts Queries
    /// to `Cql` when focus leaves.
    pub(super) fn arrive(self, state: &mut State) {
        match self {
            Self::Tables => state.focus_on(Panel::Tables),
            Self::Ddl(tab) => {
                state.focus_on(Panel::Ddl);
                state.ddl.show(tab);
            }
            Self::Contents => state.focus_on(Panel::Contents),
            Self::Queries(tab) => {
                state.focus_on(Panel::Queries);
                state.show_queries_tab(tab);
            }
        }
    }
}

pub(super) fn walk_landing(state: &mut State, step: Step) {
    let stops = Landing::all(state);
    let current = Landing::current(state);
    let index = stops.iter().position(|stop| *stop == current).unwrap_or(0);

    let next = match step {
        Step::Forward => (index + 1) % stops.len(),
        Step::Back => (index + stops.len() - 1) % stops.len(),
    };

    stops[next].arrive(state);
}

/// Reaches the same `show` the keys do, so pointer and key cannot diverge.
pub(super) fn show_tab(state: &mut State, panel: Panel, tab: usize) {
    match panel {
        // The map describes the previous frame: a strip that lost a tab drops
        // the press.
        Panel::Ddl => {
            if let Some(tab) = crate::view::ddl_tabs(state).get(tab).copied() {
                state.ddl.show(tab);
            }
        }
        Panel::Queries => {
            if let Some(tab) = QueriesTab::ALL.get(tab).copied() {
                state.show_queries_tab(tab);
            }
        }
        Panel::Tables | Panel::Contents => {}
    }
}

#[derive(Clone, Copy)]
pub(super) enum Transition {
    Open,
    Close,
}

pub(super) fn go_right(state: &mut State) -> Vec<Effect> {
    match state.focus() {
        Panel::Tables => return step_in(state),
        Panel::Contents => step_column(state, Side::Right),
        Panel::Ddl | Panel::Queries => {}
    }

    Vec::new()
}

/// `⏎` folds an open keyspace; `l` steps into it.
pub(super) fn step_in(state: &mut State) -> Vec<Effect> {
    let table = {
        let Some((tables, schema)) = state.tables_and_schema() else {
            return Vec::new();
        };

        match tables.selected(schema) {
            // Reading needs one table. Folding addresses the cursor's row, so
            // it is left alone.
            Some(Node::Table { .. }) if tables.selects_many() => return Vec::new(),
            Some(Node::Table { keyspace, table }) => TableRef::new(&keyspace.name, &table.name),
            _ => {
                transition(state, Transition::Open);
                return Vec::new();
            }
        }
    };

    read(state, table)
}

pub(super) fn go_left(state: &mut State) {
    match state.focus() {
        Panel::Tables => transition(state, Transition::Close),
        Panel::Contents => step_column(state, Side::Left),
        Panel::Ddl | Panel::Queries => {}
    }
}

pub(super) fn transition(state: &mut State, transition: Transition) {
    if state.focus() != Panel::Tables {
        return;
    }

    // The search guard lives in `TablesPanel::open`/`close`, so every entry
    // point inherits it.
    let Some((tables, schema)) = state.tables_and_schema() else {
        return;
    };

    match transition {
        Transition::Open => tables.open(schema),
        Transition::Close => tables.close(schema),
    }
}

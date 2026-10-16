//! Moving between panels, tabs and tree levels.

use crate::contents::TableRef;
use crate::ddl::DdlTab;
use crate::panel::Panel;
use crate::queries::QueriesTab;
use crate::state::State;
use crate::tree::Node;

use super::{Effect, Side, read, step_column};

/// Which way [`walk_tab`] steps through a panel's strip, or [`walk_landing`]
/// through the `Tab` cycle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Step {
    Forward,
    Back,
}

/// Focuses the panel `key` numbers and, where it already had focus, steps to
/// its next tab.
///
/// One key for both: a panel is the place, and its tabs are what that place
/// can show — so a reader who knows how to arrive already knows how to look at
/// the rest (docs/shortcuts.md §4). A panel with one tab answers the second
/// press by staying where it is. Queries is the exception: each of its tabs
/// has a number, and `4` arrives ready to type.
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

/// Steps the focused panel's strip one tab in `step`, wrapping.
///
/// A tab the strip does not draw is not a place to arrive at: on a keyspace
/// the DDL panel offers one tab, and walking stays on it. Queries reaches
/// here only with its buffer closed — an open one has already claimed `]`
/// and `[` as text (cql edit mode design §2).
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

/// One stop in the `Tab`/`BackTab` cycle: a panel, and — where it has more
/// than one — the tab it lands on.
///
/// Every `(panel, tab)` pair a strip can draw is a stop, DDL's tabs and
/// Queries' two included, so `Tab` alone walks everything a number and its
/// repeats reach (docs/shortcuts.md §2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Landing {
    Tables,
    Ddl(DdlTab),
    Contents,
    Queries(QueriesTab),
}

impl Landing {
    /// Every stop the cycle can be at right now, in the order it visits them.
    ///
    /// Built fresh rather than cached: DDL's strip depends on what the
    /// Tables cursor is on, so a keyspace's single tab is one stop here and a
    /// table's two are two, the same distinction [`walk_tab`] draws for `2`.
    pub(super) fn all(state: &State) -> Vec<Self> {
        std::iter::once(Self::Tables)
            .chain(crate::view::ddl_tabs(state).iter().copied().map(Self::Ddl))
            .chain(std::iter::once(Self::Contents))
            .chain(QueriesTab::ALL.into_iter().map(Self::Queries))
            .collect()
    }

    /// The stop the frame is sitting on right now.
    pub(super) fn current(state: &State) -> Self {
        match state.focus() {
            Panel::Tables => Self::Tables,
            Panel::Ddl => Self::Ddl(state.ddl.tab()),
            Panel::Contents => Self::Contents,
            Panel::Queries => Self::Queries(state.queries.tab()),
        }
    }

    /// Focuses the panel this stop names and draws the tab it names, in one
    /// move.
    ///
    /// Both, even where the panel already has focus: [`State::focus_on`]
    /// reverts the Queries tab to `Cql` the moment focus leaves it, so a
    /// landing on `Queries(Logs)` reached only by focusing and never told
    /// which tab to draw would be undone by its own arrival.
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

/// Steps `Tab`/`BackTab` to the next or previous [`Landing`], wrapping.
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

/// Draws the `tab`th tab of the strip `panel` last drew. `click_at` has
/// already focused it.
///
/// Reaches the same `show` the keys do — [`walk_tab`] for DDL, [`Landing`]
/// for Queries — so a tab arrived at by pointer and one arrived at by key
/// cannot diverge.
pub(super) fn show_tab(state: &mut State, panel: Panel, tab: usize) {
    match panel {
        // The map a press was resolved against describes the previous frame,
        // so a strip that has since lost a tab drops the press rather than
        // showing whichever tab took that position (design §6).
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

/// `l` and `→`: into the tree, or on to the next column.
///
/// The four horizontal keys mean *move left* and *move right*, and each panel
/// resolves that in its own vocabulary — the shape `j`/`k` already have
/// (docs/shortcuts.md §2, docs/superpowers/specs/2026-08-10-table-display-design.md §2).
pub(super) fn go_right(state: &mut State) -> Vec<Effect> {
    match state.focus() {
        Panel::Tables => return step_in(state),
        Panel::Contents => step_column(state, Side::Right),
        Panel::Ddl | Panel::Queries => {}
    }

    Vec::new()
}

/// `l` and `→` in Tables: a keyspace opens, a table is read.
///
/// *Move right* out of a table row is the panel to its right — the tree has no
/// further level, and Contents is what a table's contents are drawn in — so
/// the key lands where `⏎` already does rather than doing nothing
/// (docs/shortcuts.md §4). The keyspace arm keeps [`TablesPanel::open`]'s own
/// vocabulary, which is what makes it not merely `⏎`'s synonym: `⏎` folds an
/// open keyspace, `l` steps into it.
pub(super) fn step_in(state: &mut State) -> Vec<Effect> {
    let table = {
        let Some((tables, schema)) = state.tables_and_schema() else {
            return Vec::new();
        };

        match tables.selected(schema) {
            // Reading a table needs one table, the argument `activate` makes
            // for `⏎` (docs/shortcuts.md §4). Folding is left alone: it
            // addresses the cursor's row, not the selection.
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

    // The search guard lives in TablesPanel::open/close themselves (design
    // §4.1), so every entry point — this one, and activate's click/Enter/Space
    // path — inherits it from one place rather than two that must agree.
    let Some((tables, schema)) = state.tables_and_schema() else {
        return;
    };

    match transition {
        Transition::Open => tables.open(schema),
        Transition::Close => tables.close(schema),
    }
}

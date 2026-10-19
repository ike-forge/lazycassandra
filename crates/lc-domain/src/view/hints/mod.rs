//! What the focused panel responds to, as the bar says it. A hint must be true
//! in the frame it is drawn: greyed where its key would do nothing this frame,
//! absent where it never could.

mod bindings;
#[cfg(test)]
mod tests;

use bindings::{
    CLEAR, CLEAR_FILTER, CLOSE, COMMIT, COMMIT_HINT, COMMIT_ROW, COMPLETE, CONNECTIONS,
    CONNECTIONS_GLOBAL, COPY, COPY_LINE, DELETE, DELETE_TYPED, DELETE_YES_NO, DIAGNOSTICS_GLOBAL,
    DISCARD, DONE, DROP, DROP_TYPED, DROP_YES_NO, EDIT, ENTER, EVERY_PAGE, EXPORT,
    EXPORT_OVERWRITE, EXPORT_ROWS, FIELD, FILTER, FORMAT, HEALTH, HEALTH_GLOBAL, HEALTH_OF,
    HISTORY, HISTORY_GLOBAL, HISTORY_HINT, INSERT, INSERT_ROW, INSPECT_GLOBAL, INSPECT_ROW, LEAVE,
    MOVE, NEXT_MATCH, NEXT_OPTION, NEXT_PAGE, PALETTE_GLOBAL, RUN, SEARCH, SEARCH_GLOBAL, SELECT,
    SORT, STOP, TOGGLE, UNSET,
};
pub(crate) use bindings::{GLOBAL, THEMES, THEMES_GLOBAL};

use crate::drop::DropPrompt;
use crate::export::{ExportForm, Stop};
use crate::inspect::Inspector;
use crate::model::{Key, KeyPress, Statement};
use crate::overlay::Overlay;
use crate::panel::Panel;
use crate::queries::QueriesTab;
use crate::search::{Direction, Searched};
use crate::state::State;

/// Keyed by presses, not display text, so `update::tests` can press every one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Binding {
    pub keys: &'static [Stroke],
    pub action: &'static str,
}

impl Binding {
    const fn new(keys: &'static [Stroke], action: &'static str) -> Self {
        Self { keys, action }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Stroke(KeyPress);

impl Stroke {
    #[must_use]
    pub const fn key(key: Key) -> Self {
        Self(KeyPress::new(key))
    }

    #[must_use]
    pub const fn ctrl(key: Key) -> Self {
        Self(KeyPress::ctrl(key))
    }

    #[must_use]
    pub const fn press(self) -> KeyPress {
        self.0
    }
}

/// Greyed rather than dropped, so the bar does not shift as the buffer fills.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Liveness {
    Live,
    Inert,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hint {
    pub keys: String,
    pub action: &'static str,
    pub liveness: Liveness,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HintBar {
    pub panel: Vec<Hint>,
    pub global: Vec<Hint>,
    pub export: Option<String>,
}

pub(crate) fn bar(state: &State) -> HintBar {
    HintBar {
        export: state
            .export
            .as_ref()
            .map(|job| format!("exporting {} rows…", job.rows())),
        ..overlay_bar(state).unwrap_or_else(|| panel_bar(state))
    }
}

fn fixed(panel: &[Binding], global: &[Binding]) -> HintBar {
    HintBar {
        panel: panel.iter().map(rendered).collect(),
        global: global.iter().map(rendered).collect(),
        export: None,
    }
}

fn overlay_bar(state: &State) -> Option<HintBar> {
    // A sifting overlay takes text: the prompt's two ways out.
    if sifting(state) {
        return Some(fixed(&[], SEARCH_GLOBAL));
    }

    Some(match state.overlay.as_ref()? {
        // No panel group: motion is assumed.
        Overlay::Commands(_) => fixed(&[], PALETTE_GLOBAL),
        Overlay::Themes { .. } => fixed(THEMES, THEMES_GLOBAL),
        Overlay::Connections { .. } => connections_bar(state),
        Overlay::History { .. } => fixed(HISTORY, HISTORY_GLOBAL),
        Overlay::Health(_) => fixed(HEALTH, HEALTH_GLOBAL),
        Overlay::Diagnostics(_) => fixed(&[], DIAGNOSTICS_GLOBAL),
        Overlay::Inspect(inspector) => inspect_bar(state, inspector),
        Overlay::Commit(_) => fixed(&[], COMMIT),
        Overlay::Discard(_) => fixed(&[], DISCARD),
        Overlay::Search(_) | Overlay::Filter(_) => fixed(&[], SEARCH_GLOBAL),
        Overlay::Export(form) => export_bar(form),
        Overlay::Drop(prompt) => drop_bar(prompt),
        Overlay::Message(_) | Overlay::Fatal(_) => return None,
    })
}

/// Panel-specific hints first, then the common tail: `/`, `n`, `v`, the `esc`
/// rung, `?`. The tail is what survives a narrow frame, `?` longest.
fn panel_bar(state: &State) -> HintBar {
    let completing = state.queries.completion().is_some() && state.takes_text();
    let mut panel = Vec::new();
    let (mut search, mut select) = (None, None);
    for binding in state
        .hints()
        .iter()
        .filter(|binding| !completing || !binding.keys.contains(&ENTER))
        .filter(|binding| !refused(state, binding))
        .filter(|binding| **binding != INSPECT_ROW || crate::update::inspection(state).is_some())
        .filter(|binding| !pages(binding) || state.contents.has_more())
    {
        let hint = rendered_live(binding, live(state, binding));
        match *binding {
            SEARCH => search = Some(hint),
            SELECT => select = Some(hint),
            _ => panel.push(hint),
        }
    }
    if completing {
        panel.insert(0, rendered(&COMPLETE));
    }

    let target = Searched::of(state.focus()).filter(|target| state.search(*target).is_some());
    let next_match =
        target.map(|target| rendered_live(&NEXT_MATCH, next_match_is_live(state, target)));
    // `y` and `v` are bound in the same panels.
    let copy = select.as_ref().map(|_| rendered(&COPY));
    let mut common: Vec<Hint> = [search, next_match, select, copy]
        .into_iter()
        .flatten()
        .collect();
    if state.focus() == Panel::Contents && crate::update::has_uncommitted(state) {
        panel.push(rendered(&COMMIT_HINT));
    }
    if state.focus() == Panel::Queries
        && state.queries.tab() == QueriesTab::Cql
        && !state.history().is_empty()
        && state.geometry.history_rows() > 0
    {
        panel.push(rendered(&HISTORY_HINT));
    }
    if state.focus() == Panel::Contents && state.contents.fetching_all() {
        common.push(rendered(&STOP));
    } else if state.takes_text() {
        common.push(rendered(&DONE));
    } else if target.is_some() || selected(state) {
        common.push(rendered(&CLEAR));
    } else if state.focus() != Panel::default() {
        common.push(rendered(&LEAVE));
    }

    // `?` is text in an open buffer; dropped where no palette fits.
    if !state.takes_text() && state.geometry.command_rows() > 0 {
        common.extend(GLOBAL.iter().map(rendered));
    }

    HintBar {
        panel,
        global: common,
        export: None,
    }
}

/// An empty picker names neither `/` nor `⏎`.
fn connections_bar(state: &State) -> HintBar {
    let listed = !state.config.connections.is_empty();

    HintBar {
        panel: if listed {
            CONNECTIONS.iter().map(rendered).collect()
        } else {
            Vec::new()
        },
        global: CONNECTIONS_GLOBAL
            .iter()
            .filter(|binding| listed || !binding.keys.contains(&ENTER))
            .map(rendered)
            .collect(),
        export: None,
    }
}

/// No panel group: the confirmation claims every key.
fn drop_bar(prompt: &DropPrompt) -> HintBar {
    let bindings = match (prompt.takes_text(), prompt.target().row_count().is_some()) {
        (true, false) => DROP_TYPED,
        (true, true) => DELETE_TYPED,
        (false, false) => DROP_YES_NO,
        (false, true) => DELETE_YES_NO,
    };

    HintBar {
        panel: Vec::new(),
        global: bindings
            .iter()
            .map(|binding| rendered_live(binding, binding.keys != [ENTER] || prompt.confirmed()))
            .collect(),
        export: None,
    }
}

/// `y` greys over a null value, `Y` over a null field's line.
fn inspect_bar(state: &State, inspector: &Inspector) -> HintBar {
    // An open field names only what it answers.
    if inspector.field().is_some() {
        return HintBar {
            panel: Vec::new(),
            global: FIELD
                .iter()
                .map(|binding| {
                    rendered_live(binding, binding.keys != [ENTER] || inspector.can_accept())
                })
                .collect(),
            export: None,
        };
    }

    let mut panel = vec![rendered(&MOVE)];
    if !inspector.is_inserting() {
        panel.push(rendered_live(&COPY, inspector.yank().is_some()));
        panel.push(rendered_live(&COPY_LINE, inspector.line().is_some()));
    }
    panel.push(rendered_live(&EDIT, crate::update::editable_here(state)));
    if inspector.is_inserting() {
        panel.push(rendered_live(&UNSET, inspector.is_set_at_cursor()));
        panel.push(rendered(&INSERT));
    } else if state.posture().permits_mutation() {
        panel.push(rendered_live(&COMMIT_ROW, !inspector.edited().is_empty()));
    }

    HintBar {
        panel,
        global: INSPECT_GLOBAL.iter().map(rendered).collect(),
        export: None,
    }
}

/// The stop under `tab` stands in for the panel group. No `q`: the form claims
/// it.
fn export_bar(form: &ExportForm) -> HintBar {
    if form.confirm().is_some() {
        return fixed(&[], EXPORT_OVERWRITE);
    }

    let stop = match form.stop() {
        Stop::Scope | Stop::Format => Some(rendered(&TOGGLE)),
        Stop::Path | Stop::Rows => None,
        Stop::Export => Some(rendered(&EXPORT)),
    };

    HintBar {
        panel: std::iter::once(rendered(&NEXT_OPTION))
            .chain(stop)
            .collect(),
        global: vec![rendered(&CLOSE)],
        export: None,
    }
}

fn sifting(state: &State) -> bool {
    match &state.overlay {
        Some(Overlay::Commands(panel)) => panel.sift().is_some(),
        Some(
            Overlay::Themes { sift, .. }
            | Overlay::Connections { sift, .. }
            | Overlay::History { sift, .. },
        ) => sift.is_some(),
        _ => false,
    }
}

/// Under a live range `esc` clears it, so the bar names it.
fn selected(state: &State) -> bool {
    match state.focus() {
        Panel::Tables => state.tables.selection.is_active(),
        Panel::Ddl => state.ddl.selection.is_active(),
        Panel::Contents => state.contents.rows.is_active() || state.contents.columns.is_active(),
        // The Cql tab is an editor with no selection.
        Panel::Queries => {
            state.queries.tab() == QueriesTab::Logs && state.log.selection.is_active()
        }
    }
}

fn live(state: &State, binding: &Binding) -> bool {
    match *binding {
        // Follows the statement as typed: the same key runs a `SELECT` and is
        // refused a `DELETE`.
        RUN => state.queries.statement().is_some_and(|cql| {
            state.posture().permits_mutation() || !Statement::typed(&cql).mutates()
        }),
        FORMAT => {
            let text = state.queries.text();
            crate::cql::format(&text) != text
        }
        NEXT_PAGE | EVERY_PAGE => state.contents.can_fetch(),
        EXPORT_ROWS => state.contents.row_count() > 0 && state.geometry.export_fits(),
        SORT | FILTER => state.contents.loaded().is_some(),
        CLEAR_FILTER => state
            .contents
            .filter(state.contents.column.index())
            .is_some(),
        // Reads the same derivation as `d` itself.
        DROP | DELETE => {
            state.geometry.drop_fits()
                && state.dropping.is_none()
                && crate::update::drop_target(state).is_some()
        }
        INSERT_ROW => {
            state.geometry.inspector_rows() > 0 && crate::update::insert_form(state).is_ok()
        }
        HEALTH_OF => {
            state.geometry.health_rows() > 0 && crate::update::health_target(state).is_some()
        }
        _ => true,
    }
}

fn pages(binding: &Binding) -> bool {
    matches!(*binding, NEXT_PAGE | EVERY_PAGE)
}

/// Left off the bar rather than greyed; the palette still names it. Not
/// `ctrl-r`, which follows the buffer.
fn refused(state: &State, binding: &Binding) -> bool {
    matches!(*binding, DROP | DELETE | INSERT_ROW) && !state.posture().permits_mutation()
}

/// `false` when the term matches nothing, and for a Contents search with no
/// page loaded.
fn next_match_is_live(state: &State, target: Searched) -> bool {
    match target {
        Searched::Tables => state.schema().is_some_and(|schema| {
            state
                .tables
                .next_match(schema, state.tables.cursor().index(), Direction::Forwards)
                .is_some()
        }),
        Searched::Ddl => {
            crate::view::ddl_next_match(state, state.ddl.cursor.index(), Direction::Forwards)
                .is_some()
        }
        Searched::Contents => state
            .contents
            .next_match(
                (state.contents.cursor.index(), state.contents.column.index()),
                Direction::Forwards,
            )
            .is_some(),
        Searched::Queries => state
            .log
            .next_match(state.log.cursor.index(), Direction::Forwards)
            .is_some(),
    }
}

fn rendered(binding: &Binding) -> Hint {
    rendered_live(binding, true)
}

fn rendered_live(binding: &Binding, live: bool) -> Hint {
    Hint {
        keys: label(binding.keys),
        action: binding.action,
        liveness: if live {
            Liveness::Live
        } else {
            Liveness::Inert
        },
    }
}

pub(crate) fn label(keys: &[Stroke]) -> String {
    keys.iter()
        .map(|stroke| name(stroke.press()))
        .collect::<Vec<_>>()
        .join("/")
}

fn name(press: KeyPress) -> String {
    let key = key_name(press.key);

    if press.modifiers.ctrl {
        format!("ctrl-{key}")
    } else {
        key
    }
}

/// Named, not drawn: arrow glyphs collide and fonts may draw them as boxes.
fn key_name(key: Key) -> String {
    match key {
        Key::Char(' ') => "space".to_owned(),
        Key::Char(character) => character.to_string(),
        Key::Enter => "⏎".to_owned(),
        Key::Escape => "esc".to_owned(),
        Key::Backspace => "bksp".to_owned(),
        Key::Tab => "tab".to_owned(),
        Key::BackTab => "shift-tab".to_owned(),
        Key::Delete => "del".to_owned(),
        Key::Insert => "ins".to_owned(),
        Key::Home => "home".to_owned(),
        Key::End => "end".to_owned(),
        Key::PageUp => "pgup".to_owned(),
        Key::PageDown => "pgdn".to_owned(),
        Key::Up => "↑".to_owned(),
        Key::Down => "↓".to_owned(),
        Key::Left => "←".to_owned(),
        Key::Right => "→".to_owned(),
        Key::F(number) => format!("F{number}"),
    }
}

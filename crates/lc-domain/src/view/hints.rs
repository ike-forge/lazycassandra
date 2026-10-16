//! What the focused panel responds to, as the bar says it. A hint must be true
//! in the frame it is drawn: greyed where its key would do nothing this frame,
//! absent where it never could.

use crate::contents::SelectionMode;
use crate::model::{Key, KeyPress};
use crate::overlay::Overlay;
use crate::panel::Panel;
use crate::queries::{CqlMode, QueriesTab};
use crate::search::{Direction, Searched};
use crate::state::State;

/// Keyed by presses, not display text, so `update::tests` can press every one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Binding {
    pub keys: &'static [Stroke],
    pub action: &'static str,
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

/// Repeated in every bar: a user who never presses `tab` cannot find the other
/// panels. The bar drops from the left as the frame narrows, so order is
/// survival order; [`QUIT`] goes last.
pub(crate) const GLOBAL: &[Binding] = &[
    Binding {
        keys: &[Stroke::key(Key::Tab)],
        action: "panel",
    },
    Binding {
        keys: &[Stroke::key(Key::Char('?'))],
        action: "commands",
    },
];

/// Not in [`GLOBAL`]: in a panel that takes text `q` types itself.
pub(crate) const QUIT: Binding = Binding {
    keys: &[Stroke::key(Key::Char('q'))],
    action: "quit",
};

/// `esc` is named only here among the globals: with the palette open it always
/// closes it. `⏎ run` first, so it is dropped first.
const PALETTE_GLOBAL: &[Binding] = &[
    Binding {
        keys: &[Stroke::key(Key::Enter)],
        action: "run",
    },
    Binding {
        keys: &[Stroke::key(Key::Char('?')), Stroke::key(Key::Escape)],
        action: "close",
    },
    Binding {
        keys: &[Stroke::key(Key::Char('q'))],
        action: "quit",
    },
    Binding {
        keys: &[Stroke::key(Key::Char('/'))],
        action: "search",
    },
];

/// No `quit`: `diagnostics_key` claims `q`.
const DIAGNOSTICS_GLOBAL: &[Binding] = &[Binding {
    keys: &[Stroke::key(Key::Escape)],
    action: "close",
}];

/// `j/k` named: nothing else says the block moves.
pub(crate) const INSPECT: &[Binding] = &[
    Binding {
        keys: &[Stroke::key(Key::Char('j')), Stroke::key(Key::Char('k'))],
        action: "move",
    },
    Binding {
        keys: &[Stroke::key(Key::Char('y'))],
        action: "copy",
    },
    Binding {
        keys: &[Stroke::key(Key::Char('Y'))],
        action: "copy line",
    },
];

const INSPECT_GLOBAL: &[Binding] = &[Binding {
    keys: &[Stroke::key(Key::Escape)],
    action: "close",
}];

const EDIT: Binding = Binding {
    keys: &[Stroke::key(Key::Char('e'))],
    action: "edit",
};

const INSERT: Binding = Binding {
    keys: &[Stroke::key(Key::Enter)],
    action: "insert",
};

const UNSET: Binding = Binding {
    keys: &[Stroke::key(Key::Char('x'))],
    action: "unset",
};

const COMMIT_ROW: Binding = Binding {
    keys: &[Stroke::ctrl(Key::Char('s'))],
    action: "commit row",
};

/// No panel group and no `q`: the field claims every printable key.
const FIELD: &[Binding] = &[
    Binding {
        keys: &[Stroke::key(Key::Enter)],
        action: "save",
    },
    Binding {
        keys: &[Stroke::key(Key::Escape)],
        action: "cancel",
    },
];

const COMMIT: &[Binding] = &[
    Binding {
        keys: &[Stroke::key(Key::Char('y'))],
        action: "write them",
    },
    Binding {
        keys: &[Stroke::key(Key::Char('n')), Stroke::key(Key::Escape)],
        action: "cancel",
    },
];

const DISCARD: &[Binding] = &[
    Binding {
        keys: &[Stroke::key(Key::Char('y'))],
        action: "quit and discard",
    },
    Binding {
        keys: &[Stroke::key(Key::Char('n')), Stroke::key(Key::Escape)],
        action: "stay",
    },
];

/// Left off entirely where nothing is uncommitted.
const COMMIT_HINT: Binding = Binding {
    keys: &[Stroke::ctrl(Key::Char('s'))],
    action: "commit",
};

/// `j`/`k` named as "preview": the motion's side effect is what it teaches.
pub(crate) const THEMES: &[Binding] = &[
    Binding {
        keys: &[Stroke::key(Key::Char('j')), Stroke::key(Key::Char('k'))],
        action: "preview",
    },
    Binding {
        keys: &[Stroke::key(Key::Char('/'))],
        action: "search",
    },
];

/// `⏎` keeps the previewed theme; `esc` restores the original.
pub(crate) const THEMES_GLOBAL: &[Binding] = &[
    Binding {
        keys: &[Stroke::key(Key::Enter)],
        action: "keep",
    },
    Binding {
        keys: &[Stroke::key(Key::Escape)],
        action: "cancel",
    },
    Binding {
        keys: &[Stroke::key(Key::Char('q'))],
        action: "quit",
    },
];

/// No `j`/`k`: moving this cursor previews nothing.
pub(crate) const CONNECTIONS: &[Binding] = &[Binding {
    keys: &[Stroke::key(Key::Char('/'))],
    action: "search",
}];

/// Last of the panel's hints, so a narrow bar drops it before `run`.
const HISTORY_HINT: Binding = Binding {
    keys: &[Stroke::ctrl(Key::Char('g'))],
    action: "history",
};

/// `⏎` appends and runs nothing, the difference from `ctrl-r` worth saying.
pub(crate) const HISTORY: &[Binding] = &[Binding {
    keys: &[Stroke::key(Key::Char('/'))],
    action: "search",
}];

pub(crate) const HISTORY_GLOBAL: &[Binding] = &[
    Binding {
        keys: &[Stroke::key(Key::Enter)],
        action: "append",
    },
    Binding {
        keys: &[Stroke::key(Key::Escape)],
        action: "close",
    },
    Binding {
        keys: &[Stroke::key(Key::Char('q'))],
        action: "quit",
    },
];

/// `⏎` named: it folds here rather than choosing.
pub(crate) const HEALTH: &[Binding] = &[Binding {
    keys: &[Stroke::key(Key::Enter)],
    action: "fold/unfold",
}];

pub(crate) const HEALTH_GLOBAL: &[Binding] = &[
    Binding {
        keys: &[Stroke::key(Key::Escape)],
        action: "close",
    },
    Binding {
        keys: &[Stroke::key(Key::Char('q'))],
        action: "quit",
    },
];

pub(crate) const CONNECTIONS_GLOBAL: &[Binding] = &[
    Binding {
        keys: &[Stroke::key(Key::Enter)],
        action: "connect",
    },
    Binding {
        keys: &[Stroke::key(Key::Escape)],
        action: "close",
    },
    Binding {
        keys: &[Stroke::key(Key::Char('q'))],
        action: "quit",
    },
];

/// Two sets: the spellings answer different keys. No `q`: `drop_key` claims it.
pub(crate) const DROP_YES_NO: &[Binding] = &[
    Binding {
        keys: &[Stroke::key(Key::Char('y'))],
        action: "drop it",
    },
    Binding {
        keys: &[Stroke::key(Key::Char('n')), Stroke::key(Key::Escape)],
        action: "cancel",
    },
];

/// The verb is the only difference worth a word.
pub(crate) const DELETE_YES_NO: &[Binding] = &[
    Binding {
        keys: &[Stroke::key(Key::Char('y'))],
        action: "delete them",
    },
    Binding {
        keys: &[Stroke::key(Key::Char('n')), Stroke::key(Key::Escape)],
        action: "cancel",
    },
];

/// `⏎` is greyed until the typed name matches.
pub(crate) const DROP_TYPED: &[Binding] = &[
    Binding {
        keys: &[Stroke::key(Key::Enter)],
        action: "drop it",
    },
    Binding {
        keys: &[Stroke::key(Key::Escape)],
        action: "cancel",
    },
];

pub(crate) const DELETE_TYPED: &[Binding] = &[
    Binding {
        keys: &[Stroke::key(Key::Enter)],
        action: "delete them",
    },
    Binding {
        keys: &[Stroke::key(Key::Escape)],
        action: "cancel",
    },
];

const NEXT_OPTION: Binding = Binding {
    keys: &[Stroke::key(Key::Tab)],
    action: "next option",
};

/// Text on the path stop.
const TOGGLE: Binding = Binding {
    keys: &[Stroke::key(Key::Char(' '))],
    action: "toggle",
};

const EXPORT: Binding = Binding {
    keys: &[Stroke::key(Key::Char(' ')), Stroke::key(Key::Enter)],
    action: "export",
};

/// No `q`: the form claims it.
const EXPORT_CLOSE: Binding = Binding {
    keys: &[Stroke::key(Key::Escape)],
    action: "close",
};

pub(crate) const EXPORT_OVERWRITE: &[Binding] = &[
    Binding {
        keys: &[Stroke::key(Key::Char(' ')), Stroke::key(Key::Enter)],
        action: "overwrite",
    },
    Binding {
        keys: &[Stroke::key(Key::Escape)],
        action: "back",
    },
];

/// Empty panel group: the focused panel responds to nothing under a prompt.
const SEARCH_GLOBAL: &[Binding] = &[
    Binding {
        keys: &[Stroke::key(Key::Enter)],
        action: "apply",
    },
    Binding {
        keys: &[Stroke::key(Key::Escape)],
        action: "cancel",
    },
];

/// Only while a popup is up; the `tab panel` and `⏎ newline` hints drop for
/// those frames.
const COMPLETE: Binding = Binding {
    keys: &[Stroke::key(Key::Tab), Stroke::key(Key::Enter)],
    action: "complete",
};

const CLEAR: Binding = Binding {
    keys: &[Stroke::key(Key::Escape)],
    action: "clear",
};

/// [`CLEAR`] wins where both apply: one key, one hint.
const LEAVE: Binding = Binding {
    keys: &[Stroke::key(Key::Escape)],
    action: "leave",
};

const STOP: Binding = Binding {
    keys: &[Stroke::key(Key::Escape)],
    action: "stop",
};

/// Not [`LEAVE`]: closing the buffer keeps focus.
const DONE: Binding = Binding {
    keys: &[Stroke::key(Key::Escape)],
    action: "done",
};

const NEXT_MATCH: Binding = Binding {
    keys: &[Stroke::key(Key::Char('n'))],
    action: "next match",
};

/// An empty picker names neither `/` nor `⏎`.
fn connections_bar(state: &State, export: Option<String>) -> HintBar {
    let listed = !state.config.connections.is_empty();

    HintBar {
        panel: if listed {
            CONNECTIONS.iter().map(rendered).collect()
        } else {
            Vec::new()
        },
        global: CONNECTIONS_GLOBAL
            .iter()
            .filter(|binding| listed || !binding.keys.contains(&Stroke::key(Key::Enter)))
            .map(rendered)
            .collect(),
        export,
    }
}

fn overlay_bar(state: &State, export: Option<&String>) -> Option<HintBar> {
    let export = export.cloned();

    // A sifting overlay takes text: the prompt's two ways out.
    if sifting(state) {
        return Some(HintBar {
            panel: Vec::new(),
            global: SEARCH_GLOBAL.iter().map(rendered).collect(),
            export,
        });
    }

    // No panel group: motion is assumed.
    if matches!(state.overlay, Some(Overlay::Commands(_))) {
        return Some(HintBar {
            panel: Vec::new(),
            global: PALETTE_GLOBAL.iter().map(rendered).collect(),
            export,
        });
    }

    if matches!(state.overlay, Some(Overlay::Themes { .. })) {
        return Some(HintBar {
            panel: THEMES.iter().map(rendered).collect(),
            global: THEMES_GLOBAL.iter().map(rendered).collect(),
            export,
        });
    }

    if matches!(state.overlay, Some(Overlay::Connections { .. })) {
        return Some(connections_bar(state, export));
    }

    if matches!(state.overlay, Some(Overlay::History { .. })) {
        return Some(HintBar {
            panel: HISTORY.iter().map(rendered).collect(),
            global: HISTORY_GLOBAL.iter().map(rendered).collect(),
            export,
        });
    }

    if matches!(state.overlay, Some(Overlay::Health(_))) {
        return Some(HintBar {
            panel: HEALTH.iter().map(rendered).collect(),
            global: HEALTH_GLOBAL.iter().map(rendered).collect(),
            export,
        });
    }

    if matches!(state.overlay, Some(Overlay::Diagnostics(_))) {
        return Some(HintBar {
            panel: Vec::new(),
            global: DIAGNOSTICS_GLOBAL.iter().map(rendered).collect(),
            export,
        });
    }

    if let Some(Overlay::Inspect(inspector)) = &state.overlay {
        return Some(inspect_bar(state, inspector, export));
    }

    if matches!(state.overlay, Some(Overlay::Commit(_))) {
        return Some(HintBar {
            panel: Vec::new(),
            global: COMMIT.iter().map(rendered).collect(),
            export,
        });
    }

    if matches!(state.overlay, Some(Overlay::Discard(_))) {
        return Some(HintBar {
            panel: Vec::new(),
            global: DISCARD.iter().map(rendered).collect(),
            export,
        });
    }

    if matches!(state.overlay, Some(Overlay::Search(_) | Overlay::Filter(_))) {
        return Some(HintBar {
            panel: Vec::new(),
            global: SEARCH_GLOBAL.iter().map(rendered).collect(),
            export,
        });
    }

    if let Some(Overlay::Export(form)) = &state.overlay {
        return Some(export_bar(form, export));
    }

    if let Some(Overlay::Drop(prompt)) = &state.overlay {
        return Some(drop_bar(prompt, export));
    }

    None
}

pub(crate) fn bar(state: &State) -> HintBar {
    let export = state
        .export
        .as_ref()
        .map(|job| format!("exporting {} rows…", job.rows()));

    if let Some(bar) = overlay_bar(state, export.as_ref()) {
        return bar;
    }

    // `?` is dropped where no palette fits, matched by key so a reorder of
    // `GLOBAL` cannot drop the wrong one. `tab` accepts a completion while a
    // popup is up.
    let completing = state.queries.completion().is_some() && state.takes_text();
    let fits = state.geometry.command_rows() > 0;
    let mut global: Vec<Hint> = GLOBAL
        .iter()
        .filter(|binding| fits || !binding.keys.contains(&Stroke::key(Key::Char('?'))))
        .filter(|binding| !completing || !binding.keys.contains(&Stroke::key(Key::Tab)))
        .map(rendered)
        .collect();
    if !state.takes_text() {
        global.push(rendered(&QUIT));
    }

    // `⏎ newline` drops while `⏎` accepts a completion.
    let mut panel: Vec<Hint> = state
        .hints()
        .iter()
        .filter(|binding| !completing || !binding.keys.contains(&Stroke::key(Key::Enter)))
        .filter(|binding| !refused(state, binding))
        .filter(|binding| binding.action != "inspect" || crate::update::inspection(state).is_some())
        .filter(|binding| !pages(binding) || state.contents.has_more())
        .map(|binding| Hint {
            liveness: liveness(state, binding),
            ..rendered(binding)
        })
        .collect();
    if completing {
        panel.insert(0, rendered(&COMPLETE));
    }

    let target = Searched::of(state.focus()).filter(|target| state.search(*target).is_some());
    if let Some(target) = target {
        panel.push(Hint {
            liveness: if next_match_is_live(state, target) {
                Liveness::Live
            } else {
                Liveness::Inert
            },
            ..rendered(&NEXT_MATCH)
        });
    }
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
        panel.push(rendered(&STOP));
    } else if state.takes_text() {
        panel.push(rendered(&DONE));
    } else if target.is_some() || selected(state) {
        panel.push(rendered(&CLEAR));
    } else if state.focus() != Panel::default() {
        panel.push(rendered(&LEAVE));
    }

    HintBar {
        panel,
        global,
        export,
    }
}

/// No panel group: the confirmation claims every key.
fn drop_bar(prompt: &crate::drop::DropPrompt, export: Option<String>) -> HintBar {
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
            .map(|binding| Hint {
                liveness: if binding.keys == [Stroke::key(Key::Enter)] && !prompt.confirmed() {
                    Liveness::Inert
                } else {
                    Liveness::Live
                },
                ..rendered(binding)
            })
            .collect(),
        export,
    }
}

/// `y` greys over a null value, `Y` over a null field's line.
fn inspect_bar(
    state: &State,
    inspector: &crate::inspect::Inspector,
    export: Option<String>,
) -> HintBar {
    // An open field names only what it answers.
    if inspector.field().is_some() {
        return HintBar {
            panel: Vec::new(),
            global: FIELD
                .iter()
                .map(|binding| Hint {
                    liveness: if binding.keys == [Stroke::key(Key::Enter)]
                        && !inspector.can_accept()
                    {
                        Liveness::Inert
                    } else {
                        Liveness::Live
                    },
                    ..rendered(binding)
                })
                .collect(),
            export,
        };
    }

    let copies = |action: &str| match action {
        "copy" => inspector.yank().is_some(),
        "copy line" => inspector.line().is_some(),
        _ => true,
    };
    let mut panel: Vec<Hint> = INSPECT
        .iter()
        .map(|binding| Hint {
            liveness: if copies(binding.action) {
                Liveness::Live
            } else {
                Liveness::Inert
            },
            ..rendered(binding)
        })
        .collect();
    panel.push(Hint {
        liveness: if crate::update::editable_here(state) {
            Liveness::Live
        } else {
            Liveness::Inert
        },
        ..rendered(&EDIT)
    });
    if state.posture().permits_mutation() && !inspector.is_inserting() {
        panel.push(Hint {
            liveness: if inspector.edited().is_empty() {
                Liveness::Inert
            } else {
                Liveness::Live
            },
            ..rendered(&COMMIT_ROW)
        });
    }
    if inspector.is_inserting() {
        panel.retain(|hint| !matches!(hint.action, "copy" | "copy line"));
        panel.push(Hint {
            liveness: if inspector.is_set_at_cursor() {
                Liveness::Live
            } else {
                Liveness::Inert
            },
            ..rendered(&UNSET)
        });
        panel.push(rendered(&INSERT));
    }

    HintBar {
        panel,
        global: INSPECT_GLOBAL.iter().map(rendered).collect(),
        export,
    }
}

/// The stop under `tab` stands in for the panel group.
fn export_bar(form: &crate::export::ExportForm, export: Option<String>) -> HintBar {
    use crate::export::Stop;

    if form.confirm().is_some() {
        return HintBar {
            panel: Vec::new(),
            global: EXPORT_OVERWRITE.iter().map(rendered).collect(),
            export,
        };
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
        global: vec![rendered(&EXPORT_CLOSE)],
        export,
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

/// `ctrl-r` greys on a buffer with no statement, `ctrl-f` on one the formatter
/// would leave alone, `ctrl-e` over an empty grid or no room for the form.
fn liveness(state: &State, binding: &Binding) -> Liveness {
    let runs = binding.keys == [Stroke::ctrl(Key::Char('r'))];
    if runs {
        let Some(cql) = state.queries.statement() else {
            return Liveness::Inert;
        };

        // Follows the statement as typed: the same key runs a `SELECT` and is
        // refused a `DELETE`.
        let refused =
            !state.posture().permits_mutation() && crate::model::Statement::typed(&cql).mutates();

        if refused {
            return Liveness::Inert;
        }
    }

    let formats = binding.keys == [Stroke::ctrl(Key::Char('f'))];
    if formats {
        let text = state.queries.text();
        if crate::cql::format(&text) == text {
            return Liveness::Inert;
        }
    }

    if pages(binding) && !state.contents.can_fetch() {
        return Liveness::Inert;
    }

    let exports = binding.action == "export";
    if exports && (state.contents.row_count() == 0 || !state.geometry.export_fits()) {
        return Liveness::Inert;
    }

    // `c` also greys with no filter on the cursor's column.
    let arranges = matches!(binding.action, "sort" | "filter");
    if arranges && state.contents.loaded().is_none() {
        return Liveness::Inert;
    }

    let clears = binding.action == "clear filter";
    if clears
        && state
            .contents
            .filter(state.contents.column.index())
            .is_none()
    {
        return Liveness::Inert;
    }

    // Reads the same derivation as `d` itself.
    let drops = binding.keys == [Stroke::key(Key::Char('d'))];
    if drops
        && (!state.geometry.drop_fits()
            || state.dropping.is_some()
            || crate::update::drop_target(state).is_none())
    {
        return Liveness::Inert;
    }

    let inserts = binding.keys == [Stroke::key(Key::Char('i'))] && state.focus() == Panel::Contents;
    if inserts
        && (state.geometry.inspector_rows() == 0 || crate::update::insert_form(state).is_err())
    {
        return Liveness::Inert;
    }

    let reads_health = binding.keys == [Stroke::key(Key::Char('H'))];
    if reads_health
        && (state.geometry.health_rows() == 0 || crate::update::health_target(state).is_none())
    {
        return Liveness::Inert;
    }

    Liveness::Live
}

fn pages(binding: &Binding) -> bool {
    *binding == crate::view::hints::NEXT_PAGE || *binding == crate::view::hints::EVERY_PAGE
}

/// Left off the bar rather than greyed; the palette still names it. Not
/// `ctrl-r`, which follows the buffer.
fn refused(state: &State, binding: &Binding) -> bool {
    (binding.keys == [Stroke::key(Key::Char('d'))]
        || (binding.keys == [Stroke::key(Key::Char('i'))] && state.focus() == Panel::Contents))
        && !state.posture().permits_mutation()
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
    Hint {
        keys: label(binding.keys),
        action: binding.action,
        liveness: Liveness::Live,
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

// Motion is in no panel's bar: users already reach for it, and the palette
// lists it.
const TABLES_HINTS: &[Binding] = &[
    // "open" is true of both a keyspace and a table row.
    Binding {
        keys: &[Stroke::key(Key::Enter)],
        action: "open",
    },
    // Not global: `/` is text in Queries.
    Binding {
        keys: &[Stroke::key(Key::Char('/'))],
        action: "search",
    },
    Binding {
        keys: &[Stroke::key(Key::Char('d'))],
        action: "drop",
    },
    Binding {
        keys: &[Stroke::key(Key::Char('H'))],
        action: "health",
    },
    Binding {
        keys: &[Stroke::key(Key::Char('v'))],
        action: "select",
    },
];

const DDL_HINTS: &[Binding] = &[
    Binding {
        keys: &[Stroke::key(Key::Char('/'))],
        action: "search",
    },
    Binding {
        keys: &[Stroke::key(Key::Char('v'))],
        action: "select",
    },
];

pub(crate) const NEXT_PAGE: Binding = Binding {
    keys: &[Stroke::key(Key::Char('p'))],
    action: "next page",
};

pub(crate) const EVERY_PAGE: Binding = Binding {
    keys: &[Stroke::key(Key::Char('P'))],
    action: "all",
};

const CONTENTS_HINTS: &[Binding] = &[
    // Only where it opens.
    Binding {
        keys: &[Stroke::key(Key::Enter)],
        action: "inspect",
    },
    // Only while there is more to fetch; greyed while a page is in flight.
    NEXT_PAGE,
    EVERY_PAGE,
    Binding {
        keys: &[Stroke::ctrl(Key::Char('e'))],
        action: "export",
    },
    // Greyed where the rows lack an addressable primary key.
    Binding {
        keys: &[Stroke::key(Key::Char('d'))],
        action: "delete",
    },
    // Greyed where no form could open; off on a read-only connection.
    Binding {
        keys: &[Stroke::key(Key::Char('i'))],
        action: "insert",
    },
    Binding {
        keys: &[Stroke::key(Key::Char('m'))],
        action: "mode",
    },
    Binding {
        keys: &[Stroke::key(Key::Char('/'))],
        action: "search",
    },
    Binding {
        keys: &[Stroke::key(Key::Char('v'))],
        action: "select",
    },
];

/// Named only in column mode, whose cursor is a column.
const CONTENTS_COLUMN_HINTS: &[Binding] = &[
    Binding {
        keys: &[Stroke::key(Key::Char('s'))],
        action: "sort",
    },
    Binding {
        keys: &[Stroke::key(Key::Char('f'))],
        action: "filter",
    },
    // Clears this column's filter only, unlike `esc clear`.
    Binding {
        keys: &[Stroke::key(Key::Char('c'))],
        action: "clear filter",
    },
    Binding {
        keys: &[Stroke::key(Key::Enter)],
        action: "inspect",
    },
    NEXT_PAGE,
    EVERY_PAGE,
    Binding {
        keys: &[Stroke::ctrl(Key::Char('e'))],
        action: "export",
    },
    Binding {
        keys: &[Stroke::key(Key::Char('m'))],
        action: "mode",
    },
    Binding {
        keys: &[Stroke::key(Key::Char('/'))],
        action: "search",
    },
    Binding {
        keys: &[Stroke::key(Key::Char('v'))],
        action: "select",
    },
];

/// `⏎` first: nothing else says how to start typing.
const CQL_HINTS: &[Binding] = &[
    Binding {
        keys: &[Stroke::key(Key::Enter)],
        action: "edit",
    },
    Binding {
        keys: &[Stroke::ctrl(Key::Char('r'))],
        action: "run",
    },
    Binding {
        keys: &[Stroke::ctrl(Key::Char('e'))],
        action: "$EDITOR",
    },
];

const QUERIES_HINTS: &[Binding] = &[
    // First: it is what the panel is for.
    Binding {
        keys: &[Stroke::ctrl(Key::Char('r'))],
        action: "run",
    },
    // Never greyed: always true while Queries is focused.
    Binding {
        keys: &[Stroke::ctrl(Key::Char('e'))],
        action: "$EDITOR",
    },
    Binding {
        keys: &[Stroke::ctrl(Key::Char('f'))],
        action: "format",
    },
    Binding {
        keys: &[Stroke::key(Key::Enter)],
        action: "newline",
    },
];

const LOGS_HINTS: &[Binding] = &[
    Binding {
        keys: &[Stroke::key(Key::Char('/'))],
        action: "search",
    },
    Binding {
        keys: &[Stroke::key(Key::Char('v'))],
        action: "select",
    },
];

impl Panel {
    /// Exhaustive, so a new panel cannot compile without its bindings.
    #[must_use]
    pub const fn hints(self) -> &'static [Binding] {
        match self {
            Self::Tables => TABLES_HINTS,
            Self::Ddl => DDL_HINTS,
            Self::Contents => CONTENTS_HINTS,
            Self::Queries => QUERIES_HINTS,
        }
    }
}

impl State {
    /// On `State`, not `Panel`: Queries' bindings depend on its tab, and
    /// Contents' on its mode.
    #[must_use]
    pub fn hints(&self) -> &'static [Binding] {
        match self.focus() {
            Panel::Queries => match self.queries.mode() {
                None => LOGS_HINTS,
                Some(CqlMode::Browsing) => CQL_HINTS,
                Some(CqlMode::Editing) => QUERIES_HINTS,
            },
            Panel::Contents if self.contents.mode == SelectionMode::Column => CONTENTS_COLUMN_HINTS,
            panel => panel.hints(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::CommandsPanel;
    use crate::geometry::Geometry;
    use crate::overlay::Overlay;
    use crate::panel::Panel;
    use crate::search::{Prompt, Search, Searched};
    use crate::state::State;
    use crate::theme::ThemeChoice;
    use crate::viewport::Cursor;

    fn fits() -> State {
        let mut state = State::default();
        state.geometry = Geometry::new([8, 14, 20, 1], 37, 14);
        state
    }

    #[test]
    fn a_binding_behind_ctrl_is_named_with_a_ctrl_prefix() {
        assert_eq!(
            label(&[Stroke::ctrl(Key::Char('d')), Stroke::ctrl(Key::Char('u')),]),
            "ctrl-d/ctrl-u"
        );
        assert_eq!(label(&[Stroke::key(Key::Char('j'))]), "j");
    }

    #[test]
    fn several_keys_for_one_action_join_with_a_slash() {
        assert_eq!(
            label(&[Stroke::key(Key::Char('j')), Stroke::key(Key::Char('k'))]),
            "j/k"
        );
        assert_eq!(
            label(&[Stroke::key(Key::PageDown), Stroke::key(Key::PageUp)]),
            "pgdn/pgup"
        );
    }

    #[test]
    fn a_key_with_no_printable_form_is_named_rather_than_drawn() {
        assert_eq!(
            label(&[Stroke::key(Key::Home), Stroke::key(Key::End)]),
            "home/end"
        );
        assert_eq!(label(&[Stroke::key(Key::Char(' '))]), "space");
        assert_eq!(label(&[Stroke::key(Key::Tab)]), "tab");
    }

    #[test]
    fn the_bar_carries_the_focused_panels_bindings_and_the_global_ones() {
        let mut state = State::default();
        state.focus_on(Panel::Tables);

        let bar = bar(&state);

        assert_eq!(bar.panel[0].keys, "⏎");
        assert_eq!(bar.panel[0].action, "open");
        assert_eq!(bar.global.last().expect("q quit is last").action, "quit");
    }

    #[test]
    fn no_panel_contributes_an_empty_group_the_reader_has_to_interpret() {
        for panel in Panel::ALL {
            let mut state = fits();
            state.focus_on(panel);

            assert!(
                !bar(&state).panel.is_empty(),
                "{panel:?} would draw a group with nothing in it"
            );
        }
    }

    #[test]
    fn the_logs_tabs_bar_advertises_a_list_not_an_editor() {
        let mut state = State::default();
        state.focus_on(Panel::Queries);
        state.queries.show(crate::queries::QueriesTab::Logs);

        let bar = bar(&state);

        assert!(
            bar.panel.iter().all(|hint| hint.action != "newline"),
            "⏎ types nothing on the Logs tab: {:?}",
            bar.panel
        );
        assert!(
            bar.panel.iter().any(|hint| hint.action == "leave"),
            "esc leaves the panel here as it does on Cql: {:?}",
            bar.panel
        );
        assert!(
            bar.panel.iter().any(|hint| hint.action == "search"),
            "/ search is genuinely bound here: {:?}",
            bar.panel
        );
        assert!(
            bar.panel.iter().any(|hint| hint.action == "select"),
            "v select is genuinely bound here: {:?}",
            bar.panel
        );
    }

    #[test]
    fn a_panel_that_takes_text_does_not_claim_q_quits() {
        let mut state = fits();
        state.focus_on(Panel::Queries);
        state.queries.start_editing();

        let bar = bar(&state);

        assert_eq!(bar.panel[0].action, "run");
        assert!(
            bar.global.iter().all(|hint| hint.action != "quit"),
            "q types a q in the editor, and a bar that lies is worse than no bar"
        );
        assert!(
            bar.global.iter().any(|hint| hint.action == "panel"),
            "tab is still the way out"
        );
    }

    #[test]
    fn the_bar_names_the_palette_between_panel_and_quit() {
        let global = bar(&fits()).global;
        let position = |action: &str| {
            global
                .iter()
                .position(|hint| hint.action == action)
                .unwrap_or_else(|| panic!("{action} is on the bar"))
        };

        assert!(
            position("commands") > position("panel"),
            "? outlives tab panel"
        );
        assert!(position("quit") > position("commands"), "q quit outlives ?");
    }

    #[test]
    fn the_bar_drops_the_palette_hint_where_no_palette_would_fit() {
        assert!(
            bar(&State::default())
                .global
                .iter()
                .all(|hint| hint.action != "commands"),
            "a hint for a key that can do nothing here is a bar that lies"
        );
    }

    #[test]
    fn the_bar_names_what_the_palette_responds_to() {
        let mut state = State::default();
        state.overlay = Some(Overlay::Commands(CommandsPanel::opened()));

        let bar = bar(&state);

        assert!(
            bar.panel.is_empty(),
            "the palette answers to nothing a terminal list does not: {:?}",
            bar.panel
        );
        assert_eq!(bar.global[0].keys, "⏎");
        assert_eq!(bar.global[0].action, "run");
        assert_eq!(bar.global[1].keys, "?/esc");
        assert_eq!(bar.global[1].action, "close");
        assert_eq!(bar.global[2].keys, "q");
        assert_eq!(bar.global[2].action, "quit");
        assert_eq!(bar.global.last().expect("/ is last").action, "search");
    }

    #[test]
    fn the_overlay_bars_name_the_search_key() {
        let mut state = State::default();
        state.overlay = Some(Overlay::Commands(CommandsPanel::opened()));

        let bar = bar(&state);

        assert!(
            bar.global.iter().any(|hint| hint.action == "search"),
            "the palette's bar names /"
        );
    }

    #[test]
    fn a_live_sift_shows_the_prompts_bar() {
        let mut state = State::default();
        let mut panel = CommandsPanel::opened();
        panel.open_sift();
        state.overlay = Some(Overlay::Commands(panel));

        let bar = bar(&state);
        let actions: Vec<&str> = bar.global.iter().map(|hint| hint.action).collect();

        assert_eq!(actions, ["apply", "cancel"]);
    }

    #[test]
    fn a_live_sift_over_the_picker_shows_the_prompts_bar() {
        let mut state = State::default();
        state.overlay = Some(Overlay::Themes {
            cursor: Cursor::default(),
            restored: ThemeChoice::default(),
            sift: Some(crate::search::Sift::opened(0)),
        });

        let bar = bar(&state);
        let actions: Vec<&str> = bar.global.iter().map(|hint| hint.action).collect();

        assert_eq!(actions, ["apply", "cancel"]);
    }

    #[test]
    fn the_bar_names_what_the_picker_responds_to() {
        let mut state = State::default();
        state.overlay = Some(Overlay::Themes {
            cursor: Cursor::default(),
            restored: ThemeChoice::default(),
            sift: None,
        });

        let bar = bar(&state);

        assert_eq!(
            bar.panel
                .iter()
                .map(|hint| (hint.keys.as_str(), hint.action))
                .collect::<Vec<_>>(),
            [("j/k", "preview"), ("/", "search")],
            "the focused panel responds to nothing while the picker is up"
        );
        assert_eq!(
            bar.global
                .iter()
                .map(|hint| (hint.keys.as_str(), hint.action))
                .collect::<Vec<_>>(),
            [("⏎", "keep"), ("esc", "cancel"), ("q", "quit")]
        );
    }

    #[test]
    fn the_connection_pickers_bar_names_connect_and_teaches_no_motion() {
        let mut state = State::default();
        state.config.connections = vec![connection()];
        state.overlay = Some(Overlay::Connections {
            cursor: Cursor::default(),
            sift: None,
        });

        let bar = bar(&state);

        assert_eq!(
            bar.panel
                .iter()
                .map(|hint| (hint.keys.as_str(), hint.action))
                .collect::<Vec<_>>(),
            [("/", "search")],
            "a motion that previews nothing is not worth the bar's width"
        );
        assert_eq!(
            bar.global
                .iter()
                .map(|hint| (hint.keys.as_str(), hint.action))
                .collect::<Vec<_>>(),
            [("⏎", "connect"), ("esc", "close"), ("q", "quit")]
        );
    }

    fn connection() -> crate::config::ConnectionConfig {
        crate::config::ConnectionConfig {
            name: crate::config::ConnectionName::try_new("local").expect("a valid name"),
            hosts: vec!["127.0.0.1:9042".to_owned()],
            posture: crate::posture::Posture::ReadOnly,
            discovery: crate::config::Discovery::default(),
            datacenter: None,
            credentials: None,
            tls: crate::config::Tls::Off,
        }
    }

    #[test]
    fn an_empty_connection_picker_names_neither_search_nor_connect() {
        let mut state = State::default();
        state.overlay = Some(Overlay::Connections {
            cursor: Cursor::default(),
            sift: None,
        });

        let bar = bar(&state);

        assert!(bar.panel.is_empty(), "there is nothing to search");
        assert_eq!(
            bar.global
                .iter()
                .map(|hint| (hint.keys.as_str(), hint.action))
                .collect::<Vec<_>>(),
            [("esc", "close"), ("q", "quit")],
            "the two ways out are all that is true of an empty picker"
        );
    }

    #[test]
    fn a_live_sift_in_the_connection_picker_shows_the_prompts_bar() {
        let mut state = State::default();
        state.config.connections = vec![connection()];
        state.overlay = Some(Overlay::Connections {
            cursor: Cursor::default(),
            sift: Some(crate::search::Sift::opened(0)),
        });

        let bar = bar(&state);

        assert!(bar.panel.is_empty());
        assert_eq!(
            bar.global
                .iter()
                .map(|hint| (hint.keys.as_str(), hint.action))
                .collect::<Vec<_>>(),
            [("⏎", "apply"), ("esc", "cancel")]
        );
    }

    #[test]
    fn the_bar_names_what_diagnostics_responds_to() {
        use crate::config::Diagnostic;
        use crate::overlay::DiagnosticsPanel;

        let mut state = State::default();
        state.overlay = Some(Overlay::Diagnostics(DiagnosticsPanel::new(vec![
            Diagnostic {
                origin: crate::config::Origin::new("./.lazycassandra.toml"),
                span: None,
                severity: crate::style::Severity::Warning,
                message: "unknown key `k0` — ignored".to_owned(),
            },
        ])));

        let bar = bar(&state);

        assert!(
            bar.panel.is_empty(),
            "the overlay answers to nothing the focused panel does: {:?}",
            bar.panel
        );
        assert_eq!(
            bar.global
                .iter()
                .map(|hint| (hint.keys.as_str(), hint.action))
                .collect::<Vec<_>>(),
            [("esc", "close")],
            "esc is the only way out, and the one binding worth naming"
        );
    }

    #[test]
    fn the_bar_names_search_in_every_panel_that_can_hold_a_search() {
        for panel in [Panel::Tables, Panel::Ddl, Panel::Contents] {
            let mut state = fits();
            state.focus_on(panel);

            assert!(
                bar(&state).panel.iter().any(|hint| hint.keys == "/"),
                "{panel:?} responds to / and must say so"
            );
        }
    }

    #[test]
    fn the_bar_does_not_name_search_in_queries() {
        let mut state = fits();
        state.focus_on(Panel::Queries);

        assert!(bar(&state).panel.iter().all(|hint| hint.keys != "/"));
    }

    #[test]
    fn the_bar_names_esc_clear_only_where_a_search_is_applied() {
        let mut state = fits();
        state.focus_on(Panel::Tables);
        assert!(bar(&state).panel.iter().all(|hint| hint.action != "clear"));

        state.apply_search(Searched::Tables, Search::new("o"));

        assert!(
            bar(&state)
                .panel
                .iter()
                .any(|hint| hint.keys == "esc" && hint.action == "clear"),
            "in this frame pressing esc unconditionally does something"
        );
    }

    #[test]
    fn the_bar_names_n_only_where_a_search_is_applied() {
        let mut state = fits();
        state.focus_on(Panel::Tables);
        assert!(
            bar(&state)
                .panel
                .iter()
                .all(|hint| hint.action != "next match")
        );

        state.apply_search(Searched::Tables, Search::new("o"));

        assert!(
            bar(&state)
                .panel
                .iter()
                .any(|hint| hint.keys == "n" && hint.action == "next match")
        );
    }

    fn next_match_hint(bar: &HintBar) -> &Hint {
        bar.panel
            .iter()
            .find(|hint| hint.action == "next match")
            .expect("a search is applied, so the hint is named")
    }

    #[test]
    fn the_next_match_hint_is_inert_when_the_term_matches_nothing() {
        let mut state = crate::view::tests_support::tabled(&[("shop", &["orders"])]);
        state.focus_on(Panel::Tables);
        state.apply_search(Searched::Tables, Search::new("zzz"));

        assert_eq!(
            next_match_hint(&bar(&state)).liveness,
            Liveness::Inert,
            "n does nothing when nothing matches, and a bar that lies is worse than no bar"
        );
    }

    #[test]
    fn the_next_match_hint_is_live_when_something_matches() {
        let mut state = crate::view::tests_support::tabled(&[("shop", &["orders"])]);
        state.focus_on(Panel::Tables);
        state.apply_search(Searched::Tables, Search::new("shop"));

        assert_eq!(next_match_hint(&bar(&state)).liveness, Liveness::Live);
    }

    #[test]
    fn the_next_match_hint_is_inert_in_contents_with_no_page_loaded() {
        let mut state = fits();
        state.focus_on(Panel::Contents);
        state.apply_search(Searched::Contents, Search::new("id"));

        assert_eq!(
            next_match_hint(&bar(&state)).liveness,
            Liveness::Inert,
            "n does nothing over an empty grid"
        );
    }

    #[test]
    fn the_bar_names_what_the_prompt_responds_to() {
        let mut state = fits();
        state.overlay = Some(Overlay::Search(Prompt::opened(
            Searched::Tables,
            None,
            crate::search::Origin::default(),
        )));

        let bar = bar(&state);

        assert!(
            bar.panel.is_empty(),
            "the panel underneath responds to nothing"
        );
        assert_eq!(
            bar.global
                .iter()
                .map(|hint| (hint.keys.as_str(), hint.action))
                .collect::<Vec<_>>(),
            [("⏎", "apply"), ("esc", "cancel")]
        );
    }

    #[test]
    fn the_queries_bar_names_the_run_binding() {
        let mut state = fits();
        state.focus_on(Panel::Queries);
        state.queries.start_editing();

        let panel = bar(&state).panel;

        assert_eq!(
            panel.first().map(|hint| (hint.keys.as_str(), hint.action)),
            Some(("ctrl-r", "run")),
            "the panel exists to run queries, so the bar says that first"
        );
    }

    #[test]
    fn the_queries_bar_names_the_editor_binding_after_run() {
        let mut state = fits();
        state.focus_on(Panel::Queries);
        state.queries.start_editing();

        let panel = bar(&state).panel;

        assert_eq!(
            panel
                .iter()
                .map(|hint| (hint.keys.as_str(), hint.action))
                .take(2)
                .collect::<Vec<_>>(),
            [("ctrl-r", "run"), ("ctrl-e", "$EDITOR")],
            "the panel exists to run queries; editing one is the next thing it does"
        );
    }

    #[test]
    fn a_closed_buffer_names_the_way_in_first_and_still_offers_quit() {
        let mut state = fits();
        state.focus_on(Panel::Queries);

        let bar = bar(&state);

        assert_eq!(
            bar.panel
                .first()
                .map(|hint| (hint.keys.as_str(), hint.action)),
            Some(("⏎", "edit"))
        );
        assert!(bar.global.iter().any(|hint| hint.action == "quit"));
        assert!(bar.panel.iter().any(|hint| hint.action == "leave"));
    }

    #[test]
    fn an_open_buffer_names_esc_as_done_rather_than_leave() {
        let mut state = fits();
        state.focus_on(Panel::Queries);
        state.queries.start_editing();

        let panel = bar(&state).panel;

        assert!(
            panel
                .iter()
                .any(|hint| (hint.keys.as_str(), hint.action) == ("esc", "done"))
        );
        assert!(panel.iter().all(|hint| hint.action != "leave"));
    }

    #[test]
    fn no_other_bar_names_the_editor_binding() {
        for focus in [Panel::Tables, Panel::Ddl, Panel::Contents] {
            let mut state = fits();
            state.focus_on(focus);

            assert!(
                bar(&state)
                    .panel
                    .iter()
                    .all(|hint| hint.action != "$EDITOR"),
                "{focus:?} does not open an editor"
            );
        }
    }

    /// `\n` means Enter.
    fn queries_bar(text: &str) -> HintBar {
        let mut state = fits();
        state.focus_on(Panel::Queries);
        state.queries.start_editing();
        for character in text.chars() {
            let key = if character == '\n' {
                Key::Enter
            } else {
                Key::Char(character)
            };
            state.queries.edit(key, 1);
        }
        bar(&state)
    }

    fn run_hint(bar: &HintBar) -> &Hint {
        bar.panel
            .iter()
            .find(|hint| hint.action == "run")
            .expect("the queries bar names the run binding")
    }

    #[test]
    fn the_run_hint_is_inert_while_there_is_nothing_to_run() {
        for buffer in ["", "   ", " \n\t "] {
            let bar = queries_bar(buffer);

            assert_eq!(
                run_hint(&bar).liveness,
                Liveness::Inert,
                "ctrl-r on {buffer:?} submits nothing, and a bar that lies is worse than no bar"
            );
        }
    }

    #[test]
    fn the_run_hint_is_live_once_there_is_a_statement() {
        assert_eq!(
            run_hint(&queries_bar("SELECT * FROM shop.orders")).liveness,
            Liveness::Live
        );
    }

    #[test]
    fn the_run_hint_greys_against_a_buffer_the_posture_would_refuse() {
        for (posture, cql, expected) in [
            (
                crate::Posture::ReadOnly,
                "SELECT * FROM shop.orders",
                Liveness::Live,
            ),
            (
                crate::Posture::ReadOnly,
                "DELETE FROM shop.orders WHERE id = 1",
                Liveness::Inert,
            ),
            (
                crate::Posture::ReadOnly,
                "DROP TABLE shop.orders",
                Liveness::Inert,
            ),
            (
                crate::Posture::Writable,
                "DELETE FROM shop.orders WHERE id = 1",
                Liveness::Live,
            ),
        ] {
            let mut state = connected(posture, crate::connection::SchemaState::Loading);
            state.focus_on(Panel::Queries);
            state.queries.start_editing();
            for character in cql.chars() {
                state.queries.edit(Key::Char(character), 1);
            }

            assert_eq!(
                run_hint(&bar(&state)).liveness,
                expected,
                "{cql} under a {} connection",
                posture.label()
            );
        }
    }

    #[test]
    fn the_bar_names_esc_while_a_range_stands() {
        let mut state = fits();
        state.focus_on(Panel::Tables);
        state.tables.selection.start(0);

        let bar = bar(&state);

        assert!(
            bar.panel.iter().any(|hint| hint.keys == "esc"),
            "under a live range, pressing esc unconditionally does something"
        );
    }

    #[test]
    fn the_bar_leaves_esc_off_a_panel_with_neither_a_range_nor_a_search() {
        let mut state = fits();
        state.focus_on(Panel::Tables);

        assert!(!bar(&state).panel.iter().any(|hint| hint.keys == "esc"));
    }

    fn hint(state: &State, action: &str) -> Liveness {
        bar(state)
            .panel
            .into_iter()
            .find(|hint| hint.action == action)
            .unwrap_or_else(|| panic!("the panel names {action}"))
            .liveness
    }

    fn connected(posture: crate::Posture, schema: crate::connection::SchemaState) -> State {
        let mut state = fits();
        state.connection =
            crate::connection::ConnectionState::Connected(crate::connection::Connection {
                id: crate::model::ConnId::next(),
                name: crate::config::ConnectionName::try_new("local").expect("a valid name"),
                posture,
                schema,
            });
        state
    }

    #[test]
    fn the_drop_hint_is_live_only_where_the_key_would_open_something() {
        let loading = crate::connection::SchemaState::Loading;
        let loaded = crate::connection::SchemaState::Loaded(crate::tree::tests_support::schema_of(
            &[("shop", &["orders"])],
        ));

        let mut state = connected(crate::Posture::Writable, loaded.clone());
        state.focus_on(Panel::Tables);
        assert_eq!(
            hint(&state, "drop"),
            Liveness::Inert,
            "a frame with no room for the block draws no question"
        );

        let mut state = connected(crate::Posture::Writable, loading);
        state.focus_on(Panel::Tables);
        state.geometry = state.geometry.with_drop_fits(true);
        assert_eq!(
            hint(&state, "drop"),
            Liveness::Inert,
            "an unread schema names no node to aim at"
        );

        let mut state = connected(crate::Posture::Writable, loaded);
        state.focus_on(Panel::Tables);
        state.geometry = state.geometry.with_drop_fits(true);
        assert_eq!(hint(&state, "drop"), Liveness::Live);
    }

    #[test]
    fn the_health_hint_is_live_on_a_table_with_room_for_the_block() {
        let loaded = crate::connection::SchemaState::Loaded(crate::tree::tests_support::schema_of(
            &[("shop", &["orders"])],
        ));
        let mut state = connected(crate::Posture::ReadOnly, loaded);
        state.focus_on(Panel::Tables);
        let schema = state.schema().expect("loaded").clone();
        state.tables.open(&schema);
        state.tables.open(&schema);
        state.geometry = state.geometry.with_health_rows(0);

        assert_eq!(hint(&state, "health"), Liveness::Inert, "no room");

        state.geometry = state.geometry.with_health_rows(10);
        assert_eq!(hint(&state, "health"), Liveness::Live);

        state.tables.cursor_mut().up();
        assert_eq!(
            hint(&state, "health"),
            Liveness::Inert,
            "a keyspace has no health of its own"
        );
    }

    #[test]
    fn a_read_only_connection_leaves_d_off_the_bar() {
        let loaded = crate::connection::SchemaState::Loaded(crate::tree::tests_support::schema_of(
            &[("shop", &["orders"])],
        ));

        for panel in [Panel::Tables, Panel::Contents] {
            let mut state = connected(crate::Posture::ReadOnly, loaded.clone());
            state.focus_on(panel);
            state.geometry = state.geometry.with_drop_fits(true);

            assert!(
                bar(&state).panel.iter().all(|hint| hint.keys != "d"),
                "{panel:?} names a key the posture refuses"
            );
        }
    }

    #[test]
    fn the_delete_hint_is_inert_over_a_grid_holding_no_addressable_row() {
        let mut state = connected(
            crate::Posture::Writable,
            crate::connection::SchemaState::Loaded(crate::tree::tests_support::schema_of(&[(
                "shop",
                &["orders"],
            )])),
        );
        state.focus_on(Panel::Contents);
        state.geometry = state.geometry.with_drop_fits(true);

        assert_eq!(hint(&state, "delete"), Liveness::Inert);
    }

    #[test]
    fn nothing_but_a_hint_that_would_do_nothing_is_ever_inert() {
        for panel in Panel::ALL {
            let mut state = fits();
            state.focus_on(panel);
            let bar = bar(&state);

            for hint in bar.panel.iter().chain(&bar.global) {
                assert!(
                    hint.liveness == Liveness::Live
                        || hint.action == "run"
                        || hint.action == "format"
                        || hint.action == "export"
                        || hint.action == "drop"
                        || hint.action == "health"
                        || hint.action == "delete",
                    "{panel:?}'s '{} {}' is inert and nothing says why",
                    hint.keys,
                    hint.action
                );
            }
        }
    }

    #[test]
    fn every_panel_says_what_it_responds_to() {
        for panel in Panel::ALL {
            assert!(
                !panel.hints().is_empty(),
                "{panel:?} responds to nothing, which no panel does any more"
            );
        }
    }

    #[test]
    fn no_binding_claims_a_key_with_no_action_to_go_with_it() {
        for panel in Panel::ALL {
            for binding in panel.hints() {
                assert!(!binding.keys.is_empty(), "{panel:?} has a keyless hint");
                assert!(!binding.action.is_empty(), "{panel:?} has a nameless hint");
            }
        }
    }
}

//! What the focused panel responds to, as the bar will say it.

use crate::model::{Key, KeyPress};
use crate::queries::QueriesTab;
use crate::search::{Direction, Searched};
use crate::state::{Overlay, Panel, State};

/// A binding, keyed by the presses `handle_key` actually matches on.
///
/// Not a display string: a string could name a press no arm of `handle_key`
/// claims and nothing would ever catch it, and a bar that lies is worse than
/// no bar (design §2). `update::tests` presses every one of these.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Binding {
    pub keys: &'static [Stroke],
    pub action: &'static str,
}

/// One way of pressing a binding.
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

/// Whether pressing a hint's key in this frame would do anything.
///
/// A hint that would not is drawn greyed rather than dropped: the binding is
/// still what the panel is for, and a bar whose contents move as the buffer
/// fills teaches less than one that stays put (docs/shortcuts.md §2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Liveness {
    Live,
    Inert,
}

/// One binding, as text, for a renderer that never sees a [`Key`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hint {
    pub keys: String,
    pub action: &'static str,
    pub liveness: Liveness,
}

/// What the focused panel does, and what works everywhere.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HintBar {
    pub panel: Vec<Hint>,
    pub global: Vec<Hint>,
    /// A running export's count, drawn on the bar's right (export design §5).
    pub export: Option<String>,
}

/// Bindings every panel answers to. Repeated in each panel's bar rather than
/// learned once: a user who never presses `tab` has no way to find the other
/// three panels (design §3).
///
/// The bar drops global hints from the left as the frame narrows, so position
/// is survival order, and [`QUIT`] is appended after these: `q quit` is last
/// to go because a user who can read one hint needs the way out, `?` sits just
/// ahead of it because it is the way to every binding the bar had no room for,
/// and `tab panel` — which teaches only one binding — is the first to go.
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

/// Global everywhere a letter is a binding — which is every panel but one.
/// In a panel that takes text `q` types a `q`, and naming it would be a lie
/// (queries editor design §6).
///
/// `pub(crate)` so `update`'s "every global hint does something" test can
/// still reach it now that it has left [`GLOBAL`].
pub(crate) const QUIT: Binding = Binding {
    keys: &[Stroke::key(Key::Char('q'))],
    action: "quit",
};

/// `esc` is named only here. The bar leaves it out everywhere else because it
/// does something only when an overlay is up, and naming it always would be a
/// lie in most frames (hint bar design §3). With the palette open it is never
/// a lie, and `?`/`esc` together are what the user is owed for `q` still
/// quitting (design §7) — both close it, so both are named.
///
/// `⏎ run` comes first so it is the first hint width-fitting gives up
/// (`lc-inbound::hints::fit` drops from the front of `global`) — a user
/// who has read this far still has `?`/`esc`/`q` to fall back on, the
/// ways out the bar keeps longest everywhere else (palette execution
/// design §12).
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

/// What the bar names while the diagnostics overlay is up. `esc` is the only
/// way out — `diagnostics_key` claims every key motion does not, including
/// `q`, so naming `quit` here would be the exact lie this bar exists to
/// prevent (design §9).
const DIAGNOSTICS_GLOBAL: &[Binding] = &[Binding {
    keys: &[Stroke::key(Key::Escape)],
    action: "close",
}];

/// What the bar names while the inspector is up. `j/k` is named for the
/// reason the theme picker names it: nothing else on screen says the block
/// moves (cell inspector design §6).
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

/// The `e` hint, appended to the inspector's bar and greyed wherever §2
/// refuses it — so the refusal is visible before the key is pressed, the
/// promise every greyed hint keeps
/// (docs/superpowers/specs/2026-09-18-cell-editing-design.md §7).
const EDIT: Binding = Binding {
    keys: &[Stroke::key(Key::Char('e'))],
    action: "edit",
};

/// What the bar names while a field is open. No panel group and no `q`: the
/// field claims every printable key, so naming anything else would be the
/// lie this bar exists to prevent (design §3).
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

/// The commit confirmation's own bar, and the quit warning's.
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

/// The `ctrl-s` hint, appended to Table contents' bar wherever anything is
/// uncommitted and left off entirely where nothing is: a key that would do
/// nothing at all is not an affordance (design §7).
const COMMIT_HINT: Binding = Binding {
    keys: &[Stroke::ctrl(Key::Char('s'))],
    action: "commit",
};

/// What the bar names in place of the focused panel's group while the picker
/// is open. Moving is the whole panel group, and it is named "preview"
/// because that is what the press does — the cursor writes `State.theme` on
/// every step (themes design §6). The one bar that still names `j`/`k`: what
/// it teaches is the side effect, not the motion.
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

/// The two ways out are named apart, because they differ in what they leave
/// behind: `⏎` keeps the previewed theme and `esc` puts back the one the
/// picker opened on (themes design §6). `q` is named for the reason
/// [`PALETTE_GLOBAL`] names it — the picker leaves quitting alone.
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

/// What the bar names in place of the focused panel's group while the
/// connection picker is open.
///
/// No `j`/`k` here where [`THEMES`] names them: the theme picker teaches the
/// motion because moving it previews a theme, and there is a side effect
/// worth naming. Moving this cursor does nothing until `⏎`, so the motion is
/// assumed as it is in every panel bar (docs/shortcuts.md §2).
pub(crate) const CONNECTIONS: &[Binding] = &[Binding {
    keys: &[Stroke::key(Key::Char('/'))],
    action: "search",
}];

/// The two ways out differ in what they leave behind, as the theme picker's
/// do: `⏎` opens the connection under the cursor, and `esc` leaves the tool
/// connected to whatever it already was — which at startup is nothing.
/// `q` is named for the reason [`PALETTE_GLOBAL`] names it.
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

/// What the bar names while the drop confirmation is up. Two sets, because
/// the two spellings are answered with different keys and a bar that named
/// both would be lying in every frame (drop design §2.1).
///
/// `q` is not named on either: `drop_key` claims every key it does not use,
/// quitting included, the same rule `DIAGNOSTICS_GLOBAL` follows.
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

/// The same two, for the target that deletes rows rather than dropping a
/// named thing. The verb is what the bar is for: `y` is the key either way,
/// and which of the two it answers is the only thing worth a word.
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

/// `⏎` is greyed until the field says the name it asked for — the promise
/// every other greyed hint keeps (docs/shortcuts.md §2).
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

/// `space` means what the stop under `tab` makes it mean, and nothing on the
/// path stop, where it is text (export design §2).
const TOGGLE: Binding = Binding {
    keys: &[Stroke::key(Key::Char(' '))],
    action: "toggle",
};

const EXPORT: Binding = Binding {
    keys: &[Stroke::key(Key::Char(' ')), Stroke::key(Key::Enter)],
    action: "export",
};

/// `q` is not named: the form claims it and does nothing with it.
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

/// What the bar names while a prompt is up, and while the filter modal is —
/// two text fields with the same two ways out, `⏎` writing what was typed and
/// `esc` leaving what was there (grid sort and filter design §3). The panel
/// group is empty for both: the focused panel responds to nothing, and naming
/// its bindings would be the lie design §2 exists to prevent (panel search
/// design §5.2).
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

/// Named only while a popup is up, which is the only frame in which it is
/// true: `tab` moves focus every other one, and `⏎` splits the line, and the
/// hints that say so are dropped for exactly these frames
/// (query editor design §5.5).
const COMPLETE: Binding = Binding {
    keys: &[Stroke::key(Key::Tab), Stroke::key(Key::Enter)],
    action: "complete",
};

/// `esc` is named only where pressing it unconditionally does something
/// (hint bar design §3). With a search applied, it clears one — which is
/// what keeps clearing discoverable (panel search design §5.3).
const CLEAR: Binding = Binding {
    keys: &[Stroke::key(Key::Escape)],
    action: "clear",
};

/// What `esc` does once nothing nearer claims it: every panel but Tables
/// hands focus back to Tables, so the bar says so in exactly those frames.
/// [`CLEAR`] wins where both would be true — one key gets one hint, and the
/// rung that fires first is the one worth naming (docs/shortcuts.md §2).
const LEAVE: Binding = Binding {
    keys: &[Stroke::key(Key::Escape)],
    action: "leave",
};

/// Named while `P` runs, the rung of `esc`'s chain that fires first there
/// (grid paging design §2).
const STOP: Binding = Binding {
    keys: &[Stroke::key(Key::Escape)],
    action: "stop",
};

/// `esc` while the Cql buffer is open: it closes the buffer and keeps focus,
/// so [`LEAVE`] would promise a move that does not happen.
const DONE: Binding = Binding {
    keys: &[Stroke::key(Key::Escape)],
    action: "done",
};

/// Named on the same condition as [`CLEAR`]: `n`/`N` are inert without a
/// search applied, and a hint that would do nothing this frame is a lie
/// (docs/shortcuts.md §2, design §5.3).
const NEXT_MATCH: Binding = Binding {
    keys: &[Stroke::key(Key::Char('n'))],
    action: "next match",
};

/// The connection picker's bar.
///
/// With nothing to list the picker is guidance rather than a list, and neither
/// searching it nor pressing `⏎` on it does anything — so the bar names
/// neither. A hint that would do nothing this frame is the lie this bar exists
/// to prevent (design §2, connection picker design §3).
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

/// The bar an overlay draws in place of the focused panel's, or `None` where
/// no overlay claims the row. Split out of [`bar`] only for its length.
fn overlay_bar(state: &State, export: Option<&String>) -> Option<HintBar> {
    let export = export.cloned();

    // An overlay with a prompt up is a panel that takes text, and the two
    // ways out are the prompt's two (overlay search design §8).
    if sifting(state) {
        return Some(HintBar {
            panel: Vec::new(),
            global: SEARCH_GLOBAL.iter().map(rendered).collect(),
            export,
        });
    }

    // No panel group: moving — by a row, by a screen, or to an end — is
    // assumed rather than taught, in here as in every panel bar
    // (docs/shortcuts.md §2). What is left, `⏎ run` and the two ways out,
    // is all global (palette execution design §12).
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

    // `?` promises a palette that draws; below `command_rows()` it would
    // not, so the bar drops the promise rather than make it (design §5.1).
    // Matched by key rather than sliced off the front, so a reorder of
    // `GLOBAL` cannot silently drop the wrong hint instead.
    // `tab` moves focus in every frame but one: with a completion popup up it
    // accepts what is selected, so the global hint would be a lie exactly
    // where the panel hint is true (query editor design §5.5).
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

    // `⏎ newline` is dropped for the frames `⏎` accepts instead, the same
    // way the global `tab panel` hint is above: one key, one true hint.
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

/// The confirmation's own bar. No panel group: the overlay claims every key,
/// so the tree underneath answers to nothing — the rule `Search` and the
/// export form already follow above.
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

/// The inspector's own bar. Each copy key is greyed where it copies nothing:
/// `y` over a null value, `Y` over a null field's line.
fn inspect_bar(
    state: &State,
    inspector: &crate::inspect::Inspector,
    export: Option<String>,
) -> HintBar {
    // An open field claims every key the block otherwise answers, so its bar
    // names only what it answers itself (design §3).
    if inspector.field().is_some() {
        return HintBar {
            panel: Vec::new(),
            global: FIELD
                .iter()
                .map(|binding| Hint {
                    liveness: if binding.keys == [Stroke::key(Key::Enter)]
                        && inspector
                            .field()
                            .is_none_or(|field| field.parsed().is_none())
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

    HintBar {
        panel,
        global: INSPECT_GLOBAL.iter().map(rendered).collect(),
        export,
    }
}

/// The form's own bar. The panel underneath answers to nothing, so it has no
/// panel group of its own; what the stop under `tab` answers to is named in
/// its place.
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
        Stop::Path => None,
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

/// Whether an overlay has a live search over its list.
fn sifting(state: &State) -> bool {
    match &state.overlay {
        Some(Overlay::Commands(panel)) => panel.sift().is_some(),
        Some(Overlay::Themes { sift, .. } | Overlay::Connections { sift, .. }) => sift.is_some(),
        _ => false,
    }
}

/// Whether the focused panel is carrying a live range. A hint must be true in
/// the frame it is drawn in, and under a live range `esc` unconditionally
/// clears it (docs/shortcuts.md §2) — the same rule that already puts `esc`
/// on the bar for a panel carrying a search.
fn selected(state: &State) -> bool {
    match state.focus() {
        Panel::Tables => state.tables.selection.is_active(),
        Panel::Ddl => state.ddl.selection.is_active(),
        Panel::Contents => state.contents.rows.is_active() || state.contents.columns.is_active(),
        // `false` on the Cql tab, which is an editor and has no selection to
        // be active; the Logs tab is a list like the other three.
        Panel::Queries => {
            state.queries.tab() == QueriesTab::Logs && state.log.selection.is_active()
        }
    }
}

/// `ctrl-r` on a buffer holding no statement submits nothing, silently
/// (query execution design §3), and an empty buffer is the panel's starting
/// state — so the hint the panel leads with would otherwise be a lie in the
/// first frame the user sees. It is greyed against the connection's posture
/// too, on the statement the band is over rather than on the whole buffer:
/// that is the one `ctrl-r` would send (cql statement selection design §5).
/// Matched by press rather than by action, for the same reason `?` is above.
///
/// `ctrl-f` is greyed on the same rule, against a buffer the formatter would
/// leave alone — which an empty one is (query editor design §4.3).
///
/// `ctrl-e` over Contents is refused the same way, on an empty grid, and on a
/// frame with no room to draw the form — the same promise `?` keeps above
/// (export design §1).
fn liveness(state: &State, binding: &Binding) -> Liveness {
    let runs = binding.keys == [Stroke::ctrl(Key::Char('r'))];
    if runs {
        let Some(cql) = state.queries.statement() else {
            return Liveness::Inert;
        };

        // Greyed against the posture as well as against an empty buffer: a
        // statement that writes on a read-only connection would be refused,
        // and the bar may not promise what `submit` will not do. Unlike `d`
        // this is not a property of the key — the same key runs a `SELECT`
        // in the same frame — so the hint follows the buffer as it is typed
        // (posture design §3).
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

    // The three column keys are named in column mode and answer only over a
    // page: `s` and `f` are greyed with no grid loaded, and `c` also with no
    // filter on the column the cursor is on — a key that would clear nothing
    // (grid sort and filter design §6).
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

    // `d` names a question, so it is greyed wherever there is none to ask —
    // read off the very derivation the key itself reads, so the bar cannot
    // promise one `open_drop` would refuse (drop design §1). That covers the
    // frame, which a user cannot see for themselves, and the grid's own gate:
    // a page whose primary key is not present addresses no row, and the greyed
    // key is where that is said (design §3.1).
    let drops = binding.keys == [Stroke::key(Key::Char('d'))];
    if drops
        && (!state.geometry.drop_fits()
            || state.dropping.is_some()
            || crate::update::drop_target(state).is_none())
    {
        return Liveness::Inert;
    }

    Liveness::Live
}

fn pages(binding: &Binding) -> bool {
    *binding == crate::state::NEXT_PAGE || *binding == crate::state::EVERY_PAGE
}

/// Whether the connection's posture forbids the binding outright. Such a hint
/// is left off the bar rather than greyed; the palette still names it.
///
/// `ctrl-r` is not one: whether it is refused follows the buffer as typed.
fn refused(state: &State, binding: &Binding) -> bool {
    binding.keys == [Stroke::key(Key::Char('d'))] && !state.posture().permits_mutation()
}

/// Whether `n`/`N` would move `target`'s cursor this frame: whether the
/// panel's own `next_match`, from where the cursor already sits, finds
/// anything — which is `false` both when the term matches nothing and when
/// Contents holds a search with no page loaded (design §5.3, §5.1).
fn next_match_is_live(state: &State, target: Searched) -> bool {
    match target {
        Searched::Tables => state.schema().is_some_and(|schema| {
            state
                .tables
                .next_match(schema, state.tables.cursor.index(), Direction::Forwards)
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

/// Named rather than drawn, for the keys with no printable form: the arrow
/// glyphs collide with each other, and a terminal font may draw any of them
/// as a box (design §3).
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::CommandsPanel;
    use crate::search::{Prompt, Search, Searched};
    use crate::state::{Cursor, Geometry, Overlay, Panel, State};
    use crate::theme::ThemeChoice;

    /// A frame big enough for a palette to draw in.
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

    // I2: the Logs tab is read-only and takes no text, so its bar must not
    // carry the Cql tab's editor bindings — `⏎ newline` types nothing there
    // (query log design §5.1) — and it must carry what the tab actually
    // does, the same `/`/`v` the other list panels advertise, plus the
    // `esc leave` every panel but Tables answers to.
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

    /// Design §8: the overlays name the key that searches them.
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

    /// Design §8: a live sift shows the prompt's own bar — the two ways out
    /// are the same two, and `q quit` goes with it, because `q` is text now.
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

    /// Design §8: the picker's own live sift collapses its bar the same way
    /// the palette's does — `sifting()`'s `Themes` arm, not only its
    /// `Commands` one.
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

    /// Connection picker design §5: the bar names `⏎ connect` rather than
    /// `⏎ keep`, and no `j/k` — moving this cursor previews nothing, so the
    /// motion is assumed as it is in every panel bar.
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
        }
    }

    /// Connection picker design §3: with nothing listed, `/` searches nothing
    /// and `⏎` opens nothing, so the bar names neither — the rule against a
    /// hint that would do nothing this frame (design §2).
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

    /// Overlay search design §8: a picker with a live sift is a panel that
    /// takes text, and the two ways out are the prompt's two.
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
        use crate::state::DiagnosticsPanel;

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

    /// The `Queries` bar with `text` in its buffer, `\n` meaning Enter.
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

    /// The buffer as typed, against the connection as opened: the same key in
    /// the same frame runs a `SELECT` and would be refused a `DELETE`, so the
    /// hint follows the text rather than the binding (posture design §3).
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
            let mut state = connected(posture, crate::state::SchemaState::Loading);
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

    fn connected(posture: crate::Posture, schema: crate::state::SchemaState) -> State {
        let mut state = fits();
        state.connection = crate::state::ConnectionState::Connected(crate::state::Connection {
            id: crate::model::ConnId::next(),
            name: crate::config::ConnectionName::try_new("local").expect("a valid name"),
            posture,
            schema,
        });
        state
    }

    /// Each half of the gate on its own, since neither is visible on screen: a
    /// frame with no room for the block, and a cursor naming nothing (drop
    /// design §1).
    #[test]
    fn the_drop_hint_is_live_only_where_the_key_would_open_something() {
        let loading = crate::state::SchemaState::Loading;
        let loaded = crate::state::SchemaState::Loaded(crate::tree::tests_support::schema_of(&[(
            "shop",
            &["orders"],
        )]));

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
    fn a_read_only_connection_leaves_d_off_the_bar() {
        let loaded = crate::state::SchemaState::Loaded(crate::tree::tests_support::schema_of(&[(
            "shop",
            &["orders"],
        )]));

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

    /// The grid's own gate: with no page loaded there is no row to address,
    /// so the key that would delete one is greyed (drop design §3.1).
    #[test]
    fn the_delete_hint_is_inert_over_a_grid_holding_no_addressable_row() {
        let mut state = connected(
            crate::Posture::Writable,
            crate::state::SchemaState::Loaded(crate::tree::tests_support::schema_of(&[(
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
                        || hint.action == "delete",
                    "{panel:?}'s '{} {}' is inert and nothing says why",
                    hint.keys,
                    hint.action
                );
            }
        }
    }
}

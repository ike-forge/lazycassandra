//! What the focused panel responds to, as the bar will say it.

use crate::model::{Key, KeyPress};
use crate::search::Filtered;
use crate::state::{Overlay, Panel, State};

/// A binding, keyed by the presses `handle_key` actually matches on.
///
/// Not a display string: a string could name a press no arm of `handle_key`
/// claims and nothing would ever catch it, and a bar that lies is worse than
/// no bar (design §2). `update::tests` presses every one of these.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Binding {
    pub keys: &'static [KeyPress],
    pub action: &'static str,
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
        keys: &[KeyPress::new(Key::Tab)],
        action: "panel",
    },
    Binding {
        keys: &[KeyPress::new(Key::Char('?'))],
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
    keys: &[KeyPress::new(Key::Char('q'))],
    action: "quit",
};

/// `esc` is named only here. The bar leaves it out everywhere else because it
/// does something only when an overlay is up, and naming it always would be a
/// lie in most frames (hint bar design §3). With the palette open it is never
/// a lie, and `?`/`esc` together are what the user is owed for `q` still
/// quitting (design §7) — both close it, so both are named.
const PALETTE_GLOBAL: &[Binding] = &[
    Binding {
        keys: &[KeyPress::new(Key::Char('?')), KeyPress::new(Key::Escape)],
        action: "close",
    },
    Binding {
        keys: &[KeyPress::new(Key::Char('q'))],
        action: "quit",
    },
];

/// What the bar names in place of the focused panel's group while the picker
/// is open. Moving is the whole panel group, and it is named "preview"
/// because that is what the press does — the cursor writes `State.theme` on
/// every step (themes design §6). The one bar that still names `j`/`k`: what
/// it teaches is the side effect, not the motion.
pub(crate) const THEMES: &[Binding] = &[Binding {
    keys: &[KeyPress::new(Key::Char('j')), KeyPress::new(Key::Char('k'))],
    action: "preview",
}];

/// The two ways out are named apart, because they differ in what they leave
/// behind: `⏎` keeps the previewed theme and `esc` puts back the one the
/// picker opened on (themes design §6). `q` is named for the reason
/// [`PALETTE_GLOBAL`] names it — the picker leaves quitting alone.
pub(crate) const THEMES_GLOBAL: &[Binding] = &[
    Binding {
        keys: &[KeyPress::new(Key::Enter)],
        action: "keep",
    },
    Binding {
        keys: &[KeyPress::new(Key::Escape)],
        action: "cancel",
    },
    Binding {
        keys: &[KeyPress::new(Key::Char('q'))],
        action: "quit",
    },
];

/// What the bar names while a prompt is up. The panel group is empty: the
/// focused panel responds to nothing, and naming its bindings would be the
/// lie design §2 exists to prevent (panel search design §5.2).
const SEARCH_GLOBAL: &[Binding] = &[
    Binding {
        keys: &[KeyPress::new(Key::Enter)],
        action: "apply",
    },
    Binding {
        keys: &[KeyPress::new(Key::Escape)],
        action: "cancel",
    },
];

/// `esc` is named only where pressing it unconditionally does something
/// (hint bar design §3). With a filter applied, it clears one — which is
/// what keeps clearing discoverable (panel search design §5.3).
const CLEAR: Binding = Binding {
    keys: &[KeyPress::new(Key::Escape)],
    action: "clear",
};

pub(crate) fn bar(state: &State) -> HintBar {
    // No panel group: `⏎` is inert until the palette can run a command
    // (design §10), and moving — by a row, by a screen, or to an end — is
    // assumed rather than taught, in here as in every panel bar
    // (docs/shortcuts.md §2). That leaves nothing the palette alone responds
    // to, so the bar names only the ways out.
    if matches!(state.overlay, Some(Overlay::Commands(_))) {
        return HintBar {
            panel: Vec::new(),
            global: PALETTE_GLOBAL.iter().map(rendered).collect(),
        };
    }

    if matches!(state.overlay, Some(Overlay::Themes { .. })) {
        return HintBar {
            panel: THEMES.iter().map(rendered).collect(),
            global: THEMES_GLOBAL.iter().map(rendered).collect(),
        };
    }

    if matches!(state.overlay, Some(Overlay::Search(_))) {
        return HintBar {
            panel: Vec::new(),
            global: SEARCH_GLOBAL.iter().map(rendered).collect(),
        };
    }

    // `?` promises a palette that draws; below `command_rows()` it would
    // not, so the bar drops the promise rather than make it (design §5.1).
    // Matched by key rather than sliced off the front, so a reorder of
    // `GLOBAL` cannot silently drop the wrong hint instead.
    let fits = state.geometry.command_rows() > 0;
    let mut global: Vec<Hint> = GLOBAL
        .iter()
        .filter(|binding| fits || !binding.keys.contains(&KeyPress::new(Key::Char('?'))))
        .map(rendered)
        .collect();
    if !state.focus.takes_text() {
        global.push(rendered(&QUIT));
    }

    let mut panel: Vec<Hint> = state
        .focus
        .hints()
        .iter()
        .map(|binding| Hint {
            liveness: liveness(state, binding),
            ..rendered(binding)
        })
        .collect();
    let filtered = Filtered::of(state.focus).is_some_and(|target| state.filter(target).is_some());
    if filtered || selected(state) {
        panel.push(rendered(&CLEAR));
    }

    HintBar { panel, global }
}

/// Whether the focused panel is carrying a live range. A hint must be true in
/// the frame it is drawn in, and under a live range `esc` unconditionally
/// clears it (docs/shortcuts.md §2) — the same rule that already puts `esc`
/// on the bar for a panel carrying a filter.
fn selected(state: &State) -> bool {
    match state.focus {
        Panel::Tables => state
            .tables
            .selection
            .range(state.tables.cursor.index())
            .is_some(),
        Panel::Ddl => state
            .ddl
            .selection
            .range(state.ddl.cursor.index())
            .is_some(),
        Panel::Contents => {
            state
                .contents
                .rows
                .range(state.contents.cursor.index())
                .is_some()
                || state
                    .contents
                    .columns
                    .range(state.contents.column.index())
                    .is_some()
        }
        Panel::Queries => false,
    }
}

/// `ctrl-r` on an empty or whitespace-only buffer submits nothing, silently
/// (query execution design §3), and an empty buffer is the panel's starting
/// state — so the hint the panel leads with would otherwise be a lie in the
/// first frame the user sees. Matched by press rather than by action, for the
/// same reason `?` is above.
///
/// Nothing else in the bar is conditional today; the mechanism is general and
/// this is its one use.
fn liveness(state: &State, binding: &Binding) -> Liveness {
    let runs = binding.keys == [KeyPress::ctrl(Key::Char('r'))];
    if runs && state.queries.text().trim().is_empty() {
        Liveness::Inert
    } else {
        Liveness::Live
    }
}

fn rendered(binding: &Binding) -> Hint {
    Hint {
        keys: label(binding.keys),
        action: binding.action,
        liveness: Liveness::Live,
    }
}

pub(crate) fn label(keys: &[KeyPress]) -> String {
    keys.iter().copied().map(name).collect::<Vec<_>>().join("/")
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
    use crate::search::{Filter, Filtered, Prompt};
    use crate::state::{Cursor, Geometry, Overlay, Panel, State};
    use crate::theme::ThemeChoice;

    /// A frame big enough for a palette to draw in.
    fn fits() -> State {
        State {
            geometry: Geometry::new([8, 14, 20, 1], 37, 14),
            ..State::default()
        }
    }

    #[test]
    fn a_binding_behind_ctrl_is_named_with_a_ctrl_prefix() {
        assert_eq!(
            label(&[
                KeyPress::ctrl(Key::Char('d')),
                KeyPress::ctrl(Key::Char('u')),
            ]),
            "ctrl-d/ctrl-u"
        );
        assert_eq!(label(&[KeyPress::new(Key::Char('j'))]), "j");
    }

    #[test]
    fn several_keys_for_one_action_join_with_a_slash() {
        assert_eq!(
            label(&[KeyPress::new(Key::Char('j')), KeyPress::new(Key::Char('k'))]),
            "j/k"
        );
        assert_eq!(
            label(&[KeyPress::new(Key::PageDown), KeyPress::new(Key::PageUp)]),
            "pgdn/pgup"
        );
    }

    #[test]
    fn a_key_with_no_printable_form_is_named_rather_than_drawn() {
        assert_eq!(
            label(&[KeyPress::new(Key::Home), KeyPress::new(Key::End)]),
            "home/end"
        );
        assert_eq!(label(&[KeyPress::new(Key::Char(' '))]), "space");
        assert_eq!(label(&[KeyPress::new(Key::Tab)]), "tab");
    }

    #[test]
    fn the_bar_carries_the_focused_panels_bindings_and_the_global_ones() {
        let state = State {
            focus: Panel::Tables,
            ..State::default()
        };

        let bar = bar(&state);

        assert_eq!(bar.panel[0].keys, "⏎");
        assert_eq!(bar.panel[0].action, "open");
        assert_eq!(bar.global.last().expect("q quit is last").action, "quit");
    }

    #[test]
    fn no_panel_contributes_an_empty_group_the_reader_has_to_interpret() {
        for panel in Panel::ALL {
            let state = State {
                focus: panel,
                ..fits()
            };

            assert!(
                !bar(&state).panel.is_empty(),
                "{panel:?} would draw a group with nothing in it"
            );
        }
    }

    #[test]
    fn a_panel_that_takes_text_does_not_claim_q_quits() {
        let state = State {
            focus: Panel::Queries,
            ..fits()
        };

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
        let state = State {
            overlay: Some(Overlay::Commands(CommandsPanel::opened())),
            ..State::default()
        };

        let bar = bar(&state);

        assert!(
            bar.panel.is_empty(),
            "the palette answers to nothing a terminal list does not: {:?}",
            bar.panel
        );
        assert_eq!(bar.global[0].keys, "?/esc");
        assert_eq!(bar.global[0].action, "close");
        assert_eq!(bar.global.last().expect("q is last").action, "quit");
    }

    #[test]
    fn the_bar_names_what_the_picker_responds_to() {
        let state = State {
            overlay: Some(Overlay::Themes {
                cursor: Cursor::default(),
                restored: ThemeChoice::default(),
            }),
            ..State::default()
        };

        let bar = bar(&state);

        assert_eq!(
            bar.panel
                .iter()
                .map(|hint| (hint.keys.as_str(), hint.action))
                .collect::<Vec<_>>(),
            [("j/k", "preview")],
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
    fn the_bar_names_search_in_every_panel_that_can_hold_a_filter() {
        for panel in [Panel::Tables, Panel::Ddl, Panel::Contents] {
            let state = State {
                focus: panel,
                ..fits()
            };

            assert!(
                bar(&state).panel.iter().any(|hint| hint.keys == "/"),
                "{panel:?} responds to / and must say so"
            );
        }
    }

    #[test]
    fn the_bar_does_not_name_search_in_queries() {
        let state = State {
            focus: Panel::Queries,
            ..fits()
        };

        assert!(bar(&state).panel.iter().all(|hint| hint.keys != "/"));
    }

    #[test]
    fn the_bar_names_esc_clear_only_where_a_filter_is_applied() {
        let mut state = State {
            focus: Panel::Tables,
            ..fits()
        };
        assert!(bar(&state).panel.iter().all(|hint| hint.action != "clear"));

        state.apply_filter(Filtered::Tables, Filter::new("o"));

        assert!(
            bar(&state)
                .panel
                .iter()
                .any(|hint| hint.keys == "esc" && hint.action == "clear"),
            "in this frame pressing esc unconditionally does something"
        );
    }

    #[test]
    fn the_bar_names_what_the_prompt_responds_to() {
        let mut state = fits();
        state.overlay = Some(Overlay::Search(Prompt::opened(Filtered::Tables, None)));

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
        state.focus = Panel::Queries;

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
        state.focus = Panel::Queries;

        let panel = bar(&state).panel;

        assert_eq!(
            panel
                .iter()
                .map(|hint| (hint.keys.as_str(), hint.action))
                .take(2)
                .collect::<Vec<_>>(),
            [("ctrl-r", "run"), ("ctrl-e", "edit")],
            "the panel exists to run queries; editing one is the next thing it does"
        );
    }

    #[test]
    fn no_other_bar_names_the_editor_binding() {
        for focus in [Panel::Tables, Panel::Ddl, Panel::Contents] {
            let state = State { focus, ..fits() };

            assert!(
                bar(&state).panel.iter().all(|hint| hint.action != "edit"),
                "{focus:?} does not open an editor"
            );
        }
    }

    /// The `Queries` bar with `text` in its buffer, `\n` meaning Enter.
    fn queries_bar(text: &str) -> HintBar {
        let mut state = fits();
        state.focus = Panel::Queries;
        for character in text.chars() {
            if character == '\n' {
                state.queries.insert_newline();
            } else {
                state.queries.insert(character);
            }
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
    fn the_bar_names_esc_while_a_range_stands() {
        let mut state = State {
            focus: Panel::Tables,
            ..fits()
        };
        state.tables.selection.start(0);

        let bar = bar(&state);

        assert!(
            bar.panel.iter().any(|hint| hint.keys == "esc"),
            "under a live range, pressing esc unconditionally does something"
        );
    }

    #[test]
    fn the_bar_leaves_esc_off_a_panel_with_neither_a_range_nor_a_filter() {
        let state = State {
            focus: Panel::Tables,
            ..fits()
        };

        assert!(!bar(&state).panel.iter().any(|hint| hint.keys == "esc"));
    }

    #[test]
    fn nothing_but_the_run_hint_is_ever_inert() {
        for panel in Panel::ALL {
            let state = State {
                focus: panel,
                ..fits()
            };
            let bar = bar(&state);

            for hint in bar.panel.iter().chain(&bar.global) {
                assert!(
                    hint.liveness == Liveness::Live || hint.action == "run",
                    "{panel:?}'s '{} {}' is inert and nothing says why",
                    hint.keys,
                    hint.action
                );
            }
        }
    }
}

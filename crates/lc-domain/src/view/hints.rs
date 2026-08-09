//! What the focused panel responds to, as the bar will say it.

use crate::model::Key;
use crate::state::{Overlay, State};

/// A binding, keyed by the values `handle_key` actually matches on.
///
/// Not a display string: a string could name a key no arm of `handle_key`
/// claims and nothing would ever catch it, and a bar that lies is worse than
/// no bar (design §2). `update::tests` presses every one of these.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Binding {
    pub keys: &'static [Key],
    pub action: &'static str,
}

/// One binding, as text, for a renderer that never sees a [`Key`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hint {
    pub keys: String,
    pub action: &'static str,
}

/// What the focused panel contributes to the bar.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HintGroup {
    Keys(Vec<Hint>),
    /// A panel that responds to nothing yet, saying so rather than drawing
    /// an empty group the reader has to interpret.
    Idle(&'static str),
}

/// What the focused panel does, and what works everywhere.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HintBar {
    pub panel: HintGroup,
    pub global: Vec<Hint>,
}

/// Bindings every panel answers to. Repeated in each panel's bar rather than
/// learned once: a user who never presses `tab` has no way to find the other
/// three panels (design §3).
///
/// The bar drops global hints from the left as the frame narrows, so position
/// is survival order: `q quit` is last to go because a user who can read one
/// hint needs the way out, `?` sits just ahead of it because it is the way to
/// every binding the bar had no room for, and `tab panel` — which teaches only
/// one binding — is the first to go.
pub(crate) const GLOBAL: &[Binding] = &[
    Binding {
        keys: &[Key::Tab],
        action: "panel",
    },
    Binding {
        keys: &[Key::Char('?')],
        action: "commands",
    },
    Binding {
        keys: &[Key::Char('q')],
        action: "quit",
    },
];

/// What the bar names in place of the focused panel's group while the
/// palette is open (design §7). Three, because that is all of it: `esc`/`q`
/// are `PALETTE_GLOBAL`'s below, and `⏎`/`space` are inert until the palette
/// can run a command (design §10), so scrolling by a row, a screen, or to an
/// end is everything left for the bar to say.
const PALETTE: &[Binding] = &[
    Binding {
        keys: &[Key::Char('j'), Key::Char('k')],
        action: "scroll",
    },
    Binding {
        keys: &[Key::PageDown, Key::PageUp],
        action: "page",
    },
    Binding {
        keys: &[Key::Home, Key::End],
        action: "ends",
    },
];

/// `esc` is named only here. The bar leaves it out everywhere else because it
/// does something only when an overlay is up, and naming it always would be a
/// lie in most frames (hint bar design §3). With the palette open it is never
/// a lie, and `?`/`esc` together are what the user is owed for `q` still
/// quitting (design §7) — both close it, so both are named.
const PALETTE_GLOBAL: &[Binding] = &[
    Binding {
        keys: &[Key::Char('?'), Key::Escape],
        action: "close",
    },
    Binding {
        keys: &[Key::Char('q')],
        action: "quit",
    },
];

pub(crate) fn bar(state: &State) -> HintBar {
    if matches!(state.overlay, Some(Overlay::Commands(_))) {
        return HintBar {
            panel: HintGroup::Keys(PALETTE.iter().map(rendered).collect()),
            global: PALETTE_GLOBAL.iter().map(rendered).collect(),
        };
    }

    // `?` promises a palette that draws; below `command_rows()` it would
    // not, so the bar drops the promise rather than make it (design §5.1).
    // Matched by key rather than sliced off the front, so a reorder of
    // `GLOBAL` cannot silently drop the wrong hint instead.
    let fits = state.geometry.command_rows() > 0;
    let global = GLOBAL
        .iter()
        .filter(|binding| fits || !binding.keys.contains(&Key::Char('?')))
        .map(rendered)
        .collect();

    HintBar {
        panel: match state.focus.hints() {
            [] => HintGroup::Idle("nothing here yet"),
            bindings => HintGroup::Keys(bindings.iter().map(rendered).collect()),
        },
        global,
    }
}

fn rendered(binding: &Binding) -> Hint {
    Hint {
        keys: label(binding.keys),
        action: binding.action,
    }
}

pub(crate) fn label(keys: &[Key]) -> String {
    keys.iter().copied().map(name).collect::<Vec<_>>().join("/")
}

/// Named rather than drawn, for the keys with no printable form: the arrow
/// glyphs collide with each other, and a terminal font may draw any of them
/// as a box (design §3).
fn name(key: Key) -> String {
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
    use crate::state::{Geometry, Overlay, Panel, State};

    /// A frame big enough for a palette to draw in.
    fn fits() -> State {
        State {
            geometry: Geometry::new([8, 14, 20, 1], 37, 14),
            ..State::default()
        }
    }

    #[test]
    fn several_keys_for_one_action_join_with_a_slash() {
        assert_eq!(label(&[Key::Char('j'), Key::Char('k')]), "j/k");
        assert_eq!(label(&[Key::PageDown, Key::PageUp]), "pgdn/pgup");
    }

    #[test]
    fn a_key_with_no_printable_form_is_named_rather_than_drawn() {
        assert_eq!(label(&[Key::Home, Key::End]), "home/end");
        assert_eq!(label(&[Key::Char(' ')]), "space");
        assert_eq!(label(&[Key::Tab]), "tab");
    }

    #[test]
    fn the_bar_carries_the_focused_panels_bindings_and_the_global_ones() {
        let state = State {
            focus: Panel::Tables,
            ..State::default()
        };

        let bar = bar(&state);

        let HintGroup::Keys(hints) = &bar.panel else {
            panic!("Tables has bindings, so its group is not idle");
        };
        assert_eq!(hints[0].keys, "j/k");
        assert_eq!(hints[0].action, "move");
        assert_eq!(bar.global.last().expect("q quit is last").action, "quit");
    }

    #[test]
    fn a_panel_with_no_bindings_says_so_rather_than_showing_an_empty_group() {
        let state = State {
            focus: Panel::Queries,
            ..State::default()
        };

        assert_eq!(bar(&state).panel, HintGroup::Idle("nothing here yet"));
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

        let HintGroup::Keys(hints) = &bar.panel else {
            panic!("an open palette has bindings");
        };
        assert_eq!(hints[0].action, "scroll");
        assert_eq!(bar.global[0].keys, "?/esc");
        assert_eq!(bar.global[0].action, "close");
        assert_eq!(bar.global.last().expect("q is last").action, "quit");
    }
}

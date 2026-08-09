//! What the focused panel responds to, as the bar will say it.

use crate::model::Key;
use crate::state::Panel;

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

/// What the focused panel does, and what works everywhere.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HintBar {
    pub panel: Vec<Hint>,
    pub global: Vec<Hint>,
}

/// Bindings every panel answers to. Repeated in each panel's bar rather than
/// learned once: a user who never presses `tab` has no way to find the other
/// three panels (design §3).
pub(crate) const GLOBAL: &[Binding] = &[Binding {
    keys: &[Key::Tab],
    action: "panel",
}];

/// Global everywhere a letter is a binding — which is every panel but one.
/// In a panel that takes text `q` types a `q`, and naming it would be a lie
/// (queries editor design §6).
///
/// `pub(crate)` so `update`'s "every global hint does something" test can
/// still reach it now that it has left [`GLOBAL`].
pub(crate) const QUIT: Binding = Binding {
    keys: &[Key::Char('q')],
    action: "quit",
};

pub(crate) fn bar(focus: Panel) -> HintBar {
    let mut global: Vec<Hint> = GLOBAL.iter().map(rendered).collect();
    if !focus.takes_text() {
        global.push(rendered(&QUIT));
    }

    HintBar {
        panel: focus.hints().iter().map(rendered).collect(),
        global,
    }
}

fn rendered(binding: &Binding) -> Hint {
    Hint {
        keys: label(binding.keys),
        action: binding.action,
    }
}

fn label(keys: &[Key]) -> String {
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
        let bar = bar(Panel::Tables);

        assert_eq!(bar.panel[0].keys, "j/k");
        assert_eq!(bar.panel[0].action, "move");
        assert_eq!(bar.global.last().expect("q quit is last").action, "quit");
    }

    #[test]
    fn a_panel_that_takes_text_does_not_claim_q_quits() {
        let bar = bar(Panel::Queries);

        assert_eq!(bar.panel[0].action, "newline");
        assert!(
            bar.global.iter().all(|hint| hint.action != "quit"),
            "q types a q in the editor, and a bar that lies is worse than no bar"
        );
        assert_eq!(
            bar.global.last().expect("tab panel survives").action,
            "panel",
            "tab is still the way out"
        );
    }
}

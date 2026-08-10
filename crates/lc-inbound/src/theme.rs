//! Semantic tokens into concrete styles.
//!
//! The default wears the terminal's clothes (vision.md §4, principle 5): it
//! never sets a background and leaves body text the terminal's own foreground,
//! so it sits inside whatever palette the user already configured rather than
//! repainting over it. Highlights are ours to choose, truecolor included
//! (doc/specs/2026-08-09-visual-polish-design.md §7).
//!
//! Meaning still comes first. The sixteen ANSI colours carry the things the
//! user must be able to read on a terminal whose palette is unusable — key
//! structure, severity, write posture — and the two accents below are spent
//! on things that are marked rather than meant.

use lc_domain::posture::Posture;
use lc_domain::style::{Severity, Token};
use ratatui::style::{Color, Modifier, Style};

/// The loader's scanner head, and the one colour spent on decoration rather
/// than meaning. `ratatui_opentui_loader` derives its whole trail from it.
pub(crate) const ACCENT: Color = Color::Rgb(0xfa, 0xb2, 0x83);
/// A property name in the Table DDL panel: light enough to stay legible on a
/// dark background without washing out on a light one.
const PROPERTY: Color = Color::Rgb(0x7a, 0xa2, 0xf7);
/// A cell the applied filter matched: warm enough to stand out from
/// `PROPERTY`'s cool blue, and unrelated to any severity or key colour.
const MATCH: Color = Color::Rgb(0xe0, 0xaf, 0x68);

#[derive(Debug, Clone, Copy, Default)]
pub enum Theme {
    #[default]
    TerminalDefault,
}

impl Theme {
    #[must_use]
    pub fn style(self, token: Token) -> Style {
        match self {
            Self::TerminalDefault => Self::terminal_default(token),
        }
    }

    fn terminal_default(token: Token) -> Style {
        // No `.bg(..)` anywhere in this function. Setting one is what makes a
        // TUI clash with the split next to it.
        match token {
            Token::PartitionKey => Style::new().fg(Color::Yellow).add_modifier(Modifier::BOLD),
            Token::ClusteringKey => Style::new().fg(Color::Cyan),
            // Info spends no colour deliberately: it is the baseline, and
            // colouring it would dilute warning and danger.
            Token::RegularColumn | Token::Severity(Severity::Info) => Style::new(),
            Token::Posture(Posture::ReadOnly) => Style::new().fg(Color::Blue),
            Token::Posture(Posture::Writable) => Style::new().fg(Color::Green),
            Token::Posture(Posture::Protected) => {
                Style::new().fg(Color::Magenta).add_modifier(Modifier::BOLD)
            }
            Token::Severity(Severity::Warning) => Style::new().fg(Color::Yellow),
            Token::Severity(Severity::Danger) => {
                Style::new().fg(Color::Red).add_modifier(Modifier::BOLD)
            }
            // Spends an accent rather than an ANSI slot: a property name marks
            // where to look, it does not mean anything the way a key column or
            // a severity does. Its value is left unstyled — two treatments on
            // one line of a twenty-column panel is one too many.
            Token::PropertyName => Style::new().fg(PROPERTY),
            // The hint bar's action reads the same as anything muted: quiet,
            // and spending no colour of its own.
            Token::Muted | Token::HintAction => Style::new().add_modifier(Modifier::DIM),
            // Bold rather than a colour: focus has to read on a terminal
            // whose palette is unusable, and it must not compete with the
            // colours that carry meaning. The hint bar's key carries the same
            // weight for the same reason.
            Token::Focused | Token::HintKey => Style::new().add_modifier(Modifier::BOLD),
            // Underlined rather than coloured: it labels a key, and it has to
            // stay readable beside a title that is bold when focused.
            Token::Shortcut => Style::new().add_modifier(Modifier::UNDERLINED),
            // Reversed video needs no explicit colour, so a selection reads
            // the same way Focused does on an impoverished palette.
            Token::Selected => Style::new().add_modifier(Modifier::REVERSED),
            // A colour, not REVERSED: `Token::Selected` already owns that
            // modifier, and REVERSED over REVERSED cancels out — the
            // cursor's row can contain matches, and both must still read.
            // `fg` alone (no `bg`) survives being patched under the row's
            // highlight style, so a matched, selected cell keeps its colour.
            Token::Match => Style::new().fg(MATCH).add_modifier(Modifier::BOLD),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn focus_is_drawn_without_spending_a_colour() {
        let style = Theme::TerminalDefault.style(Token::Focused);

        assert!(style.add_modifier.contains(Modifier::BOLD));
        assert_eq!(
            style.fg, None,
            "focus must survive a terminal with a poor palette"
        );
    }

    #[test]
    fn no_token_sets_a_background() {
        let tokens = [
            Token::PartitionKey,
            Token::ClusteringKey,
            Token::RegularColumn,
            Token::Posture(Posture::ReadOnly),
            Token::Posture(Posture::Writable),
            Token::Posture(Posture::Protected),
            Token::Severity(Severity::Info),
            Token::Severity(Severity::Warning),
            Token::Severity(Severity::Danger),
            Token::Muted,
            Token::Focused,
            Token::Selected,
            Token::Shortcut,
            Token::HintKey,
            Token::HintAction,
            Token::PropertyName,
            Token::Match,
        ];

        for token in tokens {
            assert_eq!(
                Theme::TerminalDefault.style(token).bg,
                None,
                "{token:?} set a background (vision.md §4, principle 5)"
            );
        }
    }

    #[test]
    fn the_selected_row_is_marked_without_spending_a_colour() {
        let style = Theme::TerminalDefault.style(Token::Selected);

        assert!(style.add_modifier.contains(Modifier::REVERSED));
        assert_eq!(
            style.fg, None,
            "the selection inverts the user's own palette rather than replacing it"
        );
    }

    #[test]
    fn a_shortcut_is_underlined_without_spending_a_colour() {
        let style = Theme::TerminalDefault.style(Token::Shortcut);

        assert!(style.add_modifier.contains(Modifier::UNDERLINED));
        assert_eq!(
            style.fg, None,
            "every colour in this theme already carries a meaning"
        );
    }

    #[test]
    fn a_match_is_legible_over_a_selected_row() {
        let theme = Theme::TerminalDefault;
        let selected = theme.style(Token::Selected);
        let matched = theme.style(Token::Match);

        assert_ne!(
            matched, selected,
            "the cursor's row can contain matches; both must still read"
        );
        assert!(
            matched.fg.is_some() || matched.bg.is_some(),
            "REVERSED over REVERSED cancels out, so Match must carry a colour"
        );
    }
}

//! Semantic tokens into concrete styles.
//!
//! The default wears the terminal's clothes (vision.md §4, principle 5): it
//! maps only to the sixteen ANSI colours and never sets a background, so it
//! inherits whatever palette the user already configured and stays legible on
//! light and dark alike. Themes that hardcode RGB are possible and strictly
//! opt-in.

use lc_domain::posture::Posture;
use lc_domain::style::{Severity, Token};
use ratatui::style::{Color, Modifier, Style};

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
            Token::Muted => Style::new().add_modifier(Modifier::DIM),
            // Bold rather than a colour: focus has to read on a terminal
            // whose palette is unusable, and it must not compete with the
            // colours that carry meaning.
            Token::Focused => Style::new().add_modifier(Modifier::BOLD),
            // Reversed video needs no explicit colour, so a selection reads
            // the same way Focused does on an impoverished palette.
            Token::Selected => Style::new().add_modifier(Modifier::REVERSED),
            // Underlined rather than coloured: it labels a key, and it has to
            // stay readable beside a title that is bold when focused.
            Token::Shortcut => Style::new().add_modifier(Modifier::UNDERLINED),
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
}

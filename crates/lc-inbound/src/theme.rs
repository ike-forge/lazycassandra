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
        }
    }
}

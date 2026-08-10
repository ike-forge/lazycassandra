//! Semantic tokens into concrete styles.
//!
//! A theme is a [`Palette`] — a table of named colour roles — and one mapping
//! from [`Token`] to a style, shared by every theme. Adding a theme is adding
//! a `const Palette`, not a match arm per token
//! (doc/specs/2026-08-10-themes-design.md §3).
//!
//! Four roles are `Option<Color>`. `None` is not "no colour": it is *not ours
//! to set*, which is how vision.md §4 principle 5 is stated in the type system
//! rather than in a comment.

mod catppuccin;
mod default;

#[cfg(test)]
mod contrast;

use lc_domain::posture::Posture;
use lc_domain::style::{Severity, Token};
use ratatui::style::{Color, Modifier, Style};

/// One theme's colours, by the meaning each is spent on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Palette {
    /// The whole frame. `None` leaves the terminal's own.
    pub(crate) base: Option<Color>,
    /// An overlay, so it separates from the field it covers.
    pub(crate) surface: Option<Color>,
    /// Body text, and `Severity::Info`.
    pub(crate) text: Option<Color>,
    pub(crate) border: Option<Color>,
    /// The cursor row's background.
    pub(crate) highlight: Color,
    /// The cursor row's text, where the theme cannot guarantee its own roles
    /// read against `highlight` (design §3.1). `None` keeps the row's own
    /// column colours; `Some` flattens them for legibility.
    pub(crate) highlight_text: Option<Color>,
    pub(crate) muted: Color,
    pub(crate) partition: Color,
    pub(crate) clustering: Color,
    pub(crate) keyspace: Color,
    pub(crate) table: Color,
    pub(crate) property: Color,
    pub(crate) accent: Color,
    pub(crate) match_: Color,
    pub(crate) success: Color,
    pub(crate) warning: Color,
    pub(crate) danger: Color,
    pub(crate) keyword: Color,
    pub(crate) reference: Color,
    pub(crate) literal: Color,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Theme {
    palette: &'static Palette,
}

impl Default for Theme {
    fn default() -> Self {
        Self {
            palette: &default::DEFAULT,
        }
    }
}

impl Theme {
    pub(crate) const fn palette(self) -> &'static Palette {
        self.palette
    }

    #[must_use]
    pub fn style(self, token: Token) -> Style {
        let palette = self.palette;

        match token {
            Token::PartitionKey => fg(palette.partition).add_modifier(Modifier::BOLD),
            Token::ClusteringKey => fg(palette.clustering),
            Token::Keyspace => fg(palette.keyspace),
            Token::Table => fg(palette.table),
            Token::RegularColumn
            | Token::Severity(Severity::Info)
            | Token::Posture(Posture::ReadOnly) => maybe(palette.text),
            Token::Posture(Posture::Writable) => fg(palette.success),
            Token::Posture(Posture::Protected) | Token::Severity(Severity::Danger) => {
                fg(palette.danger).add_modifier(Modifier::BOLD)
            }
            Token::Severity(Severity::Warning) => fg(palette.warning),
            Token::PropertyName => fg(palette.property),
            // `DIM` alongside the colour, not the colour alone: `Style::patch`
            // merges `add_modifier` by insertion rather than overwriting it,
            // so a muted cell composed with `Token::Match` keeps `DIM` even
            // though its colour is overwritten — the same distinction the
            // pre-`Palette` theme drew with `DIM` alone (design §4.3).
            Token::Muted | Token::HintAction => fg(palette.muted).add_modifier(Modifier::DIM),
            // Bold rather than a colour: focus must not compete with the
            // colours that carry meaning, and it has to read on a panel border
            // the theme may have coloured already.
            Token::Focused | Token::HintKey => Style::new().add_modifier(Modifier::BOLD),
            Token::Shortcut => Style::new().add_modifier(Modifier::UNDERLINED),
            Token::Selected => selected(palette),
            // `fg` and `BOLD` both: where a palette declines
            // `highlight_text`, the colour survives being patched under the
            // row's highlight style; where it spends one, that fg overwrites
            // this one on the cursor's row (`Style::patch`'s `other.fg.or`),
            // and `BOLD` is what still marks the match there.
            Token::Match => fg(palette.match_).add_modifier(Modifier::BOLD),
            // A modifier rather than a colour, because it has to read
            // *against* the highlighted row whatever colour that row is.
            Token::SelectedCell => Style::new()
                .add_modifier(Modifier::REVERSED)
                .add_modifier(Modifier::UNDERLINED),
            Token::Keyword => fg(palette.keyword),
            Token::Reference => fg(palette.reference),
            Token::Literal => fg(palette.literal),
        }
    }
}

/// The cursor row.
///
/// A background alone leaves the row's own colours alone, which is what a
/// theme that has verified its palette wants. A theme that cannot know what it
/// is drawn over spends a foreground too and gives those colours up for the
/// one row (design §3.1).
fn selected(palette: &Palette) -> Style {
    let style = Style::new().bg(palette.highlight);

    match palette.highlight_text {
        Some(color) => style.fg(color),
        None => style,
    }
}

fn fg(color: Color) -> Style {
    Style::new().fg(color)
}

fn maybe(color: Option<Color>) -> Style {
    match color {
        Some(color) => Style::new().fg(color),
        None => Style::new(),
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::theme::contrast::{BLACK, WHITE, ratio};

    /// Every role a theme can spend on text. `base`, `surface` and
    /// `highlight` are what text is drawn *on* and are absent by design.
    pub(super) fn foregrounds(palette: &Palette) -> Vec<(&'static str, Color)> {
        vec![
            ("muted", palette.muted),
            ("partition", palette.partition),
            ("clustering", palette.clustering),
            ("keyspace", palette.keyspace),
            ("table", palette.table),
            ("property", palette.property),
            ("accent", palette.accent),
            ("match_", palette.match_),
            ("success", palette.success),
            ("warning", palette.warning),
            ("danger", palette.danger),
            ("keyword", palette.keyword),
            ("reference", palette.reference),
            ("literal", palette.literal),
        ]
    }

    #[test]
    fn the_default_theme_paints_no_panel_field() {
        let palette = Theme::default().palette();

        assert_eq!(palette.base, None, "the background is the user's");
        assert_eq!(palette.surface, None);
        assert_eq!(palette.text, None, "so is the body foreground");
        assert_eq!(palette.border, None);
    }

    #[test]
    fn the_cursor_row_is_the_only_token_that_paints_a_background() {
        let theme = Theme::default();

        for token in EVERY_TOKEN {
            let background = theme.style(token).bg;

            if matches!(token, Token::Selected) {
                assert!(background.is_some(), "the cursor row is a bar");
            } else {
                assert_eq!(background, None, "{token:?} painted over the user's field");
            }
        }
    }

    #[test]
    fn the_default_palette_reads_on_black_and_on_white() {
        let palette = Theme::default().palette();

        for (role, color) in foregrounds(palette) {
            for (name, ground) in [("black", BLACK), ("white", WHITE)] {
                let ratio = ratio(color, ground);
                assert!(
                    ratio >= 3.0,
                    "{role} is {ratio:.2} against {name}; \
                     this theme does not know which it is on"
                );
            }
        }
    }

    #[test]
    fn the_default_cursor_row_carries_its_own_foreground() {
        let palette = Theme::default().palette();
        let text = palette
            .highlight_text
            .expect("a theme that cannot see its background must not rely on it");

        let ratio = ratio(text, palette.highlight);
        assert!(ratio >= 4.5, "{ratio:.2}");
    }

    #[test]
    fn focus_is_drawn_without_spending_a_colour() {
        let style = Theme::default().style(Token::Focused);

        assert!(style.add_modifier.contains(Modifier::BOLD));
        assert_eq!(style.fg, None, "focus must not compete with meaning");
    }

    #[test]
    fn a_shortcut_is_underlined_without_spending_a_colour() {
        let style = Theme::default().style(Token::Shortcut);

        assert!(style.add_modifier.contains(Modifier::UNDERLINED));
        assert_eq!(style.fg, None);
    }

    #[test]
    fn a_selected_cell_reads_against_the_selected_row_without_spending_a_colour() {
        let theme = Theme::default();
        let cell = theme.style(Token::SelectedCell);

        assert_ne!(theme.style(Token::Selected).add_modifier, cell.add_modifier);
        assert_eq!(cell.fg, None);
        assert_eq!(cell.bg, None);
    }

    #[test]
    fn a_match_is_legible_over_a_selected_row() {
        let theme = Theme::default();
        let matched = theme.style(Token::Match);
        let selected = theme.style(Token::Selected);

        assert_ne!(matched, selected);
        assert!(
            matched.add_modifier.contains(Modifier::BOLD)
                && !selected.add_modifier.contains(Modifier::BOLD),
            "a match must carry something the row's own highlight does not, \
             so it still reads once patched under it"
        );
    }

    /// Every token, so a new one cannot be added without deciding what the
    /// tests above have to say about it.
    pub(super) const EVERY_TOKEN: [Token; 23] = [
        Token::PartitionKey,
        Token::ClusteringKey,
        Token::Keyspace,
        Token::Table,
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
        Token::SelectedCell,
        Token::Shortcut,
        Token::HintKey,
        Token::HintAction,
        Token::PropertyName,
        Token::Match,
        Token::Keyword,
        Token::Reference,
        Token::Literal,
    ];
}

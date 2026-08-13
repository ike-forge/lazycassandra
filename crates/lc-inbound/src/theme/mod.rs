//! Semantic tokens into concrete styles.
//!
//! A theme is a [`Palette`] — a table of named colour roles — and one mapping
//! from [`Token`] to a style, shared by every theme. Adding a theme is adding
//! a `const Palette`, not a match arm per token
//! (docs/superpowers/specs/2026-08-10-themes-design.md §3).
//!
//! Four roles are `Option<Color>`. `None` is not "no colour": it is *not ours
//! to set*, which is how vision.md §4 principle 5 is stated in the type system
//! rather than in a comment.

mod catppuccin;
mod default;

#[cfg(test)]
mod contrast;

use lc_domain::ThemeChoice;
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
    /// A panel's frame, and a scrollbar's thumb.
    pub(crate) panel_border: Option<Color>,
    /// The cursor row's background.
    pub(crate) highlight: Color,
    /// The cursor row's text, where the theme cannot guarantee its own roles
    /// read against `highlight` (design §3.1). `None` keeps the row's own
    /// column colours; `Some` flattens them for legibility.
    pub(crate) highlight_text: Option<Color>,
    /// The wash under a selected cell's row and column: the shade between the
    /// field and `highlight` (selection modes design §5).
    pub(crate) crosshair: Color,
    /// The focused panel's border and title.
    pub(crate) focus: Color,
    /// The letter that focuses a panel, inside that panel's title. Read
    /// against both a focused title (`focus`) and an unfocused one, so it is
    /// neither of those colours.
    pub(crate) shortcut: Color,
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
        Self::of(ThemeChoice::default())
    }
}

impl Theme {
    /// The palette `choice` names. The one place a theme name becomes a
    /// colour (architecture.md §6).
    #[must_use]
    pub const fn of(choice: ThemeChoice) -> Self {
        Self {
            palette: match choice {
                ThemeChoice::TerminalDefault => &default::DEFAULT,
                ThemeChoice::CatppuccinLatte => &catppuccin::LATTE,
                ThemeChoice::CatppuccinFrappe => &catppuccin::FRAPPE,
                ThemeChoice::CatppuccinMacchiato => &catppuccin::MACCHIATO,
                ThemeChoice::CatppuccinMocha => &catppuccin::MOCHA,
            },
        }
    }

    pub(crate) const fn palette(self) -> &'static Palette {
        self.palette
    }

    /// The whole frame's ground. Painted before anything else, so a widget
    /// that sets no colour of its own inherits the theme's rather than the
    /// terminal's.
    pub(crate) fn ground(self) -> Style {
        style_of(self.palette.base, self.palette.text)
    }

    /// A floating block's ground, so an overlay separates from the field it
    /// covers.
    pub(crate) fn overlay_ground(self) -> Style {
        style_of(self.palette.surface, self.palette.text)
    }

    /// A panel's border. `Token::Focused` is patched on top of this rather
    /// than replacing it, so a focused panel's border becomes the theme's
    /// `focus` colour on top of whatever `panel_border` set (or left unset).
    pub(crate) fn border_style(self) -> Style {
        maybe(self.palette.panel_border)
    }

    #[must_use]
    pub fn style(self, token: Token) -> Style {
        let palette = self.palette;

        match token {
            Token::PartitionKey => fg(palette.partition).add_modifier(Modifier::BOLD),
            Token::ClusteringKey => fg(palette.clustering),
            // `accent`, which the loader also spends: they never share a
            // surface — one is a column in the Table DDL panel, the other the
            // spinner on the bar row — the same argument that lets `warning`
            // and `partition` share a yellow in the Catppuccin flavours.
            Token::StaticColumn => fg(palette.accent),
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
            // A colour of its own now (2026-08-11-panel-focus-contrast-design.md
            // §2 withdraws the earlier "bold only" decision): it still reads on
            // a panel border the theme may have coloured already, because
            // `Style::patch` lets this `fg` overwrite `panel_border`'s.
            Token::Focused => fg(palette.focus).add_modifier(Modifier::BOLD),
            Token::HintKey => Style::new().add_modifier(Modifier::BOLD),
            // A colour rather than an underline: the mark has to survive
            // terminals that draw no underline at all, and it reads at a
            // glance the way btop's and Midnight Commander's do.
            Token::Shortcut => fg(palette.shortcut).add_modifier(Modifier::BOLD),
            Token::Selected => selected(palette),
            Token::Crosshair => crosshair(palette),
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

/// The cursor row and column in cell mode. `highlight_text` for the reason
/// [`selected`] spends it: a theme that cannot know what it is drawn over gives
/// the row's own colours up for legibility, and one that can keeps them
/// (selection modes design §4).
fn crosshair(palette: &Palette) -> Style {
    let style = Style::new().bg(palette.crosshair);

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

fn style_of(background: Option<Color>, foreground: Option<Color>) -> Style {
    let mut style = Style::new();
    if let Some(color) = background {
        style = style.bg(color);
    }
    if let Some(color) = foreground {
        style = style.fg(color);
    }
    style
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::theme::contrast::{BLACK, WHITE, luminance, ratio};

    /// Every role a theme can spend on text, and the input to both contrast
    /// tests. `base`, `surface` and `highlight` are what text is drawn *on*.
    /// `text`, `border` and `highlight_text` are `Option`s a theme may
    /// decline, so they are answered by the tests that know which ground each
    /// is drawn against rather than swept here.
    ///
    /// Destructured with every field named and no `..`, so a new role cannot
    /// enter the palette without the compiler asking here whether it is text
    /// — an uncontrasted role would otherwise arrive silently.
    pub(super) fn foregrounds(palette: &Palette) -> Vec<(&'static str, Color)> {
        let Palette {
            base: _,
            surface: _,
            text: _,
            panel_border: _,
            highlight: _,
            highlight_text: _,
            crosshair: _,
            focus: _,
            shortcut,
            muted,
            partition,
            clustering,
            keyspace,
            table,
            property,
            accent,
            match_,
            success,
            warning,
            danger,
            keyword,
            reference,
            literal,
        } = *palette;

        vec![
            ("shortcut", shortcut),
            ("muted", muted),
            ("partition", partition),
            ("clustering", clustering),
            ("keyspace", keyspace),
            ("table", table),
            ("property", property),
            ("accent", accent),
            ("match_", match_),
            ("success", success),
            ("warning", warning),
            ("danger", danger),
            ("keyword", keyword),
            ("reference", reference),
            ("literal", literal),
        ]
    }

    #[test]
    fn the_default_theme_paints_no_panel_field() {
        let palette = Theme::default().palette();

        assert_eq!(palette.base, None, "the background is the user's");
        assert_eq!(palette.surface, None);
        assert_eq!(palette.text, None, "so is the body foreground");
        assert_eq!(palette.panel_border, None);
    }

    /// Every theme, not only the default: a flavour paints its field once,
    /// through `ground`, and a token that painted a background of its own
    /// would put a patch of some other colour over it — which is the same
    /// defect in a flavour as it is in the theme that leaves the field to the
    /// terminal (design §2, §3). The crosshair is the second exception, and a
    /// bar is what it is (selection modes design §5).
    #[test]
    fn only_the_cursor_row_and_its_crosshair_paint_a_background() {
        for choice in ThemeChoice::ALL {
            let theme = Theme::of(choice);

            for token in EVERY_TOKEN {
                let background = theme.style(token).bg;

                if matches!(token, Token::Selected | Token::Crosshair) {
                    assert!(background.is_some(), "{choice:?}: {token:?} is a bar");
                } else {
                    assert_eq!(
                        background, None,
                        "{choice:?}: {token:?} painted its own field"
                    );
                }
            }
        }
    }

    /// The wash sits between the field and the cursor bar, in luminance and
    /// not merely in intent: on the far side of `highlight` it would be the
    /// louder of the two bands and cell mode would shout over row mode, and on
    /// the ground it would not be visible at all.
    ///
    /// The ordering runs upwards for a dark flavour and downwards for Latte,
    /// so what is asserted is that `crosshair` lies strictly between the two —
    /// `base`'s side of `highlight`, by a margin. The floor is 0.015 rather
    /// than the 0.02 `a_highlight_bar_is_distinct_from_its_field` holds
    /// `highlight` to: Mocha's `surface0` clears its own base by 0.0195, and
    /// upstream Catppuccin's spacing of its surfaces is not ours to retune
    /// (catppuccin design §8).
    ///
    /// The terminal-default theme declares no `base` — it cannot see the
    /// ground it is drawn on — so there it is only the gap to `highlight` that
    /// can be checked.
    #[test]
    fn the_crosshair_is_fainter_than_the_cursor_row() {
        const FLOOR: f64 = 0.015;

        for choice in ThemeChoice::ALL {
            let palette = Theme::of(choice).palette();
            let (crosshair, highlight) =
                (luminance(palette.crosshair), luminance(palette.highlight));

            assert!(
                (highlight - crosshair).abs() >= FLOOR,
                "{choice:?}: a crosshair this close to the cursor row \
                 says nothing about which mode is up"
            );

            let Some(base) = palette.base.map(luminance) else {
                continue;
            };

            assert!(
                (crosshair - base).abs() >= FLOOR,
                "{choice:?}: an invisible wash"
            );
            assert!(
                (base < crosshair && crosshair < highlight)
                    || (base > crosshair && crosshair > highlight),
                "{choice:?}: the wash is not between the field and the cursor bar"
            );
        }
    }

    /// The wash's whole purpose is to leave the column colouring readable
    /// under it (vision.md §2). A theme may give that up only where it has
    /// already given it up for the cursor row, by declaring `highlight_text`.
    #[test]
    fn the_crosshair_flattens_no_colour_the_cursor_row_keeps() {
        for choice in ThemeChoice::ALL {
            let theme = Theme::of(choice);

            assert_eq!(
                theme.style(Token::Crosshair).fg,
                theme.style(Token::Selected).fg,
                "{choice:?}: the crosshair spends a foreground the cursor row does not"
            );
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
    fn focus_is_drawn_with_the_focus_colour() {
        let theme = Theme::default();
        let style = theme.style(Token::Focused);

        assert!(style.add_modifier.contains(Modifier::BOLD));
        assert_eq!(style.fg, Some(theme.palette().focus));
    }

    #[test]
    fn a_shortcut_is_a_colour_of_its_own_on_a_focused_title() {
        for choice in ThemeChoice::ALL {
            let theme = Theme::of(choice);
            let style = theme.style(Token::Shortcut);

            assert_eq!(style.fg, Some(theme.palette().shortcut));
            assert_ne!(
                style.fg,
                theme.style(Token::Focused).fg,
                "{choice:?}: the mark would vanish into the title it sits in"
            );
        }
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

    #[test]
    fn every_choice_maps_to_a_palette_of_its_own() {
        let palettes: Vec<Palette> = ThemeChoice::ALL
            .iter()
            .map(|choice| *Theme::of(*choice).palette())
            .collect();

        for (index, palette) in palettes.iter().enumerate() {
            assert!(
                palettes
                    .iter()
                    .skip(index + 1)
                    .all(|other| other != palette),
                "{:?} shares a palette with a later theme",
                ThemeChoice::ALL[index]
            );
        }
    }

    /// Every token, so a new one cannot be added without deciding what the
    /// tests above have to say about it.
    pub(super) const EVERY_TOKEN: [Token; 25] = [
        Token::PartitionKey,
        Token::ClusteringKey,
        Token::StaticColumn,
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
        Token::Crosshair,
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

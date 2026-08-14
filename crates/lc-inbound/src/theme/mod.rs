//! Semantic tokens into concrete styles.
//!
//! A theme is a [`Palette`] — a table of named colour roles — and one mapping
//! from [`Token`] to a style, shared by every theme. Adding a theme is adding
//! a `const Palette`, not a match arm per token
//! (docs/superpowers/specs/2026-08-13-colour-palettes-design.md §2).

mod ours;
mod ported;

#[cfg(test)]
mod contrast;

use lc_domain::posture::Posture;
use lc_domain::style::{Severity, Token};
use lc_domain::{Ours, Ported, ThemeChoice};
use ratatui::style::{Color, Modifier, Style};

pub(super) const fn rgb(value: u32) -> Color {
    Color::Rgb(
        ((value >> 16) & 0xff) as u8,
        ((value >> 8) & 0xff) as u8,
        (value & 0xff) as u8,
    )
}

/// One theme's colours, by the meaning each is spent on.
///
/// Every role is a colour. A palette that declined one could not be measured
/// against the ground it is drawn on, and the contrast invariant would become
/// unfalsifiable (design §1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Palette {
    /// The terminal background this theme assumes, painted before anything else.
    pub(crate) base: Color,
    /// An overlay's fill, one step up from `base`. Panels sit on `base`; this
    /// is what separates a floating block from the field it covers.
    pub(crate) surface: Color,
    /// Primary text: column values, table names in focus.
    pub(crate) text: Color,
    /// Secondary text: option names, column headers, footer labels.
    pub(crate) dim: Color,
    /// De-emphasis: system keyspaces, nulls, separators, counts.
    pub(crate) faint: Color,
    /// An unfocused panel's frame, and a scrollbar's track.
    pub(crate) border: Color,
    /// The focused panel's accent: its border, its title, its active tab.
    pub(crate) focus: Color,
    /// The cursor row's background.
    pub(crate) selection_bg: Color,
    /// The background wash under the cursor's row *and* column in
    /// cell-selection mode: one step off `base`, so it says where the cursor is
    /// without shouting over the bar row mode draws (selection modes design
    /// §5). A ground rather than text — which is why it is swept as one: a
    /// colour under text whose contrast nobody measures is how the palette this
    /// replaces became unreadable (design §1).
    pub(crate) crosshair: Color,
    /// The cursor row's text, where a cell spends no colour of its own.
    pub(crate) selection_fg: Color,
    /// The one-cell gutter mark on the cursor row.
    pub(crate) selection_bar: Color,
    /// Hot key letters, in panel titles and the footer. Always underlined.
    pub(crate) shortcut: Color,
    pub(crate) keyspace: Color,
    pub(crate) table: Color,
    pub(crate) partition: Color,
    pub(crate) clustering: Color,
    pub(crate) column: Color,
    /// Field labels in the DDL pane.
    pub(crate) label: Color,
    /// Section headers naming no schema concept (Options, Indexes).
    pub(crate) section: Color,
    /// CQL types, secondary emphasis.
    pub(crate) accent: Color,
    pub(crate) match_: Color,
    pub(crate) success: Color,
    pub(crate) warning: Color,
    pub(crate) danger: Color,
    /// Neutral notice, hints.
    pub(crate) info: Color,
    pub(crate) keyword: Color,
    pub(crate) reference: Color,
    pub(crate) literal: Color,
    pub(crate) comment: Color,
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
                ThemeChoice::Ours(Ours::DefaultDark) => &ours::default::DARK,
                ThemeChoice::Ours(Ours::DefaultLight) => &ours::default::LIGHT,
                ThemeChoice::Ours(Ours::AshDark) => &ours::ash::DARK,
                ThemeChoice::Ours(Ours::AshLight) => &ours::ash::LIGHT,
                ThemeChoice::Ours(Ours::BeaconDark) => &ours::beacon::DARK,
                ThemeChoice::Ours(Ours::BeaconLight) => &ours::beacon::LIGHT,
                ThemeChoice::Ours(Ours::QuillDark) => &ours::quill::DARK,
                ThemeChoice::Ours(Ours::QuillLight) => &ours::quill::LIGHT,
                ThemeChoice::Ported(Ported::CatppuccinLatte) => &ported::catppuccin::LATTE,
                ThemeChoice::Ported(Ported::CatppuccinFrappe) => &ported::catppuccin::FRAPPE,
                ThemeChoice::Ported(Ported::CatppuccinMacchiato) => &ported::catppuccin::MACCHIATO,
                ThemeChoice::Ported(Ported::CatppuccinMocha) => &ported::catppuccin::MOCHA,
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
        Style::new().bg(self.palette.base).fg(self.palette.text)
    }

    /// A floating block's ground, so an overlay separates from the field it
    /// covers.
    pub(crate) fn overlay_ground(self) -> Style {
        Style::new().bg(self.palette.surface).fg(self.palette.text)
    }

    /// A panel's border. `Token::Focused` is patched on top of this rather
    /// than replacing it, so a focused panel's border becomes the theme's
    /// `focus` colour on top of `border`.
    pub(crate) fn border_style(self) -> Style {
        Style::new().fg(self.palette.border)
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
            Token::RegularColumn | Token::Posture(Posture::ReadOnly) => fg(palette.text),
            Token::Posture(Posture::Writable) => fg(palette.success),
            Token::Posture(Posture::Protected) | Token::Severity(Severity::Danger) => {
                fg(palette.danger).add_modifier(Modifier::BOLD)
            }
            Token::Severity(Severity::Warning) => fg(palette.warning),
            Token::Severity(Severity::Info) => fg(palette.info),
            Token::PropertyName => fg(palette.label),
            Token::Muted | Token::HintAction => fg(palette.dim),
            Token::Faint => fg(palette.faint),
            Token::Section => fg(palette.section).add_modifier(Modifier::BOLD),
            Token::SelectionBar => fg(palette.selection_bar),
            Token::Comment => fg(palette.comment),
            // A colour of its own now (2026-08-11-panel-focus-contrast-design.md
            // §2 withdraws the earlier "bold only" decision): it still reads on
            // a panel border the theme may have coloured already, because
            // `Style::patch` lets this `fg` overwrite `border`'s.
            Token::Focused => fg(palette.focus).add_modifier(Modifier::BOLD),
            // Underlined as well as coloured, in every state: the letter sits
            // inside a title whose colour changes with focus, `focus` and
            // `shortcut` are one hex in Beacon, and colour is the channel a
            // user may have remapped or be unable to see. This withdraws
            // 2026-08-11-panel-focus-contrast-design.md §2's "a colour rather
            // than an underline" (design §3.1, rule 5).
            Token::Shortcut | Token::HintKey => fg(palette.shortcut)
                .add_modifier(Modifier::BOLD)
                .add_modifier(Modifier::UNDERLINED),
            Token::Selected => selected(palette),
            Token::Crosshair => crosshair(palette),
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
/// `selection_fg` is the ground text on that row and is not spent by
/// `Token::Selected`: patching a foreground there would overwrite every role
/// colour on the row (`Style::patch`'s `other.fg.or`), flattening
/// `partition`, `table` and the rest. It exists so the contrast sweep
/// measures the row's text, and every palette sets it equal to `text`.
fn selected(palette: &Palette) -> Style {
    Style::new().bg(palette.selection_bg)
}

/// The cursor row and column in cell mode.
///
/// Spends no foreground, for the reason [`selected`] spends none: the wash's
/// whole purpose is to leave the column colouring readable under it, and a
/// foreground here would overwrite every one of them through `Style::patch`
/// (selection modes design §4).
fn crosshair(palette: &Palette) -> Style {
    Style::new().bg(palette.crosshair)
}

fn fg(color: Color) -> Style {
    Style::new().fg(color)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::theme::contrast::{luminance, ratio};

    /// Text is unreadable below this against the ground it sits on.
    const FLOOR: f64 = 4.5;

    /// Every role a palette of ours spends on text.
    ///
    /// Destructured with every field named and no `..`, so a new role cannot
    /// enter the palette without the compiler asking here whether it is text.
    /// `base`, `surface`, `selection_bg` and `crosshair` are what text is drawn
    /// *on*, and `border` is not text — each is answered by the test that knows
    /// what to ask of it rather than swept in here.
    pub(super) fn foregrounds(palette: &Palette) -> Vec<(&'static str, Color)> {
        let Palette {
            base: _,
            surface: _,
            selection_bg: _,
            crosshair: _,
            border: _,
            text,
            dim,
            faint,
            focus,
            selection_fg,
            selection_bar,
            shortcut,
            keyspace,
            table,
            partition,
            clustering,
            column,
            label,
            section,
            accent,
            match_,
            success,
            warning,
            danger,
            info,
            keyword,
            reference,
            literal,
            comment,
        } = *palette;

        vec![
            ("text", text),
            ("dim", dim),
            ("faint", faint),
            ("focus", focus),
            ("selection_fg", selection_fg),
            ("selection_bar", selection_bar),
            ("shortcut", shortcut),
            ("keyspace", keyspace),
            ("table", table),
            ("partition", partition),
            ("clustering", clustering),
            ("column", column),
            ("label", label),
            ("section", section),
            ("accent", accent),
            ("match_", match_),
            ("success", success),
            ("warning", warning),
            ("danger", danger),
            ("info", info),
            ("keyword", keyword),
            ("reference", reference),
            ("literal", literal),
            ("comment", comment),
        ]
    }

    /// Rule 1: no colour that carries text drops below 4.5:1 on any ground it
    /// can land on. `Ours::ALL` and not `ThemeChoice::ALL` — a ported palette
    /// is unreachable from here by type, which is how "we impose nothing on
    /// Catppuccin" is stated in the type system rather than in a comment
    /// (design §2).
    #[test]
    fn every_palette_of_ours_reads_on_every_ground_it_declares() {
        for choice in Ours::ALL {
            let palette = Theme::of(ThemeChoice::Ours(choice)).palette();

            for (role, colour) in foregrounds(palette) {
                for (ground, name) in [
                    (palette.base, "base"),
                    (palette.surface, "surface"),
                    (palette.selection_bg, "selection_bg"),
                    (palette.crosshair, "crosshair"),
                ] {
                    let ratio = ratio(colour, ground);
                    assert!(
                        ratio >= FLOOR,
                        "{}: {role} is {ratio:.2}:1 on {name}",
                        choice.label()
                    );
                }
            }
        }
    }

    /// Rule 2: the ramp has to read as a staircase, not as three shades of one
    /// grey.
    #[test]
    fn the_neutral_ramp_of_ours_is_a_staircase() {
        for choice in Ours::ALL {
            let palette = Theme::of(ThemeChoice::Ours(choice)).palette();

            assert!(
                ratio(palette.text, palette.dim) >= 1.35,
                "{}: text and dim are one tier",
                choice.label()
            );
            assert!(
                ratio(palette.dim, palette.faint) >= 1.2,
                "{}: dim and faint are one tier",
                choice.label()
            );
        }
    }

    /// A border is not text: it only has to be seen. Both the colours a frame
    /// can wear — `border` unfocused, `focus` patched over it by
    /// `Token::Focused` when the panel has focus.
    #[test]
    fn every_border_of_ours_is_perceptible() {
        for choice in Ours::ALL {
            let palette = Theme::of(ThemeChoice::Ours(choice)).palette();

            for (role, colour) in [("border", palette.border), ("focus", palette.focus)] {
                let ratio = ratio(colour, palette.base);
                assert!(ratio >= 1.5, "{}: {role} is {ratio:.2}:1", choice.label());
            }
        }
    }

    /// Every theme, ported ones included: a palette paints its field once,
    /// through `ground`, and a token that painted a background of its own
    /// would put a patch of some other colour over it (design §4.2).
    ///
    /// The cursor row is one of the two that do, and the crosshair — the wash
    /// under the cursor's row and column in cell mode — is the other. Neither
    /// spends anything else: a foreground there would overwrite every role
    /// colour under the band through `Style::patch`, flattening `partition`,
    /// `table` and the rest — which is why `selection_fg` exists as a measured
    /// colour the token never spends.
    #[test]
    fn the_two_bands_paint_a_background_and_spend_no_foreground() {
        for choice in ThemeChoice::ALL {
            let theme = Theme::of(choice);

            for token in EVERY_TOKEN {
                let background = theme.style(token).bg;

                if matches!(token, Token::Selected | Token::Crosshair) {
                    assert!(background.is_some(), "{choice:?}: {token:?} is a band");
                    assert_eq!(
                        theme.style(token).fg,
                        None,
                        "{choice:?}: {token:?} overwrote the colours under it"
                    );
                } else {
                    assert_eq!(
                        background, None,
                        "{choice:?}: {token:?} painted its own field"
                    );
                }
            }
        }
    }

    /// The wash sits between the field and the cursor bar, in luminance and not
    /// merely in intent: on the far side of `selection_bg` it would be the
    /// louder of the two bands and cell mode would shout over row mode, and on
    /// `base` it would not be visible at all.
    ///
    /// The ordering runs upwards for a dark palette and downwards for a light
    /// one, so what is asserted is that `crosshair` lies strictly between the
    /// two — `base`'s side of `selection_bg`, by a margin. The margin is a
    /// contrast ratio, the currency the rest of this module measures grounds in,
    /// and 1.04 is where the narrowest of the twelve sits: the wash is one step
    /// off its own `base` by construction, a tint of the ground rather than a
    /// colour of its own.
    #[test]
    fn the_crosshair_is_fainter_than_the_cursor_row() {
        const FLOOR: f64 = 1.04;

        for choice in ThemeChoice::ALL {
            let palette = Theme::of(choice).palette();

            for (role, colour) in [
                ("the cursor row", palette.selection_bg),
                ("the field", palette.base),
            ] {
                let ratio = ratio(palette.crosshair, colour);
                assert!(
                    ratio >= FLOOR,
                    "{choice:?}: the wash is {ratio:.3}:1 against {role}"
                );
            }

            let (base, crosshair, selection) = (
                luminance(palette.base),
                luminance(palette.crosshair),
                luminance(palette.selection_bg),
            );

            assert!(
                (base < crosshair && crosshair < selection)
                    || (base > crosshair && crosshair > selection),
                "{choice:?}: the wash is not between the field and the cursor bar"
            );
        }
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

    #[test]
    fn focus_is_drawn_with_the_focus_colour() {
        let theme = Theme::default();
        let style = theme.style(Token::Focused);

        assert!(style.add_modifier.contains(Modifier::BOLD));
        assert_eq!(style.fg, Some(theme.palette().focus));
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

    /// Rule 5: colour alone cannot carry the hot key. The letter sits inside a
    /// title whose colour changes with focus, and in Beacon `focus` and
    /// `shortcut` are the same amber — underline is the channel that survives
    /// both, and nothing else in the UI spends it (design §3.1).
    #[test]
    fn a_hot_key_is_underlined_and_nothing_else_is() {
        for choice in ThemeChoice::ALL {
            let theme = Theme::of(choice);

            for token in EVERY_TOKEN {
                let underlined = theme
                    .style(token)
                    .add_modifier
                    .contains(Modifier::UNDERLINED);

                if matches!(token, Token::Shortcut | Token::HintKey) {
                    assert!(underlined, "{choice:?}: {token:?} is not underlined");
                } else {
                    assert!(
                        !underlined || matches!(token, Token::SelectedCell),
                        "{choice:?}: {token:?} spends the underline a hot key needs"
                    );
                }
            }
        }
    }

    /// Every token, so a new one cannot be added without deciding what the
    /// tests above have to say about it.
    pub(super) const EVERY_TOKEN: [Token; 29] = [
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
        Token::Faint,
        Token::Section,
        Token::SelectionBar,
        Token::Comment,
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

    /// The palette ranks by lightness, so nothing needs a modifier to recede.
    /// `DIM` is a terminal-dependent multiplier and would invalidate every
    /// ratio `every_palette_of_ours_reads_on_every_ground_it_declares`
    /// measures (design §4.1).
    #[test]
    fn no_token_recedes_with_a_modifier() {
        for choice in ThemeChoice::ALL {
            let theme = Theme::of(choice);

            for token in EVERY_TOKEN {
                assert!(
                    !theme.style(token).add_modifier.contains(Modifier::DIM),
                    "{choice:?}: {token:?} spends DIM instead of the ramp"
                );
            }
        }
    }

    /// Two tiers below `text`, and they are distinct: a call site that meant
    /// "recede one step" and one that meant "recede two" must not draw the
    /// same.
    #[test]
    fn the_two_de_emphasis_tokens_are_different_colours() {
        for choice in ThemeChoice::ALL {
            let theme = Theme::of(choice);

            assert_ne!(
                theme.style(Token::Muted).fg,
                theme.style(Token::Faint).fg,
                "{choice:?}"
            );
        }
    }
}

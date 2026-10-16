//! Semantic tokens into concrete styles. A theme is a [`Palette`] plus one
//! shared token mapping, so adding a theme adds a `const Palette`, not match
//! arms.

mod ours;
mod ported;

#[cfg(test)]
mod contrast;

use std::collections::BTreeMap;

use lc_domain::config::{Background, CustomTheme};
use lc_domain::posture::Posture;
use lc_domain::style::{Severity, Token};
use lc_domain::theme::{Role, ThemeName};
use lc_domain::{Ours, Ported, ThemeChoice};
use ratatui::style::{Color, Modifier, Style};

pub(super) const fn rgb(value: u32) -> Color {
    Color::Rgb(
        ((value >> 16) & 0xff) as u8,
        ((value >> 8) & 0xff) as u8,
        (value & 0xff) as u8,
    )
}

/// Every role is a colour, so every one can be measured against its ground.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Palette {
    /// Painted before anything else.
    pub(crate) base: Color,
    pub(crate) text: Color,
    pub(crate) dim: Color,
    /// System keyspaces, nulls, separators, counts.
    pub(crate) faint: Color,
    /// Also a scrollbar's track.
    pub(crate) border: Color,
    pub(crate) focus: Color,
    pub(crate) selection_bg: Color,
    /// One step off `base`. Swept as a ground: an unmeasured colour under text
    /// is how the previous palette became unreadable.
    pub(crate) crosshair: Color,
    /// The per-channel midpoint of `crosshair` and `selection_bg`.
    pub(crate) ranged: Color,
    /// Where a cell spends no colour of its own.
    pub(crate) selection_fg: Color,
    pub(crate) selection_bar: Color,
    /// Always underlined.
    pub(crate) shortcut: Color,
    pub(crate) keyspace: Color,
    pub(crate) table: Color,
    pub(crate) partition: Color,
    pub(crate) clustering: Color,
    pub(crate) column: Color,
    pub(crate) label: Color,
    pub(crate) section: Color,
    pub(crate) accent: Color,
    pub(crate) match_: Color,
    pub(crate) success: Color,
    pub(crate) warning: Color,
    pub(crate) danger: Color,
    pub(crate) info: Color,
    pub(crate) keyword: Color,
    pub(crate) reference: Color,
    pub(crate) literal: Color,
    pub(crate) comment: Color,
}

impl Palette {
    /// Exhaustive over [`Role`]; a field here that `Role` lacks is not caught.
    pub(crate) const fn set(&mut self, role: Role, color: Color) {
        match role {
            Role::Base => self.base = color,
            Role::Text => self.text = color,
            Role::Dim => self.dim = color,
            Role::Faint => self.faint = color,
            Role::Border => self.border = color,
            Role::Focus => self.focus = color,
            Role::SelectionBg => self.selection_bg = color,
            Role::Crosshair => self.crosshair = color,
            Role::Ranged => self.ranged = color,
            Role::SelectionFg => self.selection_fg = color,
            Role::SelectionBar => self.selection_bar = color,
            Role::Shortcut => self.shortcut = color,
            Role::Keyspace => self.keyspace = color,
            Role::Table => self.table = color,
            Role::Partition => self.partition = color,
            Role::Clustering => self.clustering = color,
            Role::Column => self.column = color,
            Role::Label => self.label = color,
            Role::Section => self.section = color,
            Role::Accent => self.accent = color,
            Role::Match => self.match_ = color,
            Role::Success => self.success = color,
            Role::Warning => self.warning = color,
            Role::Danger => self.danger = color,
            Role::Info => self.info = color,
            Role::Keyword => self.keyword = color,
            Role::Reference => self.reference = color,
            Role::Literal => self.literal = color,
            Role::Comment => self.comment = color,
        }
    }
}

/// Under a modal the panels are context: they keep their colours and lose
/// focus, cursor and wash marks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Emphasis {
    Lit,
    Dimmed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Theme {
    palette: Palette,
    emphasis: Emphasis,
}

impl Default for Theme {
    fn default() -> Self {
        Self::of(&ThemeChoice::default())
    }
}

impl Theme {
    /// `Custom` falls back to the default here; [`Themes::get`] is where it
    /// resolves. Keeps `Theme::of` total for callers without a `Themes` table.
    #[must_use]
    #[allow(
        clippy::match_same_arms,
        reason = "`Custom` borrows the default: `validate` refuses dangling names before `State`"
    )]
    pub fn of(choice: &ThemeChoice) -> Self {
        Self {
            palette: match choice {
                ThemeChoice::Ours(Ours::DefaultDark) => ours::default::DARK,
                ThemeChoice::Ours(Ours::DefaultLight) => ours::default::LIGHT,
                ThemeChoice::Ours(Ours::AshDark) => ours::ash::DARK,
                ThemeChoice::Ours(Ours::AshLight) => ours::ash::LIGHT,
                ThemeChoice::Ours(Ours::BeaconDark) => ours::beacon::DARK,
                ThemeChoice::Ours(Ours::BeaconLight) => ours::beacon::LIGHT,
                ThemeChoice::Ours(Ours::QuillDark) => ours::quill::DARK,
                ThemeChoice::Ours(Ours::QuillLight) => ours::quill::LIGHT,
                ThemeChoice::Ported(Ported::CatppuccinLatte) => ported::catppuccin::LATTE,
                ThemeChoice::Ported(Ported::CatppuccinFrappe) => ported::catppuccin::FRAPPE,
                ThemeChoice::Ported(Ported::CatppuccinMacchiato) => ported::catppuccin::MACCHIATO,
                ThemeChoice::Ported(Ported::CatppuccinMocha) => ported::catppuccin::MOCHA,
                ThemeChoice::Custom(_) => ours::default::DARK,
            },
            emphasis: Emphasis::Lit,
        }
    }

    pub(crate) const fn palette(self) -> Palette {
        self.palette
    }

    pub(crate) const fn dimmed(self) -> Self {
        Self {
            emphasis: Emphasis::Dimmed,
            ..self
        }
    }

    /// Painted first, so a widget setting no colour inherits the theme's.
    /// Overlays repaint it over what `Clear` reset.
    pub(crate) fn ground(self) -> Style {
        Style::new().bg(self.palette.base).fg(self.palette.text)
    }

    /// The focused panel, or a modal that took focus from all of them.
    pub(crate) fn focus_border(self) -> Style {
        self.border_style().patch(self.style(Token::Focused))
    }

    /// `Token::Focused` patches over this.
    pub(crate) fn border_style(self) -> Style {
        Style::new().fg(self.palette.border)
    }

    #[must_use]
    pub fn style(self, token: Token) -> Style {
        let palette = self.palette;

        if matches!(self.emphasis, Emphasis::Dimmed) {
            if is_emphasis(token) {
                return Style::new();
            }
            // A hint under a modal is inert, and inert hints are already faint.
            if matches!(token, Token::HintKey | Token::HintAction) {
                return fg(self.palette.faint);
            }
        }

        match token {
            Token::PartitionKey => fg(palette.partition).add_modifier(Modifier::BOLD),
            Token::PartitionKeyValue => fg(palette.partition),
            Token::ClusteringKey | Token::ClusteringKeyValue => fg(palette.clustering),
            Token::StaticColumn => fg(palette.accent),
            Token::Keyspace => fg(palette.keyspace),
            Token::Table => fg(palette.table),
            Token::RegularColumn => fg(palette.text),
            // Only a writable connection is lit, so the badge is noticed when
            // it matters.
            Token::Posture(Posture::Writable) => fg(palette.warning).add_modifier(Modifier::BOLD),
            Token::Severity(Severity::Danger) => fg(palette.danger).add_modifier(Modifier::BOLD),
            Token::Severity(Severity::Warning) => fg(palette.warning),
            // Reversed rather than `bg(warning)`: ratatui paints the cursor
            // row's highlight over cell backgrounds, and that is where an edit
            // just happened. A modifier survives.
            Token::Dirty => fg(palette.warning).add_modifier(Modifier::REVERSED),
            Token::Severity(Severity::Info) => fg(palette.info),
            Token::PropertyName => fg(palette.label),
            Token::Muted
            | Token::HintAction
            | Token::Posture(Posture::ReadOnly | Posture::Protected) => fg(palette.dim),
            Token::Faint => fg(palette.faint),
            Token::Section => fg(palette.section).add_modifier(Modifier::BOLD),
            Token::SelectionBar => fg(palette.selection_bar),
            Token::Comment => fg(palette.comment),
            // `Style::patch` lets this `fg` overwrite the border's.
            Token::Focused => fg(palette.focus).add_modifier(Modifier::BOLD),
            // Underlined as well: `focus` and `shortcut` are one hex in Beacon,
            // and colour may be remapped or unseen.
            Token::HintKey => fg(palette.shortcut)
                .add_modifier(Modifier::BOLD)
                .add_modifier(Modifier::UNDERLINED),
            Token::Selected => selected(&palette),
            Token::Crosshair => crosshair(&palette),
            Token::Ranged => ranged(&palette),
            Token::Match => fg(palette.match_).add_modifier(Modifier::BOLD),
            // Unbolded: weight on one glyph reads as a different glyph. Not
            // emphasis: it survives under a modal.
            Token::Marker => fg(palette.focus),
            // A modifier, so it reads against any row colour.
            Token::SelectedCell => Style::new()
                .add_modifier(Modifier::REVERSED)
                .add_modifier(Modifier::UNDERLINED),
            // No underline: the cursor cell spends that to stand apart from a
            // flashed row.
            Token::Flash => Style::new().add_modifier(Modifier::REVERSED),
            Token::Keyword => fg(palette.keyword),
            Token::Reference => fg(palette.reference),
            Token::Literal => fg(palette.literal),
        }
    }
}

/// A dimmed theme draws these as nothing. Not `Match`: a filter still applies
/// under an overlay. Not `Focused`: under a modal `render` stops asking for it.
const fn is_emphasis(token: Token) -> bool {
    matches!(
        token,
        Token::Selected
            | Token::SelectedCell
            | Token::SelectionBar
            | Token::Crosshair
            | Token::Ranged
            | Token::Flash
    )
}

/// Resolved once at startup: a custom palette is fixed data.
#[derive(Debug, Clone, Default)]
pub struct Themes {
    custom: BTreeMap<ThemeName, Palette>,
    background: Background,
}

impl Themes {
    #[must_use]
    pub fn new(themes: &[CustomTheme], background: Background) -> Self {
        let custom = themes
            .iter()
            .map(|theme| {
                let mut palette = Theme::of(&ThemeChoice::from(theme.extends)).palette();

                for (role, colour) in &theme.colors {
                    palette.set(*role, Color::Rgb(colour.red, colour.green, colour.blue));
                }

                (theme.name.clone(), palette)
            })
            .collect();

        Self { custom, background }
    }

    /// Falls back to the default; `validate` makes that unreachable.
    #[must_use]
    pub fn get(&self, choice: &ThemeChoice) -> Theme {
        let palette = match choice {
            ThemeChoice::Custom(name) => self.custom.get(name).map_or_else(
                || Theme::of(&ThemeChoice::default()).palette(),
                |palette| *palette,
            ),
            built_in => Theme::of(built_in).palette(),
        };

        Theme {
            palette: self.grounded(palette),
            emphasis: Emphasis::Lit,
        }
    }

    /// `selection_bg` and `crosshair` stay the palette's, keeping their
    /// measured contrast.
    fn grounded(&self, mut palette: Palette) -> Palette {
        match self.background {
            Background::Theme => {}
            // Paints nothing, letting a translucent terminal show through.
            Background::Terminal => palette.base = Color::Reset,
            Background::Fixed(rgb) => palette.base = Color::Rgb(rgb.red, rgb.green, rgb.blue),
        }

        palette
    }
}

/// No foreground: `Style::patch` would overwrite every role colour on the row.
/// `selection_fg` exists only for the contrast sweep and equals `text`.
fn selected(palette: &Palette) -> Style {
    Style::new().bg(palette.selection_bg)
}

/// No foreground, for [`selected`]'s reason.
fn crosshair(palette: &Palette) -> Style {
    Style::new().bg(palette.crosshair)
}

/// No foreground, for [`crosshair`]'s reason.
fn ranged(palette: &Palette) -> Style {
    Style::new().bg(palette.ranged)
}

fn fg(color: Color) -> Style {
    Style::new().fg(color)
}

#[cfg(test)]
pub(crate) mod tests {
    use std::collections::BTreeMap;

    use lc_domain::BuiltIn;
    use lc_domain::config::Rgb;

    use super::*;
    use crate::theme::contrast::{luminance, ratio};

    const FLOOR: f64 = 4.5;

    /// Destructured without `..`, so a new role must be classified here.
    /// Grounds and `border` are tested elsewhere.
    pub(super) fn foregrounds(palette: &Palette) -> Vec<(&'static str, Color)> {
        let Palette {
            base: _,
            selection_bg: _,
            crosshair: _,
            ranged: _,
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

    /// `Ours::ALL`, not every theme: ported palettes are out of reach by type.
    #[test]
    fn every_palette_of_ours_reads_on_every_ground_it_declares() {
        for choice in Ours::ALL {
            let palette = Theme::of(&ThemeChoice::Ours(choice)).palette();

            for (role, colour) in foregrounds(&palette) {
                for (ground, name) in [
                    (palette.base, "base"),
                    (palette.selection_bg, "selection_bg"),
                    (palette.crosshair, "crosshair"),
                    (palette.ranged, "ranged"),
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

    #[test]
    fn the_neutral_ramp_of_ours_is_a_staircase() {
        for choice in Ours::ALL {
            let palette = Theme::of(&ThemeChoice::Ours(choice)).palette();

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

    /// A border only has to be seen, focused or not.
    #[test]
    fn every_border_of_ours_is_perceptible() {
        for choice in Ours::ALL {
            let palette = Theme::of(&ThemeChoice::Ours(choice)).palette();

            for (role, colour) in [("border", palette.border), ("focus", palette.focus)] {
                let ratio = ratio(colour, palette.base);
                assert!(ratio >= 1.5, "{}: {role} is {ratio:.2}:1", choice.label());
            }
        }
    }

    /// A foreground on a band would flatten the role colours under it.
    #[test]
    fn the_bands_paint_a_background_and_spend_no_foreground() {
        for choice in BuiltIn::ALL {
            let theme = Theme::of(&choice.into());

            for token in EVERY_TOKEN {
                let style = theme.style(token);

                if matches!(token, Token::Selected | Token::Crosshair | Token::Ranged) {
                    assert!(style.bg.is_some(), "{choice:?}: {token:?} is a band");
                    assert_eq!(
                        style.fg, None,
                        "{choice:?}: {token:?} overwrote the colours under it"
                    );
                } else {
                    assert_eq!(
                        style.bg, None,
                        "{choice:?}: {token:?} painted its own field"
                    );
                }
            }
        }
    }

    #[test]
    fn the_range_is_louder_than_the_crosshair_and_quieter_than_the_cursor_row() {
        for choice in BuiltIn::ALL {
            let palette = Theme::of(&choice.into()).palette();

            let base = luminance(palette.base);
            let wash = luminance(palette.crosshair);
            let range = luminance(palette.ranged);
            let row = luminance(palette.selection_bg);

            let ordered = if wash >= base {
                wash <= range && range <= row
            } else {
                wash >= range && range >= row
            };

            assert!(
                ordered,
                "{choice:?}: crosshair {wash:.4}, ranged {range:.4}, selection_bg {row:.4} \
                 do not run in one direction from base {base:.4}"
            );
        }
    }

    /// Strictly between `base` and `selection_bg` by a contrast margin; 1.04 is
    /// where the narrowest palette sits.
    #[test]
    fn the_crosshair_is_fainter_than_the_cursor_row() {
        const FLOOR: f64 = 1.04;

        for choice in BuiltIn::ALL {
            let palette = Theme::of(&choice.into()).palette();

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
        let palettes: Vec<Palette> = BuiltIn::ALL
            .iter()
            .map(|built_in| Theme::of(&(*built_in).into()).palette())
            .collect();

        for (index, palette) in palettes.iter().enumerate() {
            assert!(
                palettes
                    .iter()
                    .skip(index + 1)
                    .all(|other| other != palette),
                "{:?} shares a palette with a later theme",
                BuiltIn::ALL[index]
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

    /// Underline is what survives `focus` and `shortcut` sharing a colour.
    /// Dimmed keeps content colours.
    #[test]
    fn a_dimmed_theme_draws_no_focus_and_no_cursor() {
        let theme = Theme::default().dimmed();

        for token in [
            Token::Selected,
            Token::SelectedCell,
            Token::SelectionBar,
            Token::Crosshair,
            Token::Ranged,
            Token::Flash,
        ] {
            assert_eq!(
                theme.style(token),
                Style::new(),
                "{token:?} still marks the panels a modal covers"
            );
        }

        assert_eq!(
            theme.style(Token::Keyspace),
            Theme::default().style(Token::Keyspace),
            "dimming is not a second palette"
        );
        assert_eq!(
            theme.style(Token::HintKey),
            Theme::default().style(Token::Faint),
            "a hint the modal has taken over recedes the way an inert one does"
        );
    }

    #[test]
    fn a_hot_key_is_underlined_and_nothing_else_is() {
        for choice in BuiltIn::ALL {
            let theme = Theme::of(&choice.into());

            for token in EVERY_TOKEN {
                let underlined = theme
                    .style(token)
                    .add_modifier
                    .contains(Modifier::UNDERLINED);

                if matches!(token, Token::HintKey) {
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

    /// Every token, so a new one must be decided here.
    pub(super) const EVERY_TOKEN: [Token; 32] = [
        Token::PartitionKey,
        Token::PartitionKeyValue,
        Token::ClusteringKey,
        Token::ClusteringKeyValue,
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
        Token::Ranged,
        Token::Flash,
        Token::HintKey,
        Token::HintAction,
        Token::PropertyName,
        Token::Match,
        Token::Keyword,
        Token::Reference,
        Token::Literal,
    ];

    /// `DIM` is terminal-dependent and would invalidate every measured ratio.
    #[test]
    fn no_token_recedes_with_a_modifier() {
        for choice in BuiltIn::ALL {
            let theme = Theme::of(&choice.into());

            for token in EVERY_TOKEN {
                assert!(
                    !theme.style(token).add_modifier.contains(Modifier::DIM),
                    "{choice:?}: {token:?} spends DIM instead of the ramp"
                );
            }
        }
    }

    /// Receding one step and two must not draw alike.
    #[test]
    fn the_two_de_emphasis_tokens_are_different_colours() {
        for choice in BuiltIn::ALL {
            let theme = Theme::of(&choice.into());

            assert_ne!(
                theme.style(Token::Muted).fg,
                theme.style(Token::Faint).fg,
                "{choice:?}"
            );
        }
    }

    #[test]
    fn a_custom_palette_inherits_every_role_it_does_not_state() {
        let custom = CustomTheme {
            name: ThemeName::try_new("midnight").expect("valid"),
            extends: BuiltIn::Ours(Ours::AshDark),
            colors: BTreeMap::from([(
                Role::Base,
                Rgb {
                    red: 0x0b,
                    green: 0x0d,
                    blue: 0x12,
                },
            )]),
        };
        let themes = Themes::new(std::slice::from_ref(&custom), Background::Theme);
        let resolved = themes
            .get(&ThemeChoice::Custom(custom.name.clone()))
            .palette();
        let inherited = Theme::of(&ThemeChoice::Ours(Ours::AshDark)).palette();

        assert_eq!(resolved.base, rgb(0x000b_0d12));
        assert_eq!(
            resolved.focus, inherited.focus,
            "an unstated role comes from `extends`"
        );
        assert_eq!(resolved.danger, inherited.danger);
    }

    #[test]
    fn an_explicit_background_overrides_a_custom_themes_own_stated_base() {
        let stated_danger = Rgb {
            red: 0xaa,
            green: 0x11,
            blue: 0x22,
        };
        let custom = CustomTheme {
            name: ThemeName::try_new("midnight").expect("valid"),
            extends: BuiltIn::Ours(Ours::AshDark),
            colors: BTreeMap::from([
                (
                    Role::Base,
                    Rgb {
                        red: 0x0b,
                        green: 0x0d,
                        blue: 0x12,
                    },
                ),
                (Role::Danger, stated_danger),
            ]),
        };
        let background = Rgb {
            red: 0x22,
            green: 0x33,
            blue: 0x44,
        };
        let themes = Themes::new(std::slice::from_ref(&custom), Background::Fixed(background));
        let resolved = themes
            .get(&ThemeChoice::Custom(custom.name.clone()))
            .palette();

        assert_eq!(
            resolved.base,
            Color::Rgb(background.red, background.green, background.blue),
            "`background` is the narrower instruction and wins over the theme's own `base`"
        );
        assert_eq!(
            resolved.danger,
            Color::Rgb(stated_danger.red, stated_danger.green, stated_danger.blue),
            "the override is narrow: a role the theme itself stated is untouched"
        );
    }

    #[test]
    fn a_built_in_resolves_the_same_through_the_table_as_it_does_directly() {
        let themes = Themes::new(&[], Background::Theme);

        for built_in in BuiltIn::ALL {
            let choice = ThemeChoice::from(built_in);

            assert_eq!(themes.get(&choice).palette(), Theme::of(&choice).palette());
        }
    }

    #[test]
    fn a_fixed_background_replaces_base_and_leaves_the_measured_grounds_alone() {
        let themes = Themes::new(
            &[],
            Background::Fixed(Rgb {
                red: 0x0b,
                green: 0x0d,
                blue: 0x12,
            }),
        );
        let resolved = themes.get(&ThemeChoice::Ours(Ours::AshDark)).palette();
        let original = Theme::of(&ThemeChoice::Ours(Ours::AshDark)).palette();

        assert_eq!(resolved.base, Color::Rgb(0x0b, 0x0d, 0x12));
        assert_eq!(resolved.selection_bg, original.selection_bg);
        assert_eq!(resolved.crosshair, original.crosshair);
    }

    #[test]
    fn a_terminal_background_paints_nothing_so_a_translucent_terminal_shows_through() {
        let themes = Themes::new(&[], Background::Terminal);
        let resolved = themes.get(&ThemeChoice::Ours(Ours::AshDark)).palette();

        assert_eq!(
            resolved.base,
            Color::Reset,
            "a colour would paint over exactly what this option exists to reveal"
        );
    }

    #[test]
    fn a_name_no_theme_claims_falls_back_to_the_default_rather_than_panicking() {
        let themes = Themes::new(&[], Background::Theme);
        let missing = ThemeChoice::Custom(ThemeName::try_new("nowhere").expect("valid"));

        assert_eq!(
            themes.get(&missing).palette(),
            Theme::of(&ThemeChoice::default()).palette(),
            "validation resolves dangling names, so this is defence rather than a path"
        );
    }

    /// Totality alone misses two arms aliasing one field.
    #[test]
    fn every_role_reaches_a_distinct_field_of_the_palette() {
        let marker = Color::Rgb(1, 2, 3);
        let palettes: Vec<Palette> = Role::ALL
            .iter()
            .map(|role| {
                let mut palette = ours::default::DARK;
                palette.set(*role, marker);
                palette
            })
            .collect();

        for (index, palette) in palettes.iter().enumerate() {
            assert_ne!(
                *palette,
                ours::default::DARK,
                "`{}` set nothing — Role and Palette have drifted",
                Role::ALL[index].key()
            );
            assert!(
                palettes
                    .iter()
                    .skip(index + 1)
                    .all(|other| other != palette),
                "`{}` sets the same field as a later role — Role and Palette have aliased",
                Role::ALL[index].key()
            );
        }
    }
}

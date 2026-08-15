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
    /// A row inside a selection the cursor is not on: the per-channel
    /// midpoint of `crosshair` and `selection_bg`, so a range outreads the
    /// wash without drowning out the cursor row itself.
    pub(crate) ranged: Color,
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

impl Palette {
    /// Overwrites one role, for a palette a config file supplied.
    ///
    /// Exhaustive, so a [`Role`] this does not handle fails to compile. The
    /// reverse — a field here that `Role` does not name — is not caught, the
    /// same limit `Ours::ALL` carries (design §8).
    pub(crate) const fn set(&mut self, role: Role, color: Color) {
        match role {
            Role::Base => self.base = color,
            Role::Surface => self.surface = color,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Theme {
    palette: Palette,
}

impl Default for Theme {
    fn default() -> Self {
        Self::of(&ThemeChoice::default())
    }
}

impl Theme {
    /// The palette `choice` names. The one place a theme name becomes a
    /// colour (architecture.md §6).
    ///
    /// `Custom` falls back to the default palette: this lookup has no config
    /// to resolve a custom theme's declared roles against. [`Themes::get`] is
    /// where a `Custom` name actually resolves; this arm exists so `Theme::of`
    /// stays total for callers — tests among them — that have no `Themes`
    /// table.
    #[must_use]
    // `DefaultDark` and `Custom` share a body: the latter never resolves here
    // in practice (`Themes::get` is where a `Custom` name is actually
    // resolved, and `validate` refuses a dangling one before it can reach
    // `State`), so there is no real palette to give it and it borrows the
    // default's rather than invent one.
    #[allow(clippy::match_same_arms)]
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
        }
    }

    pub(crate) const fn palette(self) -> Palette {
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
            Token::PartitionKeyValue => fg(palette.partition),
            Token::ClusteringKey | Token::ClusteringKeyValue => fg(palette.clustering),
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
            // Underlined as well as coloured, in every state: the key sits in
            // a bar whose colour changes with focus, `focus` and `shortcut`
            // are one hex in Beacon, and colour is the channel a user may have
            // remapped or be unable to see. This withdraws
            // 2026-08-11-panel-focus-contrast-design.md §2's "a colour rather
            // than an underline" (design §3.1, rule 5).
            Token::HintKey => fg(palette.shortcut)
                .add_modifier(Modifier::BOLD)
                .add_modifier(Modifier::UNDERLINED),
            Token::Selected => selected(&palette),
            Token::Crosshair => crosshair(&palette),
            Token::Ranged => ranged(&palette),
            Token::Match => fg(palette.match_).add_modifier(Modifier::BOLD),
            // A modifier rather than a colour, because it has to read
            // *against* the highlighted row whatever colour that row is.
            Token::SelectedCell => Style::new()
                .add_modifier(Modifier::REVERSED)
                .add_modifier(Modifier::UNDERLINED),
            // No underline beside the reverse: the cursor cell spends that one
            // to read apart from the row it sits on, and a flash covering
            // whole rows would leave it nothing to spend.
            Token::Flash => Style::new().add_modifier(Modifier::REVERSED),
            Token::Keyword => fg(palette.keyword),
            Token::Reference => fg(palette.reference),
            Token::Literal => fg(palette.literal),
        }
    }
}

/// Every palette a name can resolve to, built once at startup.
///
/// Resolution happens here rather than per frame: a custom palette is
/// `extends`' palette with the stated roles overwritten, and that is a fixed
/// computation over data that does not change while the tool runs (design §8).
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

    /// The palette `choice` names.
    ///
    /// A name no theme claims falls back to the default. `validate` resolves
    /// dangling names before one can reach `State`, so this is defence rather
    /// than a path anything takes.
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
        }
    }

    /// Overrides `base` and nothing else.
    ///
    /// `surface`, `selection_bg` and `crosshair` stay the palette's, so an
    /// overlay and the cursor row keep the contrast they were measured for
    /// even when the field behind them is the user's (design §4).
    fn grounded(&self, mut palette: Palette) -> Palette {
        match self.background {
            Background::Theme => {}
            // Not a colour: paint nothing, which is the only mechanism that
            // lets a translucent or blurred terminal show through.
            Background::Terminal => palette.base = Color::Reset,
            Background::Fixed(rgb) => palette.base = Color::Rgb(rgb.red, rgb.green, rgb.blue),
        }

        palette
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

/// The rows a range holds, other than the cursor's own.
///
/// Spends no foreground, for the reason [`crosshair`] spends none.
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

    /// Rule 1: no colour that carries text drops below 4.5:1 on any ground it
    /// can land on. `Ours::ALL` and not `ThemeChoice::ALL` — a ported palette
    /// is unreachable from here by type, which is how "we impose nothing on
    /// Catppuccin" is stated in the type system rather than in a comment
    /// (design §2).
    #[test]
    fn every_palette_of_ours_reads_on_every_ground_it_declares() {
        for choice in Ours::ALL {
            let palette = Theme::of(&ThemeChoice::Ours(choice)).palette();

            for (role, colour) in foregrounds(&palette) {
                for (ground, name) in [
                    (palette.base, "base"),
                    (palette.surface, "surface"),
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

    /// Rule 2: the ramp has to read as a staircase, not as three shades of one
    /// grey.
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

    /// A border is not text: it only has to be seen. Both the colours a frame
    /// can wear — `border` unfocused, `focus` patched over it by
    /// `Token::Focused` when the panel has focus.
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

    /// The three tokens that paint a field, and no others. `Ranged` joins the
    /// two: a range is a band like the cursor row, and spends no foreground for
    /// the same reason — a foreground would flatten every role colour under it
    /// (visual mode design §6).
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

    /// The range sits between the wash and the cursor row: on the far side of
    /// either it would make one of the three unreadable as a rank.
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

    /// Rule 5: colour alone cannot carry the hot key. The key sits in a bar
    /// whose colour changes with focus, and in Beacon `focus` and `shortcut`
    /// are the same amber — underline is the channel that survives both, and
    /// nothing else in the UI spends it (design §3.1).
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

    /// Every token, so a new one cannot be added without deciding what the
    /// tests above have to say about it.
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

    /// The palette ranks by lightness, so nothing needs a modifier to recede.
    /// `DIM` is a terminal-dependent multiplier and would invalidate every
    /// ratio `every_palette_of_ours_reads_on_every_ground_it_declares`
    /// measures (design §4.1).
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

    /// Two tiers below `text`, and they are distinct: a call site that meant
    /// "recede one step" and one that meant "recede two" must not draw the
    /// same.
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
        assert_eq!(
            resolved.surface, original.surface,
            "an overlay keeps the contrast it was measured for"
        );
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

    /// Totality alone would pass if two arms aliased the same field — both
    /// roles would still differ from the default. Pairwise distinctness is
    /// what catches that, the same shape
    /// `every_choice_maps_to_a_palette_of_its_own` checks palettes with.
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

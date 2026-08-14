//! Which theme is on.
//!
//! A name, never a colour: the mapping from one of these to a palette lives in
//! `lc-inbound`, which is what keeps the domain free of a terminal
//! (architecture.md §6).
//!
//! The split between [`Ours`] and [`Ported`] is what the contrast invariant is
//! written against. A palette of ours holds 4.5:1 on every ground it declares;
//! a ported one is its author's work and we impose nothing on it
//! (docs/superpowers/specs/2026-08-13-colour-palettes-design.md §2).

/// A palette written here, and held to the contrast rules.
///
/// Flat rather than a family crossed with a mode: nothing mandates that a
/// family has both a light and a dark variant, and a product type would claim
/// it does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ours {
    DefaultDark,
    DefaultLight,
    AshDark,
    AshLight,
    BeaconDark,
    BeaconLight,
    QuillDark,
    QuillLight,
}

/// A palette taken from elsewhere, as its authors wrote it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ported {
    CatppuccinLatte,
    CatppuccinFrappe,
    CatppuccinMacchiato,
    CatppuccinMocha,
}

/// A theme compiled in, of either kind.
///
/// Separate from [`ThemeChoice`] so a [`crate::config::CustomTheme`] can only
/// extend one of these — a custom theme extending another, and the cycle that
/// permits, is unrepresentable rather than checked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuiltIn {
    Ours(Ours),
    Ported(Ported),
}

impl BuiltIn {
    pub const ALL: [Self; 12] = [
        Self::Ours(Ours::DefaultDark),
        Self::Ours(Ours::DefaultLight),
        Self::Ours(Ours::AshDark),
        Self::Ours(Ours::AshLight),
        Self::Ours(Ours::BeaconDark),
        Self::Ours(Ours::BeaconLight),
        Self::Ours(Ours::QuillDark),
        Self::Ours(Ours::QuillLight),
        Self::Ported(Ported::CatppuccinLatte),
        Self::Ported(Ported::CatppuccinFrappe),
        Self::Ported(Ported::CatppuccinMacchiato),
        Self::Ported(Ported::CatppuccinMocha),
    ];

    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Ours(ours) => ours.label(),
            Self::Ported(ported) => ported.label(),
        }
    }

    /// The name a config file writes.
    ///
    /// An exhaustive match rather than a kebab-casing of [`Self::label`]: a
    /// thirteenth theme cannot be added without naming its slug, and
    /// `"Catppuccin Frappé"` is exactly what a `to_lowercase().replace(' ',
    /// "-")` gets silently wrong.
    #[must_use]
    pub const fn slug(self) -> &'static str {
        match self {
            Self::Ours(Ours::DefaultDark) => "default-dark",
            Self::Ours(Ours::DefaultLight) => "default-light",
            Self::Ours(Ours::AshDark) => "ash-dark",
            Self::Ours(Ours::AshLight) => "ash-light",
            Self::Ours(Ours::BeaconDark) => "beacon-dark",
            Self::Ours(Ours::BeaconLight) => "beacon-light",
            Self::Ours(Ours::QuillDark) => "quill-dark",
            Self::Ours(Ours::QuillLight) => "quill-light",
            Self::Ported(Ported::CatppuccinLatte) => "catppuccin-latte",
            Self::Ported(Ported::CatppuccinFrappe) => "catppuccin-frappe",
            Self::Ported(Ported::CatppuccinMacchiato) => "catppuccin-macchiato",
            Self::Ported(Ported::CatppuccinMocha) => "catppuccin-mocha",
        }
    }
}

impl From<BuiltIn> for ThemeChoice {
    fn from(built_in: BuiltIn) -> Self {
        match built_in {
            BuiltIn::Ours(ours) => Self::Ours(ours),
            BuiltIn::Ported(ported) => Self::Ported(ported),
        }
    }
}

/// A custom theme's name, as `[[themes]]` gives it and `theme = "…"` names it.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct ThemeName(std::sync::Arc<str>);

impl ThemeName {
    /// # Errors
    /// When `name` is blank or holds whitespace.
    pub fn try_new(name: &str) -> Result<Self, crate::config::OutOfRange> {
        if name.is_empty() || name.chars().any(char::is_whitespace) {
            return Err(crate::config::OutOfRange {
                what: "theme name",
                value: name.to_owned(),
                expected: "non-empty, no whitespace",
            });
        }

        Ok(Self(std::sync::Arc::from(name)))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A theme the user can choose.
///
/// `Default` is written out rather than derived: `#[default]` may only mark a
/// unit variant, and the default is a palette of ours by name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ThemeChoice {
    Ours(Ours),
    Ported(Ported),
    /// A palette a config file supplied. A name rather than an index into the
    /// config's list: an index would rot the moment config reload lands
    /// (design §8).
    Custom(ThemeName),
}

impl Default for ThemeChoice {
    fn default() -> Self {
        Self::Ours(Ours::DefaultDark)
    }
}

impl ThemeChoice {
    /// The name a config file writes for this theme.
    #[must_use]
    pub fn slug(&self) -> String {
        match self {
            Self::Ours(ours) => BuiltIn::Ours(*ours).slug().to_owned(),
            Self::Ported(ported) => BuiltIn::Ported(*ported).slug().to_owned(),
            Self::Custom(name) => name.as_str().to_owned(),
        }
    }

    /// The built-in `slug` names, and `None` for anything else — including a
    /// custom theme's name, which is resolved against the config instead.
    #[must_use]
    pub fn builtin_from_slug(slug: &str) -> Option<BuiltIn> {
        BuiltIn::ALL
            .into_iter()
            .find(|built_in| built_in.slug() == slug)
    }

    #[must_use]
    pub fn label(&self) -> String {
        match self {
            Self::Ours(ours) => ours.label().to_owned(),
            Self::Ported(ported) => ported.label().to_owned(),
            Self::Custom(name) => name.as_str().to_owned(),
        }
    }
}

impl Ours {
    pub const ALL: [Self; 8] = [
        Self::DefaultDark,
        Self::DefaultLight,
        Self::AshDark,
        Self::AshLight,
        Self::BeaconDark,
        Self::BeaconLight,
        Self::QuillDark,
        Self::QuillLight,
    ];

    /// The variant declared after `self`, and `None` after the last.
    ///
    /// `ALL` is hand-written, and a ninth variant added without extending it
    /// would never reach the contrast sweep. This match is exhaustive, so the
    /// variant breaks it at compile time, and
    /// `walking_from_the_first_theme_of_ours_reaches_every_one` fails until
    /// `ALL` names it too.
    #[must_use]
    pub const fn next(self) -> Option<Self> {
        match self {
            Self::DefaultDark => Some(Self::DefaultLight),
            Self::DefaultLight => Some(Self::AshDark),
            Self::AshDark => Some(Self::AshLight),
            Self::AshLight => Some(Self::BeaconDark),
            Self::BeaconDark => Some(Self::BeaconLight),
            Self::BeaconLight => Some(Self::QuillDark),
            Self::QuillDark => Some(Self::QuillLight),
            Self::QuillLight => None,
        }
    }

    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::DefaultDark => "Default dark",
            Self::DefaultLight => "Default light",
            Self::AshDark => "Ash dark",
            Self::AshLight => "Ash light",
            Self::BeaconDark => "Beacon dark",
            Self::BeaconLight => "Beacon light",
            Self::QuillDark => "Quill dark",
            Self::QuillLight => "Quill light",
        }
    }
}

impl Ported {
    pub const ALL: [Self; 4] = [
        Self::CatppuccinLatte,
        Self::CatppuccinFrappe,
        Self::CatppuccinMacchiato,
        Self::CatppuccinMocha,
    ];

    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::CatppuccinLatte => "Catppuccin Latte",
            Self::CatppuccinFrappe => "Catppuccin Frappé",
            Self::CatppuccinMacchiato => "Catppuccin Macchiato",
            Self::CatppuccinMocha => "Catppuccin Mocha",
        }
    }
}

/// One colour role a palette fills, as `[themes.colors]` names it.
///
/// Mirrors `lc-inbound`'s `Palette` field for field. `Palette::set` is an
/// exhaustive match over this, so a role here the palette does not handle
/// fails to compile; the reverse — a new `Palette` field nobody adds here —
/// is not caught, the same limit [`Ours::ALL`] already carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Role {
    Base,
    Surface,
    Text,
    Dim,
    Faint,
    Border,
    Focus,
    SelectionBg,
    Crosshair,
    Ranged,
    SelectionFg,
    SelectionBar,
    Shortcut,
    Keyspace,
    Table,
    Partition,
    Clustering,
    Column,
    Label,
    Section,
    Accent,
    Match,
    Success,
    Warning,
    Danger,
    Info,
    Keyword,
    Reference,
    Literal,
    Comment,
}

impl Role {
    pub const ALL: [Self; 30] = [
        Self::Base,
        Self::Surface,
        Self::Text,
        Self::Dim,
        Self::Faint,
        Self::Border,
        Self::Focus,
        Self::SelectionBg,
        Self::Crosshair,
        Self::Ranged,
        Self::SelectionFg,
        Self::SelectionBar,
        Self::Shortcut,
        Self::Keyspace,
        Self::Table,
        Self::Partition,
        Self::Clustering,
        Self::Column,
        Self::Label,
        Self::Section,
        Self::Accent,
        Self::Match,
        Self::Success,
        Self::Warning,
        Self::Danger,
        Self::Info,
        Self::Keyword,
        Self::Reference,
        Self::Literal,
        Self::Comment,
    ];

    #[must_use]
    pub const fn key(self) -> &'static str {
        match self {
            Self::Base => "base",
            Self::Surface => "surface",
            Self::Text => "text",
            Self::Dim => "dim",
            Self::Faint => "faint",
            Self::Border => "border",
            Self::Focus => "focus",
            Self::SelectionBg => "selection_bg",
            Self::Crosshair => "crosshair",
            Self::Ranged => "ranged",
            Self::SelectionFg => "selection_fg",
            Self::SelectionBar => "selection_bar",
            Self::Shortcut => "shortcut",
            Self::Keyspace => "keyspace",
            Self::Table => "table",
            Self::Partition => "partition",
            Self::Clustering => "clustering",
            Self::Column => "column",
            Self::Label => "label",
            Self::Section => "section",
            Self::Accent => "accent",
            Self::Match => "match",
            Self::Success => "success",
            Self::Warning => "warning",
            Self::Danger => "danger",
            Self::Info => "info",
            Self::Keyword => "keyword",
            Self::Reference => "reference",
            Self::Literal => "literal",
            Self::Comment => "comment",
        }
    }

    #[must_use]
    pub fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|role| role.key() == key)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn walking_from_the_first_theme_of_ours_reaches_every_one() {
        let mut walked = Vec::new();
        let mut current = Some(Ours::DefaultDark);

        while let Some(choice) = current {
            walked.push(choice);
            current = choice.next();
        }

        assert_eq!(
            walked,
            Ours::ALL,
            "a palette of ours that ALL does not list is never swept for contrast"
        );
    }

    #[test]
    fn every_theme_is_listed_once() {
        for (i, built_in) in BuiltIn::ALL.iter().enumerate() {
            assert_eq!(BuiltIn::ALL.iter().position(|b| b == built_in), Some(i));
        }
    }

    #[test]
    fn the_picker_lists_ours_before_the_ported_ones() {
        let ported = BuiltIn::ALL
            .iter()
            .position(|built_in| matches!(built_in, BuiltIn::Ported(_)))
            .expect("the ported palettes are listed");

        assert!(
            BuiltIn::ALL[..ported]
                .iter()
                .all(|built_in| matches!(built_in, BuiltIn::Ours(_))),
            "a ported palette is listed among ours"
        );
        assert_eq!(ported, Ours::ALL.len());
    }

    #[test]
    fn the_default_is_one_of_ours() {
        assert_eq!(ThemeChoice::default(), ThemeChoice::Ours(Ours::DefaultDark));
        assert_eq!(
            BuiltIn::ALL[0],
            BuiltIn::Ours(Ours::DefaultDark),
            "and the picker opens on it"
        );
    }

    #[test]
    fn no_two_themes_share_a_label() {
        let mut labels: Vec<&str> = BuiltIn::ALL.iter().map(|b| b.label()).collect();
        labels.sort_unstable();
        labels.dedup();

        assert_eq!(labels.len(), BuiltIn::ALL.len());
    }

    #[test]
    fn every_built_in_round_trips_through_its_slug() {
        for built_in in BuiltIn::ALL {
            let slug = ThemeChoice::from(built_in).slug();

            assert_eq!(
                ThemeChoice::builtin_from_slug(&slug),
                Some(built_in),
                "`{slug}` did not parse back to the theme that produced it"
            );
        }
    }

    #[test]
    fn no_two_built_ins_share_a_slug() {
        let mut slugs: Vec<String> = BuiltIn::ALL
            .iter()
            .map(|b| ThemeChoice::from(*b).slug())
            .collect();
        slugs.sort();
        slugs.dedup();

        assert_eq!(slugs.len(), BuiltIn::ALL.len());
    }

    #[test]
    fn a_custom_theme_is_named_rather_than_indexed() {
        let name = ThemeName::try_new("midnight").expect("a plain name is valid");
        let choice = ThemeChoice::Custom(name.clone());

        assert_eq!(choice.slug(), "midnight");
        assert_eq!(
            ThemeChoice::builtin_from_slug("midnight"),
            None,
            "a custom name must not resolve to a built-in"
        );
    }

    #[test]
    fn a_theme_name_may_not_be_blank_or_hold_whitespace() {
        assert!(ThemeName::try_new("midnight").is_ok());
        assert!(ThemeName::try_new("").is_err());
        assert!(ThemeName::try_new("mid night").is_err());
    }

    #[test]
    fn every_role_round_trips_through_its_key() {
        for role in Role::ALL {
            assert_eq!(Role::from_key(role.key()), Some(role));
        }
    }

    #[test]
    fn no_two_roles_share_a_key() {
        let mut keys: Vec<&str> = Role::ALL.iter().map(|r| r.key()).collect();
        keys.sort_unstable();
        keys.dedup();

        assert_eq!(keys.len(), Role::ALL.len());
    }
}

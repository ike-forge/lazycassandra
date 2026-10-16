//! Which theme is on: a name, never a colour (architecture.md §6). [`Ours`]
//! holds 4.5:1 contrast on every ground; [`Ported`] palettes are their authors'
//! and we impose nothing.

/// Flat: nothing mandates that a family has both a light and a dark variant.
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ported {
    CatppuccinLatte,
    CatppuccinFrappe,
    CatppuccinMacchiato,
    CatppuccinMocha,
}

/// Separate from [`ThemeChoice`], so custom themes extending custom themes are
/// unrepresentable.
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

    /// Exhaustive rather than derived: kebab-casing gets `"Catppuccin Frappé"`
    /// wrong.
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

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct ThemeName(std::sync::Arc<str>);

impl ThemeName {
    /// # Errors
    ///
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

/// `Default` is hand-written: `#[default]` only marks unit variants.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ThemeChoice {
    Ours(Ours),
    Ported(Ported),
    /// A name, not an index, which config reload would invalidate.
    Custom(ThemeName),
}

impl Default for ThemeChoice {
    fn default() -> Self {
        Self::Ours(Ours::DefaultDark)
    }
}

impl ThemeChoice {
    #[must_use]
    pub fn slug(&self) -> String {
        match self {
            Self::Ours(ours) => BuiltIn::Ours(*ours).slug().to_owned(),
            Self::Ported(ported) => BuiltIn::Ported(*ported).slug().to_owned(),
            Self::Custom(name) => name.as_str().to_owned(),
        }
    }

    /// `None` for custom names, which resolve against the config.
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

    /// Exhaustive, so a new variant breaks the build here and
    /// `walking_from_the_first_theme_of_ours_reaches_every_one` fails until
    /// `ALL` names it.
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

/// Mirrors `lc-inbound`'s `Palette`. A role the palette lacks fails to compile;
/// a new `Palette` field missing here is not caught.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Role {
    Base,
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
    pub const ALL: [Self; 29] = [
        Self::Base,
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

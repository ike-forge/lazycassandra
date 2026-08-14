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

/// A theme the user can choose.
///
/// `Default` is written out rather than derived: `#[default]` may only mark a
/// unit variant, and the default is a palette of ours by name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThemeChoice {
    Ours(Ours),
    Ported(Ported),
}

impl Default for ThemeChoice {
    fn default() -> Self {
        Self::Ours(Ours::DefaultDark)
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

impl ThemeChoice {
    /// Every theme, in the order the picker lists them: ours first, then the
    /// ported ones. `the_picker_lists_ours_before_the_ported_ones` is what
    /// holds this to `Ours::ALL` and `Ported::ALL` rather than to a hand-kept
    /// third list.
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

    #[must_use]
    pub fn index(self) -> usize {
        Self::ALL
            .iter()
            .position(|choice| *choice == self)
            .unwrap_or(0)
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
        for choice in ThemeChoice::ALL {
            assert_eq!(ThemeChoice::ALL[choice.index()], choice);
        }
    }

    #[test]
    fn the_picker_lists_ours_before_the_ported_ones() {
        let ported = ThemeChoice::ALL
            .iter()
            .position(|choice| matches!(choice, ThemeChoice::Ported(_)))
            .expect("the ported palettes are listed");

        assert!(
            ThemeChoice::ALL[..ported]
                .iter()
                .all(|choice| matches!(choice, ThemeChoice::Ours(_))),
            "a ported palette is listed among ours"
        );
        assert_eq!(ported, Ours::ALL.len());
    }

    #[test]
    fn the_default_is_one_of_ours() {
        assert_eq!(ThemeChoice::default(), ThemeChoice::Ours(Ours::DefaultDark));
        assert_eq!(
            ThemeChoice::default().index(),
            0,
            "and the picker opens on it"
        );
    }

    #[test]
    fn no_two_themes_share_a_label() {
        let mut labels: Vec<&str> = ThemeChoice::ALL.iter().map(|c| c.label()).collect();
        labels.sort_unstable();
        labels.dedup();

        assert_eq!(labels.len(), ThemeChoice::ALL.len());
    }
}

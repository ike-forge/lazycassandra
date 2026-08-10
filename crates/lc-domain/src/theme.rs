//! Which theme is on.
//!
//! A name, never a colour: the mapping from one of these to a palette lives in
//! `lc-inbound`, which is what keeps the domain free of a terminal
//! (architecture.md §6).

/// A theme the user can choose.
///
/// Declaration order is the order the picker lists them: the default first,
/// then the flavours lightest to darkest.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ThemeChoice {
    /// Wears the terminal's own background and body text
    /// (vision.md §4, principle 5).
    #[default]
    TerminalDefault,
    CatppuccinLatte,
    CatppuccinFrappe,
    CatppuccinMacchiato,
    CatppuccinMocha,
}

impl ThemeChoice {
    /// Every theme, in the order the picker lists them.
    pub const ALL: [Self; 5] = [
        Self::TerminalDefault,
        Self::CatppuccinLatte,
        Self::CatppuccinFrappe,
        Self::CatppuccinMacchiato,
        Self::CatppuccinMocha,
    ];

    /// What the picker calls it.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::TerminalDefault => "Terminal default",
            Self::CatppuccinLatte => "Catppuccin Latte",
            Self::CatppuccinFrappe => "Catppuccin Frappé",
            Self::CatppuccinMacchiato => "Catppuccin Macchiato",
            Self::CatppuccinMocha => "Catppuccin Mocha",
        }
    }

    /// Where `self` sits in [`Self::ALL`].
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
    fn every_theme_is_listed_once() {
        for choice in ThemeChoice::ALL {
            assert_eq!(ThemeChoice::ALL[choice.index()], choice);
        }
    }

    #[test]
    fn the_default_is_the_one_that_wears_the_terminals_clothes() {
        assert_eq!(ThemeChoice::default(), ThemeChoice::TerminalDefault);
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

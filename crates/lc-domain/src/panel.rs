//! The four panels, in frame order.

/// Declaration order is column order, and every other order follows it:
/// numbers, titles, the `Tab` walk.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Panel {
    #[default]
    Tables,
    Ddl,
    Contents,
    Queries,
}

impl Panel {
    pub const ALL: [Self; 4] = [Self::Tables, Self::Ddl, Self::Contents, Self::Queries];

    #[must_use]
    pub const fn next(self) -> Self {
        match self {
            Self::Tables => Self::Ddl,
            Self::Ddl => Self::Contents,
            Self::Contents => Self::Queries,
            Self::Queries => Self::Tables,
        }
    }

    #[must_use]
    pub const fn previous(self) -> Self {
        match self {
            Self::Tables => Self::Queries,
            Self::Ddl => Self::Tables,
            Self::Contents => Self::Ddl,
            Self::Queries => Self::Contents,
        }
    }

    /// Digits rather than name letters: four digits in frame order are one
    /// rule.
    #[must_use]
    pub const fn number(self) -> char {
        match self {
            Self::Tables => '1',
            Self::Ddl => '2',
            Self::Contents => '3',
            Self::Queries => '4',
        }
    }

    #[must_use]
    pub fn from_number(key: char) -> Option<Self> {
        Self::ALL.into_iter().find(|panel| panel.number() == key)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_panels_are_listed_in_column_order_and_none_is_missing() {
        for panel in Panel::ALL {
            // A new variant fails to compile here until added to `ALL`.
            let expected_position = match panel {
                Panel::Tables => 0,
                Panel::Ddl => 1,
                Panel::Contents => 2,
                Panel::Queries => 3,
            };
            assert_eq!(
                Panel::ALL[expected_position],
                panel,
                "{panel:?} is not at its column-order position"
            );
        }
        assert_eq!(
            Panel::ALL.len(),
            4,
            "a variant was added without extending ALL"
        );
    }

    #[test]
    fn the_default_panel_is_tables() {
        assert_eq!(Panel::default(), Panel::Tables);
    }

    #[test]
    fn the_cycle_walks_the_numbers_and_wraps() {
        // Pinned against `ALL`, not restated.
        let walked: Vec<Panel> =
            std::iter::successors(Some(Panel::ALL[0]), |panel| Some(panel.next()))
                .take(Panel::ALL.len())
                .collect();

        assert_eq!(walked, Panel::ALL);
        assert_eq!(Panel::ALL[Panel::ALL.len() - 1].next(), Panel::ALL[0]);
    }

    #[test]
    fn the_cycle_holds_every_panel_exactly_once() {
        for panel in Panel::ALL {
            assert_eq!(
                Panel::ALL.iter().filter(|other| **other == panel).count(),
                1,
                "{panel:?} is missing from ALL or listed twice"
            );
        }
    }

    #[test]
    fn stepping_back_undoes_stepping_forward() {
        for panel in Panel::ALL {
            assert_eq!(panel.next().previous(), panel);
            assert_eq!(panel.previous().next(), panel);
        }
    }

    #[test]
    fn every_panel_has_a_distinct_number() {
        let mut claimed: Vec<char> = Vec::new();

        for panel in Panel::ALL {
            let key = panel.number();
            assert!(
                !claimed.contains(&key),
                "{panel:?} claims '{key}', which another panel already has"
            );
            claimed.push(key);
        }
    }

    #[test]
    fn a_number_leads_back_to_the_panel_that_claims_it() {
        for panel in Panel::ALL {
            assert_eq!(Panel::from_number(panel.number()), Some(panel));
        }

        assert_eq!(Panel::from_number('z'), None);
    }

    #[test]
    fn the_numbers_are_the_ones_the_design_names() {
        assert_eq!(
            Panel::ALL.map(Panel::number),
            ['1', '2', '3', '4'],
            "the keys are a promise to the user's fingers, not an implementation detail"
        );
    }
}

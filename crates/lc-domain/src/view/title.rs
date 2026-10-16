//! A panel title naming the key that reaches its panel, and which of its tabs
//! is drawn.

use crate::style::Segment;

pub const TAB_SEPARATOR: &str = " - ";

const NUMBER_SEPARATOR: &str = "-";

/// One strip, so a panel's name is written in one place only.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PanelTitle {
    number: char,
    /// Never empty.
    tabs: Vec<Tab>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tab {
    text: Vec<Segment>,
    /// Equal to `text` for a tab with nothing to drop.
    short: Vec<Segment>,
    /// Always true for a single-tab panel.
    pub active: bool,
}

pub(crate) struct TabSpec {
    pub(crate) label: &'static str,
    pub(crate) short: &'static str,
    pub(crate) active: bool,
    /// `None` for a tab reached by pressing the panel's number again.
    pub(crate) number: Option<char>,
}

impl PanelTitle {
    pub(crate) fn styled(number: char, segments: Vec<Segment>) -> Self {
        Self::abbreviated(number, segments.clone(), segments)
    }

    /// The same fallback rung [`PanelTitle::tabbed`] gives, spent on one
    /// growing name.
    pub(crate) fn abbreviated(number: char, text: Vec<Segment>, short: Vec<Segment>) -> Self {
        Self {
            number,
            tabs: vec![Tab::new(text, short, true)],
        }
    }

    pub(crate) fn tabbed(number: char, specs: &[TabSpec]) -> Self {
        let tabs = specs
            .iter()
            .map(|spec| {
                let named = |label: &str| {
                    Segment::plain(match spec.number {
                        Some(key) => format!("{}{label}", prefix(key)),
                        None => label.to_owned(),
                    })
                };
                Tab::new(
                    vec![named(spec.label)],
                    vec![named(spec.short)],
                    spec.active,
                )
            })
            .collect();

        Self { number, tabs }
    }

    #[must_use]
    pub fn prefix(&self) -> String {
        prefix(self.number)
    }

    /// What a renderer measures a top border against.
    #[must_use]
    pub fn text(&self) -> String {
        format!("{}{}", self.prefix(), self.strip())
    }

    #[must_use]
    pub fn strip(&self) -> String {
        self.tabs
            .iter()
            .map(Tab::text)
            .collect::<Vec<_>>()
            .join(TAB_SEPARATOR)
    }

    #[must_use]
    pub fn tabs(&self) -> &[Tab] {
        &self.tabs
    }
}

fn prefix(number: char) -> String {
    format!("[{number}]{NUMBER_SEPARATOR}")
}

impl Tab {
    fn new(text: Vec<Segment>, short: Vec<Segment>, active: bool) -> Self {
        Self {
            text,
            short,
            active,
        }
    }

    #[must_use]
    pub fn text(&self) -> String {
        crate::style::text(&self.text)
    }

    #[must_use]
    pub fn short(&self) -> String {
        crate::style::text(&self.short)
    }

    #[must_use]
    pub fn segments(&self) -> &[Segment] {
        &self.text
    }

    #[must_use]
    pub fn short_segments(&self) -> &[Segment] {
        &self.short
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strip(active: usize) -> PanelTitle {
        PanelTitle::tabbed(
            '2',
            &[
                TabSpec {
                    label: "Table DDL",
                    short: "Table DDL",
                    active: active == 0,
                    number: None,
                },
                TabSpec {
                    label: "Table columns",
                    short: "Columns",
                    active: active == 1,
                    number: None,
                },
            ],
        )
    }

    #[test]
    fn a_title_opens_with_the_key_that_reaches_its_panel() {
        let title = PanelTitle::styled('1', vec![Segment::plain("Tables")]);

        assert_eq!(title.prefix(), "[1]-");
        assert_eq!(title.text(), "[1]-Tables");
    }

    #[test]
    fn a_strip_reads_as_its_tabs_joined_under_one_number() {
        let title = strip(0);

        assert_eq!(title.text(), "[2]-Table DDL - Table columns");
        assert_eq!(
            title.tabs().iter().map(Tab::text).collect::<Vec<_>>(),
            ["Table DDL", "Table columns"]
        );
        assert_eq!(
            title
                .tabs()
                .iter()
                .map(|tab| tab.active)
                .collect::<Vec<_>>(),
            [true, false]
        );
    }

    #[test]
    fn the_strip_is_the_text_without_the_prefix() {
        let title = strip(1);

        assert_eq!(title.strip(), "Table DDL - Table columns");
        assert_eq!(title.text(), format!("{}{}", title.prefix(), title.strip()));
    }

    #[test]
    fn a_tab_with_a_number_of_its_own_draws_it_ahead_of_its_name() {
        let title = PanelTitle::tabbed(
            '4',
            &[
                TabSpec {
                    label: "Queries",
                    short: "Queries",
                    active: true,
                    number: None,
                },
                TabSpec {
                    label: "Logs",
                    short: "Logs",
                    active: false,
                    number: Some('5'),
                },
            ],
        );

        assert_eq!(title.text(), "[4]-Queries - [5]-Logs");
        assert_eq!(title.tabs()[1].short(), "[5]-Logs");
    }

    #[test]
    fn a_tab_keeps_the_short_name_a_narrow_border_falls_back_to() {
        let title = strip(0);

        assert_eq!(title.tabs()[0].short(), "Table DDL");
        assert_eq!(title.tabs()[1].short(), "Columns");
    }
}

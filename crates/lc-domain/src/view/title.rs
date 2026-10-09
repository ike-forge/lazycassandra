//! A panel title that names the key reaching its panel, and which of its tabs
//! is the one being drawn.

use crate::style::Segment;

/// What separates one tab from the next on a panel's top border.
pub const TAB_SEPARATOR: &str = " - ";

/// What separates a panel's number from the strip that follows it.
const NUMBER_SEPARATOR: &str = "-";

/// A panel title: its number, then one tab or several with one of them active.
///
/// A single strip rather than a title beside a separate list of tabs, so there
/// is no second place a panel's name is written and no way for the two to
/// disagree (docs/superpowers/specs/2026-08-12-ddl-tabs-design.md §4).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PanelTitle {
    number: char,
    /// Never empty.
    tabs: Vec<Tab>,
}

/// One tab of a panel's title strip.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tab {
    text: Vec<Segment>,
    /// The name to fall back to on a border that cannot hold every full one.
    /// Equal to `text` for a tab with nothing to drop.
    short: Vec<Segment>,
    /// Whether this is the tab whose body the panel is drawing. Always true
    /// for a panel with one tab.
    pub active: bool,
}

/// One tab as its panel asks for it: named twice — in full and in the short
/// form a narrow border falls back to — and whether it is the one drawn.
pub(crate) struct TabSpec {
    pub(crate) label: &'static str,
    pub(crate) short: &'static str,
    pub(crate) active: bool,
    /// A digit reaching this tab directly, drawn ahead of its name. `None`
    /// for a tab reached by pressing the panel's number again.
    pub(crate) number: Option<char>,
}

impl PanelTitle {
    /// A panel with one tab whose name is drawn in more than one colour.
    pub(crate) fn styled(number: char, segments: Vec<Segment>) -> Self {
        Self::abbreviated(number, segments.clone(), segments)
    }

    /// [`PanelTitle::styled`] with a shorter name for a border that cannot
    /// hold the full one — the same rung [`PanelTitle::tabbed`] gives a strip
    /// of tabs, spent here on one name that grows rather than on several.
    pub(crate) fn abbreviated(number: char, text: Vec<Segment>, short: Vec<Segment>) -> Self {
        Self {
            number,
            tabs: vec![Tab::new(text, short, true)],
        }
    }

    /// A strip of tabs under one number.
    ///
    /// A tab without a number of its own is reached by pressing the panel's
    /// again (docs/shortcuts.md §4).
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

    /// The `[1]-` a title opens with — what reaches the panel, drawn where
    /// the panel is rather than in a manual (vision.md §4, principle 1).
    #[must_use]
    pub fn prefix(&self) -> String {
        prefix(self.number)
    }

    /// The strip as one string, prefix and separators included — what a
    /// renderer measures a top border against.
    #[must_use]
    pub fn text(&self) -> String {
        format!("{}{}", self.prefix(), self.strip())
    }

    /// The tabs alone, joined, without the prefix.
    #[must_use]
    pub fn strip(&self) -> String {
        self.tabs
            .iter()
            .map(Tab::text)
            .collect::<Vec<_>>()
            .join(TAB_SEPARATOR)
    }

    /// The tabs, in the order they are drawn.
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

    /// The name to draw where the full strip does not fit.
    #[must_use]
    pub fn short(&self) -> String {
        crate::style::text(&self.short)
    }

    /// The full name, run by run — what a renderer draws when it can spend a
    /// colour per run rather than one over the whole tab.
    #[must_use]
    pub fn segments(&self) -> &[Segment] {
        &self.text
    }

    /// [`Tab::segments`] for the short name.
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

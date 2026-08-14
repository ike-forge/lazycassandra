//! A panel title that knows where its focus shortcut sits, and which of its
//! tabs is the one being drawn.

use std::ops::Range;

/// What separates one tab from the next on a panel's top border.
pub const TAB_SEPARATOR: &str = " - ";

/// A panel title: one tab, or several with one of them active.
///
/// A single strip rather than a title beside a separate list of tabs, so there
/// is no second place a panel's name is written and no way for the two to
/// disagree (docs/superpowers/specs/2026-08-12-ddl-tabs-design.md §4).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PanelTitle {
    /// Never empty.
    tabs: Vec<Tab>,
}

/// One tab of a panel's title strip.
///
/// The range is private for the reason [`crate::state::Cursor`]'s index is: it
/// is checked once, here, so no renderer can be handed one that slices a
/// character in half.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tab {
    text: String,
    /// The name to fall back to on a border that cannot hold every full one.
    /// Equal to `text` for a tab with nothing to drop.
    short: String,
    /// The key that reaches this tab. Kept beside [`Tab::mark`] rather than
    /// derived from it: the mark is a range into `text`, and a tab drawn under
    /// its short name has none to derive from.
    shortcut: Option<char>,
    mark: Option<Range<usize>>,
    /// Whether this is the tab whose body the panel is drawing. Always true
    /// for a panel with one tab.
    pub active: bool,
}

/// One tab as its panel asks for it: named twice — in full and in the short
/// form a narrow border falls back to — with the key that reaches it and
/// whether it is the one drawn.
pub(crate) struct TabSpec {
    pub(crate) label: &'static str,
    pub(crate) short: &'static str,
    pub(crate) shortcut: char,
    pub(crate) active: bool,
}

impl PanelTitle {
    /// `text` with its first case-insensitive `shortcut` marked.
    ///
    /// A title that does not contain its own shortcut draws unmarked rather
    /// than failing; `view::tests` pins that case shut for the panels that
    /// exist.
    pub(crate) fn marking(text: String, shortcut: char) -> Self {
        Self {
            tabs: vec![Tab::new(text.clone(), text, Some(shortcut), true)],
        }
    }

    /// A strip of tabs, each carrying the key that reaches it.
    ///
    /// Every tab is marked, not only the first: a tab is a place with a key of
    /// its own (docs/shortcuts.md §4), so a strip that marked one would leave
    /// the rest of its bindings unadvertised.
    pub(crate) fn tabbed(specs: &[TabSpec]) -> Self {
        let tabs = specs
            .iter()
            .map(|spec| {
                Tab::new(
                    spec.label.to_owned(),
                    spec.short.to_owned(),
                    Some(spec.shortcut),
                    spec.active,
                )
            })
            .collect();

        Self { tabs }
    }

    /// The strip as one string, separators included — what a renderer measures
    /// a top border against.
    #[must_use]
    pub fn text(&self) -> String {
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

impl Tab {
    fn new(text: String, short: String, shortcut: Option<char>, active: bool) -> Self {
        let mark = shortcut.and_then(|shortcut| {
            text.char_indices()
                .find(|(_, character)| character.eq_ignore_ascii_case(&shortcut))
                .map(|(index, character)| index..index + character.len_utf8())
        });

        Self {
            text,
            short,
            shortcut,
            mark,
            active,
        }
    }

    #[must_use]
    pub fn text(&self) -> &str {
        &self.text
    }

    /// The key that reaches this tab, whichever name it is drawn under.
    #[must_use]
    pub const fn shortcut(&self) -> Option<char> {
        self.shortcut
    }

    /// The name to draw where the full strip does not fit.
    #[must_use]
    pub fn short(&self) -> &str {
        &self.short
    }

    /// The tab in three pieces: what precedes the shortcut, the shortcut, and
    /// what follows it. The middle piece is empty for an unmarked tab.
    #[must_use]
    pub fn split(&self) -> (&str, &str, &str) {
        let Some(mark) = &self.mark else {
            return (&self.text, "", "");
        };

        (
            &self.text[..mark.start],
            &self.text[mark.start..mark.end],
            &self.text[mark.end..],
        )
    }

    /// The short name in the same three pieces. A tab whose short form is not
    /// its full one is drawn unmarked: the mark is a range into `text`, and a
    /// shortened name is not the string it was measured against.
    #[must_use]
    pub fn split_short(&self) -> (&str, &str, &str) {
        if self.short == self.text {
            return self.split();
        }

        (&self.short, "", "")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn split(text: &str, shortcut: char) -> (&str, &str, &str) {
        let title = PanelTitle::marking(text.to_owned(), shortcut);
        let (before, mark, _) = title.tabs()[0].split();
        let start = before.len();
        let end = start + mark.len();

        (&text[..start], &text[start..end], &text[end..])
    }

    #[test]
    fn a_leading_shortcut_is_marked_despite_its_case() {
        assert_eq!(split("Tables", 't'), ("", "T", "ables"));
    }

    #[test]
    fn a_shortcut_inside_a_word_is_marked_where_it_sits() {
        assert_eq!(split("Queries", 'u'), ("Q", "u", "eries"));
        assert_eq!(split("Table DDL", 'a'), ("T", "a", "ble DDL"));
    }

    #[test]
    fn only_the_first_occurrence_is_marked() {
        assert_eq!(split("Table contents", 't'), ("", "T", "able contents"));
    }

    #[test]
    fn a_title_that_lacks_its_shortcut_draws_unmarked() {
        assert_eq!(split("Tables", 'z'), ("Tables", "", ""));
    }

    #[test]
    fn a_multibyte_character_before_the_mark_does_not_shift_it() {
        // The Contents title interpolates an em dash, so the mark's index is
        // a byte offset and not a character count.
        let (before, mark, after) = split("Table contents — shop.orders", 'p');

        assert_eq!(mark, "p");
        assert_eq!(before, "Table contents — sho");
        assert_eq!(after, ".orders");
    }

    #[test]
    fn the_three_pieces_reassemble_into_the_title() {
        for shortcut in ['c', 'z'] {
            let title = PanelTitle::marking("Table contents — shop.orders".to_owned(), shortcut);
            let (before, mark, after) = title.tabs()[0].split();

            assert_eq!(format!("{before}{mark}{after}"), title.text());
        }
    }

    fn strip(active: usize) -> PanelTitle {
        PanelTitle::tabbed(&[
            TabSpec {
                label: "Table DDL",
                short: "Table DDL",
                shortcut: 'd',
                active: active == 0,
            },
            TabSpec {
                label: "Table columns",
                short: "Columns",
                shortcut: 'c',
                active: active == 1,
            },
        ])
    }

    #[test]
    fn a_strip_reads_as_its_tabs_joined() {
        let title = strip(0);

        assert_eq!(title.text(), "Table DDL - Table columns");
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
    fn a_shortened_tab_gives_up_its_mark_and_a_tab_with_nothing_to_drop_keeps_it() {
        let title = strip(0);

        assert_eq!(
            title.tabs()[0].split_short(),
            ("Table ", "D", "DL"),
            "a short form equal to the full name keeps the mark"
        );
        assert_eq!(title.tabs()[1].split_short(), ("Columns", "", ""));
    }

    #[test]
    fn every_tab_carries_its_own_shortcut() {
        let title = strip(1);

        assert_eq!(title.tabs()[0].split(), ("Table ", "D", "DL"));
        assert_eq!(title.tabs()[1].split(), ("Table ", "c", "olumns"));
    }
}

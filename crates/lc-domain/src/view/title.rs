//! A panel title that knows where its focus shortcut sits.

use std::ops::Range;

/// A panel title and the position of the key that focuses the panel.
///
/// The range is private for the reason [`crate::state::Cursor`]'s index is: it
/// is checked once, here, so no renderer can be handed one that slices a
/// character in half.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PanelTitle {
    text: String,
    mark: Option<Range<usize>>,
}

impl PanelTitle {
    /// `text` with its first case-insensitive `shortcut` marked.
    ///
    /// A title that does not contain its own shortcut draws unmarked rather
    /// than failing; `view::tests` pins that case shut for the panels that
    /// exist.
    pub(crate) fn marking(text: String, shortcut: char) -> Self {
        let mark = text
            .char_indices()
            .find(|(_, character)| character.eq_ignore_ascii_case(&shortcut))
            .map(|(index, character)| index..index + character.len_utf8());

        Self { text, mark }
    }

    #[must_use]
    pub fn text(&self) -> &str {
        &self.text
    }

    /// The title in three pieces: what precedes the shortcut, the shortcut,
    /// and what follows it. The middle piece is empty for an unmarked title.
    #[must_use]
    pub fn split(&self) -> (&str, &str, &str) {
        let Some(mark) = self.mark.clone() else {
            return (&self.text, "", "");
        };

        (
            &self.text[..mark.start],
            &self.text[mark.start..mark.end],
            &self.text[mark.end..],
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_leading_shortcut_is_marked_despite_its_case() {
        let title = PanelTitle::marking("Tables".to_owned(), 't');

        assert_eq!(title.split(), ("", "T", "ables"));
    }

    #[test]
    fn a_shortcut_inside_a_word_is_marked_where_it_sits() {
        assert_eq!(
            PanelTitle::marking("Queries".to_owned(), 'u').split(),
            ("Q", "u", "eries")
        );
        assert_eq!(
            PanelTitle::marking("Table DDL".to_owned(), 'a').split(),
            ("T", "a", "ble DDL")
        );
    }

    #[test]
    fn only_the_first_occurrence_is_marked() {
        assert_eq!(
            PanelTitle::marking("Table contents".to_owned(), 't').split(),
            ("", "T", "able contents")
        );
    }

    #[test]
    fn a_title_that_lacks_its_shortcut_draws_unmarked() {
        assert_eq!(
            PanelTitle::marking("Tables".to_owned(), 'z').split(),
            ("Tables", "", "")
        );
    }

    #[test]
    fn a_multibyte_character_before_the_mark_does_not_shift_it() {
        // The Contents title interpolates an em dash, so the mark's index is
        // a byte offset and not a character count.
        let title = PanelTitle::marking("Table contents — shop.orders".to_owned(), 'p');
        let (before, mark, after) = title.split();

        assert_eq!(mark, "p");
        assert_eq!(before, "Table contents — sho");
        assert_eq!(after, ".orders");
    }

    #[test]
    fn the_three_pieces_reassemble_into_the_title() {
        for shortcut in ['c', 'z'] {
            let title = PanelTitle::marking("Table contents — shop.orders".to_owned(), shortcut);
            let (before, mark, after) = title.split();

            assert_eq!(format!("{before}{mark}{after}"), title.text());
        }
    }
}

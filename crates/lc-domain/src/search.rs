//! Searching a panel: the term, where it applies, and the prompt that collects
//! it.

use std::ops::Range;

use crate::panel::Panel;

/// Non-empty, so `Option<Search>` means "no search" and an empty query clears.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Search {
    term: String,
    folded: String,
}

impl Search {
    #[must_use]
    pub fn new(raw: &str) -> Option<Self> {
        if raw.is_empty() {
            return None;
        }

        Some(Self {
            term: raw.to_owned(),
            folded: raw.chars().flat_map(char::to_lowercase).collect(),
        })
    }

    /// Case-insensitive, no metacharacters: a half-typed query is never an
    /// error.
    #[must_use]
    pub fn matches(&self, haystack: &str) -> bool {
        !self.ranges(haystack).is_empty()
    }

    /// Byte ranges into `haystack`, sorted and disjoint. `to_lowercase` is not
    /// length-preserving (`İ` folds to two chars), so hits are mapped back
    /// through a per-byte table, snapping outward to whole characters.
    #[must_use]
    pub fn ranges(&self, haystack: &str) -> Vec<Range<usize>> {
        let (folded, sources) = fold(haystack);
        let mut ranges: Vec<Range<usize>> = Vec::new();
        let mut at = 0;

        while let Some(hit) = folded[at..].find(&self.folded) {
            let start = at + hit;
            let end = start + self.folded.len();
            let range = sources[start].start..sources[end - 1].end;
            at = end;

            // A hit can snap outward into a character an earlier hit claimed.
            // Source ranges are monotonic, so checking the last one suffices.
            match ranges.last() {
                Some(last) if last.end > range.start => {}
                _ => ranges.push(range),
            }
        }

        ranges
    }

    /// As typed.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.term
    }
}

/// One entry per folded byte: the source character's range.
fn fold(haystack: &str) -> (String, Vec<Range<usize>>) {
    let mut folded = String::with_capacity(haystack.len());
    let mut sources = Vec::with_capacity(haystack.len());

    for (offset, character) in haystack.char_indices() {
        let source = offset..offset + character.len_utf8();
        let before = folded.len();

        folded.extend(character.to_lowercase());
        sources.resize(folded.len(), source);
        debug_assert!(folded.len() > before, "a fold produces at least one byte");
    }

    (folded, sources)
}

/// `Queries` means its Logs tab; the Cql tab takes `/` as text. Keyed off this
/// rather than [`Panel`] so every accessor is total.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Searched {
    Tables,
    Ddl,
    Contents,
    Queries,
}

impl Searched {
    #[must_use]
    pub const fn of(panel: Panel) -> Option<Self> {
        match panel {
            Panel::Tables => Some(Self::Tables),
            Panel::Ddl => Some(Self::Ddl),
            Panel::Contents => Some(Self::Contents),
            Panel::Queries => Some(Self::Queries),
        }
    }

    #[must_use]
    pub const fn panel(self) -> Panel {
        match self {
            Self::Tables => Panel::Tables,
            Self::Ddl => Panel::Ddl,
            Self::Contents => Panel::Contents,
            Self::Queries => Panel::Queries,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    Forwards,
    Backwards,
}

impl Direction {
    /// A zero `len` returns zero rather than panicking.
    #[must_use]
    pub const fn step(self, at: usize, len: usize) -> usize {
        if len == 0 {
            return 0;
        }

        match self {
            Self::Forwards => (at + 1) % len,
            Self::Backwards => (at + len - 1) % len,
        }
    }
}

/// One type for every panel: `column` is zero in a list.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Origin {
    pub row: usize,
    pub column: usize,
}

/// Lives in [`crate::overlay::Overlay`], so a palette and a prompt at once is
/// unrepresentable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Prompt {
    target: Searched,
    query: String,
    /// Live search overwrites the target's search per keystroke, so `esc` needs
    /// this.
    previous: Option<Search>,
    /// Every keystroke searches from here, so extending a term does not walk
    /// the cursor.
    origin: Origin,
}

impl Prompt {
    #[must_use]
    pub fn opened(target: Searched, previous: Option<Search>, origin: Origin) -> Self {
        Self {
            target,
            query: String::new(),
            previous,
            origin,
        }
    }

    /// `tab` is inert under a prompt, so the target cannot change.
    #[must_use]
    pub const fn target(&self) -> Searched {
        self.target
    }

    #[must_use]
    pub fn query(&self) -> &str {
        &self.query
    }

    #[must_use]
    pub const fn previous(&self) -> Option<&Search> {
        self.previous.as_ref()
    }

    #[must_use]
    pub const fn origin(&self) -> Origin {
        self.origin
    }

    #[must_use]
    pub fn search(&self) -> Option<Search> {
        Search::new(&self.query)
    }

    pub fn push(&mut self, character: char) {
        self.query.push(character);
    }

    pub fn pop(&mut self) {
        self.query.pop();
    }
}

/// Not a [`Prompt`]: an overlay search overwrites nothing, and nothing survives
/// it closing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sift {
    query: String,
    /// Every keystroke searches from here.
    origin: usize,
}

impl Sift {
    #[must_use]
    pub const fn opened(origin: usize) -> Self {
        Self {
            query: String::new(),
            origin,
        }
    }

    #[must_use]
    pub fn query(&self) -> &str {
        &self.query
    }

    #[must_use]
    pub const fn origin(&self) -> usize {
        self.origin
    }

    #[must_use]
    pub fn search(&self) -> Option<Search> {
        Search::new(&self.query)
    }

    pub fn push(&mut self, character: char) {
        self.query.push(character);
    }

    /// `false` on an empty query, which closes the prompt.
    pub fn pop(&mut self) -> bool {
        self.query.pop().is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_empty_query_is_not_a_search() {
        assert_eq!(Search::new(""), None, "an empty search is what None means");
    }

    #[test]
    fn matching_ignores_case_in_both_directions() {
        let search = Search::new("ORD").expect("non-empty");

        assert!(search.matches("orders"));
        assert!(search.matches("ORDERS"));
        assert!(Search::new("ord").expect("non-empty").matches("ORDERS"));
    }

    #[test]
    fn matching_is_a_substring_anywhere_and_not_a_prefix() {
        let search = Search::new("by").expect("non-empty");

        assert!(search.matches("orders_by_customer"));
        assert!(!search.matches("orders"));
    }

    #[test]
    fn a_metacharacter_is_matched_literally() {
        assert!(Search::new(".*").expect("non-empty").matches("a.*b"));
        assert!(!Search::new(".*").expect("non-empty").matches("anything"));
    }

    #[test]
    fn folding_reaches_past_ascii() {
        // Cell values are arbitrary text, so full Unicode lowercasing.
        assert!(Search::new("é").expect("non-empty").matches("CAFÉ"));
    }

    #[test]
    fn the_term_and_the_haystack_fold_the_same_way() {
        // Final sigma: the term is folded per char like the haystack, so `Σ`
        // mid-word still matches.
        assert!(Search::new("ΟΣ").expect("non-empty").matches("ΟΔΟΣ"));
    }

    #[test]
    fn the_term_is_kept_as_typed_for_the_marker() {
        assert_eq!(Search::new("ORD").expect("non-empty").as_str(), "ORD");
    }

    #[test]
    fn ranges_are_every_occurrence_in_order() {
        let search = Search::new("or").expect("non-empty");

        assert_eq!(search.ranges("orders_by_order"), vec![0..2, 10..12]);
    }

    #[test]
    fn ranges_index_the_haystack_as_given() {
        let search = Search::new("ord").expect("non-empty");
        let haystack = "ORDERS";

        let ranges = search.ranges(haystack);

        assert_eq!(ranges, vec![0..3]);
        assert_eq!(
            &haystack[ranges[0].clone()],
            "ORD",
            "a valid slice, as given"
        );
    }

    #[test]
    fn ranges_survive_a_fold_that_changes_length() {
        // `İ` folds to two chars.
        let search = Search::new("i̇stanbul").expect("non-empty");
        let haystack = "in İstanbul";

        let ranges = search.ranges(haystack);

        assert_eq!(ranges.len(), 1);
        assert_eq!(&haystack[ranges[0].clone()], "İstanbul");
    }

    #[test]
    fn a_hit_inside_an_expansion_snaps_out_to_a_character() {
        // `i` hits half of `İ`'s expansion; the range must be the whole
        // character.
        let search = Search::new("i").expect("non-empty");
        let haystack = "İstanbul";

        let ranges = search.ranges(haystack);

        assert_eq!(ranges, vec![0..2]);
        assert_eq!(&haystack[ranges[0].clone()], "İ");
    }

    #[test]
    fn a_dotless_i_is_not_an_i() {
        // U+0131 and `i` are different letters.
        let search = Search::new("i").expect("non-empty");

        assert!(!search.matches("kırmızı"));
    }

    #[test]
    fn overlapping_occurrences_are_reported_once_each() {
        let search = Search::new("aa").expect("non-empty");

        assert_eq!(search.ranges("aaaa"), vec![0..2, 2..4], "disjoint");
    }

    #[test]
    fn no_occurrence_is_no_ranges_and_no_match() {
        let search = Search::new("zzz").expect("non-empty");

        assert!(search.ranges("orders").is_empty());
        assert!(!search.matches("orders"));
    }

    #[test]
    fn every_panel_can_hold_a_search() {
        assert_eq!(Searched::of(Panel::Tables), Some(Searched::Tables));
        assert_eq!(Searched::of(Panel::Ddl), Some(Searched::Ddl));
        assert_eq!(Searched::of(Panel::Contents), Some(Searched::Contents));
        assert_eq!(Searched::of(Panel::Queries), Some(Searched::Queries));
    }

    #[test]
    fn a_step_wraps_at_both_ends() {
        assert_eq!(Direction::Forwards.step(2, 3), 0);
        assert_eq!(Direction::Backwards.step(0, 3), 2);
        assert_eq!(Direction::Forwards.step(0, 1), 0, "one row is its own next");
    }

    #[test]
    fn a_target_names_the_panel_it_came_from() {
        for panel in Panel::ALL {
            if let Some(target) = Searched::of(panel) {
                assert_eq!(target.panel(), panel);
            }
        }
    }

    #[test]
    fn a_sift_remembers_the_row_it_opened_on() {
        let sift = Sift::opened(7);

        assert_eq!(sift.origin(), 7);
        assert_eq!(sift.query(), "");
        assert!(sift.search().is_none(), "an empty query matches nothing");
    }

    #[test]
    fn a_sift_builds_a_search_from_what_was_typed() {
        let mut sift = Sift::opened(0);
        sift.push('A');
        sift.push('s');

        assert_eq!(sift.query(), "As");
        assert!(
            sift.search()
                .expect("two characters is a term")
                .matches("Ash dark"),
            "the term folds case, as every search does"
        );
    }

    #[test]
    fn popping_an_empty_sift_reports_that_it_is_spent() {
        let mut sift = Sift::opened(0);
        sift.push('a');

        assert!(sift.pop(), "a sift with characters left is not spent");
        assert!(!sift.pop(), "popping nothing is what closes the prompt");
        assert_eq!(sift.query(), "");
    }
}

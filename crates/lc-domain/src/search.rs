//! Searching a panel: the term, where it can be applied, and the prompt that
//! collects it.
//!
//! docs/superpowers/specs/2026-08-10-panel-search-design.md.

use std::ops::Range;

use crate::state::Panel;

/// A search term that is applied to a panel.
///
/// The invariant is non-emptiness, which is what makes `Option<Search>` mean
/// "no search" — an empty query clears rather than matching everything, and
/// that follows from [`Search::new`] rather than from a check anyone has to
/// remember (design §2).
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
            folded: casefold(raw),
        })
    }

    /// Case-insensitive substring, with no metacharacters: every keystroke of
    /// a live prompt is literal, so no half-typed query is ever an error state
    /// the panel underneath has to render (design §2).
    #[must_use]
    pub fn matches(&self, haystack: &str) -> bool {
        !self.ranges(haystack).is_empty()
    }

    /// Every occurrence, as byte ranges into `haystack` itself — sorted,
    /// disjoint, and each a valid slice of it.
    ///
    /// Case folding is not length-preserving (`İ` folds to two chars, `ß` to
    /// `ss`), so searching the folded haystack and reusing its offsets
    /// would slice the source at a byte that is not a boundary. The fold is
    /// therefore built per character alongside a table of where each folded
    /// byte came from, and a hit's endpoints are mapped back through it.
    ///
    /// A hit that begins or ends inside one character's expansion — `s`
    /// against `ß` — snaps outward to that character, which is the only thing
    /// that can be sliced.
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

            match ranges.last() {
                Some(last) if last.end > range.start => {}
                _ => ranges.push(range),
            }
        }

        ranges
    }

    /// As typed, which is what the panel's marker shows (design §5.1).
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.term
    }
}

/// `haystack` folded, and where each of its bytes came from.
///
/// One entry per byte of the folded string, holding the source character's
/// own range — so a folded offset maps back to a character boundary whatever
/// the fold did to the length.
fn fold(haystack: &str) -> (String, Vec<Range<usize>>) {
    let mut folded = String::with_capacity(haystack.len());
    let mut sources = Vec::with_capacity(haystack.len());

    for (offset, character) in haystack.char_indices() {
        let source = offset..offset + character.len_utf8();
        let before = folded.len();

        folded.push_str(&casefold_char(character));
        sources.resize(folded.len(), source);
        debug_assert!(folded.len() > before, "a fold produces at least one byte");
    }

    (folded, sources)
}

/// Case-folds every character of `s`, independently of position.
fn casefold(s: &str) -> String {
    s.chars().map(casefold_char).collect()
}

/// `char::to_lowercase` alone leaves some characters, such as `ß`, unfolded —
/// they are already lowercase, so it has nothing to do. Round-tripping
/// through uppercase first recovers the multi-character expansion (`ß` to
/// `ss`) that full Unicode case folding would give, which is what lets a
/// single `s` match inside it.
fn casefold_char(character: char) -> String {
    let upper: String = character.to_uppercase().collect();
    upper.to_lowercase()
}

/// A panel that can hold a search.
///
/// Queries cannot, until it has a normal mode to reach a prompt from (design
/// §8). Keying a search off [`Panel`] instead would leave an unreachable
/// `Panel::Queries` arm in every accessor; this makes them total, and makes
/// the compiler list them when that slice lands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Searched {
    Tables,
    Ddl,
    Contents,
}

impl Searched {
    #[must_use]
    pub const fn of(panel: Panel) -> Option<Self> {
        match panel {
            Panel::Tables => Some(Self::Tables),
            Panel::Ddl => Some(Self::Ddl),
            Panel::Contents => Some(Self::Contents),
            Panel::Queries => None,
        }
    }

    #[must_use]
    pub const fn panel(self) -> Panel {
        match self {
            Self::Tables => Panel::Tables,
            Self::Ddl => Panel::Ddl,
            Self::Contents => Panel::Contents,
        }
    }
}

/// An open search prompt.
///
/// Lives in [`crate::state::Overlay`] rather than beside it, so "a palette and
/// a prompt at once" cannot be written down — the argument that enum's own doc
/// comment already makes about a message and a palette (design §1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Prompt {
    target: Searched,
    query: String,
    /// What was applied before this prompt opened, and what `esc` restores.
    /// Live search overwrites the target's search on every keystroke, so
    /// cancelling needs something to put back (design §3).
    previous: Option<Search>,
}

impl Prompt {
    /// Opens over `target`, remembering the search it is about to overwrite.
    #[must_use]
    pub fn opened(target: Searched, previous: Option<Search>) -> Self {
        Self {
            target,
            query: String::new(),
            previous,
        }
    }

    /// Captured at open, not re-read per keystroke: `tab` is inert while a
    /// prompt is up, so the panel being searched cannot change under a live
    /// query (design §3.2).
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

    /// What the query now means, for a caller that must apply it.
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
        // Cell values are whatever the cluster stored, so `to_lowercase`
        // rather than `to_ascii_lowercase` (design §2).
        assert!(Search::new("é").expect("non-empty").matches("CAFÉ"));
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
        // `İ` folds to two chars, so a folded offset is not a source offset.
        let search = Search::new("i̇stanbul").expect("non-empty");
        let haystack = "in İstanbul";

        let ranges = search.ranges(haystack);

        assert_eq!(ranges.len(), 1);
        assert_eq!(&haystack[ranges[0].clone()], "İstanbul");
    }

    #[test]
    fn a_hit_inside_an_expansion_snaps_out_to_a_character() {
        // `ß` folds to `ss`, so `s` hits half a character; the range has to be a
        // slice of the source, which means the whole `ß`.
        let search = Search::new("s").expect("non-empty");
        let haystack = "straße";

        let ranges = search.ranges(haystack);

        assert_eq!(ranges.len(), 2);
        assert_eq!(&haystack[ranges[0].clone()], "s");
        assert_eq!(&haystack[ranges[1].clone()], "ß");
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
    fn every_panel_but_queries_can_hold_a_search() {
        assert_eq!(Searched::of(Panel::Tables), Some(Searched::Tables));
        assert_eq!(Searched::of(Panel::Ddl), Some(Searched::Ddl));
        assert_eq!(Searched::of(Panel::Contents), Some(Searched::Contents));
        assert_eq!(Searched::of(Panel::Queries), None);
    }

    #[test]
    fn a_target_names_the_panel_it_came_from() {
        for panel in Panel::ALL {
            if let Some(target) = Searched::of(panel) {
                assert_eq!(target.panel(), panel);
            }
        }
    }
}

//! Searching a panel: the term, where it can be applied, and the prompt that
//! collects it.
//!
//! docs/superpowers/specs/2026-08-10-panel-search-design.md.

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
            folded: raw.to_lowercase(),
        })
    }

    /// Case-insensitive substring, with no metacharacters: every keystroke of
    /// a live prompt is literal, so no half-typed query is ever an error state
    /// the panel underneath has to render (design §2).
    #[must_use]
    pub fn matches(&self, haystack: &str) -> bool {
        haystack.to_lowercase().contains(&self.folded)
    }

    /// As typed, which is what the panel's marker shows (design §5.1).
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.term
    }
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

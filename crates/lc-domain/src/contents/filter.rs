use crate::search::Search;

/// Not a [`crate::search::Prompt`]: a search flags cells and keeps rows, a
/// filter drops rows. Applied on close, since a hard filter rebuilds the grid
/// under the typing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Filter {
    column: usize,
    /// Captured at open: the prompt claims every key, so the page cannot change
    /// under it.
    name: String,
    query: String,
}

impl Filter {
    /// Seeded with the standing filter, so deleting the text clears it.
    #[must_use]
    pub fn opened(column: usize, name: &str, standing: Option<&Search>) -> Self {
        Self {
            column,
            name: name.to_owned(),
            query: standing.map(Search::as_str).unwrap_or_default().to_owned(),
        }
    }

    #[must_use]
    pub const fn column(&self) -> usize {
        self.column
    }

    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    #[must_use]
    pub fn query(&self) -> &str {
        &self.query
    }

    /// `None` for an empty query, which clears the filter.
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
    fn a_filter_prompt_opens_on_the_term_already_standing() {
        let filter = Filter::opened(1, "city", Search::new("berlin").as_ref());

        assert_eq!(filter.query(), "berlin");
        assert_eq!(filter.name(), "city");
        assert_eq!(filter.column(), 1);
    }

    #[test]
    fn emptying_a_filter_prompt_is_what_clears_the_filter() {
        let mut filter = Filter::opened(0, "name", Search::new("ada").as_ref());

        for _ in 0..3 {
            filter.pop();
        }

        assert_eq!(filter.search(), None, "an empty term is no filter at all");
    }
}

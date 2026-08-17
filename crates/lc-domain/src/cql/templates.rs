//! The shapes a statement comes in, offered whole.
//!
//! A keyword completion saves the rest of one word; a template saves the rest
//! of the statement. They are offered only where a statement begins, which is
//! the one position at which the whole shape is still undecided
//! (docs/superpowers/specs/2026-08-17-completion-ranking-design.md §3).

/// One statement skeleton.
///
/// Split at the first hole rather than carrying a caret offset beside the
/// text: a caret cannot then point outside what was typed, and the two halves
/// are what `accept` walks anyway (conventions.md §1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Template {
    /// What the popup draws: the statement's shape, each hole an empty `<>`.
    pub label: &'static str,
    /// Typed before the cursor lands.
    pub head: &'static str,
    /// Typed after it.
    pub tail: &'static str,
}

impl Template {
    /// The word a prefix is matched against — the statement's opening keyword.
    pub(crate) fn keyword(self) -> &'static str {
        self.head
            .split_once(' ')
            .map_or(self.head, |(word, _)| word)
    }

    pub(crate) fn text(self) -> String {
        format!("{}{}", self.head, self.tail)
    }

    /// How far into [`Self::text`] the cursor lands, in characters.
    pub(crate) fn caret(self) -> usize {
        self.head.chars().count()
    }
}

/// Every shape offered, in the order a popup lists them.
///
/// The data statements first, then the schema ones, because the panel exists
/// to read rows (vision.md §5.1). No function, aggregate or materialised view:
/// each is a body of code or a second query rather than a shape, and a
/// skeleton would be a worse start than an empty line.
///
/// `BEGIN BATCH` is the outer wrapper alone. What goes between its two lines
/// is a statement in its own right, and the popup offers those on the line the
/// caret lands on.
pub(crate) const TEMPLATES: &[Template] = &[
    Template {
        label: "SELECT <> FROM <>",
        head: "SELECT ",
        tail: " FROM ;",
    },
    Template {
        label: "INSERT INTO <> (<>) VALUES (<>)",
        head: "INSERT INTO ",
        tail: " () VALUES ();",
    },
    Template {
        label: "UPDATE <> SET <> WHERE <>",
        head: "UPDATE ",
        tail: " SET  WHERE ;",
    },
    Template {
        label: "DELETE FROM <> WHERE <>",
        head: "DELETE FROM ",
        tail: " WHERE ;",
    },
    Template {
        label: "BEGIN BATCH <> APPLY BATCH",
        head: "BEGIN BATCH\n",
        tail: "\nAPPLY BATCH;",
    },
    Template {
        label: "CREATE KEYSPACE <> WITH replication = {…}",
        head: "CREATE KEYSPACE ",
        tail: " WITH replication = {'class': 'SimpleStrategy', 'replication_factor': 1};",
    },
    Template {
        label: "CREATE TABLE <> (<> PRIMARY KEY)",
        head: "CREATE TABLE ",
        tail: " ( PRIMARY KEY);",
    },
    Template {
        label: "ALTER TABLE <> ADD <> <>",
        head: "ALTER TABLE ",
        tail: " ADD  ;",
    },
    Template {
        label: "CREATE INDEX <> ON <> (<>)",
        head: "CREATE INDEX ",
        tail: " ON  ();",
    },
];

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cql::keywords::is_keyword;

    #[test]
    fn every_template_opens_with_a_keyword_a_prefix_can_reach() {
        for template in TEMPLATES {
            let keyword = template.keyword();

            assert!(is_keyword(keyword), "{keyword} is not grammar");
            assert!(
                template.label.starts_with(keyword),
                "{:?} is matched on {keyword} and must be seen to be",
                template.label
            );
        }
    }

    #[test]
    fn every_template_terminates_its_statement() {
        for template in TEMPLATES {
            assert!(
                template.text().ends_with(';'),
                "{:?} would run into whatever is typed after it",
                template.label
            );
        }
    }

    #[test]
    fn the_caret_lands_inside_the_text_it_was_split_from() {
        for template in TEMPLATES {
            assert!(template.caret() <= template.text().chars().count());
        }
    }

    /// The label draws holes because the text leaves them: a skeleton that
    /// typed everything it drew would be a keyword completion with a longer
    /// name.
    #[test]
    fn every_label_marks_a_hole_and_the_caret_lands_in_the_first_one() {
        for template in TEMPLATES {
            assert!(
                template.label.contains("<>"),
                "{:?} has nothing to fill",
                template.label
            );
            assert!(
                template.tail.starts_with([' ', '\n', ')', ';']),
                "{:?} would leave the caret mid-word",
                template.label
            );
        }
    }
}

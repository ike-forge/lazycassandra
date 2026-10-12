//! Writing the uncommitted edits: the statement they become, and the
//! confirmation that has to be answered before it runs
//! (docs/superpowers/specs/2026-09-18-cell-editing-design.md §5, §6).

use crate::contents::TableRef;
use crate::edit::CommitScope;
use crate::model::Statement;
use crate::state::Scroll;
use crate::style::Segment;

/// The open commit confirmation.
///
/// Answered with `y`/`n` alone: an `UPDATE` is a
/// [`crate::model::StatementKind::Write`], so the typed spelling the drop
/// confirmation raises on a protected connection does not apply (design §5).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommitPrompt {
    table: TableRef,
    statement: Statement,
    rows: usize,
    cells: usize,
    scope: CommitScope,
    /// The block scrolls: thirty edited rows is thirty-plus lines of CQL, and
    /// a confirmation that clips the statement it is asking about is worse
    /// than no preview at all (design §5).
    ///
    /// An offset with no cursor, unlike the inspector's: there is no line
    /// here to be *on*, only a body to read.
    scroll: Scroll,
}

impl CommitPrompt {
    #[must_use]
    pub fn opened(table: TableRef, statement: Statement, rows: usize, cells: usize) -> Self {
        Self {
            table,
            statement,
            rows,
            cells,
            scope: CommitScope::All,
            scroll: Scroll::default(),
        }
    }

    /// Narrows what answering `y` writes back into the page.
    #[must_use]
    pub fn scoped(self, scope: CommitScope) -> Self {
        Self { scope, ..self }
    }

    #[must_use]
    pub const fn scope(&self) -> CommitScope {
        self.scope
    }

    #[must_use]
    pub const fn statement(&self) -> &Statement {
        &self.statement
    }

    #[must_use]
    pub const fn table(&self) -> &TableRef {
        &self.table
    }

    /// The statement as the block draws it: laid out, coloured, one line
    /// each.
    ///
    /// Laid out for the preview alone — [`Self::statement`] keeps the CQL the
    /// builders spelled. [`crate::cql::format`] rewrites from a scan rather
    /// than a parse, and a scan it gets wrong rewrites a literal into an
    /// expression; what the user is asked to approve may be re-laid, what the
    /// cluster is sent may not (cell editing design §5).
    #[must_use]
    pub fn lines(&self) -> Vec<Vec<Segment>> {
        crate::cql::highlight(&crate::cql::format(&self.statement.cql))
    }

    #[must_use]
    pub const fn offset(&self) -> usize {
        self.scroll.offset()
    }

    pub const fn down(&mut self, lines: usize, height: usize) {
        self.scroll.down(lines, height);
    }

    pub const fn up(&mut self) {
        self.scroll.up();
    }

    pub const fn page_down(&mut self, lines: usize, height: usize) {
        self.scroll.page_down(lines, height);
    }

    pub const fn page_up(&mut self, height: usize) {
        self.scroll.page_up(height);
    }

    pub const fn to_start(&mut self) {
        self.scroll.to_start();
    }

    pub const fn to_end(&mut self, lines: usize, height: usize) {
        self.scroll.to_end(lines, height);
    }

    /// Pulls the offset back over a body that rewrapped shorter, as the
    /// inspector's does.
    pub const fn clamp_to(&mut self, lines: usize, height: usize) {
        self.scroll.clamp_to(lines, height);
    }

    /// What the border says it is about: the count first, in the colours the
    /// tree gives the table's name — the drop confirmation's rule, and its
    /// reason (drop design §2).
    #[must_use]
    pub fn title(&self) -> Vec<Segment> {
        let mut segments = vec![Segment::plain(format!(
            "Write {} {} in {} {} of ",
            self.cells,
            noun(self.cells, "cell", "cells"),
            self.rows,
            noun(self.rows, "row", "rows")
        ))];
        segments.extend(self.table.segments());
        segments
    }
}

/// The quit warning: what would be lost, and nothing else.
///
/// A variant of its own rather than a [`crate::state::Message`] with a flag:
/// the type says "answering this ends the process", which is the argument
/// `Overlay::Fatal` already makes (query log design §7).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Discard {
    cells: usize,
    rows: usize,
}

impl Discard {
    #[must_use]
    pub const fn of(cells: usize, rows: usize) -> Self {
        Self { cells, rows }
    }

    /// The sentence the block asks its question under.
    #[must_use]
    pub fn message(&self) -> String {
        format!(
            "{} {} in {} {} {} not been written to the cluster.",
            self.cells,
            noun(self.cells, "cell", "cells"),
            self.rows,
            noun(self.rows, "row", "rows"),
            noun(self.cells, "has", "have")
        )
    }
}

/// The noun for a count, so "1 rows" is never drawn — `drop`'s `rows`, over
/// two nouns.
fn noun(count: usize, one: &'static str, many: &'static str) -> &'static str {
    if count == 1 { one } else { many }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn prompt(rows: usize, cells: usize) -> CommitPrompt {
        CommitPrompt::opened(
            TableRef::new("shop", "orders"),
            Statement::update_row(
                "shop",
                "orders",
                &[("note".to_owned(), "'x'".to_owned())],
                &[("id".to_owned(), "7".to_owned())],
            ),
            rows,
            cells,
        )
    }

    #[test]
    fn the_title_counts_the_cells_and_the_rows_and_names_the_table() {
        assert_eq!(
            crate::style::text(&prompt(1, 1).title()),
            "Write 1 cell in 1 row of shop.orders"
        );
        assert_eq!(
            crate::style::text(&prompt(2, 3).title()),
            "Write 3 cells in 2 rows of shop.orders"
        );
    }

    fn drawn(prompt: &CommitPrompt) -> Vec<String> {
        prompt
            .lines()
            .iter()
            .map(|line| crate::style::text(line))
            .collect()
    }

    #[test]
    fn the_statement_is_laid_out_a_clause_to_a_line() {
        assert_eq!(
            drawn(&prompt(1, 1)),
            [
                "UPDATE \"shop\".\"orders\"",
                "SET \"note\" = 'x'",
                "WHERE \"id\" = 7"
            ]
        );
    }

    #[test]
    fn the_statement_is_coloured_as_cql() {
        let opening = prompt(1, 1).lines()[0][0].clone();

        assert_eq!(opening.text, "UPDATE");
        assert_eq!(opening.token, Some(crate::style::Token::Keyword));
    }

    #[test]
    fn a_batch_indents_its_members_under_the_brackets() {
        let batch = CommitPrompt::opened(
            TableRef::new("shop", "orders"),
            Statement::update_rows(
                "shop",
                "orders",
                &[
                    (
                        vec![("note".to_owned(), "'x'".to_owned())],
                        vec![("id".to_owned(), "7".to_owned())],
                    ),
                    (
                        vec![("note".to_owned(), "'y'".to_owned())],
                        vec![("id".to_owned(), "8".to_owned())],
                    ),
                ],
            ),
            2,
            2,
        );

        assert_eq!(
            drawn(&batch),
            [
                "BEGIN BATCH",
                "  UPDATE \"shop\".\"orders\"",
                "  SET \"note\" = 'x'",
                "  WHERE \"id\" = 7;",
                "  UPDATE \"shop\".\"orders\"",
                "  SET \"note\" = 'y'",
                "  WHERE \"id\" = 8;",
                "APPLY BATCH"
            ]
        );
    }

    #[test]
    fn the_warning_never_says_one_rows() {
        assert_eq!(
            Discard::of(1, 1).message(),
            "1 cell in 1 row has not been written to the cluster."
        );
        assert_eq!(
            Discard::of(3, 2).message(),
            "3 cells in 2 rows have not been written to the cluster."
        );
    }
}

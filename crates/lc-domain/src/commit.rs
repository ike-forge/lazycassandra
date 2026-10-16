//! Writing uncommitted edits: the statement and its confirmation.

use crate::contents::TableRef;
use crate::edit::CommitScope;
use crate::inspect::Inspector;
use crate::model::Statement;
use crate::style::Segment;
use crate::viewport::Scroll;

/// `y`/`n` only: an `UPDATE` is a `Write`, so no typed spelling.
#[derive(Debug, Clone, PartialEq)]
pub struct CommitPrompt {
    table: TableRef,
    statement: Statement,
    rows: usize,
    cells: usize,
    /// Handed back on `n` and `Esc`: a hand-filled form is not lost to a
    /// mistyped key.
    form: Option<Box<Inspector>>,
    scope: CommitScope,
    /// The block scrolls, since a clipped statement is worse than no preview.
    /// No cursor: nothing to be on.
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
            form: None,
            scope: CommitScope::All,
            scroll: Scroll::default(),
        }
    }

    #[must_use]
    pub fn inserting(table: TableRef, statement: Statement, form: Inspector) -> Self {
        let cells = form.typed().count();

        Self {
            form: Some(Box::new(form)),
            ..Self::opened(table, statement, 1, cells)
        }
    }

    /// `None` for a commit of edits.
    pub fn take_form(&mut self) -> Option<Inspector> {
        self.form.take().map(|form| *form)
    }

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

    /// Formatted for display only: [`Self::statement`] keeps the builders' CQL,
    /// since a misread scan could rewrite a literal.
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

    pub const fn clamp_to(&mut self, lines: usize, height: usize) {
        self.scroll.clamp_to(lines, height);
    }

    #[must_use]
    pub fn title(&self) -> Vec<Segment> {
        let mut segments = vec![Segment::plain(if self.form.is_some() {
            format!(
                "Insert {} {} into ",
                self.cells,
                noun(self.cells, "cell", "cells")
            )
        } else {
            format!(
                "Write {} {} in {} {} of ",
                self.cells,
                noun(self.cells, "cell", "cells"),
                self.rows,
                noun(self.rows, "row", "rows")
            )
        })];
        segments.extend(self.table.segments());
        segments
    }
}

/// Not a `Message`: answering it ends the process.
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

/// Never "1 rows".
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
    fn an_insert_names_the_cells_and_the_table_and_hands_the_form_back() {
        let form = Inspector::inserting(
            crate::contents::Subject::Table(TableRef::new("shop", "orders")),
            vec!["id".to_owned()],
        );
        let mut prompt = CommitPrompt::inserting(
            TableRef::new("shop", "orders"),
            Statement::insert_row("shop", "orders", &[("id".to_owned(), "7".to_owned())]),
            form.clone(),
        );

        assert_eq!(
            crate::style::text(&prompt.title()),
            "Insert 0 cells into shop.orders"
        );
        assert_eq!(prompt.take_form(), Some(form));
        assert_eq!(prompt.take_form(), None);
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

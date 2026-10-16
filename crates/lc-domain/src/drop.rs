//! Dropping a keyspace or table, or deleting rows, behind a confirmation.

use crate::contents::TableRef;
use crate::key::{self, RowKey};
use crate::line::Line;
use crate::model::{KeyspaceKind, Row, Statement, Table};
use crate::posture::Posture;
use crate::style::{Segment, Token};

/// Only what a cursor can sit on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    Keyspace(String),
    Table(TableRef),
    /// Never empty: the grid's cursor is always on a row.
    Rows {
        table: TableRef,
        keys: Vec<RowKey>,
    },
}

impl Target {
    /// `None` where any row lacks an addressable primary key. `schema_table`
    /// supplies the key: the page only has the columns a `SELECT` returned.
    #[must_use]
    pub fn rows<'a>(
        table: &TableRef,
        schema_table: &Table,
        columns: &[String],
        selected: impl Iterator<Item = &'a Row>,
    ) -> Option<Self> {
        let key = key::columns(schema_table);

        if key.is_empty() {
            return None;
        }

        let keys: Vec<RowKey> = selected
            .map(|row| RowKey::of(&key, columns, row))
            .collect::<Option<Vec<_>>>()?;

        (!keys.is_empty()).then(|| Self::Rows {
            table: table.clone(),
            keys,
        })
    }

    #[must_use]
    pub fn row_count(&self) -> Option<usize> {
        match self {
            Self::Rows { keys, .. } => Some(keys.len()),
            Self::Keyspace(_) | Self::Table(_) => None,
        }
    }

    #[must_use]
    pub fn statement(&self) -> Statement {
        match self {
            Self::Keyspace(keyspace) => Statement::drop_keyspace(keyspace),
            Self::Table(table) => Statement::drop_table(&table.keyspace, &table.table),
            Self::Rows { table, keys } => {
                let pairs: Vec<Vec<(String, String)>> =
                    keys.iter().map(|key| key.pairs().to_vec()).collect();

                Statement::delete_rows(&table.keyspace, &table.table, &pairs)
            }
        }
    }

    /// Verbatim: `system_schema` reports true-case names, so typing what is on
    /// screen is the rule.
    #[must_use]
    pub fn name(&self) -> &str {
        match self {
            Self::Keyspace(keyspace) => keyspace,
            Self::Table(table) | Self::Rows { table, .. } => &table.table,
        }
    }

    #[must_use]
    pub fn qualified(&self) -> String {
        match self {
            Self::Keyspace(keyspace) => keyspace.clone(),
            Self::Table(table) => table.qualified(),
            Self::Rows { table, keys } => {
                format!(
                    "{} {} from {}",
                    keys.len(),
                    rows(keys.len()),
                    table.qualified()
                )
            }
        }
    }

    #[must_use]
    pub fn title(&self) -> Vec<Segment> {
        match self {
            Self::Keyspace(keyspace) => {
                let token = match KeyspaceKind::of(keyspace) {
                    KeyspaceKind::System => Token::Faint,
                    KeyspaceKind::User => Token::Keyspace,
                };

                vec![
                    Segment::plain("Drop keyspace "),
                    Segment::styled(keyspace.clone(), token),
                ]
            }
            Self::Table(table) => {
                let mut segments = vec![Segment::plain("Drop table ")];
                segments.extend(table.segments());
                segments
            }
            // The count is in the border: it is what changes the cost of
            // answering.
            Self::Rows { table, keys } => {
                let mut segments = vec![Segment::plain(format!(
                    "Delete {} {} from ",
                    keys.len(),
                    rows(keys.len())
                ))];
                segments.extend(table.segments());
                segments
            }
        }
    }
}

/// Never "1 rows".
fn rows(count: usize) -> &'static str {
    if count == 1 { "row" } else { "rows" }
}

/// "Typed, and also answerable with `y`" is unrepresentable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Answer {
    YesNo,
    Typed(Line),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DropPrompt {
    target: Target,
    answer: Answer,
}

impl DropPrompt {
    /// A keyspace is always typed; a protected connection raises a table to the
    /// same bar.
    #[must_use]
    pub fn opened(target: Target, posture: Posture) -> Self {
        // Read off the statement: `Protected` is about destructive statements,
        // and a row delete is a `Write`.
        let destructive = target.statement().kind.is_destructive();
        let typed = matches!(target, Target::Keyspace(_))
            || (posture.requires_confirmation() && destructive);
        let answer = if typed {
            Answer::Typed(Line::default())
        } else {
            Answer::YesNo
        };

        Self { target, answer }
    }

    #[must_use]
    pub const fn target(&self) -> &Target {
        &self.target
    }

    #[must_use]
    pub const fn answer(&self) -> &Answer {
        &self.answer
    }

    #[must_use]
    pub const fn typed(&self) -> Option<&Line> {
        match &self.answer {
            Answer::Typed(line) => Some(line),
            Answer::YesNo => None,
        }
    }

    pub const fn typed_mut(&mut self) -> Option<&mut Line> {
        match &mut self.answer {
            Answer::Typed(line) => Some(line),
            Answer::YesNo => None,
        }
    }

    /// Always `false` for [`Answer::YesNo`].
    #[must_use]
    pub fn confirmed(&self) -> bool {
        self.typed()
            .is_some_and(|line| line.text() == self.target.name())
    }

    #[must_use]
    pub const fn takes_text(&self) -> bool {
        matches!(self.answer, Answer::Typed(_))
    }

    #[must_use]
    pub fn statement(&self) -> Statement {
        self.target.statement()
    }

    #[must_use]
    pub fn title(&self) -> Vec<Segment> {
        self.target.title()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::style;

    fn keyspace() -> Target {
        Target::Keyspace("shop".to_owned())
    }

    fn table() -> Target {
        Target::Table(TableRef::new("shop", "orders"))
    }

    fn typing(prompt: &mut DropPrompt, text: &str) {
        let line = prompt.typed_mut().expect("a typed confirmation");
        for character in text.chars() {
            line.insert(character);
        }
    }

    #[test]
    fn a_keyspace_is_always_typed_back() {
        for posture in [Posture::Writable, Posture::Protected] {
            assert!(DropPrompt::opened(keyspace(), posture).takes_text());
        }
    }

    #[test]
    fn a_table_is_yes_or_no_until_the_posture_is_protected() {
        assert_eq!(
            DropPrompt::opened(table(), Posture::Writable).answer(),
            &Answer::YesNo
        );
        assert!(DropPrompt::opened(table(), Posture::Protected).takes_text());
    }

    #[test]
    fn a_typed_confirmation_runs_only_on_the_name_it_asked_for() {
        let mut prompt = DropPrompt::opened(keyspace(), Posture::Writable);
        assert!(!prompt.confirmed(), "an empty field confirms nothing");

        typing(&mut prompt, "sho");
        assert!(!prompt.confirmed());

        typing(&mut prompt, "p");
        assert!(prompt.confirmed());
    }

    #[test]
    fn a_name_is_matched_verbatim_rather_than_folded() {
        // The field asks for what the tree draws, not the folded name.
        let mut prompt = DropPrompt::opened(Target::Keyspace("MyKs".to_owned()), Posture::Writable);
        typing(&mut prompt, "myks");

        assert!(!prompt.confirmed());
    }

    #[test]
    fn a_yes_no_confirmation_is_never_confirmed_by_a_line_it_does_not_have() {
        assert!(!DropPrompt::opened(table(), Posture::Writable).confirmed());
    }

    #[test]
    fn a_table_drops_only_the_table_and_a_keyspace_the_whole_keyspace() {
        assert_eq!(table().statement().cql, "DROP TABLE \"shop\".\"orders\"");
        assert_eq!(keyspace().statement().cql, "DROP KEYSPACE \"shop\"");
    }

    #[test]
    fn the_title_names_the_node_the_tree_was_standing_on() {
        assert_eq!(style::text(&table().title()), "Drop table shop.orders");
        assert_eq!(style::text(&keyspace().title()), "Drop keyspace shop");
    }
}

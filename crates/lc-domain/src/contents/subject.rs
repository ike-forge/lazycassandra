use crate::model::KeyspaceKind;
use crate::style::{Segment, Token};

/// Owned: a result outlives the schema reload that dropped its table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TableRef {
    pub keyspace: String,
    pub table: String,
}

impl TableRef {
    #[must_use]
    pub fn new(keyspace: &str, table: &str) -> Self {
        Self {
            keyspace: keyspace.to_owned(),
            table: table.to_owned(),
        }
    }

    #[must_use]
    pub fn qualified(&self) -> String {
        format!("{}.{}", self.keyspace, self.table)
    }

    /// A system keyspace is faint on both halves, as in the tree.
    #[must_use]
    pub fn segments(&self) -> Vec<Segment> {
        let (keyspace, table) = match KeyspaceKind::of(&self.keyspace) {
            KeyspaceKind::System => (Token::Faint, Token::Faint),
            KeyspaceKind::User => (Token::Keyspace, Token::Table),
        };

        vec![
            Segment::styled(self.keyspace.clone(), keyspace),
            Segment::styled(".", Token::Faint),
            Segment::styled(self.table.clone(), table),
        ]
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Subject {
    Table(TableRef),
    Typed(String),
}

impl Subject {
    /// Whitespace runs collapse, so a multi-line buffer titles in one line.
    #[must_use]
    pub fn label(&self) -> String {
        match self {
            Self::Table(table) => table.qualified(),
            Self::Typed(cql) => cql.split_whitespace().collect::<Vec<_>>().join(" "),
        }
    }

    /// `None` for a typed statement, whose columns belong to no one table.
    #[must_use]
    pub const fn table(&self) -> Option<&TableRef> {
        match self {
            Self::Table(table) => Some(table),
            Self::Typed(_) => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_qualified_name_reads_the_way_cql_writes_it() {
        assert_eq!(TableRef::new("shop", "orders").qualified(), "shop.orders");
    }

    #[test]
    fn a_table_subject_is_labelled_by_its_qualified_name() {
        assert_eq!(
            Subject::Table(TableRef::new("shop", "orders")).label(),
            "shop.orders"
        );
    }

    #[test]
    fn a_typed_subject_collapses_to_one_line() {
        assert_eq!(
            Subject::Typed("SELECT *\n  FROM shop.orders\nLIMIT 10".to_owned()).label(),
            "SELECT * FROM shop.orders LIMIT 10"
        );
    }

    #[test]
    fn only_a_table_subject_names_a_table() {
        assert_eq!(
            Subject::Table(TableRef::new("shop", "orders")).table(),
            Some(&TableRef::new("shop", "orders"))
        );
        assert_eq!(Subject::Typed("SELECT 1".to_owned()).table(), None);
    }
}

//! Semantic style tokens.
//!
//! The domain never names a colour. It names a *meaning*, and the theme in
//! `lc-inbound` decides how to draw it (architecture.md §6). This is what lets
//! the default theme inherit the terminal's own palette.

use crate::posture::Posture;

/// A thing worth distinguishing visually.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Token {
    PartitionKey,
    ClusteringKey,
    RegularColumn,
    Posture(Posture),
    Severity(Severity),
    Muted,
    /// The panel accepting input.
    Focused,
    /// The key that focuses a panel, inside that panel's title.
    Shortcut,
    /// A key the focused panel responds to, in the hint bar.
    HintKey,
    /// The name of a table option or replication setting, in the Table DDL
    /// panel. Named for what it marks rather than for the colour it gets: the
    /// domain still names no colour (architecture.md §6).
    PropertyName,
    /// What that key does.
    HintAction,
    /// The row the cursor is on.
    Selected,
    /// A cell whose text answered the applied filter.
    ///
    /// Named for what it marks, like [`Token::PropertyName`]: the domain
    /// still names no colour (architecture.md §6).
    Match,
    /// The cell the cursor is on, drawn over the selected row: the row says
    /// where you are, the cell says what a command would act on
    /// (doc/specs/2026-08-10-table-display-design.md §3).
    SelectedCell,
    /// A keyspace the user made, in the Tables tree.
    Keyspace,
    /// A table the user made, in the Tables tree.
    Table,
    /// A CQL keyword.
    ///
    /// Defined and themed but not yet emitted: there is no highlighter. A
    /// theme is not complete without it (doc/specs/2026-08-10-themes-design.md
    /// §3).
    Keyword,
    /// A keyspace, table or column name inside a statement.
    Reference,
    /// A string or numeric literal inside a statement.
    Literal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    Info,
    Warning,
    Danger,
}

//! Semantic style tokens.
//!
//! The domain never names a colour. It names a *meaning*, and the theme in
//! `lc-inbound` decides how to draw it (architecture.md §6). This is what lets
//! the default theme inherit the terminal's own palette.

use crate::posture::Posture;

/// A run of text drawn in one token.
///
/// `None` is the run whatever style the caller is drawing in covers — a
/// panel's name is the panel's name, in whatever colour focus gives it. A
/// `Some` is a run that carries a meaning of its own: a schema name reads as
/// one wherever it is written, and the tree, the DDL panel and a title all
/// spend the same two colours on a keyspace and a table (vision.md §2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Segment {
    pub text: String,
    pub token: Option<Token>,
}

impl Segment {
    #[must_use]
    pub fn plain(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            token: None,
        }
    }

    #[must_use]
    pub fn styled(text: impl Into<String>, token: Token) -> Self {
        Self {
            text: text.into(),
            token: Some(token),
        }
    }
}

/// Runs read as one string — what a border is measured against, and what a
/// test asserts a title says.
#[must_use]
pub fn text(segments: &[Segment]) -> String {
    segments
        .iter()
        .map(|segment| segment.text.as_str())
        .collect()
}

/// A thing worth distinguishing visually.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Token {
    PartitionKey,
    ClusteringKey,
    /// A value in a partition key column, drawn in that column's colour so
    /// the key can be read off the rows themselves (vision.md §2).
    ///
    /// A token of its own rather than [`Token::PartitionKey`] reused: a
    /// heading is one word and can carry weight, while a column of bold
    /// values would take the emphasis a matched cell spends on the row the
    /// cursor is on.
    PartitionKeyValue,
    /// [`Token::PartitionKeyValue`]'s counterpart for a clustering column.
    ClusteringKeyValue,
    /// A column stored once per partition rather than once per row. Worth a
    /// colour because static-ness changes what a write to it means, and the
    /// Table columns tab is the one place every column is listed together
    /// (docs/superpowers/specs/2026-08-12-ddl-tabs-design.md §5).
    StaticColumn,
    RegularColumn,
    Posture(Posture),
    Severity(Severity),
    /// One step below body text: option names, column headers, footer
    /// actions. [`Token::Faint`] is the step below that.
    Muted,
    /// Two steps below body text: system keyspaces, nulls, separators,
    /// counts, and a hint the focused panel does not answer.
    Faint,
    /// A heading that names no schema concept — Options, Indexes. Named for
    /// what it marks rather than for the colour it gets (architecture.md §6).
    Section,
    /// The one-cell mark in the gutter of the row the cursor is on.
    SelectionBar,
    /// A comment inside a statement.
    ///
    /// Defined and themed but not yet emitted: there is no highlighter, the
    /// status [`Token::Keyword`] already has.
    Comment,
    /// The panel accepting input.
    Focused,
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
    /// The span of text a search matched, painted where it sits rather than
    /// hiding the rest of the row — in Tables, Table DDL, and Table contents
    /// alike.
    ///
    /// Named for what it marks, like [`Token::PropertyName`]: the domain
    /// still names no colour (architecture.md §6).
    Match,
    /// The cell the cursor is on, drawn over the selected row: the row says
    /// where you are, the cell says what a command would act on
    /// (docs/superpowers/specs/2026-08-10-table-display-design.md §3).
    SelectedCell,
    /// The faint wash across a selected cell's row and column, so a cell in
    /// the middle of a wide grid can be traced back to its heading and its row
    /// number (docs/superpowers/specs/2026-08-13-selection-modes-design.md §4).
    Crosshair,
    /// A row inside the selection the cursor is not on.
    ///
    /// A band of its own between [`Token::Selected`] and [`Token::Crosshair`]:
    /// the cursor has to stay findable inside a range, and the range has to be
    /// louder than the wash that merely traces a cell back to its heading
    /// (docs/superpowers/specs/2026-08-14-visual-mode-design.md §6).
    Ranged,
    /// What a yank took, in the frames the afterglow lasts.
    ///
    /// Inverted rather than banded, like [`Token::SelectedCell`] and for the
    /// same reason: it has to read *against* whatever band was standing there,
    /// and [`Token::Selected`] — the loudest colour a palette spends — is only
    /// one step above [`Token::Ranged`], which is a step too quiet to read as
    /// feedback. Swapping ground for text needs nothing new from a ported
    /// palette (docs/superpowers/specs/2026-08-14-visual-mode-design.md §6.1).
    Flash,
    /// A cell typed into and not yet written to the cluster.
    ///
    /// Named for what it marks rather than for the colour it gets, like
    /// [`Token::Marker`], and mapped to a role a palette already declares:
    /// an uncommitted value is a claim about the screen that the cluster has
    /// not agreed to yet, which is what the warning colour already says
    /// (docs/superpowers/specs/2026-09-18-cell-editing-design.md §7).
    Dirty,
    /// A sort or filter mark over a grid heading, while it is in force.
    ///
    /// Named for what it marks rather than for the colour it gets, like
    /// [`Token::Match`]: the mark says this column is arranging what the grid
    /// shows, and the theme decides how loudly. A mark that is *not* in force
    /// is drawn [`Token::Faint`] — it is an affordance rather than a fact
    /// about the page
    /// (docs/superpowers/specs/2026-08-31-grid-sort-filter-design.md §5).
    Marker,
    /// A keyspace the user made, in the Tables tree.
    Keyspace,
    /// A table the user made, in the Tables tree.
    Table,
    /// A CQL keyword.
    ///
    /// Defined and themed but not yet emitted: there is no highlighter. A
    /// theme is not complete without it (docs/superpowers/specs/2026-08-10-themes-design.md
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

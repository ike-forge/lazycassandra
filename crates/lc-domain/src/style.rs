//! Semantic style tokens. The domain names a meaning, never a colour;
//! `lc-inbound`'s theme draws it (architecture.md §6).

use crate::posture::Posture;

/// `None` takes whatever style the caller draws in; `Some` carries its own
/// meaning wherever it is written.
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

#[must_use]
pub fn text(segments: &[Segment]) -> String {
    segments
        .iter()
        .map(|segment| segment.text.as_str())
        .collect()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Token {
    PartitionKey,
    ClusteringKey,
    /// Apart from [`Token::PartitionKey`]: a bold column of values would steal
    /// the emphasis a matched cell spends.
    PartitionKeyValue,
    ClusteringKeyValue,
    /// Static-ness changes what a write means.
    StaticColumn,
    RegularColumn,
    Posture(Posture),
    Severity(Severity),
    /// [`Token::Faint`] is one step below.
    Muted,
    /// Also a hint the focused panel does not answer.
    Faint,
    Section,
    /// The one-cell gutter mark of the cursor's row.
    SelectionBar,
    Comment,
    Focused,
    HintKey,
    PropertyName,
    HintAction,
    Selected,
    Match,
    /// The row says where you are, the cell what a command acts on.
    SelectedCell,
    /// Traces a cell in a wide grid back to its heading and row number.
    Crosshair,
    /// Between [`Token::Selected`] and [`Token::Crosshair`]: the cursor stays
    /// findable inside a range, which stays louder than the wash.
    Ranged,
    /// Inverted, like [`Token::SelectedCell`]: one step above [`Token::Ranged`]
    /// is too quiet to read as feedback.
    Flash,
    /// Mapped to the warning role, worn as a ground so the cell is found in a
    /// full grid.
    Dirty,
    /// Only while in force; an idle mark is [`Token::Faint`].
    Marker,
    Keyspace,
    Table,
    Keyword,
    Reference,
    Literal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    Info,
    Warning,
    Danger,
}

//! CQL statements.
//!
//! Every action the tool takes is expressible as CQL and is shown before it
//! runs (vision.md §4, principle 3), so a statement is a first-class value
//! that can be previewed, logged, and copied — not a string assembled at the
//! point of execution.

/// A statement plus what it is allowed to do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Statement {
    pub cql: String,
    pub kind: StatementKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatementKind {
    Read,
    Write,
    /// `DROP`, `TRUNCATE`, and friends. Gated by posture *and* by typed
    /// confirmation (vision.md §4, principle 6).
    Destructive,
    Schema,
}

impl StatementKind {
    /// Whether executing this changes anything on the cluster.
    #[must_use]
    pub const fn mutates(self) -> bool {
        matches!(self, Self::Write | Self::Destructive | Self::Schema)
    }

    #[must_use]
    pub const fn is_destructive(self) -> bool {
        matches!(self, Self::Destructive)
    }
}

impl Statement {
    #[must_use]
    pub const fn mutates(&self) -> bool {
        self.kind.mutates()
    }
}

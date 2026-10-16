//! Write posture, and the rule that gates mutation.

use serde::{Deserialize, Serialize};

/// Defaults to [`Posture::ReadOnly`]: a connection that never states its
/// posture cannot be written to (vision.md §6).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Posture {
    #[default]
    ReadOnly,
    Writable,
    /// Destructive statements need typed confirmation.
    Protected,
}

impl Posture {
    /// Enforced in [`crate::update`]: the TUI constructs no effects, so it
    /// cannot route around this.
    #[must_use]
    pub const fn permits_mutation(self) -> bool {
        matches!(self, Self::Writable | Self::Protected)
    }

    #[must_use]
    pub const fn requires_confirmation(self) -> bool {
        matches!(self, Self::Protected)
    }

    /// The config spelling, also what the picker shows, so the user learns a
    /// value that parses.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::ReadOnly => "read-only",
            Self::Writable => "writable",
            Self::Protected => "protected",
        }
    }
}

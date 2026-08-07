//! Write posture, and the rule that gates mutation.

use serde::{Deserialize, Serialize};

/// What a connection is permitted to do.
///
/// The default is [`Posture::ReadOnly`]: a connection that never states its
/// posture cannot be written to (vision.md §6).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Posture {
    #[default]
    ReadOnly,
    Writable,
    /// Writable, but destructive statements require typed confirmation.
    Protected,
}

impl Posture {
    /// Whether a mutating statement may be turned into an effect at all.
    ///
    /// Enforced in [`crate::update`], not in the TUI — the inbound adapter
    /// does not construct effects, so it cannot route around this.
    #[must_use]
    pub const fn permits_mutation(self) -> bool {
        matches!(self, Self::Writable | Self::Protected)
    }

    /// Whether a destructive statement additionally needs typed confirmation.
    #[must_use]
    pub const fn requires_confirmation(self) -> bool {
        matches!(self, Self::Protected)
    }
}

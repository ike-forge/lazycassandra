//! The whole application state.
//!
//! One struct for now. architecture.md §11 flags that this may need to become
//! nested per-panel state with message routing once Phase 1 has five real
//! panels; deferring until that shape is known rather than guessing at it.

use std::collections::BTreeMap;

use crate::model::{ConnId, QueryId, Schema, Statement};
use crate::posture::Posture;

#[derive(Debug, Clone, Default)]
pub struct State {
    pub connection: Option<Connection>,
    pub focus: Panel,
    /// Queries issued and not yet resolved. A result whose id is absent here
    /// was cancelled, and is dropped rather than applied.
    pub in_flight: BTreeMap<QueryId, Statement>,
    /// Set when something changed that the next frame must show. Avoids
    /// repainting on every keypress of a held-down `j` (architecture.md §3).
    pub dirty: bool,
    pub should_quit: bool,
}

#[derive(Debug, Clone)]
pub struct Connection {
    pub id: ConnId,
    pub posture: Posture,
    pub schema: Option<Schema>,
}

/// Panels are places: fixed positions, navigated with predictable motions
/// (vision.md §4, principle 4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Panel {
    #[default]
    Keyspaces,
    Tables,
    Editor,
    Results,
    Log,
}

impl State {
    /// Posture of the current connection, or the safe default when there is
    /// none.
    #[must_use]
    pub fn posture(&self) -> Posture {
        self.connection
            .as_ref()
            .map_or(Posture::ReadOnly, |c| c.posture)
    }

    pub const fn mark_dirty(&mut self) {
        self.dirty = true;
    }
}

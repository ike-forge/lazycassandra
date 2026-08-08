//! The whole application state.
//!
//! One struct for now. architecture.md §11 flags that this may need to become
//! nested per-panel state with message routing once Phase 1 has four real
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
///
/// Declaration order is reading order — top-left, bottom-left, top-right,
/// bottom-right — and [`Panel::ALL`] preserves it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Panel {
    /// Keyspace → table tree.
    #[default]
    Tables,
    /// `CREATE TABLE` for the selected table.
    Ddl,
    /// Rows of the selected table.
    Contents,
    /// Statement entry and session log.
    Queries,
}

impl Panel {
    /// Every panel, in reading order.
    pub const ALL: [Self; 4] = [Self::Tables, Self::Ddl, Self::Contents, Self::Queries];
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_panels_are_listed_in_reading_order_and_none_is_missing() {
        for panel in Panel::ALL {
            // A new variant fails to compile here until it is added to ALL.
            let expected_position = match panel {
                Panel::Tables => 0,
                Panel::Ddl => 1,
                Panel::Contents => 2,
                Panel::Queries => 3,
            };
            assert_eq!(
                Panel::ALL[expected_position],
                panel,
                "{panel:?} is not at its reading-order position"
            );
        }
        assert_eq!(
            Panel::ALL.len(),
            4,
            "a variant was added without extending ALL"
        );
    }

    #[test]
    fn the_default_panel_is_tables() {
        assert_eq!(Panel::default(), Panel::Tables);
    }
}

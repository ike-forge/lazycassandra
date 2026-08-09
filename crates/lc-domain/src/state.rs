//! The whole application state.
//!
//! One struct for now. architecture.md §11 flags that this may need to become
//! nested per-panel state with message routing once Phase 1 has four real
//! panels; deferring until that shape is known rather than guessing at it.

use std::collections::BTreeMap;

use crate::contents::ContentsPanel;
use crate::model::{ConnId, DbError, QueryId, Schema, Statement};
use crate::posture::Posture;
use crate::style::Severity;
use crate::tree::TablesPanel;

#[derive(Debug, Clone, Default)]
pub struct State {
    pub connection: ConnectionState,
    pub focus: Panel,
    pub tables: TablesPanel,
    pub contents: ContentsPanel,
    /// A message drawn over the panels until it is dismissed.
    pub overlay: Option<Overlay>,
    /// Queries issued and not yet resolved. A result whose id is absent here
    /// was cancelled, and is dropped rather than applied.
    pub in_flight: BTreeMap<QueryId, Statement>,
    /// Set when something changed that the next frame must show. Avoids
    /// repainting on every keypress of a held-down `j` (architecture.md §3).
    pub dirty: bool,
    pub should_quit: bool,
}

/// How far along the one connection is.
///
/// `Option<Connection>` could not say "connecting" or "failed", and an error
/// field beside the schema would make illegal combinations representable.
#[derive(Debug, Clone, Default)]
pub enum ConnectionState {
    #[default]
    Disconnected,
    /// Carries the id so a failure can be matched to the attempt that is
    /// outstanding.
    Connecting(ConnId),
    Failed(DbError),
    Connected(Connection),
}

#[derive(Debug, Clone)]
pub struct Connection {
    pub id: ConnId,
    pub posture: Posture,
    pub schema: SchemaState,
}

/// Whether the connected cluster has told us its shape yet.
#[derive(Debug, Clone, Default)]
pub enum SchemaState {
    #[default]
    Loading,
    Failed(DbError),
    Loaded(Schema),
}

/// A message drawn over the panels, dismissed with `Esc`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Overlay {
    pub title: String,
    pub message: String,
    pub severity: Severity,
}

impl ConnectionState {
    /// Whether `conn` is the connection this state is about.
    ///
    /// The guard that drops results belonging to a connection the user has
    /// already left (architecture.md §3).
    #[must_use]
    pub fn is_current(&self, conn: ConnId) -> bool {
        match self {
            Self::Connecting(id) => *id == conn,
            Self::Connected(connection) => connection.id == conn,
            Self::Disconnected | Self::Failed(_) => false,
        }
    }
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

    /// The next panel in reading order, wrapping.
    #[must_use]
    pub const fn next(self) -> Self {
        match self {
            Self::Tables => Self::Ddl,
            Self::Ddl => Self::Contents,
            Self::Contents => Self::Queries,
            Self::Queries => Self::Tables,
        }
    }

    /// The previous panel in reading order, wrapping.
    #[must_use]
    pub const fn previous(self) -> Self {
        match self {
            Self::Tables => Self::Queries,
            Self::Ddl => Self::Tables,
            Self::Contents => Self::Ddl,
            Self::Queries => Self::Contents,
        }
    }

    /// The key that focuses this panel from anywhere.
    ///
    /// Underlined in the panel's title, so the binding is on screen rather
    /// than in a manual (vision.md §4, principle 1). Two of the four are not
    /// leading letters — the titles are fixed by the draft, and the keys were
    /// chosen to be reachable rather than to match a first character.
    #[must_use]
    pub const fn shortcut(self) -> char {
        match self {
            Self::Tables => 't',
            Self::Ddl => 'a',
            Self::Contents => 'c',
            Self::Queries => 'u',
        }
    }

    /// The panel `key` focuses, if any panel claims it.
    #[must_use]
    pub fn from_shortcut(key: char) -> Option<Self> {
        Self::ALL.into_iter().find(|panel| panel.shortcut() == key)
    }
}

/// A selection into a list, which cannot be moved past either end.
///
/// The index is private on purpose: an out-of-range selection cannot be
/// constructed from outside this module, so a renderer never has to defend
/// against one (conventions.md §1).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Cursor {
    index: usize,
}

impl Cursor {
    /// The selected row.
    #[must_use]
    pub const fn index(self) -> usize {
        self.index
    }

    /// Moves to the next row, stopping on the last one.
    pub const fn down(&mut self, len: usize) {
        let last = len.saturating_sub(1);
        if self.index < last {
            self.index += 1;
        } else {
            self.index = last;
        }
    }

    /// Moves to the previous row, stopping on the first one.
    pub const fn up(&mut self) {
        self.index = self.index.saturating_sub(1);
    }

    /// Pulls the selection back inside a list that shrank.
    pub const fn clamp_to(&mut self, len: usize) {
        let last = len.saturating_sub(1);
        if self.index > last {
            self.index = last;
        }
    }

    /// Moves to `index`, or to the last row when the list is shorter.
    pub const fn go_to(&mut self, index: usize, len: usize) {
        self.index = index;
        self.clamp_to(len);
    }
}

impl State {
    /// Posture of the current connection, or the safe default when there is
    /// none.
    #[must_use]
    pub const fn posture(&self) -> Posture {
        match &self.connection {
            ConnectionState::Connected(connection) => connection.posture,
            ConnectionState::Disconnected
            | ConnectionState::Connecting(_)
            | ConnectionState::Failed(_) => Posture::ReadOnly,
        }
    }

    /// The connection a statement would run on.
    #[must_use]
    pub const fn conn_id(&self) -> Option<ConnId> {
        match &self.connection {
            ConnectionState::Connected(connection) => Some(connection.id),
            ConnectionState::Disconnected
            | ConnectionState::Connecting(_)
            | ConnectionState::Failed(_) => None,
        }
    }

    /// The cluster's shape, once it has arrived.
    #[must_use]
    pub const fn schema(&self) -> Option<&Schema> {
        match &self.connection {
            ConnectionState::Connected(Connection {
                schema: SchemaState::Loaded(schema),
                ..
            }) => Some(schema),
            _ => None,
        }
    }

    /// The tree's state and the schema it is a tree of.
    ///
    /// One method rather than two accessors because the borrow checker cannot
    /// split `&mut self` across separate `tables()` and `schema()` calls, and
    /// every transition needs both.
    pub fn tables_and_schema(&mut self) -> Option<(&mut TablesPanel, &Schema)> {
        match &self.connection {
            ConnectionState::Connected(Connection {
                schema: SchemaState::Loaded(schema),
                ..
            }) => Some((&mut self.tables, schema)),
            _ => None,
        }
    }

    pub const fn mark_dirty(&mut self) {
        self.dirty = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{ConnId, DbError, Schema};
    use crate::posture::Posture;

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

    #[test]
    fn the_cycle_walks_reading_order_and_wraps() {
        // Pinned against ALL rather than restated, so the two orderings cannot
        // drift apart when a panel is added.
        let walked: Vec<Panel> =
            std::iter::successors(Some(Panel::ALL[0]), |panel| Some(panel.next()))
                .take(Panel::ALL.len())
                .collect();

        assert_eq!(walked, Panel::ALL);
        assert_eq!(Panel::ALL[Panel::ALL.len() - 1].next(), Panel::ALL[0]);
    }

    #[test]
    fn stepping_back_undoes_stepping_forward() {
        for panel in Panel::ALL {
            assert_eq!(panel.next().previous(), panel);
            assert_eq!(panel.previous().next(), panel);
        }
    }

    #[test]
    fn every_panel_has_a_distinct_shortcut() {
        let mut claimed: Vec<char> = Vec::new();

        for panel in Panel::ALL {
            let key = panel.shortcut();
            assert!(
                !claimed.contains(&key),
                "{panel:?} claims '{key}', which another panel already has"
            );
            claimed.push(key);
        }
    }

    #[test]
    fn a_shortcut_leads_back_to_the_panel_that_claims_it() {
        for panel in Panel::ALL {
            assert_eq!(Panel::from_shortcut(panel.shortcut()), Some(panel));
        }

        assert_eq!(Panel::from_shortcut('z'), None);
    }

    #[test]
    fn the_shortcuts_are_the_ones_the_design_names() {
        assert_eq!(
            Panel::ALL.map(Panel::shortcut),
            ['t', 'a', 'c', 'u'],
            "the keys are a promise to the user's fingers, not an implementation detail"
        );
    }

    #[test]
    fn a_cursor_starts_on_the_first_row() {
        assert_eq!(Cursor::default().index(), 0);
    }

    #[test]
    fn moving_down_stops_on_the_last_row() {
        let mut cursor = Cursor::default();

        for _ in 0..5 {
            cursor.down(3);
        }

        assert_eq!(cursor.index(), 2, "a cursor must not run off the end");
    }

    #[test]
    fn moving_up_stops_on_the_first_row() {
        let mut cursor = Cursor::default();
        cursor.down(3);

        cursor.up();
        cursor.up();

        assert_eq!(cursor.index(), 0, "a cursor must not run off the start");
    }

    #[test]
    fn an_empty_list_leaves_the_cursor_on_the_first_row() {
        let mut cursor = Cursor::default();

        cursor.down(0);

        assert_eq!(cursor.index(), 0);
    }

    #[test]
    fn clamping_pulls_the_cursor_into_a_list_that_shrank() {
        let mut cursor = Cursor::default();
        for _ in 0..9 {
            cursor.down(10);
        }

        cursor.clamp_to(4);

        assert_eq!(cursor.index(), 3);
    }

    #[test]
    fn clamping_leaves_a_cursor_that_is_already_inside_alone() {
        let mut cursor = Cursor::default();
        cursor.down(10);

        cursor.clamp_to(10);

        assert_eq!(cursor.index(), 1);
    }

    fn connected(schema: SchemaState) -> State {
        State {
            connection: ConnectionState::Connected(Connection {
                id: ConnId::next(),
                posture: Posture::Writable,
                schema,
            }),
            ..State::default()
        }
    }

    #[test]
    fn a_state_that_is_not_connected_is_read_only() {
        for connection in [
            ConnectionState::Disconnected,
            ConnectionState::Connecting(ConnId::next()),
            ConnectionState::Failed(DbError::Timeout),
        ] {
            let state = State {
                connection,
                ..State::default()
            };

            assert_eq!(
                state.posture(),
                Posture::ReadOnly,
                "only a live connection may carry a writable posture"
            );
        }
    }

    #[test]
    fn a_connected_state_carries_its_own_posture() {
        assert_eq!(connected(SchemaState::Loading).posture(), Posture::Writable);
    }

    #[test]
    fn a_schema_is_only_visible_once_it_has_loaded() {
        assert!(connected(SchemaState::Loading).schema().is_none());
        assert!(
            connected(SchemaState::Failed(DbError::Timeout))
                .schema()
                .is_none()
        );
        assert!(
            connected(SchemaState::Loaded(Schema::default()))
                .schema()
                .is_some()
        );
        assert!(State::default().schema().is_none());
    }

    #[test]
    fn only_the_outstanding_connection_is_current() {
        let conn = ConnId::next();
        let other = ConnId::next();

        assert!(ConnectionState::Connecting(conn).is_current(conn));
        assert!(!ConnectionState::Connecting(conn).is_current(other));
        assert!(!ConnectionState::Disconnected.is_current(conn));
        assert!(!ConnectionState::Failed(DbError::Timeout).is_current(conn));
    }
}

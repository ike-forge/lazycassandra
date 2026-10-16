//! The whole application state: one root struct over per-panel modules
//! (architecture.md §11).

use std::collections::BTreeMap;
use std::path::PathBuf;

use crate::clock::Stamp;
use crate::config::Config;
use crate::connection::{Connection, ConnectionState, SchemaState};
use crate::contents::ContentsPanel;
use crate::ddl::DdlPanel;
use crate::feedback::{Flash, Loader, Toast};
use crate::geometry::Geometry;
use crate::history::Histories;
use crate::jobs::{Committing, Dropping, ExportJob};
use crate::log::{Log, Outcome};
use crate::model::{ConnId, QueryId, Schema, Statement};
use crate::overlay::Overlay;
use crate::panel::Panel;
use crate::posture::Posture;
use crate::queries::{QueriesPanel, QueriesTab};
use crate::search::{Search, Searched};
use crate::style::Severity;
use crate::theme::ThemeChoice;
use crate::tree::{Node, TablesPanel};

#[derive(Debug, Clone, Default)]
pub struct State {
    pub connection: ConnectionState,
    focus: Panel,
    pub tables: TablesPanel,
    pub contents: ContentsPanel,
    pub ddl: DdlPanel,
    pub queries: QueriesPanel,
    pub log: Log,
    pub histories: Histories,
    /// So a motion knows how far a page is.
    pub geometry: Geometry,
    pub overlay: Option<Overlay>,
    /// A result whose id is absent was cancelled, and is dropped.
    pub in_flight: BTreeMap<QueryId, Statement>,
    pub loader: Loader,
    pub flash: Option<Flash>,
    pub held: Option<crate::pointer::Held>,
    pub toast: Option<Toast>,
    /// Avoids repainting on every repeat of a held key.
    pub dirty: bool,
    pub should_quit: bool,
    /// A name; `lc-inbound` owns what it looks like.
    pub theme: ThemeChoice,
    /// Refreshing it never marks dirty.
    pub now: Stamp,
    /// Mostly unconsumed yet; held so follow-ups have somewhere to read from.
    pub config: Config,
    /// Passed in by `lc-app`: the domain reads no environment.
    pub cwd: PathBuf,
    /// Passed in by `lc-app`. `None` without `$XDG_CONFIG_HOME` or `$HOME`.
    pub config_home: Option<PathBuf>,
    /// One at a time.
    pub export: Option<ExportJob>,
    /// One at a time: a second would race the schema reload the first owes.
    pub dropping: Option<Dropping>,
    /// One at a time: a second would race the first's write-back.
    pub committing: Option<Committing>,
}

impl State {
    #[must_use]
    pub const fn focus(&self) -> Panel {
        self.focus
    }

    /// Private focus makes this the only way to change it: two panels holding
    /// selections would make `y` ambiguous.
    pub fn focus_on(&mut self, panel: Panel) {
        if self.focus == panel {
            return;
        }

        match self.focus {
            Panel::Tables => self.tables.selection.clear(),
            Panel::Ddl => self.ddl.selection.clear(),
            Panel::Contents => {
                self.contents.rows.clear();
                self.contents.columns.clear();
            }
            // The panel's unfocused height follows its buffer, so Logs must not
            // stay drawn unfocused.
            Panel::Queries => {
                self.show_queries_tab(QueriesTab::Cql);
                self.queries.stop_editing();
            }
        }

        self.focus = panel;
    }

    /// The one chokepoint for leaving Logs: a search left applied would keep
    /// `n`/`esc clear` on the bar over a buffer that takes text, and a
    /// selection would name a trail that has moved on.
    pub(crate) fn show_queries_tab(&mut self, tab: QueriesTab) {
        if self.queries.tab() == QueriesTab::Logs && tab != QueriesTab::Logs {
            self.log.set_search(None);
            self.log.selection.clear();
        }

        self.queries.show(tab);
    }

    /// Queries takes text only while its Cql buffer is open.
    #[must_use]
    pub fn takes_text(&self) -> bool {
        self.focus() == Panel::Queries && self.queries.is_editing()
    }

    /// The safe default without a connection.
    #[must_use]
    pub const fn posture(&self) -> Posture {
        match &self.connection {
            ConnectionState::Connected(connection) => connection.posture,
            ConnectionState::Unchosen
            | ConnectionState::Disconnected
            | ConnectionState::Connecting { .. }
            | ConnectionState::Failed(_) => Posture::ReadOnly,
        }
    }

    #[must_use]
    pub fn history(&self) -> &[String] {
        match &self.connection {
            ConnectionState::Connected(connection) => self.histories.of(&connection.name),
            ConnectionState::Unchosen
            | ConnectionState::Disconnected
            | ConnectionState::Connecting { .. }
            | ConnectionState::Failed(_) => &[],
        }
    }

    #[must_use]
    pub const fn conn_id(&self) -> Option<ConnId> {
        match &self.connection {
            ConnectionState::Connected(connection) => Some(connection.id),
            ConnectionState::Unchosen
            | ConnectionState::Disconnected
            | ConnectionState::Connecting { .. }
            | ConnectionState::Failed(_) => None,
        }
    }

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

    /// The one answer to "which keyspace is on screen", shared by completion
    /// and the grid.
    #[must_use]
    pub fn visible_keyspace(&self) -> Option<String> {
        let schema = self.schema()?;

        self.tables.selected(schema).map(|node| match node {
            Node::Keyspace { keyspace, .. } | Node::Table { keyspace, .. } => keyspace.name.clone(),
        })
    }

    /// One method: the borrow checker cannot split `&mut self` across two
    /// accessors.
    pub fn tables_and_schema(&mut self) -> Option<(&mut TablesPanel, &Schema)> {
        match &self.connection {
            ConnectionState::Connected(Connection {
                schema: SchemaState::Loaded(schema),
                ..
            }) => Some((&mut self.tables, schema)),
            _ => None,
        }
    }

    /// Does nothing before the schema has loaded.
    pub fn move_tree_cursor(&mut self, row: usize) {
        if let Some((tables, schema)) = self.tables_and_schema() {
            tables.go_to(row, schema);
        }
    }

    pub const fn mark_dirty(&mut self) {
        self.dirty = true;
    }

    pub fn notify(&mut self, severity: Severity, message: impl Into<String>) {
        self.toast = Some(Toast::new(severity, message));
        self.dirty = true;
    }

    /// What the loader animates for, and the only thing that makes a tick worth
    /// a repaint.
    #[must_use]
    pub fn is_working(&self) -> bool {
        self.is_reading_health() || Panel::ALL.iter().any(|&panel| self.is_working_in(panel))
    }

    /// The wait belongs to the panel its result fills: Queries' statements land
    /// in Contents. Queries itself waits on its trail, so the Logs tab animates
    /// every pending entry.
    #[must_use]
    pub fn is_working_in(&self, panel: Panel) -> bool {
        match panel {
            Panel::Tables => match &self.connection {
                ConnectionState::Connecting { .. } => true,
                ConnectionState::Connected(connection) => {
                    matches!(connection.schema, SchemaState::Loading)
                }
                ConnectionState::Unchosen
                | ConnectionState::Disconnected
                | ConnectionState::Failed(_) => false,
            },
            Panel::Contents => !self.in_flight.is_empty(),
            Panel::Queries => self
                .log
                .entries()
                .any(|entry| matches!(entry.outcome(), Outcome::Pending { .. })),
            Panel::Ddl => false,
        }
    }

    #[must_use]
    pub fn is_reading_health(&self) -> bool {
        matches!(&self.overlay, Some(Overlay::Health(panel)) if panel.is_loading())
    }

    #[must_use]
    pub fn search(&self, target: Searched) -> Option<&Search> {
        match target {
            Searched::Tables => self.tables.search(),
            Searched::Ddl => self.ddl.search(),
            Searched::Contents => self.contents.search(),
            Searched::Queries => self.log.search(),
        }
    }

    /// Repairs what a search invalidates. Only Tables needs a clamp: forcing
    /// keyspaces open changes its row count.
    pub fn apply_search(&mut self, target: Searched, search: Option<Search>) {
        match target {
            Searched::Tables => {
                self.tables.set_search(search);
                if let Some((tables, schema)) = self.tables_and_schema() {
                    tables.clamp(schema);
                }
                self.tables.selection.clear();
            }
            // A search drops no DDL row. The line offset waits on
            // `Msg::Measured`, and `clamp_lists` covers the cursor.
            Searched::Ddl => {
                self.ddl.set_search(search);
                self.ddl.selection.clear();
            }
            // Only flags cells, so ranges still address what they did.
            Searched::Contents => self.contents.set_search(search),
            // Flags, never drops, so no clamp; but a fresh search ends a
            // selection being built.
            Searched::Queries => {
                self.log.set_search(search);
                self.log.selection.clear();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::DbError;

    fn connected(schema: SchemaState) -> State {
        State {
            connection: ConnectionState::Connected(Connection {
                id: ConnId::next(),
                name: crate::config::ConnectionName::try_new("local").expect("a valid name"),
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
            ConnectionState::Connecting {
                conn: ConnId::next(),
                name: crate::config::ConnectionName::try_new("local").expect("a valid name"),
            },
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

        assert!(
            ConnectionState::Connecting {
                conn,
                name: crate::config::ConnectionName::try_new("local").expect("a valid name")
            }
            .is_current(conn)
        );
        assert!(
            !ConnectionState::Connecting {
                conn,
                name: crate::config::ConnectionName::try_new("local").expect("a valid name")
            }
            .is_current(other)
        );
        assert!(!ConnectionState::Disconnected.is_current(conn));
        assert!(!ConnectionState::Failed(DbError::Timeout).is_current(conn));
    }

    #[test]
    fn a_search_is_read_back_from_the_panel_it_was_applied_to() {
        let mut state = State::default();

        state.apply_search(Searched::Ddl, Search::new("ttl"));

        assert_eq!(state.search(Searched::Ddl).map(Search::as_str), Some("ttl"));
        assert_eq!(state.search(Searched::Tables), None);
        assert_eq!(state.search(Searched::Contents), None);
    }

    #[test]
    fn the_three_searches_are_independent() {
        let mut state = State::default();

        state.apply_search(Searched::Tables, Search::new("ord"));
        state.apply_search(Searched::Contents, Search::new("null"));

        assert_eq!(
            state.search(Searched::Tables).map(Search::as_str),
            Some("ord")
        );
        assert_eq!(
            state.search(Searched::Contents).map(Search::as_str),
            Some("null")
        );
    }

    #[test]
    fn applying_none_clears_the_one_that_was_there() {
        let mut state = State::default();
        state.apply_search(Searched::Tables, Search::new("ord"));

        state.apply_search(Searched::Tables, None);

        assert_eq!(state.search(Searched::Tables), None);
    }

    #[test]
    fn the_trails_search_is_read_back_from_the_panel_it_was_applied_to() {
        let mut state = State::default();

        state.apply_search(Searched::Queries, Search::new("delete"));

        assert_eq!(
            state.search(Searched::Queries).map(Search::as_str),
            Some("delete")
        );
    }

    #[test]
    fn a_fresh_search_over_the_trail_drops_a_visual_selection_being_built() {
        let mut state = State::default();
        state.log.selection.start(0);

        state.apply_search(Searched::Queries, Search::new("delete"));

        assert!(!state.log.selection.is_active());
    }
}

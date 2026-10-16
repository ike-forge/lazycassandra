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
    /// The Queries panel: its buffer, and whatever completion is being
    /// offered into it (query editor design §5.4).
    pub queries: QueriesPanel,
    /// The Logs tab: a bounded trail of what the session issued and what came
    /// back (query log design §3).
    pub log: Log,
    /// What each connection has been asked from the Queries buffer, for
    /// `ctrl-g` (query history design §1).
    pub histories: Histories,
    /// What the last frame turned out to be, so a motion knows how far a page
    /// is and where the end is.
    pub geometry: Geometry,
    /// A message or the command palette, drawn over the panels until it is
    /// dismissed.
    pub overlay: Option<Overlay>,
    /// Queries issued and not yet resolved. A result whose id is absent here
    /// was cancelled, and is dropped rather than applied.
    pub in_flight: BTreeMap<QueryId, Statement>,
    /// How far into its cycle the loader is. Elapsed time reaches the domain
    /// only as [`crate::Msg::Tick`], so this is the whole of what the renderer
    /// is told about it.
    pub loader: Loader,
    /// What the last `y` copied, lit for as long as [`Flash`] lasts. `None`
    /// once it has faded or the next key has ended it.
    pub flash: Option<Flash>,
    /// The modal button the pointer is holding down.
    pub held: Option<crate::pointer::Held>,
    /// A one-line answer to a key that did nothing, for as long as [`Toast`]
    /// lasts.
    pub toast: Option<Toast>,
    /// Set when something changed that the next frame must show. Avoids
    /// repainting on every keypress of a held-down `j` (architecture.md §3).
    pub dirty: bool,
    pub should_quit: bool,
    /// The theme in force. A name; `lc-inbound` owns what it looks like.
    pub theme: ThemeChoice,
    /// The wall-clock as of the last tick, which is what a log entry stamps
    /// itself from. Refreshing it never marks dirty: a tick that changes
    /// nothing must not cost a frame (query log design §4).
    pub now: Stamp,
    /// The configuration the tool started on. Most of it has no consumer yet
    /// (design §11); it is held rather than dropped so those follow-ups have
    /// somewhere to read from.
    pub config: Config,
    /// The process's working directory, for the export form's prefilled path.
    /// Passed in by `lc-app` at startup: the domain cannot read it and must
    /// not learn how (export design §2.1).
    pub cwd: PathBuf,
    /// Where the global config file goes, which the picker names when there
    /// are no connections to list. Passed in by `lc-app` for `cwd`'s reason:
    /// the domain cannot read the environment and must not learn how. `None`
    /// when neither `$XDG_CONFIG_HOME` nor `$HOME` is set, and there is no
    /// path to name (connection picker design §3).
    pub config_home: Option<PathBuf>,
    /// The one export in flight, if any. One at a time (export design §5).
    pub export: Option<ExportJob>,
    /// The one drop in flight, if any. One at a time for the reason an export
    /// is, and for one more: a second would race the schema reload the first
    /// owes (drop design §1, §4).
    pub dropping: Option<Dropping>,
    /// The one commit in flight, if any. One at a time for the drop's reason:
    /// a second would race the first's write back into the page
    /// (cell editing design §5).
    pub committing: Option<Committing>,
}

impl State {
    #[must_use]
    pub const fn focus(&self) -> Panel {
        self.focus
    }

    /// Moves focus, dropping the selection of the panel being left.
    ///
    /// A selection says what the next command acts on and the next command
    /// acts on what has focus, so two panels holding one say two things and
    /// `y` answers only one of them. The field is private so this is the only
    /// way to change focus and the rule cannot be forgotten by a seventh path
    /// (multi-group selection design §4).
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
            // The panel's unfocused height is a function of its buffer
            // (layout.rs), so an unfocused Logs tab would be sized by
            // something it is not drawing (query log design §5.2).
            Panel::Queries => {
                self.show_queries_tab(QueriesTab::Cql);
                self.queries.stop_editing();
            }
        }

        self.focus = panel;
    }

    /// Draws `tab` on the Queries panel, dropping what only makes sense while
    /// the Logs tab is the one being drawn.
    ///
    /// The one chokepoint for leaving the Logs tab, whether that is
    /// `focus_on` reverting it or `Tab`/`BackTab` walking the panel's own
    /// strip past it: a search left applied would keep the hint bar
    /// advertising `n`/`esc clear` while the Cql buffer takes text, and
    /// `escape` would spend a press clearing it instead of leaving the
    /// buffer — a lie the same rule that governs every other hint exists to
    /// prevent (`view/hints.rs`'s own "a bar that lies is worse than no
    /// bar"). A held selection is the same problem `focus_on`'s doc comment
    /// already states for every other panel: it says what the next `y`
    /// answers, and a trail the ring has moved on from by the time Logs is
    /// shown again is not what a stale selection still names (query log
    /// design §5.1, §5.2).
    pub(crate) fn show_queries_tab(&mut self, tab: QueriesTab) {
        if self.queries.tab() == QueriesTab::Logs && tab != QueriesTab::Logs {
            self.log.set_search(None);
            self.log.selection.clear();
        }

        self.queries.show(tab);
    }

    /// Whether the focused panel is taking text this frame.
    ///
    /// A question about `State` rather than about `Panel`: the Queries panel
    /// takes text only while its Cql buffer is open (cql edit mode design §2).
    #[must_use]
    pub fn takes_text(&self) -> bool {
        self.focus() == Panel::Queries && self.queries.is_editing()
    }

    /// Posture of the current connection, or the safe default when there is
    /// none.
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

    /// The statements the connection in use has been asked, newest first.
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

    /// The connection a statement would run on.
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

    /// The keyspace the tree cursor is on, which is the one on screen.
    ///
    /// The single answer to "which keyspace is the user looking at": the
    /// editor completes an unqualified name against it (`update::keys::complete`)
    /// and the grid reads a typed statement's unqualified table against it
    /// (`view::contents`), and two derivations of one rule would be two
    /// things to keep in step (cql semantics design §7).
    #[must_use]
    pub fn visible_keyspace(&self) -> Option<String> {
        let schema = self.schema()?;

        self.tables.selected(schema).map(|node| match node {
            Node::Keyspace { keyspace, .. } | Node::Table { keyspace, .. } => keyspace.name.clone(),
        })
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

    /// Replaces whatever toast is up.
    pub fn notify(&mut self, severity: Severity, message: impl Into<String>) {
        self.toast = Some(Toast::new(severity, message));
        self.dirty = true;
    }

    /// Whether the tool is waiting on the cluster for anything.
    ///
    /// What the loader animates for, and the only thing that makes a tick
    /// worth a repaint.
    #[must_use]
    pub fn is_working(&self) -> bool {
        self.is_reading_health() || Panel::ALL.iter().any(|&panel| self.is_working_in(panel))
    }

    /// Whether `panel` is the one waiting on the cluster.
    ///
    /// The wait is attributed to the panel whose content it will fill, because
    /// that is where the loader is drawn: the tree is what a schema read
    /// populates, and the grid is what every statement lands in — including
    /// one submitted from Queries, which puts its result in Contents
    /// (`submit_into_contents`). Ddl reads off a loaded schema and issues
    /// nothing of its own, so it never waits.
    ///
    /// Queries waits on its own trail rather than on `in_flight`: a connect
    /// or a schema read is a pending entry too, and the Logs tab's `⋯
    /// running` animation exists to distinguish a hung wait from a fast one
    /// (query log design §6), not only for a query the panel itself issued.
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

    /// Whether the health block is waiting on its read, which turns the
    /// loader on its border.
    #[must_use]
    pub fn is_reading_health(&self) -> bool {
        matches!(&self.overlay, Some(Overlay::Health(panel)) if panel.is_loading())
    }

    /// The search applied to `target`, if any.
    #[must_use]
    pub fn search(&self, target: Searched) -> Option<&Search> {
        match target {
            Searched::Tables => self.tables.search(),
            Searched::Ddl => self.ddl.search(),
            Searched::Contents => self.contents.search(),
            Searched::Queries => self.log.search(),
        }
    }

    /// Applies `search` to `target`, and repairs what it invalidates.
    ///
    /// Not a `&mut Option<Search>` handed out: repairing what a search
    /// invalidates — a cursor left pointing past the rows the panel now has —
    /// belongs here rather than to every caller that sets one. Tables is the
    /// only target whose row count a search can still move: no search drops
    /// a row (design §4.1), but forcing one keyspace open while another
    /// falls back closed changes how many rows are on screen between two
    /// calls here, so Tables' cursor still needs the clamp. DDL, Table
    /// contents and the trail never move a row for a search, so none of the
    /// three clamps anything here — see their own arms below.
    pub fn apply_search(&mut self, target: Searched, search: Option<Search>) {
        match target {
            Searched::Tables => {
                self.tables.set_search(search);
                let rows = self.schema().map(|schema| self.tables.rows(schema).len());
                if let Some(rows) = rows {
                    self.tables.cursor.clamp_to(rows);
                }
                self.tables.selection.clear();
            }
            // No repair here: a search never drops a DDL row (design §4.2),
            // so neither the row count nor the line count is a search's to
            // invalidate. What repair is still owed runs elsewhere, the same
            // way it does for any other change to the panel (design §3.3).
            // The offset is counted in wrapped lines, a width-dependent
            // count only the renderer can produce, so it waits on the next
            // `Msg::Measured` rather than being clamped against a stale
            // count here. The cursor counts rows, and `clamp_lists`, run in
            // `update`'s epilogue, clamps it against `view::ddl_rows` on
            // every message — search included — so the clamp this arm would
            // otherwise owe is already covered before the frame is drawn.
            Searched::Ddl => {
                self.ddl.set_search(search);
                self.ddl.selection.clear();
            }
            // No range repair here either: this search only flags matching
            // cells and never renumbers a row, so a range still addresses
            // exactly what it did before the search changed.
            Searched::Contents => self.contents.set_search(search),
            // Same shape as DDL, and for the same reason: the trail is a
            // list of rows a search only flags, never drops or reorders, so
            // there is nothing here for a cursor clamp to repair — but a
            // fresh search is a fresh look at the list, so the row-based
            // selection it might be mid-building does not survive it either
            // (query log design §5.3).
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

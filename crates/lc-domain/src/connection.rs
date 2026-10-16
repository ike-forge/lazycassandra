//! The one connection and how far along it is.

use crate::config::ConnectionName;
use crate::model::{ConnId, DbError, Schema};
use crate::posture::Posture;

/// How far along the one connection is.
///
/// `Option<Connection>` could not say "connecting" or "failed", and an error
/// field beside the schema would make illegal combinations representable.
#[derive(Debug, Clone, Default)]
pub enum ConnectionState {
    #[default]
    Disconnected,
    /// No connection has been chosen yet, and none will be until the user
    /// picks one. Distinct from [`Self::Disconnected`], which is what a
    /// *dismissed* picker leaves behind: the difference is whether the picker
    /// is still owed, so a startup that had to show diagnostics first can
    /// open it when they close without reopening one the user has waved away
    /// (connection picker design §4).
    Unchosen,
    /// Carries the id so a failure can be matched to the attempt that is
    /// outstanding, and the name so the connection it becomes can say which
    /// one it is without the config being searched a second time.
    Connecting {
        conn: ConnId,
        name: ConnectionName,
    },
    Failed(DbError),
    Connected(Connection),
}

#[derive(Debug, Clone)]
pub struct Connection {
    pub id: ConnId,
    /// The `[[connections]]` entry this was opened from. Held rather than
    /// looked up again: the config is a list the user may search, and the
    /// live connection is not a row of it.
    pub name: ConnectionName,
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

impl ConnectionState {
    /// Whether `conn` is the connection this state is about.
    ///
    /// The guard that drops results belonging to a connection the user has
    /// already left (architecture.md §3).
    #[must_use]
    pub fn is_current(&self, conn: ConnId) -> bool {
        match self {
            Self::Connecting { conn: id, .. } => *id == conn,
            Self::Connected(connection) => connection.id == conn,
            Self::Unchosen | Self::Disconnected | Self::Failed(_) => false,
        }
    }
}

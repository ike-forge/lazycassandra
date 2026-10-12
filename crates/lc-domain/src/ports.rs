//! Port definitions.
//!
//! Defined here, implemented in `lc-outbound`, and called by neither — the
//! effect runtime in `lc-app` drives them (architecture.md §5). That is what
//! lets these be async while `update` stays synchronous.
//!
//! Every port is `dyn`-compatible so a fake can be swapped in wholesale, which
//! is why they use `#[async_trait]` rather than native async fns in traits.

use std::path::Path;

use async_trait::async_trait;

use crate::config::{
    Config, ConnectionConfig, ConnectionName, Credential, Diagnostic, DriverConfig, HistoryLimit,
    PageSize,
};
use crate::history::Histories;
use crate::model::{ConnId, DbError, Page, PagingState, Schema, Statement};
use crate::posture::Posture;
use crate::theme::ThemeChoice;

/// A cluster, in our vocabulary rather than a driver's.
///
/// Returns our [`Schema`] and [`Page`]; driver types stop at the adapter, so
/// choosing a driver (architecture.md §8) touches one crate.
#[async_trait]
pub trait Cluster: Send + Sync {
    /// Opens a session to `target` and binds it to `conn`.
    ///
    /// The address arrives here rather than at the adapter's constructor: a
    /// connection is chosen from the picker at runtime, so there is no one
    /// contact point to build the adapter around. The posture that comes back
    /// is the connection's own — nothing else knows it
    /// (connection picker design §6).
    async fn connect(
        &self,
        conn: ConnId,
        target: &ConnectionConfig,
        driver: &DriverConfig,
    ) -> Result<Posture, DbError>;
    async fn describe(&self, conn: ConnId) -> Result<Schema, DbError>;
    async fn execute(&self, conn: ConnId, stmt: &Statement) -> Result<Page, DbError>;

    /// One page of at most `page_size` rows of `stmt`, continuing from
    /// `cursor` when there is one.
    ///
    /// Replaces a `fetch_page(conn, cursor)` that could not be implemented:
    /// scylla's `query_single_page` needs the statement alongside the paging
    /// state, and a cursor alone identifies nothing the driver can resume
    /// (export design §6.2).
    async fn execute_paged(
        &self,
        conn: ConnId,
        stmt: &Statement,
        page_size: PageSize,
        cursor: Option<&PagingState>,
    ) -> Result<Page, DbError>;
}

/// Locates and layers configuration files (design §7).
#[async_trait]
pub trait ConfigSource: Send + Sync {
    /// Never fails: a file that cannot be read or parsed produces
    /// diagnostics and contributes nothing (design §6).
    async fn load(&self) -> (Config, Vec<Diagnostic>);
}

/// Writes one setting back into the file it was read from
/// (theme persistence design §5).
///
/// A trait of its own rather than a method on [`ConfigSource`]: every run
/// reads config at startup, one keystroke writes it, and a fake for one should
/// not have to implement the other.
#[async_trait]
pub trait ConfigSink: Send + Sync {
    async fn save_theme(&self, theme: &ThemeChoice) -> Result<(), ConfigWriteError>;
}

/// Resolves a credential to a value at connect time.
#[async_trait]
pub trait SecretResolver: Send + Sync {
    async fn resolve(&self, credential: &Credential) -> Result<String, ConfigError>;
}

/// Query history, kept between runs (query history design §1).
#[async_trait]
pub trait Store: Send + Sync {
    /// Every connection's history, trimmed to `limit`. Never fails, as
    /// [`ConfigSource::load`] does not: a file that cannot be read yields
    /// nothing and a diagnostic.
    async fn load(&self, limit: HistoryLimit) -> (Histories, Vec<Diagnostic>);

    /// Replaces `connection`'s stored history, newest first, leaving every
    /// other connection's alone.
    async fn save(&self, connection: &ConnectionName, entries: &[String])
    -> Result<(), StoreError>;
}

/// Opens a destination for an export.
///
/// Refusing to overwrite is this port's job rather than a separate `exists`
/// call, which would leave a window between asking and creating
/// (export design §6.1).
#[async_trait]
pub trait Writer: Send + Sync {
    async fn open(&self, path: &Path, overwrite: bool) -> Result<Box<dyn Sink>, ExportError>;
}

/// An open destination.
#[async_trait]
pub trait Sink: Send {
    async fn write(&mut self, chunk: &str) -> Result<(), ExportError>;
    /// Flushes and closes.
    async fn finish(&mut self) -> Result<(), ExportError>;
    /// Removes the partial file. Infallible from the caller's side: the export
    /// has already failed, and a failure to clean up is not a second error the
    /// user can act on (design §4).
    async fn abort(&mut self);
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ConfigError {
    #[error("config is invalid: {0}")]
    Invalid(String),
    #[error("could not resolve credential: {0}")]
    Unresolvable(String),
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ConfigWriteError {
    #[error("could not write {0}")]
    Io(String),
    /// The file was left exactly as it was. Splicing into a document we cannot
    /// parse would mean writing over what the user wrote (theme persistence
    /// design §4).
    #[error("{0} could not be parsed, so it was left alone: {1}")]
    Unparsable(String, String),
    #[error("nowhere to save it: neither XDG_CONFIG_HOME nor HOME is set")]
    NoPath,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum StoreError {
    #[error("history could not be saved: {0}")]
    Unavailable(String),
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ExportError {
    #[error("that file already exists")]
    Exists,
    #[error("could not write the file: {0}")]
    Io(String),
    #[error("the cluster stopped answering: {0}")]
    Cluster(DbError),
}

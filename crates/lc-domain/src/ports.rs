//! Ports: defined here, implemented in `lc-outbound`, driven by `lc-app`'s
//! effect runtime, so `update` stays synchronous (architecture.md §5).
//! `#[async_trait]` keeps them `dyn`-compatible for fakes.

use std::path::Path;

use async_trait::async_trait;

use crate::config::{
    Config, ConnectionConfig, ConnectionName, Credential, Diagnostic, DriverConfig, HistoryLimit,
    PageSize,
};
use crate::contents::TableRef;
use crate::health::Health;
use crate::history::Histories;
use crate::model::{ConnId, DbError, Page, PagingState, Schema, Statement};
use crate::posture::Posture;
use crate::theme::ThemeChoice;

/// Driver types stop at the adapter.
#[async_trait]
pub trait Cluster: Send + Sync {
    /// The address arrives here: connections are chosen at runtime. The posture
    /// returned is the connection's own.
    async fn connect(
        &self,
        conn: ConnId,
        target: &ConnectionConfig,
        driver: &DriverConfig,
    ) -> Result<Posture, DbError>;
    async fn describe(&self, conn: ConnId) -> Result<Schema, DbError>;
    async fn execute(&self, conn: ConnId, stmt: &Statement) -> Result<Page, DbError>;

    /// Needs the statement as well as the cursor: a paging state alone resumes
    /// nothing.
    async fn execute_paged(
        &self,
        conn: ConnId,
        stmt: &Statement,
        page_size: PageSize,
        cursor: Option<&PagingState>,
    ) -> Result<Page, DbError>;

    /// A failing node is a [`crate::health::NodeOutcome`]; `Err` means no node
    /// was reached.
    async fn table_health(&self, conn: ConnId, table: &TableRef) -> Result<Health, DbError>;
}

#[async_trait]
pub trait ConfigSource: Send + Sync {
    /// Never fails: an unreadable file yields diagnostics and contributes
    /// nothing.
    async fn load(&self) -> (Config, Vec<Diagnostic>);
}

/// Separate from [`ConfigSource`]: a fake for one need not implement the other.
#[async_trait]
pub trait ConfigSink: Send + Sync {
    async fn save_theme(&self, theme: &ThemeChoice) -> Result<(), ConfigWriteError>;
}

#[async_trait]
pub trait SecretResolver: Send + Sync {
    async fn resolve(&self, credential: &Credential) -> Result<String, ConfigError>;
}

#[async_trait]
pub trait Store: Send + Sync {
    /// Never fails, as [`ConfigSource::load`].
    async fn load(&self, limit: HistoryLimit) -> (Histories, Vec<Diagnostic>);

    async fn save(&self, connection: &ConnectionName, entries: &[String])
    -> Result<(), StoreError>;
}

/// Refusing to overwrite is this port's job: a separate `exists` call would
/// race.
#[async_trait]
pub trait Writer: Send + Sync {
    async fn open(&self, path: &Path, overwrite: bool) -> Result<Box<dyn Sink>, ExportError>;
}

#[async_trait]
pub trait Sink: Send {
    async fn write(&mut self, chunk: &str) -> Result<(), ExportError>;
    async fn finish(&mut self) -> Result<(), ExportError>;
    /// Infallible to the caller: a cleanup failure is not an error the user can
    /// act on.
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
    /// Splicing into a document we cannot parse would overwrite what the user
    /// wrote.
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

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

use crate::model::{ConnId, DbError, Page, PagingState, Schema, Statement};
use crate::posture::Posture;

/// A cluster, in our vocabulary rather than a driver's.
///
/// Returns our [`Schema`] and [`Page`]; driver types stop at the adapter, so
/// choosing a driver (architecture.md §8) touches one crate.
#[async_trait]
pub trait Cluster: Send + Sync {
    async fn connect(&self, conn: ConnId) -> Result<Posture, DbError>;
    async fn describe(&self, conn: ConnId) -> Result<Schema, DbError>;
    async fn execute(&self, conn: ConnId, stmt: &Statement) -> Result<Page, DbError>;
    async fn fetch_page(&self, conn: ConnId, cursor: &PagingState) -> Result<Page, DbError>;
}

/// Locates and layers configuration files (architecture.md §7).
#[async_trait]
pub trait ConfigSource: Send + Sync {
    async fn load(&self) -> Result<Config, ConfigError>;
}

/// Resolves a credential *reference* to a value at connect time.
///
/// Resolved secrets are never written back to disk or into the history store
/// (architecture.md §7).
#[async_trait]
pub trait SecretResolver: Send + Sync {
    async fn resolve(&self, reference: &CredentialRef) -> Result<String, ConfigError>;
}

/// Query history and saved queries.
#[async_trait]
pub trait Store: Send + Sync {
    async fn record(&self, stmt: &Statement) -> Result<(), StoreError>;
    async fn history(&self, limit: usize) -> Result<Vec<Statement>, StoreError>;
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

/// Where a credential comes from.
///
/// There is deliberately no `Password(String)` variant: the project config is
/// meant to be committed, so a literal secret in it is a parse error rather
/// than a leak (vision.md §6).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CredentialRef {
    Env(String),
    File(std::path::PathBuf),
    Command(Vec<String>),
}

/// The merged, validated result of layering every config source.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Config {
    pub connections: Vec<ConnectionConfig>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConnectionConfig {
    pub name: String,
    pub hosts: Vec<String>,
    pub posture: Posture,
    pub username: Option<String>,
    pub password: Option<CredentialRef>,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ConfigError {
    #[error("config is invalid: {0}")]
    Invalid(String),
    #[error("could not resolve credential: {0}")]
    Unresolvable(String),
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum StoreError {
    #[error("store is unavailable: {0}")]
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

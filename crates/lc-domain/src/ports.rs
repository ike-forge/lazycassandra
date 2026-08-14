//! Port definitions.
//!
//! Defined here, implemented in `lc-outbound`, and called by neither — the
//! effect runtime in `lc-app` drives them (architecture.md §5). That is what
//! lets these be async while `update` stays synchronous.
//!
//! Every port is `dyn`-compatible so a fake can be swapped in wholesale, which
//! is why they use `#[async_trait]` rather than native async fns in traits.

use async_trait::async_trait;

use crate::config::{Config, Credential, Diagnostic};
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

/// Locates and layers configuration files (design §7).
#[async_trait]
pub trait ConfigSource: Send + Sync {
    /// Never fails: a file that cannot be read or parsed produces
    /// diagnostics and contributes nothing (design §6).
    async fn load(&self) -> (Config, Vec<Diagnostic>);
}

/// Resolves a credential to a value at connect time.
#[async_trait]
pub trait SecretResolver: Send + Sync {
    async fn resolve(&self, credential: &Credential) -> Result<String, ConfigError>;
}

/// Query history and saved queries.
#[async_trait]
pub trait Store: Send + Sync {
    async fn record(&self, stmt: &Statement) -> Result<(), StoreError>;
    async fn history(&self, limit: usize) -> Result<Vec<Statement>, StoreError>;
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

//! Failures that come back from a cluster.
//!
//! Typed and matched on, never a panic and never an early return that unwinds
//! the loop (architecture.md §12). A failed query is a message like any other.

use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum DbError {
    #[error("could not reach the cluster: {0}")]
    Unreachable(String),

    #[error("authentication failed: {0}")]
    Authentication(String),

    #[error("TLS failed: {0}")]
    Tls(String),

    #[error("query timed out")]
    Timeout,

    #[error("cluster rejected the statement: {0}")]
    Invalid(String),

    #[error("statement is not permitted on a read-only connection")]
    ReadOnly,

    #[error("{0}")]
    Other(String),
}

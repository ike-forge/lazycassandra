//! Port implementations: cluster, config, secrets, local store.
//!
//! The only crate that talks to the outside world. The [`Cluster`] port is
//! implemented over `scylla` (doc/specs/2026-08-08-keyspace-list-design.md §2);
//! driver types stop at the adapter.
//!
//! [`Cluster`]: lc_domain::ports::Cluster

pub mod cassandra;
pub mod fake;
mod value;

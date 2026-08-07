//! Port implementations: cluster, config, secrets, local store.
//!
//! The only crate that talks to the outside world. No CQL driver is wired up
//! yet — architecture.md §8 defers that to a Phase 0 spike, and the [`Cluster`]
//! port is what keeps the choice cheap.
//!
//! [`Cluster`]: lc_domain::ports::Cluster

pub mod fake;

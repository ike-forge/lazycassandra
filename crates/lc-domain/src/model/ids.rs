//! Opaque handles.
//!
//! Newtypes rather than bare integers so a connection cannot be passed where a
//! query is expected (conventions.md §1).

use std::sync::atomic::{AtomicU64, Ordering};

macro_rules! opaque_id {
    ($(#[$meta:meta])* $name:ident) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
        pub struct $name(u64);

        impl $name {
            /// Allocates an id unique within this process run.
            #[must_use]
            pub fn next() -> Self {
                static COUNTER: AtomicU64 = AtomicU64::new(0);
                Self(COUNTER.fetch_add(1, Ordering::Relaxed))
            }
        }
    };
}

opaque_id!(
    /// Identifies one connection to a cluster.
    ConnId
);

opaque_id!(
    /// Identifies one in-flight query.
    ///
    /// Carried by every effect and every result so that a late result for a
    /// cancelled query can be dropped in `update` (architecture.md §3).
    QueryId
);

opaque_id!(
    /// Identifies one export.
    ///
    /// Distinct from [`QueryId`]: an export outlives the panel's own request
    /// and is not cancelled by navigating away from it.
    JobId
);

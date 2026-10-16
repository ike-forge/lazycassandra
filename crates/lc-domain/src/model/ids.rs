//! Opaque handles, so a connection id cannot pass for a query id.

use std::sync::atomic::{AtomicU64, Ordering};

macro_rules! opaque_id {
    ($(#[$meta:meta])* $name:ident) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
        pub struct $name(u64);

        impl $name {
            /// Unique within this process run.
            #[must_use]
            pub fn next() -> Self {
                static COUNTER: AtomicU64 = AtomicU64::new(0);
                Self(COUNTER.fetch_add(1, Ordering::Relaxed))
            }
        }
    };
}

opaque_id!(ConnId);

opaque_id!(
    /// Lets `update` drop a late result for a cancelled query.
    QueryId
);

opaque_id!(
    /// Distinct from [`QueryId`]: an export outlives the panel's request.
    JobId
);

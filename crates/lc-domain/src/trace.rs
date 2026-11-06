//! Query tracing: the coordinator's `system_traces` record of one request, and
//! the Queries panel's Trace tab over it.

use std::net::IpAddr;
use std::time::Duration;

use crate::model::{Page, QueryId};
use crate::viewport::{Cursor, Scroll};

/// Opaque, like [`crate::model::PagingState`]: only handed back.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TraceId(pub [u8; 16]);

/// What a traced request answers before its trace has been read.
#[derive(Debug, Clone, PartialEq)]
pub struct Traced {
    pub page: Page,
    /// `None` when the coordinator ignored the tracing flag.
    pub trace: Option<TraceId>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueryTrace {
    pub coordinator: Option<IpAddr>,
    pub duration: Option<Duration>,
    pub events: Vec<TraceEvent>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TraceEvent {
    pub activity: String,
    pub source: Option<IpAddr>,
    /// Since the request reached `source`, not the coordinator: events from
    /// different nodes do not share a clock.
    pub elapsed: Option<Duration>,
    pub thread: Option<String>,
}

impl QueryTrace {
    /// In first-seen order, which is the coordinator first.
    #[must_use]
    pub fn nodes(&self) -> Vec<IpAddr> {
        let mut nodes = Vec::new();
        for source in self.events.iter().filter_map(|event| event.source) {
            if !nodes.contains(&source) {
                nodes.push(source);
            }
        }
        nodes
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum Tracing {
    #[default]
    Untraced,
    /// Covers both the request and the read of its trace: the page lands in
    /// between.
    Waiting {
        query: QueryId,
        cql: String,
    },
    Traced {
        cql: String,
        trace: QueryTrace,
    },
    Failed {
        cql: String,
        reason: String,
    },
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TracePanel {
    tracing: Tracing,
    pub cursor: Cursor,
    pub scroll: Scroll,
}

impl TracePanel {
    #[must_use]
    pub const fn tracing(&self) -> &Tracing {
        &self.tracing
    }

    #[must_use]
    pub const fn waiting(&self) -> Option<QueryId> {
        match self.tracing {
            Tracing::Waiting { query, .. } => Some(query),
            Tracing::Untraced | Tracing::Traced { .. } | Tracing::Failed { .. } => None,
        }
    }

    /// A summary row over one per event; a failure is its one row.
    #[must_use]
    pub const fn rows(&self) -> usize {
        match &self.tracing {
            Tracing::Traced { trace, .. } => trace.events.len() + 1,
            Tracing::Failed { .. } => 1,
            Tracing::Untraced | Tracing::Waiting { .. } => 0,
        }
    }

    pub fn wait(&mut self, query: QueryId, cql: String) {
        self.show(Tracing::Waiting { query, cql });
    }

    /// `false` for an answer the panel is not waiting on.
    pub fn answer(&mut self, query: QueryId, trace: Result<QueryTrace, String>) -> bool {
        if self.waiting() != Some(query) {
            return false;
        }
        let Tracing::Waiting { cql, .. } = std::mem::take(&mut self.tracing) else {
            return false;
        };

        self.show(match trace {
            Ok(trace) => Tracing::Traced { cql, trace },
            Err(reason) => Tracing::Failed { cql, reason },
        });
        true
    }

    fn show(&mut self, tracing: Tracing) {
        self.tracing = tracing;
        self.cursor.to_start();
        self.scroll.to_start();
    }
}

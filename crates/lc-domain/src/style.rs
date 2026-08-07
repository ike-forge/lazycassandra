//! Semantic style tokens.
//!
//! The domain never names a colour. It names a *meaning*, and the theme in
//! `lc-inbound` decides how to draw it (architecture.md §6). This is what lets
//! the default theme inherit the terminal's own palette.

use crate::posture::Posture;

/// A thing worth distinguishing visually.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Token {
    PartitionKey,
    ClusteringKey,
    RegularColumn,
    Posture(Posture),
    Severity(Severity),
    Muted,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    Info,
    Warning,
    Danger,
}

//! Interprets the effects `update` returned.
//!
//! This is the only place that performs I/O on the domain's behalf. Effects
//! become tokio tasks; their results re-enter through the same channel as a
//! keystroke (architecture.md §3).

use lc_domain::{Effect, Msg};
use tokio::sync::mpsc::UnboundedSender;

/// Every effect is inert: no port is implemented yet, because the driver is
/// still deferred to a Phase 0 spike (architecture.md §8). `Quit` needs no
/// work here — `update` has already set `State::should_quit`.
///
/// The sender is taken now because it is how results will re-enter the loop.
pub(crate) fn run(_tx: &UnboundedSender<Msg>, _effect: &Effect) {}

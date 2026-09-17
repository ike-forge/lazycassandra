//! The TUI adapter: computes panel geometry, draws the frame, turns terminal
//! input into messages.
//!
//! Knows nothing about CQL. Its largest job is laying out and drawing the
//! panels described by [`lc_domain::ViewModel`]; it also translates in both
//! directions at the boundary — crossterm events into
//! [`lc_domain::model::KeyPress`], and [`lc_domain::style::Token`] into a
//! concrete `ratatui` style.

mod body;
mod connections;
mod details;
mod diagnostics;
mod drop;
mod export;
mod filter;
mod footer;
mod highlight;
mod hints;
pub mod hit;
mod icons;
pub mod input;
mod inspect;
pub mod layout;
mod loader;
mod logs;
mod overlay;
mod palette;
pub mod render;
pub mod theme;
mod themes;
mod too_small;
mod wrap;

pub use hit::FrameMap;
pub use render::{Rendered, render};
pub use theme::{Theme, Themes};

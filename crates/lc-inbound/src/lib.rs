//! The TUI adapter: computes panel geometry, draws the frame, turns terminal
//! input into messages.
//!
//! Knows nothing about CQL. Its largest job is laying out and drawing the
//! panels described by [`lc_domain::ViewModel`]; it also translates in both
//! directions at the boundary — crossterm events into
//! [`lc_domain::model::KeyPress`], and [`lc_domain::style::Token`] into a
//! concrete `ratatui` style.

mod body;
mod details;
mod hints;
pub mod input;
pub mod layout;
mod loader;
mod overlay;
pub mod render;
pub mod theme;
mod too_small;

pub use render::render;
pub use theme::Theme;

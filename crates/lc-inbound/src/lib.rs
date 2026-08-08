//! The TUI adapter: renders state, turns terminal input into messages.
//!
//! Knows nothing about CQL. Its two jobs are translations in opposite
//! directions — crossterm events into [`lc_domain::model::KeyPress`], and
//! [`lc_domain::style::Token`] into a concrete `ratatui` style.

pub mod input;
pub mod layout;
pub mod render;
pub mod theme;

pub use render::render;
pub use theme::Theme;

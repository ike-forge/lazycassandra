//! The TUI adapter: lays out and draws [`lc_domain::ViewModel`], and translates
//! crossterm events in and style tokens out. Knows nothing about CQL.

mod body;
mod commit;
mod connections;
mod details;
mod diagnostics;
mod drop;
mod export;
mod filter;
mod footer;
mod health;
mod highlight;
mod hints;
mod history;
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
mod toast;
mod too_small;
mod wrap;

pub use hit::FrameMap;
pub use render::{Rendered, render};
pub use theme::{Theme, Themes};

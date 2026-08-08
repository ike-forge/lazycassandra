//! The pure core of `LazyCassandra`.
//!
//! Holds the state, the messages that change it, and the effects that change
//! anything else. No I/O, no runtime, no terminal — see architecture.md §2.

pub mod model;
pub mod ports;
pub mod posture;
pub mod state;
pub mod style;
pub mod tree;
pub mod update;
pub mod view;

pub use posture::Posture;
pub use state::State;
pub use update::{Effect, Msg, init, update};
pub use view::{PanelView, ViewModel, view_model};

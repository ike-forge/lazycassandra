//! The palettes written here, each declaring its own background. Contrast is a
//! relation to a ground: with an unknown ground no foreground can be
//! guaranteed, and "every text colour holds 4.5:1" is only testable against a
//! declared `base`. A full-screen TUI paints every cell anyway; `background =
//! "terminal"` is the opt-in for translucent terminals. Design rules: colour
//! palettes design §3.1.

pub(super) mod ash;
pub(super) mod beacon;
pub(super) mod default;
pub(super) mod quill;

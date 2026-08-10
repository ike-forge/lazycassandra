//! Keyboard input, in domain terms.
//!
//! Deliberately not `crossterm::event::KeyEvent`: `lc-domain` must not depend
//! on a terminal library (architecture.md §2). `lc-inbound` translates
//! crossterm events into these, the same way it translates [`crate::style`]
//! tokens the other way.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeyPress {
    pub key: Key,
    pub modifiers: Modifiers,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    Char(char),
    Enter,
    Escape,
    Backspace,
    Tab,
    BackTab,
    Delete,
    Insert,
    Home,
    End,
    PageUp,
    PageDown,
    Up,
    Down,
    Left,
    Right,
    F(u8),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Modifiers {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
}

impl Modifiers {
    /// Nothing held — what a bare key press carries, and what all but one
    /// binding declares.
    pub const NONE: Self = Self {
        ctrl: false,
        alt: false,
        shift: false,
    };
}

impl KeyPress {
    #[must_use]
    pub const fn new(key: Key) -> Self {
        Self {
            key,
            modifiers: Modifiers {
                ctrl: false,
                alt: false,
                shift: false,
            },
        }
    }
}

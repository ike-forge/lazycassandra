//! Keyboard input in domain terms; `lc-domain` must not depend on a terminal
//! library (architecture.md §2).

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
            modifiers: Modifiers::NONE,
        }
    }

    /// A binding of its own: `handle_key` refuses ctrl except where an arm
    /// names it.
    #[must_use]
    pub const fn ctrl(key: Key) -> Self {
        Self {
            key,
            modifiers: Modifiers {
                ctrl: true,
                ..Modifiers::NONE
            },
        }
    }
}

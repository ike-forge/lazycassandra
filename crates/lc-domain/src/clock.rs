//! The wall-clock, as the domain is allowed to know it.
//!
//! The domain reads no clock and must not learn how (architecture.md §2). A
//! [`Stamp`] is a value `lc-outbound` builds and hands in — through
//! [`Msg::Tick`](crate::Msg::Tick), or through
//! [`init`](crate::init) for the entries pushed before any tick has arrived
//! (query log design §4, §4.1).

use std::fmt;

/// A time of day, to the second.
///
/// Three integers rather than a preformatted `String`: it is `Copy`, so
/// refreshing [`State::now`](crate::State::now) twenty-five times a second
/// allocates nothing, and a domain holding `"14:02:09"` that cannot say what
/// hour that is has escaped its own type system for no gain
/// (conventions.md §1).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct Stamp {
    hour: u8,
    minute: u8,
    second: u8,
}

impl Stamp {
    #[must_use]
    pub const fn new(hour: u8, minute: u8, second: u8) -> Self {
        Self {
            hour,
            minute,
            second,
        }
    }
}

impl fmt::Display for Stamp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:02}:{:02}:{:02}", self.hour, self.minute, self.second)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_stamp_reads_as_a_wall_clock_time() {
        assert_eq!(Stamp::new(14, 2, 9).to_string(), "14:02:09");
    }

    #[test]
    fn a_stamp_pads_every_field_to_two_digits() {
        assert_eq!(Stamp::new(0, 0, 0).to_string(), "00:00:00");
        assert_eq!(Stamp::new(23, 59, 59).to_string(), "23:59:59");
    }

    #[test]
    fn the_default_stamp_is_midnight() {
        assert_eq!(Stamp::default(), Stamp::new(0, 0, 0));
    }
}

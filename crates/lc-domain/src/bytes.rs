//! A byte count, and the one way the app draws one.

use std::fmt;

/// A count of bytes, measured or estimated.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct Bytes(u64);

const UNIT: u64 = 1024;

impl Bytes {
    pub const KB: Self = Self(UNIT);
    pub const MB: Self = Self(UNIT * UNIT);

    #[must_use]
    pub const fn new(bytes: u64) -> Self {
        Self(bytes)
    }

    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }

    #[must_use]
    pub const fn times(self, factor: u64) -> Self {
        Self(self.0.saturating_mul(factor))
    }
}

impl std::ops::Add for Bytes {
    type Output = Self;

    fn add(self, other: Self) -> Self {
        Self(self.0.saturating_add(other.0))
    }
}

impl std::iter::Sum for Bytes {
    fn sum<I: Iterator<Item = Self>>(iter: I) -> Self {
        iter.fold(Self::default(), std::ops::Add::add)
    }
}

/// The largest 1024-based unit that keeps a whole part, to one decimal:
/// `812 B`, `1.2 KB`, `3.4 MB`, `1.1 GB`.
impl fmt::Display for Bytes {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.0 < UNIT {
            return write!(f, "{} B", self.0);
        }

        let mut scale = UNIT;
        for label in ["KB", "MB"] {
            let scaled = tenths(self.0, scale);
            if scaled < UNIT * 10 {
                return write!(f, "{}.{} {label}", scaled / 10, scaled % 10);
            }
            scale *= UNIT;
        }

        let giga = tenths(self.0, scale);
        write!(f, "{}.{} GB", giga / 10, giga % 10)
    }
}

/// `bytes / unit` in tenths, rounded half up.
const fn tenths(bytes: u64, unit: u64) -> u64 {
    (bytes.saturating_mul(10).saturating_add(unit / 2)) / unit
}

#[cfg(test)]
mod tests {
    use super::*;

    fn drawn(bytes: u64) -> String {
        Bytes::new(bytes).to_string()
    }

    #[test]
    fn a_size_is_1024_based_to_one_decimal() {
        assert_eq!(drawn(0), "0 B");
        assert_eq!(drawn(812), "812 B");
        assert_eq!(drawn(1023), "1023 B");
        assert_eq!(drawn(1024), "1.0 KB");
        assert_eq!(drawn(1229), "1.2 KB");
        assert_eq!(drawn(3_565_158), "3.4 MB");
        assert_eq!(drawn(100 * 1024 * 1024), "100.0 MB");
        assert_eq!(drawn(1_181_116_006), "1.1 GB");
    }

    #[test]
    fn a_value_that_rounds_up_to_the_next_unit_is_drawn_in_it() {
        assert_eq!(drawn(1024 * 1024 - 1), "1.0 MB");
    }
}

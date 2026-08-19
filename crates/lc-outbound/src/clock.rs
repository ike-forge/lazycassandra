//! The one place the wall-clock is read.
//!
//! `lc-domain` has no clock and no way to grow one (architecture.md §2); it
//! is handed [`Stamp`]s from here, through `Msg::Tick` and `init`
//! (query log design §4).

use std::time::{SystemTime, UNIX_EPOCH};

use lc_domain::clock::Stamp;

/// The current time of day, in UTC. No local offset is applied.
///
/// Derived from the Unix epoch rather than through a calendar crate: the
/// domain's dependency list is the one being protected, but adding a
/// civil-time library here to compute three integers is not worth the tree
/// either.
#[must_use]
pub fn now() -> Stamp {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_secs());

    let day = seconds % 86_400;

    Stamp::new(
        u8::try_from(day / 3_600).unwrap_or(0),
        u8::try_from((day % 3_600) / 60).unwrap_or(0),
        u8::try_from(day % 60).unwrap_or(0),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    // Not asserting the value — that would read the clock twice and race. The
    // property that matters is that every field is in range whatever the
    // clock said.
    #[test]
    fn the_clock_produces_a_time_of_day() {
        let stamp = now().to_string();

        assert_eq!(stamp.len(), 8);
        assert_eq!(stamp.as_bytes()[2], b':');
        assert_eq!(stamp.as_bytes()[5], b':');
    }
}

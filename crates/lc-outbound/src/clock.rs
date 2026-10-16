//! The one place the wall-clock is read; the domain is handed [`Stamp`]s.

use std::time::{SystemTime, UNIX_EPOCH};

use lc_domain::clock::Stamp;

/// UTC. Derived from the epoch: a calendar crate for three integers is not
/// worth it.
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

    // Asserting the value would read the clock twice and race.
    #[test]
    fn the_clock_produces_a_time_of_day() {
        let stamp = now().to_string();

        assert_eq!(stamp.len(), 8);
        assert_eq!(stamp.as_bytes()[2], b':');
        assert_eq!(stamp.as_bytes()[5], b':');
    }
}

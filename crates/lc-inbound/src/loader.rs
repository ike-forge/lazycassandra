//! The hint bar's loader: whether the tool is waiting on the cluster.
//!
//! The widget's `tick` is an integer advance over a fixed cycle, so the phase
//! can live in the domain and reach here as a frame number. That is what keeps
//! the renderer a pure function of the [`ViewModel`](lc_domain::ViewModel) and
//! the render tier deterministic — no clock is read on this side of the
//! boundary (doc/specs/2026-08-09-visual-polish-design.md §4.3).

use lc_domain::view::Activity;
use ratatui::text::Line;
use ratatui_opentui_loader::KittLoader;

/// Cells the scanner occupies, and the gap before the first hint.
pub(crate) const WIDTH: u16 = 8;
pub(crate) const GAP: u16 = 1;

/// Frames before the cycle repeats: `WIDTH` out, `HOLD_END` at the right edge,
/// `WIDTH - 1` back, `HOLD_START` at the left. The widget keeps its own frame
/// counter private, so `the_cycle_is_the_length_the_domain_is_folded_into`
/// pins this rather than a comment claiming it.
const PERIOD: usize = 54;
const TRAIL: usize = 6;
const HOLD_START: usize = 30;
const HOLD_END: usize = 9;
/// Brightness of a dot the head has left behind, and the floor the held edges
/// breathe down to.
const INACTIVE: f64 = 0.25;
const MIN_FADE: f64 = 0.55;

/// The scanner at `activity`'s phase, `WIDTH` cells wide.
pub(crate) fn line(activity: Activity) -> Line<'static> {
    let frame = match activity {
        Activity::Resting => 0,
        Activity::Working { frame } => frame % PERIOD,
    };

    let mut loader = KittLoader::build(
        crate::theme::Theme::default().palette().accent,
        usize::from(WIDTH),
        TRAIL,
        HOLD_START,
        HOLD_END,
        INACTIVE,
        MIN_FADE,
    );
    // `KittLoader` exposes no way to seek, and the cycle is 54 integer
    // increments; holding one across frames would cost the renderer the
    // statelessness that makes it testable.
    for _ in 0..frame {
        loader.tick();
    }

    loader.into_line(usize::from(WIDTH))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(activity: Activity) -> String {
        line(activity).to_string()
    }

    #[test]
    fn the_cycle_is_the_length_the_domain_is_folded_into() {
        let start = line(Activity::Working { frame: 0 });

        assert_eq!(
            line(Activity::Working { frame: PERIOD }),
            start,
            "PERIOD has to be the widget's own cycle, or the fold jumps the animation"
        );
        assert_ne!(
            line(Activity::Working { frame: PERIOD - 1 }),
            start,
            "and it has to be the shortest such length"
        );
    }

    #[test]
    fn resting_is_the_first_frame_of_the_cycle() {
        assert_eq!(
            line(Activity::Resting),
            line(Activity::Working { frame: 0 })
        );
    }

    #[test]
    fn the_scanner_is_always_the_width_it_is_given() {
        for frame in 0..PERIOD {
            assert_eq!(
                text(Activity::Working { frame }).chars().count(),
                usize::from(WIDTH),
                "frame {frame} did not fill the cells the hint bar gave up for it"
            );
        }
    }

    /// Where the bright head is, if it is on screen at all: the hold phases
    /// dissolve the trail entirely, which is the animation resting rather than
    /// a frame with nothing in it.
    fn head(activity: Activity) -> Option<usize> {
        line(activity)
            .spans
            .iter()
            .position(|span| span.style.fg == Some(crate::theme::Theme::default().palette().accent))
    }

    #[test]
    fn the_head_crosses_the_cells_while_working() {
        assert_eq!(head(Activity::Working { frame: 0 }), Some(0));
        assert_eq!(
            head(Activity::Working {
                frame: usize::from(WIDTH) - 1
            }),
            Some(usize::from(WIDTH) - 1),
            "it reaches the far edge in one pass of the cells"
        );
        assert_eq!(
            head(Activity::Working {
                frame: usize::from(WIDTH) + HOLD_END
            }),
            Some(usize::from(WIDTH) - 2),
            "and turns around after holding there"
        );
    }

    #[test]
    fn the_scanner_does_not_stand_still() {
        let frames: Vec<String> = (0..PERIOD)
            .map(|frame| text(Activity::Working { frame }))
            .collect();

        assert!(frames.iter().any(|frame| frame != &frames[0]));
    }
}

//! A panel's loader: whether that panel is waiting on the cluster.
//!
//! The widget's `tick` is an integer advance over a fixed cycle, so the phase
//! can live in the domain and reach here as a frame number. That is what keeps
//! the renderer a pure function of the [`ViewModel`](lc_domain::ViewModel) and
//! the render tier deterministic — no clock is read on this side of the
//! boundary (docs/superpowers/specs/2026-08-09-visual-polish-design.md §4.3).

use crate::theme::Theme;
use lc_domain::view::Activity;
use ratatui::layout::Rect;
use ratatui::text::Line;
use ratatui_opentui_loader::KittLoader;

/// Cells the scanner occupies, and the border it keeps between itself and the
/// corner it is drawn beside.
pub(crate) const WIDTH: u16 = 8;
pub(crate) const INSET: u16 = 1;

/// Frames before the cycle repeats: `WIDTH` out, `HOLD_END` at the right edge,
/// `WIDTH - 1` back, `HOLD_START` at the left. The widget keeps its own frame
/// counter private, so `the_cycle_is_the_length_the_domain_is_folded_into`
/// pins this rather than a comment claiming it.
const PERIOD: usize = 36;
const TRAIL: usize = 6;
/// The pause at the left edge, cut from the 30 frames the bar's loader held
/// for: a scanner on a border spends that pause as its dimmest dots against
/// border glyphs, and 1.2s of that per cycle reads as a spinner that has
/// stopped rather than one that is breathing.
const HOLD_START: usize = 12;
const HOLD_END: usize = 9;
/// Brightness of a dot the head has left behind, and the floor the held edges
/// breathe down to.
///
/// Both lifted off the bar's values (0.25 and 0.55): at those, a held dot came
/// out at 0.14 of the accent — darker than the base the panel is painted on,
/// so the loader's colour disappeared for the whole hold. The scanner has to
/// stay recognisably the accent through every frame of its cycle, because on a
/// border there is nothing else marking the cells as the loader's.
const INACTIVE: f64 = 0.45;
const MIN_FADE: f64 = 0.8;

/// Where the scanner is drawn on `block`, the panel's bordered rect: the
/// bottom border, `INSET` columns short of the corner, mirroring the search
/// marker's alignment on the top one.
///
/// `None` when the border cannot hold the scanner, its inset and both
/// corners. A panel too narrow to say *how* it is waiting says nothing rather
/// than drawing half a scanner — the rule the title marker already follows
/// (design §5.1).
pub(crate) fn placement(block: Rect) -> Option<Rect> {
    let spent = WIDTH + 2 * (1 + INSET);

    (block.width >= spent && block.height >= 2).then(|| Rect {
        x: block.right() - 1 - INSET - WIDTH,
        y: block.bottom() - 1,
        width: WIDTH,
        height: 1,
    })
}

/// The scanner at `activity`'s phase, `WIDTH` cells wide, or nothing at all
/// when the panel is not waiting on anything.
///
/// A resting panel draws no loader rather than a still one: an animation
/// frozen on a border reads as chrome the panel always has, and the whole
/// point of moving it off the bar is that it names *which* panel is waiting.
pub(crate) fn line(activity: Activity, theme: Theme) -> Option<Line<'static>> {
    let frame = match activity {
        Activity::Resting => return None,
        Activity::Working { frame } => frame % PERIOD,
    };

    let mut loader = KittLoader::build(
        theme.palette().accent,
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

    Some(loader.into_line(usize::from(WIDTH)))
}

/// The glyphs a pending entry's outcome spins through in the Logs trail, one
/// per frame — the same "an integer frame becomes a drawn thing, statelessly"
/// idiom [`line`]'s scanner already turns `Activity::Working` into, applied
/// to a single inline glyph rather than a bar (query log design §6).
const SPIN: [char; 10] = ['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];

/// The glyph a pending row draws at `frame`.
pub(crate) fn spin(frame: usize) -> char {
    SPIN[frame % SPIN.len()]
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::style::Color;

    fn drawn(activity: Activity) -> Line<'static> {
        line(activity, Theme::default()).expect("working frames draw a scanner")
    }

    fn text(activity: Activity) -> String {
        drawn(activity).to_string()
    }

    #[test]
    fn the_cycle_is_the_length_the_domain_is_folded_into() {
        let start = drawn(Activity::Working { frame: 0 });

        assert_eq!(
            drawn(Activity::Working { frame: PERIOD }),
            start,
            "PERIOD has to be the widget's own cycle, or the fold jumps the animation"
        );
        assert_ne!(
            drawn(Activity::Working { frame: PERIOD - 1 }),
            start,
            "and it has to be the shortest such length"
        );
    }

    #[test]
    fn a_resting_panel_draws_nothing() {
        assert_eq!(line(Activity::Resting, Theme::default()), None);
    }

    #[test]
    fn a_border_too_narrow_for_the_scanner_gives_it_no_room() {
        let spent = WIDTH + 2 * (1 + INSET);

        assert_eq!(placement(Rect::new(0, 0, spent - 1, 3)), None);
        assert_eq!(
            placement(Rect::new(0, 0, spent, 3)),
            Some(Rect::new(2, 2, WIDTH, 1)),
            "the scanner sits on the bottom border, INSET short of the corner"
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
        let accent = Theme::default().palette().accent;

        drawn(activity)
            .spans
            .iter()
            .position(|span| span.style.fg == Some(accent))
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

    /// The floor an idle dot has to keep, as a fraction of the accent: below
    /// this the scanner's colour is gone and the cells read as border.
    const VISIBLE: f64 = 0.3;

    #[test]
    fn an_idle_dot_stays_recognisably_the_accent_in_every_frame() {
        let theme = Theme::default();
        let Color::Rgb(accent, _, _) = theme.palette().accent else {
            panic!("the default palette is spelled in rgb");
        };
        #[expect(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "a fraction of a u8 channel"
        )]
        let floor = (f64::from(accent) * VISIBLE) as u8;

        for frame in 0..PERIOD {
            for span in drawn(Activity::Working { frame }).spans {
                if span.content != "⬝" {
                    continue;
                }

                let Some(Color::Rgb(red, _, _)) = span.style.fg else {
                    panic!("every cell is coloured");
                };

                assert!(
                    red >= floor,
                    "frame {frame} faded an idle dot to {red}, under the {floor} floor"
                );
            }
        }
    }

    #[test]
    fn the_scanner_does_not_stand_still() {
        let frames: Vec<String> = (0..PERIOD)
            .map(|frame| text(Activity::Working { frame }))
            .collect();

        assert!(frames.iter().any(|frame| frame != &frames[0]));
    }

    #[test]
    fn the_spinner_glyph_changes_with_the_frame_and_wraps() {
        assert_ne!(spin(0), spin(1), "consecutive frames must draw differently");
        assert_eq!(
            spin(0),
            spin(SPIN.len()),
            "the cycle repeats after every glyph is spent"
        );
    }
}

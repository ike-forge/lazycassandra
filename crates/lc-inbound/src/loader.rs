//! A panel's loader: whether that panel is waiting on the cluster.
//!
//! The phase lives in the domain and reaches here as a frame number. That keeps
//! the renderer a pure function of the [`ViewModel`](lc_domain::ViewModel) and
//! the render tier deterministic — no clock is read on this side of the
//! boundary (docs/superpowers/specs/2026-08-09-visual-polish-design.md §4.3).

use crate::theme::Theme;
use lc_domain::style::Token;
use lc_domain::view::Activity;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};

/// Cells the scanner occupies, and the border it keeps between itself and the
/// corner it is drawn beside.
pub(crate) const WIDTH: u16 = 8;
pub(crate) const INSET: u16 = 1;

/// Cells lit behind and including the head; the trail dissolves into the
/// border during each hold.
const TRAIL: usize = 3;
const HOLD_START: usize = 4;
const HOLD_END: usize = 4;
const PERIOD: usize = 2 * WIDTH as usize - 1 + HOLD_START + HOLD_END;

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
///
/// Spends `Muted` and `Faint` rather than a hue, and leaves idle cells
/// unstyled so they take the border's own colour, focused or not: the
/// scanner is a pulse along the frame, not a mark competing with the content.
pub(crate) fn line(activity: Activity, theme: Theme) -> Option<Line<'static>> {
    let frame = match activity {
        Activity::Resting => return None,
        Activity::Working { frame } => frame % PERIOD,
    };
    let width = usize::from(WIDTH);

    let (head, forward, hold) = if frame < width {
        (frame, true, 0)
    } else if frame < width + HOLD_END {
        (width - 1, true, frame - width)
    } else if frame < 2 * width - 1 + HOLD_END {
        (2 * width - 2 + HOLD_END - frame, false, 0)
    } else {
        (0, false, frame - (2 * width - 1 + HOLD_END))
    };

    let spans = (0..width).map(|cell| {
        let behind = if forward {
            head.checked_sub(cell)
        } else {
            cell.checked_sub(head)
        };

        match behind.map(|behind| behind + hold) {
            Some(0) => Span::styled("■", theme.style(Token::Muted)),
            Some(distance) if distance < TRAIL => Span::styled("■", theme.style(Token::Faint)),
            _ => Span::raw("⬝"),
        }
    });

    Some(spans.collect())
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
    use ratatui::style::Style;

    fn drawn(activity: Activity) -> Line<'static> {
        line(activity, Theme::default()).expect("working frames draw a scanner")
    }

    fn text(activity: Activity) -> String {
        drawn(activity).to_string()
    }

    #[test]
    fn the_cycle_rests_at_the_left_edge_before_it_repeats() {
        assert_eq!(text(Activity::Working { frame: PERIOD - 1 }), "⬝".repeat(8));
        assert_eq!(head(Activity::Working { frame: PERIOD }), Some(0));
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

    /// Where the head is, if it is on screen at all: the holds dissolve the
    /// trail entirely.
    fn head(activity: Activity) -> Option<usize> {
        let muted = Theme::default().style(Token::Muted);

        drawn(activity)
            .spans
            .iter()
            .position(|span| span.style == muted)
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
    fn idle_cells_take_the_border_colour_they_are_drawn_on() {
        for frame in 0..PERIOD {
            for span in drawn(Activity::Working { frame }).spans {
                if span.content == "⬝" {
                    assert_eq!(span.style, Style::new(), "frame {frame}");
                }
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

//! A panel's loader. The phase arrives from the domain as a frame number, so no
//! clock is read here and the render tier stays deterministic.

use crate::theme::Theme;
use lc_domain::style::Token;
use lc_domain::view::Activity;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};

/// Cells the scanner occupies, and its gap from the corner.
pub(crate) const WIDTH: u16 = 8;
pub(crate) const INSET: u16 = 1;

/// Cells lit behind and including the head; the trail dissolves during each
/// hold.
const TRAIL: usize = 3;
const HOLD_START: usize = 4;
const HOLD_END: usize = 4;
const PERIOD: usize = 2 * WIDTH as usize - 1 + HOLD_START + HOLD_END;

/// On the bottom border, mirroring the search marker on the top. `None` where
/// it does not fit whole.
pub(crate) fn placement(block: Rect) -> Option<Rect> {
    let spent = WIDTH + 2 * (1 + INSET);

    (block.width >= spent && block.height >= 2).then(|| Rect {
        x: block.right() - 1 - INSET - WIDTH,
        y: block.bottom() - 1,
        width: WIDTH,
        height: 1,
    })
}

/// A resting panel draws no loader: a frozen one reads as permanent chrome.
/// Idle cells stay unstyled and take the border's colour.
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

/// One glyph per frame for a pending Logs entry.
const SPIN: [char; 10] = ['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];

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

    /// The holds dissolve the trail entirely.
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

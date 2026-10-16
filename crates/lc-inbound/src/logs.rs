//! Drawing the trail: two lines per entry, the subject beside its time, then
//! the outcome indented under the subject.

use lc_domain::grid::BAR;
use lc_domain::search::Search;
use lc_domain::style::{Segment, Token};
use lc_domain::view::{Activity, LogRow, RUNNING};
use lc_domain::viewport::Span;
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::Line;
use ratatui::widgets::Paragraph;

use crate::body::{Marked, barred, patched};
use crate::theme::Theme;

/// Fixed: every stamp is the same width, and a ragged edge costs scannability.
const STAMP_W: u16 = 9;

pub(crate) struct Laid {
    pub(crate) lines: Vec<Line<'static>>,
    pub(crate) selection: Span,
    /// Every line here belongs to a row; `Option` matches
    /// [`crate::body::Unit::Lines`].
    pub(crate) rows: Vec<Option<usize>>,
}

/// `activity` is the panel's, so a pending entry spins without the domain
/// counting frames.
#[derive(Clone, Copy)]
pub(crate) struct LogsView<'a> {
    pub(crate) rows: &'a [LogRow],
    pub(crate) selected: usize,
    pub(crate) offset: usize,
    pub(crate) marked: &'a Marked,
    pub(crate) search: Option<&'a Search>,
    pub(crate) activity: Activity,
}

pub(crate) fn render(
    frame: &mut Frame,
    area: Rect,
    view: &LogsView,
    theme: Theme,
) -> (crate::body::Extent, Span, Vec<Option<usize>>) {
    let LogsView {
        rows,
        selected,
        offset,
        marked,
        search,
        activity,
    } = *view;

    let laid = lay_out(
        rows,
        selected,
        marked,
        usize::from(area.width),
        search,
        activity,
        theme,
    );
    let total = laid.lines.len();
    let scroll = u16::try_from(offset).unwrap_or(u16::MAX);

    frame.render_widget(Paragraph::new(laid.lines).scroll((scroll, 0)), area);

    (
        crate::body::Extent {
            position: offset,
            total,
            visible: usize::from(area.height),
        },
        laid.selection,
        laid.rows,
    )
}

/// Measured over every line, never only those in view (architecture.md §3).
pub(crate) fn lay_out(
    rows: &[LogRow],
    selected: usize,
    marked: &Marked,
    width: usize,
    search: Option<&Search>,
    activity: Activity,
    theme: Theme,
) -> Laid {
    let content = width.saturating_sub(usize::from(BAR));
    let mut lines: Vec<Line<'static>> = Vec::new();
    let mut mapped: Vec<Option<usize>> = Vec::new();
    let mut selection = Span::default();

    for (index, row) in rows.iter().enumerate() {
        let mut drawn = entry_lines(row, content, search, activity, theme);

        if marked.holds(index) {
            let band = theme.style(marked.band);
            drawn = drawn.into_iter().map(|line| patched(line, band)).collect();
        }

        if index == selected {
            let highlight = theme.style(Token::Selected);
            drawn = drawn
                .into_iter()
                .map(|line| patched(line, highlight))
                .collect();
            selection = Span {
                first: lines.len(),
                lines: drawn.len(),
            };
        }

        let bar = index == selected || marked.holds(index);
        mapped.extend(std::iter::repeat_n(Some(index), drawn.len()));
        lines.extend(drawn.into_iter().map(|line| barred(line, bar, theme)));
    }

    Laid {
        lines,
        selection,
        rows: mapped,
    }
}

/// Empty `outcome` is the dropped-count row. The glyph spins before wrapping,
/// while it is still a segment.
fn entry_lines(
    row: &LogRow,
    width: usize,
    search: Option<&Search>,
    activity: Activity,
    theme: Theme,
) -> Vec<Line<'static>> {
    let indent = usize::from(STAMP_W);

    let mut subject = crate::wrap::wrap(&crate::wrap::chars_of(&row.subject, theme), width, indent);
    if let Some(at) = row.at.as_deref()
        && let Some(first) = subject.first_mut()
    {
        *first = stamped(std::mem::take(first), at, theme);
    }

    let animated = animated_outcome(&row.outcome, activity);
    let outcome = crate::wrap::wrap(&crate::wrap::chars_of(&animated, theme), width, indent);

    subject
        .into_iter()
        .chain(outcome)
        .map(|chars| crate::wrap::line(chars, search, theme))
        .collect()
}

/// Only a still-pending sentinel spins; anything else draws as written.
fn animated_outcome(outcome: &[Segment], activity: Activity) -> Vec<Segment> {
    if let Activity::Working { frame } = activity
        && let [segment] = outcome
        && segment.text == RUNNING
    {
        return vec![Segment {
            text: format!("{}{}", crate::loader::spin(frame), running_tail()),
            token: segment.token,
        }];
    }

    outcome.to_vec()
}

/// Derived from `RUNNING`: the animated string must keep its char count, or
/// `log_lines` would vary per frame.
fn running_tail() -> &'static str {
    match RUNNING.char_indices().nth(1) {
        Some((at, _)) => &RUNNING[at..],
        None => "",
    }
}

/// The wrap indent and the stamp column are the same width.
fn stamped(line: Vec<(char, Style)>, at: &str, theme: Theme) -> Vec<(char, Style)> {
    let stamp = theme.style(Token::Faint);
    let width = usize::from(STAMP_W);
    let text: Vec<char> = format!("{at:<width$}").chars().collect();

    line.into_iter()
        .enumerate()
        .map(|(index, (character, style))| {
            text.get(index)
                .map_or((character, style), |&glyph| (glyph, stamp))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use lc_domain::selection::Ranges;
    use lc_domain::style::{Segment, Severity, Token};
    use lc_domain::view::LogRow;
    use lc_domain::{Ours, ThemeChoice};
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    use super::*;

    fn row(at: &str, subject: &str, outcome: &str, severity: Severity) -> LogRow {
        LogRow {
            at: Some(at.to_owned()),
            subject: vec![Segment::plain(subject)],
            outcome: vec![Segment::styled(outcome, Token::Severity(severity))],
            severity,
        }
    }

    fn drawn(rows: &[LogRow], width: u16, height: u16) -> String {
        drawn_at(rows, width, height, Activity::Resting)
    }

    fn drawn_at(rows: &[LogRow], width: u16, height: u16, activity: Activity) -> String {
        let mut terminal =
            Terminal::new(TestBackend::new(width, height)).expect("the test backend starts");
        terminal
            .draw(|frame| {
                let area = frame.area();
                render(
                    frame,
                    area,
                    &LogsView {
                        rows,
                        selected: 0,
                        offset: 0,
                        marked: &Marked::new(Ranges::default(), false),
                        search: None,
                        activity,
                    },
                    Theme::of(&ThemeChoice::Ours(Ours::DefaultDark)),
                );
            })
            .expect("the frame draws");

        terminal.backend().to_string()
    }

    #[test]
    fn an_entry_is_drawn_as_its_time_its_subject_and_its_outcome() {
        let drawn = drawn(
            &[row(
                "14:02:14",
                "SELECT * FROM store.orders LIMIT 200",
                "→ 200 rows, 41ms",
                Severity::Info,
            )],
            80,
            6,
        );

        assert!(drawn.contains("14:02:14"), "{drawn}");
        assert!(
            drawn.contains("SELECT * FROM store.orders LIMIT 200"),
            "{drawn}"
        );
        assert!(drawn.contains("200 rows, 41ms"), "{drawn}");
    }

    // A statement wraps rather than truncating: half of it is not what ran.
    #[test]
    fn a_statement_wider_than_the_panel_wraps_rather_than_truncating() {
        let long = format!("SELECT {} FROM store.orders", "column_name, ".repeat(12));
        let drawn = drawn(
            &[row("14:02:14", &long, "→ 1 row, 3ms", Severity::Info)],
            40,
            12,
        );

        assert!(drawn.contains("SELECT"), "{drawn}");
        assert!(
            drawn.contains("FROM store.orders"),
            "the tail of the statement survived the wrap: {drawn}"
        );
    }

    #[test]
    fn a_pending_entry_is_drawn_as_still_running() {
        let drawn = drawn(
            &[row("14:02:14", "Reading schema", RUNNING, Severity::Info)],
            60,
            6,
        );

        assert!(drawn.contains("running"), "{drawn}");
    }

    // A hung query and a fast one must not look identical.
    #[test]
    fn a_pending_entrys_glyph_spins_with_the_frame() {
        let pending = [row("14:02:14", "Reading schema", RUNNING, Severity::Info)];

        let first = drawn_at(&pending, 60, 6, Activity::Working { frame: 0 });
        let second = drawn_at(&pending, 60, 6, Activity::Working { frame: 1 });

        assert_ne!(
            first, second,
            "the outcome row must draw differently across frames"
        );
    }

    #[test]
    fn a_resting_panel_draws_the_pending_sentinel_unanimated() {
        let pending = [row("14:02:14", "Reading schema", RUNNING, Severity::Info)];

        let first = drawn_at(&pending, 60, 6, Activity::Resting);
        let second = drawn_at(&pending, 60, 6, Activity::Resting);

        assert_eq!(
            first, second,
            "with nothing outstanding there is no frame to animate from"
        );
        assert!(first.contains(RUNNING), "{first}");
    }

    // A frame-dependent width would make the wrap count, and so `Geometry`,
    // vary per frame.
    #[test]
    fn the_animated_pending_outcome_is_always_the_same_char_count_as_the_sentinel() {
        let pending = Segment::styled(RUNNING, Token::Severity(Severity::Info));

        for frame in 0..10 {
            let animated =
                animated_outcome(std::slice::from_ref(&pending), Activity::Working { frame });
            let [animated] = animated.as_slice() else {
                panic!("one segment in, one segment out");
            };

            assert_eq!(
                animated.text.chars().count(),
                RUNNING.chars().count(),
                "frame {frame}: {animated:?}"
            );
        }
    }

    #[test]
    fn a_dropped_count_row_is_drawn_without_a_time() {
        let dropped = LogRow {
            at: None,
            subject: vec![Segment::styled(
                "… 340 earlier entries dropped",
                Token::Faint,
            )],
            outcome: Vec::new(),
            severity: Severity::Info,
        };

        let drawn = drawn(&[dropped], 60, 6);

        assert!(drawn.contains("340 earlier entries dropped"), "{drawn}");
    }
}

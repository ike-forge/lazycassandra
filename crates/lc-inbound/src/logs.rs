//! Drawing a [`PanelBody::Log`](lc_domain::view::PanelBody::Log) body: the
//! trail's own renderer, structured like [`crate::details`] — the DDL body's
//! — because both are panels whose rows stand one to several wrapped lines
//! rather than one to a `List`/`Table` row of their own.
//!
//! An entry occupies two lines: the subject beside its time, then the
//! outcome indented to the column the subject starts in (query log design
//! §6).

use lc_domain::grid::BAR;
use lc_domain::search::Search;
use lc_domain::state::Span;
use lc_domain::style::{Segment, Token};
use lc_domain::view::{Activity, LogRow, RUNNING};
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span as TextSpan};
use ratatui::widgets::Paragraph;

use crate::body::{Marked, barred, patched};
use crate::theme::Theme;

/// `"HH:MM:SS"` and the space after it. Fixed because every stamp is the
/// same width — a ragged left edge would cost the scannability the trail is
/// for.
const STAMP_W: u16 = 9;

/// A body's lines, and where the selected row landed among them.
pub(crate) struct Laid {
    pub(crate) lines: Vec<Line<'static>>,
    pub(crate) selection: Span,
    /// The row each line was wrapped from, in step with `lines`. Every line
    /// in this panel belongs to a row — unlike Ddl there are no headings or
    /// group blanks — but the type stays `Option` to match what
    /// [`crate::body::Unit::Lines`] carries.
    pub(crate) rows: Vec<Option<usize>>,
}

/// The view model's Log body, as the renderer reads it.
///
/// `activity` is the Queries panel's own — never a frame the domain carries
/// on a `LogRow`, which stays a plain projection (design §6) — so a pending
/// entry's outcome can spin without the domain ever counting a frame.
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

/// Every line the body occupies at `width`, with the selected row highlighted
/// across all of its own.
///
/// The span is what the frame reports back as `Geometry::log_selection`, so
/// the domain can bound an offset over text it cannot wrap itself
/// (architecture.md §3) — measured against every line this function
/// produces, never against the ones a caller happens to scroll into view.
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

/// One entry's lines: its subject wrapped from [`STAMP_W`], its time
/// overwriting the blank the wrap opened with, then its outcome wrapped the
/// same way — skipped entirely where `outcome` is empty, which is the
/// dropped-count row (query log design §6). The outcome's leading glyph
/// spins first, while it is still segments — wrapping afterwards would have
/// to find it again inside already-split characters.
fn entry_lines(
    row: &LogRow,
    width: usize,
    search: Option<&Search>,
    activity: Activity,
    theme: Theme,
) -> Vec<Line<'static>> {
    let indent = usize::from(STAMP_W);

    let mut subject = crate::wrap::wrap(&chars_of(&row.subject, theme), width, indent);
    if let Some(at) = row.at.as_deref()
        && let Some(first) = subject.first_mut()
    {
        *first = stamped(std::mem::take(first), at, theme);
    }

    let animated = animated_outcome(&row.outcome, activity);
    let outcome = crate::wrap::wrap(&chars_of(&animated, theme), width, indent);

    subject
        .into_iter()
        .chain(outcome)
        .map(|chars| styled(chars, search, theme))
        .collect()
}

/// `outcome`, unless it is the trail's pending sentinel ([`RUNNING`]) and
/// `activity` says the panel is still waiting — in which case its leading
/// glyph is swapped for [`crate::loader::spin`]'s glyph at the current
/// frame, the one thing about a pending row this renderer draws
/// differently frame to frame (query log design §6). A resting or finished
/// panel — including a pending row left over from a frame the completion
/// has not yet reached — draws the sentinel exactly as the domain wrote it.
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

/// [`RUNNING`] past its first character — the part the spinner glyph
/// overwrites, and the part it never replaces. Derived rather than a second
/// `" running"` literal: once the animated string is actually drawn, its
/// char count matching `RUNNING`'s is what keeps `Geometry::log_lines` a
/// function of the terminal size and the view model alone, never of the
/// frame (architecture.md §3) — a second literal could drift from `RUNNING`
/// and nothing would catch it until the wrap count started oscillating.
fn running_tail() -> &'static str {
    match RUNNING.char_indices().nth(1) {
        Some((at, _)) => &RUNNING[at..],
        None => "",
    }
}

/// `segments`, flattened to one character per element, each carrying the
/// style its own segment's [`Token`] resolves to.
fn chars_of(segments: &[Segment], theme: Theme) -> Vec<(char, Style)> {
    segments
        .iter()
        .flat_map(|segment| {
            let style = segment
                .token
                .map_or_else(Style::default, |token| theme.style(token));
            segment
                .text
                .chars()
                .map(move |character| (character, style))
        })
        .collect()
}

/// `line`'s leading [`STAMP_W`] characters replaced with `at`, faint —
/// overwriting the blank indent [`crate::wrap::wrap`] always opens a line
/// with, since the indent and the stamp column are the same width by
/// construction.
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

/// `chars` as a [`Line`], grouped into spans wherever the style changes and
/// split further at whatever `search` matches inside each run — the same
/// split [`crate::highlight::spans`] does for a uniformly-styled value,
/// applied once per run rather than once per line.
fn styled(chars: Vec<(char, Style)>, search: Option<&Search>, theme: Theme) -> Line<'static> {
    let mut spans: Vec<TextSpan<'static>> = Vec::new();
    let mut run = String::new();
    let mut run_style: Option<Style> = None;

    for (character, style) in chars {
        if run_style != Some(style) {
            if let Some(previous) = run_style.take() {
                spans.extend(crate::highlight::spans(&run, search, previous, theme));
                run.clear();
            }
            run_style = Some(style);
        }
        run.push(character);
    }
    if let Some(style) = run_style {
        spans.extend(crate::highlight::spans(&run, search, style, theme));
    }

    Line::from(spans)
}

#[cfg(test)]
mod tests {
    use lc_domain::state::Ranges;
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

    // A long statement wraps rather than being cut off: the trail exists to
    // show what ran, and half of it is not what ran.
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

    // Design §6: the pending outcome animates from the panel's own
    // `Activity`, not a static glyph — a hung query and a fast one must not
    // look identical.
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

    // Geometry::log_lines is a function of the terminal size and the view
    // model alone (architecture.md §3): if the animated string's width
    // varied per frame, the wrap count would too, and a frame-dependent
    // value would cross into `Geometry`. `spin()` returning one char is what
    // keeps this true — verify it holds across the whole cycle, not just
    // frame 0.
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

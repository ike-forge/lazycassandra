//! What the config files got wrong, drawn over the panels at startup.
//!
//! A `List` rather than `overlay.rs`'s `Paragraph`: diagnostics are a list,
//! and that block's sizing under-counts an embedded newline by its own
//! admission, so a multi-line message truncates (design §9).

use lc_domain::style::Token;
use lc_domain::view::DiagnosticRow;
use ratatui::Frame;
use ratatui::layout::{Margin, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Clear, List, ListItem, ListState};

use crate::body::Extent;
use crate::theme::Theme;

const WIDTH_PERCENT: u16 = 70;
const HEIGHT_PERCENT: u16 = 60;
const MIN_W: u16 = 32;
const MIN_H: u16 = 6;
const MAX_W: u16 = 90;
/// Between the left border and the location.
const PAD: u16 = 1;
/// The severity's column, wide enough for "warning".
const SEVERITY: usize = 7;

fn rect(area: Rect, rows: usize) -> Option<Rect> {
    let width = percent(area.width, WIDTH_PERCENT).min(MAX_W);
    let wanted = u16::try_from(rows.saturating_mul(2).saturating_add(2)).unwrap_or(u16::MAX);
    let height = percent(area.height, HEIGHT_PERCENT).max(MIN_H).min(wanted);

    if width < MIN_W || area.height < MIN_H {
        return None;
    }

    Some(Rect::new(
        area.x + (area.width - width) / 2,
        area.y + (area.height - height) / 2,
        width,
        height,
    ))
}

fn percent(value: u16, share: u16) -> u16 {
    u16::try_from(u32::from(value) * u32::from(share) / 100).unwrap_or(u16::MAX)
}

pub(crate) fn render(frame: &mut Frame, rows: &[DiagnosticRow], selected: usize, theme: Theme) {
    let Some(area) = rect(frame.area(), rows.len()) else {
        return;
    };

    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(theme.border_style())
        .title("Configuration");
    let inner = block.inner(area).inner(Margin {
        horizontal: PAD,
        vertical: 0,
    });

    frame.render_widget(Clear, area);
    frame.buffer_mut().set_style(area, theme.overlay_ground());
    frame.render_widget(block, area);

    let items: Vec<ListItem> = rows.iter().map(|row| item(row, theme)).collect();
    let mut state = ListState::default().with_selected(Some(selected));

    frame.render_stateful_widget(
        List::new(items).highlight_style(theme.style(Token::Selected)),
        inner,
        &mut state,
    );

    let extent = Extent {
        position: state.offset(),
        total: rows.len(),
        visible: usize::from(inner.height) / 2,
    };

    if extent.overflows() {
        crate::render::scrollbar(frame, area, &extent, theme.border_style(), theme);
    }
}

/// Two lines: where, then how bad and what. The location is `Muted` because
/// the severity is what the eye should land on first.
fn item(row: &DiagnosticRow, theme: Theme) -> ListItem<'static> {
    let severity = match row.severity {
        lc_domain::style::Severity::Danger => "error",
        lc_domain::style::Severity::Warning => "warning",
        lc_domain::style::Severity::Info => "note",
    };

    ListItem::new(vec![
        Line::from(Span::styled(
            row.location.clone(),
            theme.style(Token::Muted),
        )),
        Line::from(vec![
            Span::styled(
                format!("  {severity:<SEVERITY$}"),
                theme.style(Token::Severity(row.severity)),
            ),
            Span::styled(row.message.clone(), theme.style(Token::Faint)),
        ]),
    ])
}

#[cfg(test)]
mod tests {
    use lc_domain::style::Severity;
    use lc_domain::{Ours, ThemeChoice};
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    use super::*;

    fn rows(count: usize) -> Vec<DiagnosticRow> {
        (0..count)
            .map(|index| DiagnosticRow {
                location: format!("./.lazycassandra.toml:{}:1", index + 1),
                severity: Severity::Warning,
                message: format!("unknown key `k{index}` — ignored"),
            })
            .collect()
    }

    fn drawn(rows: &[DiagnosticRow], width: u16, height: u16) -> String {
        let mut terminal =
            Terminal::new(TestBackend::new(width, height)).expect("the test backend starts");
        terminal
            .draw(|frame| {
                render(
                    frame,
                    rows,
                    0,
                    Theme::of(&ThemeChoice::Ours(Ours::DefaultDark)),
                );
            })
            .expect("the frame draws");

        terminal.backend().to_string()
    }

    #[test]
    fn a_diagnostic_is_drawn_with_the_file_and_the_position_it_names() {
        let drawn = drawn(&rows(1), 60, 12);

        assert!(drawn.contains("./.lazycassandra.toml:1:1"), "{drawn}");
        assert!(drawn.contains("unknown key `k0`"), "{drawn}");
        assert!(drawn.contains("Configuration"), "{drawn}");
    }

    #[test]
    fn a_list_longer_than_the_block_gets_a_scrollbar_rather_than_losing_entries() {
        let without_scrollbar = drawn(&rows(1), 60, 12);
        let with_scrollbar = drawn(&rows(40), 60, 12);

        // `▐` is the scrollbar thumb glyph (render.rs's `THUMB`), not the box
        // border's `│` — asserting on the border character would pass even
        // with the scrollbar deleted, which is the bug this test exists to
        // catch.
        assert!(
            !without_scrollbar.contains('▐'),
            "a block with everything visible must not draw a scrollbar thumb: {without_scrollbar}"
        );
        assert!(
            with_scrollbar.contains('▐'),
            "a config with more diagnostics than fit must not silently drop them: {with_scrollbar}"
        );
    }

    #[test]
    fn a_row_count_that_fits_by_lines_but_not_by_rows_still_gets_a_scrollbar() {
        // Each row is two lines. A block whose inner height fits `rows.len()`
        // *lines* still hides every row past `inner.height / 2` — the bug
        // this test exists to catch counted lines where it should have
        // counted rows.
        let drawn = drawn(&rows(7), 80, 24);

        assert!(
            drawn.contains('▐'),
            "seven two-line rows overflow a block that only fits six: {drawn}"
        );
    }

    #[test]
    fn a_frame_too_small_for_the_block_draws_nothing_rather_than_a_stub() {
        let drawn = drawn(&rows(1), 12, 3);

        assert!(!drawn.contains("Configuration"), "{drawn}");
    }
}

//! The table health block: four fifths of the frame, the metric tree inside
//! (docs/superpowers/specs/2026-10-13-table-health-design.md §3).

use lc_domain::health::{Fold, Indent, Place, Row, Section};
use lc_domain::style::{Segment, Severity, Token};
use lc_domain::view::{HealthBody, HealthView};
use ratatui::Frame;
use ratatui::layout::{Margin, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Clear, List, ListItem, ListState, Paragraph};

use crate::hit::{Listed, Modal};
use crate::theme::Theme;

const MIN_WIDTH: u16 = 40;
/// The border and three rows of body.
const MIN_HEIGHT: u16 = 5;
const PAD: u16 = 1;
/// Wide enough for `1023.9 MB` and `12345/s`, right-aligned so magnitudes line
/// up down the column.
const VALUE_WIDTH: usize = 10;
const GAP: &str = "  ";

/// Four fifths of the frame each way, centred. A function of the frame alone,
/// so the rows the domain pages by never move with the content.
fn rect(area: Rect) -> Option<Rect> {
    let width = area.width.saturating_mul(4) / 5;
    let height = area.height.saturating_mul(4) / 5;

    if width < MIN_WIDTH || height < MIN_HEIGHT {
        return None;
    }

    Some(Rect::new(
        area.x + (area.width - width) / 2,
        area.y + (area.height - height) / 2,
        width,
        height,
    ))
}

fn body(block: Rect) -> Rect {
    block.inner(Margin {
        horizontal: 1 + PAD,
        vertical: 1,
    })
}

/// Rows of body the block would show in `area`; zero where it does not fit,
/// which is what makes `H` inert there.
pub(crate) fn visible_rows(area: Rect) -> usize {
    rect(area).map_or(0, |block| usize::from(body(block).height))
}

/// Draws the block, and answers where it and its rows landed: a press inside
/// it is the block's, never the click outside that dismisses it.
pub(crate) fn render(frame: &mut Frame, view: &HealthView, theme: Theme) -> Option<Modal> {
    let area = rect(frame.area())?;

    let mut title = vec![Segment::plain("Health of ")];
    title.extend(view.table.segments());
    let mut block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(theme.focus_border())
        .title(crate::drop::title(&title, theme));
    if let Some(coverage) = &view.coverage {
        block = block.title_bottom(Line::from(Span::styled(
            format!(" {coverage} "),
            theme.style(Token::Muted),
        )));
    }
    let inner = body(area);

    frame.render_widget(Clear, area);
    frame.buffer_mut().set_style(area, theme.ground());
    frame.render_widget(block, area);

    if let Some(line) = crate::loader::line(view.activity, theme)
        && let Some(place) = crate::loader::placement(area)
    {
        frame.render_widget(line, place);
    }

    let list = match &view.body {
        HealthBody::Loading => {
            frame.render_widget(
                Paragraph::new(Span::styled(
                    "Asking every node…",
                    theme.style(Token::Faint),
                )),
                inner,
            );
            None
        }
        HealthBody::Failed(reason) => {
            frame.render_widget(
                Paragraph::new(Span::styled(
                    reason.clone(),
                    theme.style(Token::Severity(Severity::Danger)),
                )),
                inner,
            );
            None
        }
        HealthBody::Rows { rows, selected } => {
            let names = rows
                .iter()
                .filter(|row| !is_sentence(row))
                .map(|row| lead(row).chars().count() + row.label.chars().count())
                .max()
                .unwrap_or(0);
            let items: Vec<ListItem> = rows
                .iter()
                .map(|row| ListItem::new(line(row, names, theme)))
                .collect();
            let mut state = ListState::default().with_selected(Some(*selected));

            frame.render_stateful_widget(
                List::new(items).highlight_style(theme.style(Token::Selected)),
                inner,
                &mut state,
            );

            Some(Listed {
                area: inner,
                first: state.offset(),
                len: rows.len(),
            })
        }
    };

    Some(Modal::new(area, Vec::new()).with_list(list))
}

/// A warning, or a node's status: prose rather than a name, so it sets no
/// width for the name column and runs on instead.
fn is_sentence(row: &Row) -> bool {
    matches!(
        row.place,
        Place::Leaf(Fold::Section(Section::Warnings | Section::Nodes))
    )
}

/// The limbs and the fold marker in front of a row's label.
fn lead(row: &Row) -> String {
    let limbs = match row.indent {
        Indent::Root => String::new(),
        Indent::Child { last } => limb(last).to_owned(),
        Indent::Grandchild { last, parent_last } => {
            format!("{}{}", if parent_last { "  " } else { "│ " }, limb(last))
        }
    };
    let marker = match row.fold {
        Some(true) => "▾ ",
        Some(false) => "▸ ",
        None => "",
    };

    format!("{limbs}{marker}")
}

const fn limb(last: bool) -> &'static str {
    if last { "└ " } else { "├ " }
}

fn mark(severity: Severity) -> &'static str {
    match severity {
        Severity::Info => "ℹ",
        Severity::Warning => "⚠",
        Severity::Danger => "✖",
    }
}

fn line(row: &Row, names: usize, theme: Theme) -> Line<'static> {
    let label_style = match row.indent {
        Indent::Root => theme.style(Token::Section),
        Indent::Child { .. } | Indent::Grandchild { .. } => Style::default(),
    };
    let severity = row
        .severity
        .map(|severity| theme.style(Token::Severity(severity)));
    let lead = lead(row);
    let mut spans = vec![
        Span::styled(lead.clone(), theme.style(Token::Faint)),
        Span::styled(
            row.label.clone(),
            severity.filter(|_| is_sentence(row)).unwrap_or(label_style),
        ),
    ];

    if is_sentence(row) {
        for (text, token) in [(&row.value, Token::Muted), (&row.note, Token::Faint)] {
            if !text.is_empty() {
                spans.push(Span::raw(GAP));
                spans.push(Span::styled(text.clone(), theme.style(token)));
            }
        }
    } else {
        let width = lead.chars().count() + row.label.chars().count();
        spans.push(Span::raw(" ".repeat(names.saturating_sub(width))));
        if !row.value.is_empty() {
            spans.push(Span::raw(GAP));
            spans.push(Span::styled(
                format!("{:>VALUE_WIDTH$}", row.value),
                severity.unwrap_or_default(),
            ));
        }
        if !row.note.is_empty() {
            spans.push(Span::raw(GAP));
            spans.push(Span::styled(row.note.clone(), theme.style(Token::Faint)));
        }
    }

    if let (Some(severity), Some(style)) = (row.severity, severity) {
        spans.push(Span::raw(GAP));
        spans.push(Span::styled(mark(severity), style));
    }

    Line::from(spans)
}

#[cfg(test)]
mod tests {
    use lc_domain::ThemeChoice;
    use lc_domain::contents::TableRef;
    use lc_domain::health::{
        Health, HealthPanel, Metric, NodeOutcome, NodeReport, Readings, Thresholds,
    };
    use lc_domain::model::QueryId;
    use lc_domain::view::Activity;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    use super::*;

    fn drawn(width: u16, height: u16, view: &HealthView) -> String {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).expect("a backend");
        let theme = Theme::of(&ThemeChoice::default());

        terminal
            .draw(|frame| {
                render(frame, view, theme);
            })
            .expect("a frame");

        terminal
            .backend()
            .buffer()
            .content()
            .chunks(usize::from(width))
            .map(|row| {
                row.iter()
                    .map(ratatui::buffer::Cell::symbol)
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn loaded() -> HealthView {
        let mut readings = Readings::default();
        readings.set(Metric::DiskUsed, 3_565_158.0);
        readings.set(Metric::TombstonesMax, 4200.0);
        let health = Health {
            nodes: vec![NodeReport {
                address: "10.0.0.1".to_owned(),
                datacenter: Some("dc1".to_owned()),
                version: Some("5.0.8".to_owned()),
                gossip: None,
                outcome: NodeOutcome::Read {
                    readings,
                    thresholds: Thresholds {
                        tombstone_warn: Some(1000.0),
                        ..Thresholds::default()
                    },
                },
            }],
        };
        let mut panel = HealthPanel::new(TableRef::new("shop", "orders"), QueryId::next());
        panel.landed(Ok(health));

        HealthView {
            table: TableRef::new("shop", "orders"),
            body: HealthBody::Rows {
                rows: panel.rows(),
                selected: 0,
            },
            coverage: Some("1 of 1 nodes answered".to_owned()),
            activity: Activity::Resting,
        }
    }

    #[test]
    fn the_block_takes_four_fifths_of_the_frame() {
        assert_eq!(
            rect(Rect::new(0, 0, 100, 50)),
            Some(Rect::new(10, 5, 80, 40))
        );
        assert_eq!(rect(Rect::new(0, 0, 40, 50)), None, "too narrow");
    }

    #[test]
    fn the_title_names_the_table_and_the_bottom_border_how_many_nodes_answered() {
        let frame = drawn(100, 30, &loaded());

        assert!(frame.contains("Health of shop.orders"));
        assert!(frame.contains(" 1 of 1 nodes answered "));
    }

    #[test]
    fn a_warning_names_the_node_and_its_metric_carries_the_mark() {
        let frame = drawn(100, 30, &loaded());

        assert!(
            frame.contains("4200 tombstones in one read, warn threshold 1000  10.0.0.1  ⚠"),
            "{frame}"
        );
        assert!(frame.contains("▸ Tombstones/read, max"), "{frame}");
        assert!(frame.contains("3.4 MB"), "{frame}");
    }

    #[test]
    fn a_waiting_block_draws_the_loader_and_says_what_it_waits_on() {
        let view = HealthView {
            table: TableRef::new("shop", "orders"),
            body: HealthBody::Loading,
            coverage: None,
            activity: Activity::Working { frame: 0 },
        };
        let frame = drawn(100, 30, &view);

        assert!(frame.contains("Asking every node…"));
        assert!(frame.contains('■'), "the scanner is on the border");
    }

    /// `view` drawn in a `width`×`height` frame, as the presses it answers.
    fn pressed(width: u16, height: u16, view: &HealthView) -> crate::hit::FrameMap {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).expect("a backend");
        let mut modal = None;
        terminal
            .draw(|frame| modal = render(frame, view, Theme::of(&ThemeChoice::default())))
            .expect("a frame");

        crate::hit::FrameMap::default().with_modal(modal)
    }

    #[test]
    fn a_press_on_a_row_names_it_and_the_rest_of_the_block_answers_nothing() {
        use lc_domain::pointer::Pointer;

        // The block's body starts at (12, 4) in a 100×30 frame.
        let map = pressed(100, 30, &loaded());

        assert_eq!(map.press(12, 4), Some(Pointer::Entry(0)));
        assert_eq!(map.press(80, 5), Some(Pointer::Entry(1)), "the whole width");
        assert_eq!(map.press(12, 3), Some(Pointer::Modal), "the title border");
        assert_eq!(map.press(12, 26), Some(Pointer::Modal), "the bottom border");
        assert_eq!(map.press(0, 0), None, "outside, which dismisses");
    }

    #[test]
    fn a_press_on_a_scrolled_block_counts_from_the_row_at_its_top() {
        use lc_domain::pointer::Pointer;

        let mut view = loaded();
        let HealthBody::Rows { rows, selected } = &mut view.body else {
            unreachable!("loaded");
        };
        *selected = rows.len() - 1;
        let last = *selected;

        // Six rows of body, at y=2..8 in a 100×10 frame.
        let map = pressed(100, 10, &view);

        assert_eq!(map.press(12, 7), Some(Pointer::Entry(last)));
        assert_eq!(map.press(12, 2), Some(Pointer::Entry(last - 5)));
    }

    #[test]
    fn a_waiting_block_still_claims_its_presses() {
        use lc_domain::pointer::Pointer;

        let view = HealthView {
            body: HealthBody::Loading,
            ..loaded()
        };

        assert_eq!(pressed(100, 30, &view).press(12, 4), Some(Pointer::Modal));
    }

    #[test]
    fn a_frame_too_small_has_no_rows_for_the_block() {
        assert_eq!(visible_rows(Rect::new(0, 0, 30, 10)), 0);
        assert_eq!(visible_rows(Rect::new(0, 0, 100, 50)), 38);
    }
}

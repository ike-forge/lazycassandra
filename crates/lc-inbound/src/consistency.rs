//! The consistency picker. Eleven fixed rows, so it never scrolls: a frame too
//! short for all of them refuses it.

use ratatui::Frame;
use ratatui::layout::{Margin, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Clear, List, ListItem, ListState};

use lc_domain::config::Consistency;
use lc_domain::style::Token;
use lc_domain::view::ConsistencyRow;

use crate::theme::Theme;

/// The marker, the widest label and a gap: a meaning may clip, a level may not.
const MIN_WIDTH: u16 = 2 + 2 + LABEL + 2;
const MAX_WIDTH: u16 = 72;
const LABEL: u16 = 12;
const PAD: u16 = 1;
const CURRENT: &str = "● ";

/// Below `Consistency::ALL.len()` gates `ctrl-l`.
pub(crate) fn visible_rows(area: Rect) -> usize {
    if area.width < MIN_WIDTH || area.height < 2 {
        return 0;
    }

    usize::from(area.height - 2)
}

fn rect(area: Rect) -> Option<Rect> {
    let rows = Consistency::ALL.len();

    if rows > visible_rows(area) {
        return None;
    }

    let height = u16::try_from(rows + 2).unwrap_or(u16::MAX);
    let width = area.width.saturating_sub(4).clamp(MIN_WIDTH, MAX_WIDTH);

    Some(Rect::new(
        area.x + (area.width - width) / 2,
        area.y + (area.height - height) / 2,
        width,
        height,
    ))
}

pub(crate) fn render(frame: &mut Frame, rows: &[ConsistencyRow], selected: usize, theme: Theme) {
    let Some(area) = rect(frame.area()) else {
        return;
    };

    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(theme.focus_border())
        .title("Consistency");
    let inner = block.inner(area).inner(Margin {
        horizontal: PAD,
        vertical: 0,
    });

    frame.render_widget(Clear, area);
    frame.buffer_mut().set_style(area, theme.ground());
    frame.render_widget(block, area);

    let items: Vec<ListItem> = rows
        .iter()
        .map(|row| {
            ListItem::new(Line::from(vec![
                Span::raw(if row.current { CURRENT } else { "  " }),
                Span::raw(format!(
                    "{:<width$}  ",
                    row.label,
                    width = usize::from(LABEL)
                )),
                Span::styled(row.meaning, theme.style(Token::Muted)),
            ]))
        })
        .collect();
    let mut state = ListState::default().with_selected(Some(selected));

    frame.render_stateful_widget(
        List::new(items).highlight_style(theme.style(Token::Selected)),
        inner,
        &mut state,
    );
}

#[cfg(test)]
mod tests {
    use lc_domain::ThemeChoice;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::buffer::Cell;

    use super::*;

    fn rows(current: Consistency) -> Vec<ConsistencyRow> {
        Consistency::ALL
            .iter()
            .map(|level| ConsistencyRow {
                label: level.label(),
                meaning: level.meaning(),
                current: *level == current,
            })
            .collect()
    }

    fn drawn(width: u16, height: u16, current: Consistency) -> Vec<String> {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).expect("a backend");
        let theme = Theme::of(&ThemeChoice::default());

        terminal
            .draw(|frame| render(frame, &rows(current), 0, theme))
            .expect("a frame");

        let buffer = terminal.backend().buffer();
        buffer
            .content()
            .chunks(usize::from(width))
            .map(|line| line.iter().map(Cell::symbol).collect())
            .collect()
    }

    #[test]
    fn the_picker_lists_every_level_with_what_it_asks_for() {
        let frame = drawn(80, 24, Consistency::LocalQuorum).join("\n");

        for level in Consistency::ALL {
            assert!(frame.contains(level.label()), "{level:?} is missing");
            assert!(frame.contains(level.meaning()), "{level:?} is unexplained");
        }
    }

    #[test]
    fn the_level_in_force_is_marked_and_no_other() {
        let frame = drawn(80, 24, Consistency::One);
        let marked: Vec<&String> = frame.iter().filter(|line| line.contains('●')).collect();

        assert_eq!(marked.len(), 1);
        assert!(marked[0].contains("● ONE "), "{marked:?}");
    }

    #[test]
    fn a_frame_too_short_for_every_level_draws_none_of_them() {
        let rows = u16::try_from(Consistency::ALL.len()).expect("small");
        let frame = drawn(80, rows + 1, Consistency::One).join("\n");

        assert!(
            !frame.contains("LOCAL_SERIAL"),
            "a clipped list hides a level"
        );
    }
}

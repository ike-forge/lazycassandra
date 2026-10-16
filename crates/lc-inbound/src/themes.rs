//! The theme picker. Config themes make its list unbounded, so it gates on its
//! own [`visible_rows`], not the palette's.

use ratatui::Frame;
use ratatui::layout::{Margin, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Clear, List, ListItem, ListState};

use lc_domain::search::Search;
use lc_domain::style::Token;

use crate::theme::Theme;

const WIDTH: u16 = 28;
const PAD: u16 = 1;

fn rect(area: Rect, rows: usize) -> Option<Rect> {
    let height = u16::try_from(rows.saturating_add(2)).unwrap_or(u16::MAX);

    if rows > visible_rows(area) {
        return None;
    }

    Some(Rect::new(
        area.x + (area.width - WIDTH) / 2,
        area.y + (area.height - height) / 2,
        WIDTH,
        height,
    ))
}

/// Does not scroll: a longer list is refused rather than opened dead. Zero
/// gates `ctrl-t`.
pub(crate) fn visible_rows(area: Rect) -> usize {
    if area.width < WIDTH || area.height < 2 {
        return 0;
    }

    usize::from(area.height - 2)
}

/// Sized from `total`, so a sift moves no border.
pub(crate) fn render(
    frame: &mut Frame,
    labels: &[String],
    total: usize,
    selected: usize,
    search: Option<&Search>,
    theme: Theme,
) {
    let Some(area) = rect(frame.area(), total) else {
        return;
    };

    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(theme.focus_border())
        .title("Theme");
    let inner = block.inner(area).inner(Margin {
        horizontal: PAD,
        vertical: 0,
    });

    frame.render_widget(Clear, area);
    frame.buffer_mut().set_style(area, theme.ground());
    frame.render_widget(block, area);

    // Says so rather than drawing an empty box; no row wears the cursor ground.
    if labels.is_empty() {
        frame.render_widget(
            List::new([ListItem::new(Line::from(Span::styled(
                "no theme matches",
                theme.style(Token::Faint),
            )))]),
            inner,
        );
        return;
    }

    let items: Vec<ListItem> = labels
        .iter()
        .map(|label| {
            ListItem::new(Line::from(crate::highlight::spans(
                label,
                search,
                Style::default(),
                theme,
            )))
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
    use lc_domain::overlay::Overlay;
    use lc_domain::state::State;
    use lc_domain::view::view_model;
    use lc_domain::viewport::Cursor;
    use lc_domain::{BuiltIn, Ported, ThemeChoice};
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::buffer::Cell;

    use super::*;

    fn index_of(built_in: BuiltIn) -> usize {
        BuiltIn::ALL
            .iter()
            .position(|b| *b == built_in)
            .expect("listed")
    }

    fn drawn(width: u16, height: u16, choice: &ThemeChoice) -> String {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).expect("a backend");
        let mut state = State::default();
        let mut cursor = Cursor::default();
        let index = BuiltIn::ALL
            .iter()
            .position(|built_in| ThemeChoice::from(*built_in) == *choice)
            .unwrap_or(0);
        cursor.go_to(index, BuiltIn::ALL.len());
        state.theme = choice.clone();
        state.overlay = Some(Overlay::Themes {
            cursor,
            restored: choice.clone(),
            sift: None,
        });

        terminal
            .draw(|frame| {
                crate::render::render(frame, &view_model(&state), Theme::of(choice));
            })
            .expect("a frame");

        terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(Cell::symbol)
            .collect()
    }

    #[test]
    fn the_picker_lists_every_theme() {
        let frame = drawn(80, 24, &ThemeChoice::default());

        for built_in in BuiltIn::ALL {
            let label = built_in.label();
            assert!(frame.contains(label), "{label} is missing");
        }
    }

    #[test]
    fn a_frame_too_narrow_for_the_list_draws_none_of_it() {
        let frame = drawn(WIDTH - 1, 24, &ThemeChoice::default());

        assert!(
            !frame.contains("Catppuccin"),
            "a truncated theme name is a lie about which one it is"
        );
    }

    fn every_label() -> Vec<String> {
        BuiltIn::ALL
            .iter()
            .map(|built_in| built_in.label().to_owned())
            .collect()
    }

    /// Pinned to the matched label's own cell: an unrelated glyph could share
    /// the colour.
    #[test]
    fn a_matched_span_is_marked_in_a_label() {
        let mut terminal = Terminal::new(TestBackend::new(80, 24)).expect("a backend");
        let choice = ThemeChoice::default();
        let search = Search::new("quill");
        let total = every_label().len();
        let kept: Vec<String> = every_label()
            .into_iter()
            .filter(|label| search.as_ref().expect("non-empty").matches(label))
            .collect();
        let area = rect(terminal.get_frame().area(), total).expect("fits this frame");
        let x = area.x + 1 + PAD;
        let y = area.y + 1;

        terminal
            .draw(|frame| render(frame, &kept, total, 0, search.as_ref(), Theme::of(&choice)))
            .expect("a frame");

        let buffer = terminal.backend().buffer().clone();
        let matched = Theme::of(&choice)
            .style(Token::Match)
            .fg
            .expect("a mark spends a colour");

        assert!(
            kept.len() < total,
            "the term has to leave something out for this to be a filter"
        );
        assert_eq!(
            buffer.cell((x, y)).expect("inside the block").fg,
            matched,
            "the matched label's own cell wears the mark"
        );
        assert_eq!(
            buffer
                .cell((area.x, area.y + u16::try_from(total).expect("small") + 1))
                .expect("the block's own corner")
                .symbol(),
            "╰",
            "and the block is the height the whole list gives it"
        );
    }

    #[test]
    fn a_term_that_matches_nothing_draws_a_line_saying_so() {
        let mut terminal = Terminal::new(TestBackend::new(80, 24)).expect("a backend");
        let choice = ThemeChoice::default();
        let total = every_label().len();
        let area = rect(terminal.get_frame().area(), total).expect("fits this frame");
        let theme = Theme::of(&choice);

        terminal
            .draw(|frame| render(frame, &[], total, 0, Search::new("zzz").as_ref(), theme))
            .expect("a frame");

        let buffer = terminal.backend().buffer().clone();
        let x = area.x + 1 + PAD;
        let y = area.y + 1;
        let drawn: String = (0..16)
            .map(|offset| {
                buffer
                    .cell((x + offset, y))
                    .expect("inside the block")
                    .symbol()
                    .to_owned()
            })
            .collect();

        assert_eq!(drawn, "no theme matches");
        assert_eq!(
            buffer.cell((x, y)).expect("inside the block").fg,
            theme.style(Token::Faint).fg.expect("faint spends a colour")
        );
        assert_ne!(
            buffer.cell((x, y)).expect("inside the block").bg,
            theme
                .style(Token::Selected)
                .bg
                .expect("the selection spends a ground"),
            "a cursor on nothing is not a cursor"
        );
    }

    #[test]
    fn the_cursor_row_is_a_bar_of_the_flavours_own_surface() {
        let mut terminal = Terminal::new(TestBackend::new(80, 24)).expect("a backend");
        let choice = ThemeChoice::Ported(Ported::CatppuccinMocha);
        let mut state = State::default();
        let mut cursor = Cursor::default();
        cursor.go_to(
            index_of(BuiltIn::Ported(Ported::CatppuccinMocha)),
            BuiltIn::ALL.len(),
        );
        state.theme = choice.clone();
        state.overlay = Some(Overlay::Themes {
            cursor,
            restored: choice.clone(),
            sift: None,
        });

        terminal
            .draw(|frame| {
                crate::render::render(frame, &view_model(&state), Theme::of(&choice));
            })
            .expect("a frame");

        let selection_bg = Theme::of(&choice).palette().selection_bg;
        assert!(
            terminal
                .backend()
                .buffer()
                .content()
                .iter()
                .any(|cell| cell.bg == selection_bg),
            "the picker's own cursor row is where a theme shows what it does"
        );
    }
}

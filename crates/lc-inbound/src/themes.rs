//! The theme picker: every theme, drawn over the panels.
//!
//! Narrower and shorter than the command palette, and gated on the palette's
//! own room rather than its own (`update::open_themes`) — a frame that holds
//! the larger block holds this one.

use ratatui::Frame;
use ratatui::layout::{Margin, Rect};
use ratatui::widgets::{Block, BorderType, Clear, List, ListItem, ListState};

use lc_domain::style::Token;

use crate::theme::Theme;

const WIDTH: u16 = 28;
const PAD: u16 = 1;

/// Where a picker listing `rows` themes goes in `area`, or `None` when it does
/// not fit.
fn rect(area: Rect, rows: usize) -> Option<Rect> {
    let height = u16::try_from(rows.saturating_add(2)).unwrap_or(u16::MAX);

    if area.width < WIDTH || area.height < height {
        return None;
    }

    Some(Rect::new(
        area.x + (area.width - WIDTH) / 2,
        area.y + (area.height - height) / 2,
        WIDTH,
        height,
    ))
}

pub(crate) fn render(frame: &mut Frame, labels: &[&str], selected: usize, theme: Theme) {
    let Some(area) = rect(frame.area(), labels.len()) else {
        return;
    };

    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(theme.border_style())
        .title("Theme");
    let inner = block.inner(area).inner(Margin {
        horizontal: PAD,
        vertical: 0,
    });

    frame.render_widget(Clear, area);
    frame.buffer_mut().set_style(area, theme.overlay_ground());
    frame.render_widget(block, area);

    let items: Vec<ListItem> = labels
        .iter()
        .map(|label| ListItem::new((*label).to_owned()))
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
    use lc_domain::state::{Cursor, Overlay, State};
    use lc_domain::view::view_model;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::buffer::Cell;

    use super::*;

    fn drawn(width: u16, height: u16, choice: ThemeChoice) -> String {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).expect("a backend");
        let mut state = State::default();
        let mut cursor = Cursor::default();
        cursor.go_to(choice.index(), ThemeChoice::ALL.len());
        state.theme = choice;
        state.overlay = Some(Overlay::Themes {
            cursor,
            restored: choice,
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
        let frame = drawn(80, 24, ThemeChoice::default());

        for choice in ThemeChoice::ALL {
            assert!(
                frame.contains(choice.label()),
                "{} is missing",
                choice.label()
            );
        }
    }

    #[test]
    fn a_frame_too_narrow_for_the_list_draws_none_of_it() {
        let frame = drawn(WIDTH - 1, 24, ThemeChoice::default());

        assert!(
            !frame.contains("Catppuccin"),
            "a truncated theme name is a lie about which one it is"
        );
    }

    #[test]
    fn the_cursor_row_is_a_bar_of_the_flavours_own_surface() {
        let mut terminal = Terminal::new(TestBackend::new(80, 24)).expect("a backend");
        let choice = ThemeChoice::CatppuccinMocha;
        let mut state = State::default();
        let mut cursor = Cursor::default();
        cursor.go_to(choice.index(), ThemeChoice::ALL.len());
        state.theme = choice;
        state.overlay = Some(Overlay::Themes {
            cursor,
            restored: choice,
        });

        terminal
            .draw(|frame| {
                crate::render::render(frame, &view_model(&state), Theme::of(choice));
            })
            .expect("a frame");

        let highlight = Theme::of(choice).palette().highlight;
        assert!(
            terminal
                .backend()
                .buffer()
                .content()
                .iter()
                .any(|cell| cell.bg == highlight),
            "the picker's own cursor row is where a theme shows what it does"
        );
    }
}

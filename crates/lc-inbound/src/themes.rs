//! The theme picker: every theme, drawn over the panels.
//!
//! Narrower than the command palette but not necessarily shorter: the list is
//! every built-in plus every theme a config defines, which config themes made
//! unbounded. `update::open_themes` gates on [`visible_rows`], this block's
//! own measurement, rather than the palette's.

use ratatui::Frame;
use ratatui::layout::{Margin, Rect};
use ratatui::style::Style;
use ratatui::text::Line;
use ratatui::widgets::{Block, BorderType, Clear, List, ListItem, ListState};

use lc_domain::search::Search;
use lc_domain::style::Token;

use crate::theme::Theme;

const WIDTH: u16 = 28;
const PAD: u16 = 1;

/// Where a picker listing `rows` themes goes in `area`, or `None` when it does
/// not fit.
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

/// Rows a picker would show in `area` before it stops fitting entirely — this
/// block does not scroll, so a list longer than this is refused rather than
/// opened dead. Zero means even an empty picker's borders do not fit, which is
/// what gates `ctrl-t` in the domain.
pub(crate) fn visible_rows(area: Rect) -> usize {
    if area.width < WIDTH || area.height < 2 {
        return 0;
    }

    usize::from(area.height - 2)
}

pub(crate) fn render(
    frame: &mut Frame,
    labels: &[String],
    selected: usize,
    search: Option<&Search>,
    theme: Theme,
) {
    let Some(area) = rect(frame.area(), labels.len()) else {
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
    frame.buffer_mut().set_style(area, theme.overlay_ground());
    frame.render_widget(block, area);

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
    use lc_domain::state::{Cursor, Overlay, State};
    use lc_domain::view::view_model;
    use lc_domain::{BuiltIn, Ours, Ported, ThemeChoice};
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

    /// Design §7: the picker marks the matched span in a label. Pinned to the
    /// matched label's own cell, the way `palette.rs`'s equivalent test does
    /// — asserting that some cell somewhere carries the mark would still pass
    /// if an unrelated glyph happened to share `Token::Match`'s colour.
    #[test]
    fn a_matched_span_is_marked_in_a_label() {
        let mut terminal = Terminal::new(TestBackend::new(80, 24)).expect("a backend");
        let choice = ThemeChoice::default();
        let search = Search::new("quill");
        let labels: Vec<String> = BuiltIn::ALL
            .iter()
            .map(|built_in| built_in.label().to_owned())
            .collect();
        let area = rect(terminal.get_frame().area(), labels.len()).expect("fits this frame");
        let row = index_of(BuiltIn::Ours(Ours::QuillDark));
        let x = area.x + 1 + PAD;
        let y = area.y + 1 + u16::try_from(row).expect("small row index");

        terminal
            .draw(|frame| render(frame, &labels, 0, search.as_ref(), Theme::of(&choice)))
            .expect("a frame");

        let matched = Theme::of(&choice)
            .style(Token::Match)
            .fg
            .expect("a mark spends a colour");
        assert_eq!(
            terminal
                .backend()
                .buffer()
                .cell((x, y))
                .expect("inside the block")
                .fg,
            matched,
            "the matched label's own cell wears the mark"
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

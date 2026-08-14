//! The export form, drawn over the panels
//! (docs/superpowers/specs/2026-08-14-export-design.md §2).

use lc_domain::export::{Confirm, Format, Scope, Stop};
use lc_domain::style::Token;
use lc_domain::view::ExportView;
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::{Line as TextLine, Span};
use ratatui::widgets::{Block, BorderType, Clear, Paragraph};

use crate::theme::Theme;

const WIDTH: u16 = 56;
const HEIGHT: u16 = 11;
const CONFIRM_WIDTH: u16 = 44;
const CONFIRM_HEIGHT: u16 = 7;

/// The path field's row inside the block, counted from the top of `inner`:
/// two radios, a blank line, the format row, a blank line.
const PATH_ROW: u16 = 5;

fn centred(area: Rect, width: u16, height: u16) -> Option<Rect> {
    if area.width < width || area.height < height {
        return None;
    }

    Some(Rect::new(
        area.x + (area.width - width) / 2,
        area.y + (area.height - height) / 2,
        width,
        height,
    ))
}

pub(crate) fn render(frame: &mut Frame, view: &ExportView, theme: Theme) {
    if view.confirm == Some(Confirm::Overwrite) {
        confirmation(frame, view, theme);
        return;
    }

    let Some(area) = centred(frame.area(), WIDTH, HEIGHT) else {
        return;
    };

    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(theme.border_style())
        .title(view.title.as_str());
    let inner = block.inner(area);

    frame.render_widget(Clear, area);
    frame.buffer_mut().set_style(area, theme.overlay_ground());
    frame.render_widget(block, area);

    let lines = vec![
        radio(
            &view.scope_labels[0],
            view.scope == Scope::Whole,
            true,
            view.stop == Stop::Scope,
            theme,
        ),
        radio(
            &view.scope_labels[1],
            view.scope == Scope::Fetched,
            view.fetched_enabled,
            view.stop == Stop::Scope,
            theme,
        ),
        TextLine::default(),
        formats(view, theme),
        TextLine::default(),
        path(view, theme),
        TextLine::default(),
        button("Export", view.stop == Stop::Export, theme),
    ];

    frame.render_widget(Paragraph::new(lines), inner);

    if view.stop == Stop::Path {
        set_caret(frame, inner, view);
    }
}

/// `(•)` or `( )`, the label, and the styling that says whether it can be
/// chosen and whether `Tab` is on its group.
fn radio(label: &str, selected: bool, enabled: bool, focused: bool, theme: Theme) -> TextLine<'_> {
    let mark = if selected { "(•) " } else { "( ) " };
    let style = if enabled {
        theme.style(if focused {
            Token::Selected
        } else {
            Token::HintAction
        })
    } else {
        theme.style(Token::Faint)
    };

    TextLine::from(vec![Span::styled(mark, style), Span::styled(label, style)])
}

fn formats(view: &ExportView, theme: Theme) -> TextLine<'static> {
    let focused = view.stop == Stop::Format;
    let mut spans = Vec::new();

    for format in [Format::Csv, Format::Json] {
        let mark = if view.format == format {
            "(•) "
        } else {
            "( ) "
        };
        let style = theme.style(if focused {
            Token::Selected
        } else {
            Token::HintAction
        });
        spans.push(Span::styled(mark, style));
        spans.push(Span::styled(format.label(), style));
        spans.push(Span::raw("    "));
    }

    TextLine::from(spans)
}

fn path(view: &ExportView, theme: Theme) -> TextLine<'_> {
    let style = theme.style(if view.stop == Stop::Path {
        Token::Selected
    } else {
        Token::HintAction
    });

    TextLine::from(Span::styled(view.path.as_str(), style))
}

fn button(label: &str, focused: bool, theme: Theme) -> TextLine<'_> {
    let style = theme.style(if focused {
        Token::Selected
    } else {
        Token::HintKey
    });

    TextLine::from(Span::styled(label, style)).right_aligned()
}

/// Positions the caret over the path field, measuring the way
/// `render::render_prompt` measures its query: `cursor` is a character
/// offset into `view.path`, converted to a column through `Line::width()`
/// rather than added straight to `x`, so a multi-byte or double-width
/// character before the caret is not counted as one column. Clamped to the
/// inner rect so a path wider than the field cannot push it outside the
/// block.
fn set_caret(frame: &mut Frame, inner: Rect, view: &ExportView) {
    let prefix: String = view.path.chars().take(view.cursor).collect();
    let taken = u16::try_from(TextLine::raw(prefix).width()).unwrap_or(inner.width);
    let cursor_x = (inner.x + taken).min(inner.right().saturating_sub(1));

    frame.set_cursor_position((cursor_x, inner.y + PATH_ROW));
}

fn confirmation(frame: &mut Frame, view: &ExportView, theme: Theme) {
    let Some(area) = centred(frame.area(), CONFIRM_WIDTH, CONFIRM_HEIGHT) else {
        return;
    };

    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(theme.style(Token::Severity(lc_domain::style::Severity::Warning)))
        .title("File exists");
    let inner = block.inner(area);

    frame.render_widget(Clear, area);
    frame.buffer_mut().set_style(area, theme.overlay_ground());
    frame.render_widget(block, area);
    frame.render_widget(
        Paragraph::new(vec![
            TextLine::from(view.path.as_str()),
            TextLine::from("already exists."),
            TextLine::default(),
            button("Yes, overwrite", true, theme),
        ]),
        inner,
    );
}

#[cfg(test)]
mod tests {
    use lc_domain::contents::{Subject, TableRef};
    use lc_domain::export::{ExportForm, Fetched};
    use lc_domain::state::{Overlay, State};
    use lc_domain::view::view_model;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::buffer::{Buffer, Cell};

    use crate::theme::Theme;

    fn drawn(width: u16, height: u16, form: ExportForm) -> Buffer {
        let state = State {
            overlay: Some(Overlay::Export(form)),
            ..State::default()
        };
        let mut terminal = Terminal::new(TestBackend::new(width, height)).expect("a backend");

        terminal
            .draw(|frame| {
                crate::render::render(frame, &view_model(&state), Theme::default());
            })
            .expect("TestBackend always draws");

        terminal.backend().buffer().clone()
    }

    fn text(buffer: &Buffer) -> String {
        buffer.content().iter().map(Cell::symbol).collect()
    }

    fn form(fetched: Fetched) -> ExportForm {
        ExportForm::opened(
            Subject::Table(TableRef::new("shop", "orders")),
            fetched,
            std::path::Path::new(""),
        )
    }

    #[test]
    fn the_form_names_its_table_its_scopes_its_formats_and_its_path() {
        let drawn = text(&drawn(80, 24, form(Fetched::Some(200))));

        assert!(drawn.contains("Export table shop.orders"));
        assert!(drawn.contains("Export the whole table"));
        assert!(drawn.contains("Export the 200 fetched rows"));
        assert!(drawn.contains("CSV"));
        assert!(drawn.contains("JSON"));
        assert!(drawn.contains("shop.orders.csv"));
        assert!(drawn.contains("Export"));
    }

    #[test]
    fn the_selected_radio_is_marked_and_the_other_is_not() {
        let drawn = text(&drawn(80, 24, form(Fetched::Some(200))));

        assert!(drawn.contains("(•) Export the whole table"));
        assert!(drawn.contains("( ) Export the 200 fetched rows"));
    }

    #[test]
    fn a_disabled_scope_is_drawn_faint() {
        let buffer = drawn(80, 24, form(Fetched::Everything(137)));
        let faint = Theme::default()
            .style(lc_domain::style::Token::Faint)
            .fg
            .expect("faint spends a colour");

        let row_text = |y: u16| -> String {
            (0..buffer.area.width)
                .filter_map(|x| buffer.cell((x, y)).map(|cell| cell.symbol().to_owned()))
                .collect()
        };
        let row = (0..buffer.area.height)
            .find(|y| row_text(*y).contains("fetched rows"))
            .expect("the fetched option is drawn");

        assert!(
            (0..buffer.area.width)
                .any(|x| buffer.cell((x, row)).is_some_and(|cell| cell.fg == faint)),
            "an unreachable option says so (design §3)"
        );
    }

    #[test]
    fn the_confirmation_replaces_the_form() {
        let mut form = form(Fetched::Some(200));
        form.set_confirm(Some(lc_domain::export::Confirm::Overwrite));

        let drawn = text(&drawn(80, 24, form));

        assert!(drawn.contains("File exists"));
        assert!(drawn.contains("Yes, overwrite"));
    }

    #[test]
    fn a_frame_too_small_for_the_form_does_not_get_a_truncated_one() {
        let drawn = text(&drawn(20, 6, form(Fetched::Some(200))));

        assert!(!drawn.contains("Export the whole table"));
    }
}

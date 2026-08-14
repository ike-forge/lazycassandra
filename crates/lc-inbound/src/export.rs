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

/// Whether `area` can hold the form at all.
///
/// The domain's gate on `ctrl-e` and on closing a form the frame shrank past
/// reads this rather than a size of its own: [`WIDTH`] and [`HEIGHT`] are
/// named once, here, beside the block they measure, so the check and the draw
/// cannot disagree about what fits (export design §2).
pub(crate) fn fits(area: Rect) -> bool {
    centred(area, WIDTH, HEIGHT).is_some()
}

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

    let (visible, caret) = window(&view.path, view.cursor, usize::from(inner.width));

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
        path(&visible, view.stop == Stop::Path, theme),
        TextLine::default(),
        button("Export", view.stop == Stop::Export, theme),
    ];

    frame.render_widget(Paragraph::new(lines), inner);

    if view.stop == Stop::Path {
        set_caret(frame, inner, &visible, caret);
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

fn path(visible: &str, focused: bool, theme: Theme) -> TextLine<'static> {
    let style = theme.style(if focused {
        Token::Selected
    } else {
        Token::HintAction
    });

    TextLine::from(Span::styled(visible.to_owned(), style))
}

/// The slice of `path` a field `width` columns wide shows, and the caret's
/// column within it.
///
/// The window follows the cursor the way [`lc_domain::grid::ColumnScroll`]
/// follows a column, computed per frame rather than kept: the field's width is
/// a constant of this module and the caret is the only thing the window has to
/// contain, so there is no offset for the domain to remember. It earns its
/// place on the first frame — the path is prefilled with `cwd` joined to a
/// filename, which on any real working directory is longer than the 54 columns
/// inside the block, and the field used to draw the head of it with the caret
/// pinned to the right edge and the filename off screen (final review I5).
fn window(path: &str, cursor: usize, width: usize) -> (String, usize) {
    let width = width.max(1);
    let len = path.chars().count();
    // One past the last character is a caret position too, so the window that
    // holds the end of a full line stops one character short of `width`.
    let furthest = (len + 1).saturating_sub(width);
    let first = cursor.saturating_sub(width - 1).min(furthest);

    (
        path.chars().skip(first).take(width).collect(),
        cursor - first,
    )
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
/// `render::render_prompt` measures its query: `caret` is a character offset
/// into the drawn window, converted to a column through `Line::width()` rather
/// than added straight to `x`, so a multi-byte or double-width character
/// before it is not counted as one column. Clamped to the inner rect, which a
/// window of double-width characters can still reach the end of.
fn set_caret(frame: &mut Frame, inner: Rect, visible: &str, caret: usize) {
    let prefix: String = visible.chars().take(caret).collect();
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
        form.ask(lc_domain::export::Pending::Overwrite(Box::new(
            lc_domain::Source::Fetched {
                columns: Vec::new(),
                rows: Vec::new(),
            },
        )));

        let drawn = text(&drawn(80, 24, form));

        assert!(drawn.contains("File exists"));
        assert!(drawn.contains("Yes, overwrite"));
    }

    #[test]
    fn a_frame_too_small_for_the_form_does_not_get_a_truncated_one() {
        let drawn = text(&drawn(20, 6, form(Fetched::Some(200))));

        assert!(!drawn.contains("Export the whole table"));
    }

    /// The prefill is `cwd` joined with a filename, which outgrows the field
    /// on any real working directory — so on the first frame the user could
    /// not see the name they were about to write, and the caret sat pinned to
    /// the right edge wherever the cursor was (final review I5).
    #[test]
    fn a_path_too_long_for_the_field_is_drawn_around_its_cursor() {
        let long = std::path::PathBuf::from(format!("/home/ike/{}", "deep/".repeat(20)));
        let form = ExportForm::opened(
            Subject::Table(TableRef::new("shop", "orders")),
            Fetched::Some(200),
            &long,
        );
        let drawn = text(&drawn(80, 24, form));

        assert!(
            drawn.contains("shop.orders.csv"),
            "the cursor opens on the filename, so the filename is what shows"
        );
    }

    #[test]
    fn a_window_that_fits_is_the_whole_line() {
        assert_eq!(super::window("abc", 3, 10), ("abc".to_owned(), 3));
    }

    #[test]
    fn a_window_holds_the_end_of_a_line_the_cursor_is_at_the_end_of() {
        let (visible, caret) = super::window("abcdef", 6, 4);

        assert_eq!(visible, "def");
        assert_eq!(caret, 3, "one past the last character, inside the field");
    }

    #[test]
    fn a_window_follows_the_cursor_back_into_the_middle_of_a_line() {
        let (visible, caret) = super::window("abcdef", 1, 3);

        assert_eq!(visible, "abc");
        assert_eq!(caret, 1);
    }

    /// Windowing by character, as the cursor counts: slicing a path by bytes
    /// panics on the ones a filesystem accepts.
    #[test]
    fn a_window_counts_characters_rather_than_bytes() {
        assert_eq!(super::window("aéb", 3, 10), ("aéb".to_owned(), 3));
    }

    /// What the domain gates `ctrl-e` on is this function, so it has to agree
    /// with the draw at every size — a frame `fits` calls big enough and the
    /// form declines to draw in is an invisible modal capturing every key
    /// (final review I1).
    #[test]
    fn fits_agrees_with_what_the_form_actually_draws() {
        for (width, height) in [(20, 6), (55, 24), (80, 10), (56, 11), (80, 24)] {
            let drawn = text(&drawn(width, height, form(Fetched::Some(200))));

            assert_eq!(
                super::fits(ratatui::layout::Rect::new(0, 0, width, height)),
                drawn.contains("Export the whole table"),
                "at {width}×{height}"
            );
        }
    }
}

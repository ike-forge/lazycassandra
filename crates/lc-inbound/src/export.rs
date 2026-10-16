//! The export form, drawn over the panels.

use lc_domain::export::{Confirm, Field, Format, Scope, Stop};
use lc_domain::model::Key;
use lc_domain::style::{Segment, Token};
use lc_domain::view::ExportView;
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Modifier;
use ratatui::text::{Line as TextLine, Span};
use ratatui::widgets::{Block, BorderType, Clear, Paragraph};
use unicode_width::UnicodeWidthChar as _;

use crate::hit::{ButtonAt, FieldAt, Modal, Target, TextAt};
use crate::theme::Theme;

const WIDTH: u16 = 84;
/// The form narrows to here before giving up.
const MIN_WIDTH: u16 = 56;
/// Ten content rows, none spare, so `Export` sits on the last.
const HEIGHT: u16 = 12;
/// A floor: [`confirm_width`] widens to show the whole path, which has no
/// cursor to scroll.
const CONFIRM_WIDTH: u16 = 44;
const CONFIRM_HEIGHT: u16 = 7;

const PATH_ROW: u16 = 0;
const PATH_LABEL: &str = "Path ";
/// Path, gap, two formats, gap, two scopes.
const ROWS_ROW: u16 = 7;
const ROWS_WIDTH: usize = 8;

/// The domain's gate reads this, so the check and the draw cannot disagree.
pub(crate) fn fits(area: Rect) -> bool {
    form_area(area).is_some()
}

fn form_area(area: Rect) -> Option<Rect> {
    if area.width < MIN_WIDTH {
        return None;
    }

    centred(area, WIDTH.min(area.width), HEIGHT)
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

fn title(segments: &[Segment], theme: Theme) -> TextLine<'static> {
    TextLine::from(
        segments
            .iter()
            .map(|segment| match segment.token {
                // A schema name is not emphasis.
                Some(token) => Span::styled(
                    segment.text.clone(),
                    theme.style(token).remove_modifier(Modifier::BOLD),
                ),
                None => Span::raw(segment.text.clone()),
            })
            .collect::<Vec<_>>(),
    )
}

pub(crate) fn render(
    frame: &mut Frame,
    view: &ExportView,
    pressed: Option<Key>,
    theme: Theme,
) -> Option<Modal> {
    if view.confirm == Some(Confirm::Overwrite) {
        return confirmation(frame, view, theme);
    }

    let area = form_area(frame.area())?;

    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(theme.focus_border())
        .title(title(&view.title, theme));
    let inner = block.inner(area);

    frame.render_widget(Clear, area);
    frame.buffer_mut().set_style(area, theme.ground());
    frame.render_widget(block, area);

    let label_width = u16::try_from(PATH_LABEL.len()).unwrap_or(0);
    let field_width = usize::from(inner.width.saturating_sub(label_width));
    let (visible, caret) = window(&view.path, view.cursor, field_width);
    let mut fields = vec![FieldAt {
        area: Rect::new(
            inner.x + label_width,
            inner.y + PATH_ROW,
            u16::try_from(field_width).unwrap_or(0),
            1,
        ),
        target: Target::Text(TextAt {
            text: view.path.clone(),
            first: view.cursor.saturating_sub(caret),
            rows: false,
        }),
    }];

    let mut lines = vec![
        path(&visible, field_width, view.stop == Stop::Path, theme),
        TextLine::default(),
    ];
    for (format, line) in [Format::Csv, Format::Json]
        .into_iter()
        .zip(formats(view, theme))
    {
        fields.push(radio_at(inner, lines.len(), &line, Field::Format(format)));
        lines.push(line);
    }
    lines.push(TextLine::default());
    if view.fetched_enabled {
        let focused = view.stop == Stop::Scope;
        for (index, scope) in [Scope::Fetched, Scope::Whole].into_iter().enumerate() {
            let line = radio(
                &view.scope_labels[index],
                view.scope == scope,
                focused,
                theme,
            );
            fields.push(radio_at(inner, lines.len(), &line, Field::Scope(scope)));
            lines.push(line);
        }
        fields.extend(first_rows_at(inner, lines.len(), view));
        lines.push(first_rows(view, focused, theme));
    } else {
        lines.push(TextLine::from(Span::styled(
            view.scope_labels[0].as_str(),
            theme.style(Token::Muted),
        )));
    }

    frame.render_widget(
        Paragraph::new(lines),
        Rect::new(
            inner.x,
            inner.y,
            inner.width,
            inner.height.saturating_sub(1),
        ),
    );
    let export = button(
        "Export",
        view.stop == Stop::Export || pressed == Some(Key::Enter),
        theme,
    );
    let row = Rect::new(inner.x, inner.bottom().saturating_sub(1), inner.width, 1);
    let button_width = u16::try_from(export.width()).unwrap_or(0);
    let buttons = vec![ButtonAt {
        area: Rect::new(
            row.x + row.width.saturating_sub(button_width) / 2,
            row.y,
            button_width.min(row.width),
            1,
        ),
        key: Key::Enter,
    }];
    frame.render_widget(Paragraph::new(export).centered(), row);

    if view.stop == Stop::Path {
        set_caret(frame, inner, PATH_ROW, label_width, &visible, caret);
    }
    if view.stop == Stop::Rows {
        let before =
            u16::try_from(TextLine::raw(format!("{MARK_ON}{} ", view.scope_labels[2])).width())
                .unwrap_or(0);
        let (shown, caret) = window(&view.rows, view.rows_cursor, ROWS_WIDTH);
        set_caret(frame, inner, ROWS_ROW, before, &shown, caret);
    }

    Some(Modal::new(area, buttons).with_fields(fields))
}

fn radio_at(inner: Rect, row: usize, line: &TextLine, field: Field) -> FieldAt {
    FieldAt {
        area: Rect::new(
            inner.x,
            inner.y + u16::try_from(row).unwrap_or(0),
            u16::try_from(line.width()).unwrap_or(0).min(inner.width),
            1,
        ),
        target: Target::Fixed(field),
    }
}

fn first_rows_at(inner: Rect, row: usize, view: &ExportView) -> [FieldAt; 2] {
    let before =
        u16::try_from(TextLine::raw(format!("{MARK_ON}{} ", view.scope_labels[2])).width())
            .unwrap_or(0);
    let (_, caret) = window(&view.rows, view.rows_cursor, ROWS_WIDTH);
    let y = inner.y + u16::try_from(row).unwrap_or(0);

    [
        FieldAt {
            area: Rect::new(inner.x, y, before.saturating_sub(1), 1),
            target: Target::Fixed(Field::Scope(Scope::First)),
        },
        FieldAt {
            area: Rect::new(
                inner.x + before,
                y,
                u16::try_from(ROWS_WIDTH).unwrap_or(0),
                1,
            ),
            target: Target::Text(TextAt {
                text: view.rows.clone(),
                first: view.rows_cursor.saturating_sub(caret),
                rows: true,
            }),
        },
    ]
}

fn first_rows(view: &ExportView, focused: bool, theme: Theme) -> TextLine<'static> {
    let style = theme.style(if focused {
        Token::Selected
    } else {
        Token::HintAction
    });
    let (shown, _) = window(&view.rows, view.rows_cursor, ROWS_WIDTH);

    TextLine::from(vec![
        Span::styled(mark(view.scope == Scope::First), style),
        Span::styled(format!("{} ", view.scope_labels[2]), style),
        Span::styled(
            format!("{shown:<ROWS_WIDTH$}"),
            backdrop(view.stop == Stop::Rows, theme),
        ),
        Span::styled(" rows", style),
    ])
}

fn backdrop(focused: bool, theme: Theme) -> ratatui::style::Style {
    theme.style(if focused {
        Token::Selected
    } else {
        Token::Ranged
    })
}

/// Both one column wide, so labels align without padding.
const MARK_ON: &str = "◉ ";
const MARK_OFF: &str = "○ ";

fn mark(selected: bool) -> &'static str {
    if selected { MARK_ON } else { MARK_OFF }
}

fn radio(label: &str, selected: bool, focused: bool, theme: Theme) -> TextLine<'_> {
    let style = theme.style(if focused {
        Token::Selected
    } else {
        Token::HintAction
    });

    TextLine::from(vec![
        Span::styled(mark(selected), style),
        Span::styled(label, style),
    ])
}

fn formats(view: &ExportView, theme: Theme) -> Vec<TextLine<'static>> {
    let focused = view.stop == Stop::Format;
    let style = theme.style(if focused {
        Token::Selected
    } else {
        Token::HintAction
    });

    [Format::Csv, Format::Json]
        .into_iter()
        .map(|format| {
            TextLine::from(vec![
                Span::styled(mark(view.format == format), style),
                Span::styled(format.label(), style),
            ])
        })
        .collect()
}

/// Padded so the backdrop is the whole field.
fn path(visible: &str, width: usize, focused: bool, theme: Theme) -> TextLine<'static> {
    let style = backdrop(focused, theme);
    let padding = width.saturating_sub(TextLine::raw(visible).width());

    TextLine::from(vec![
        Span::styled(PATH_LABEL, theme.style(Token::Muted)),
        Span::styled(format!("{visible}{}", " ".repeat(padding)), style),
    ])
}

/// Follows the cursor, recomputed per frame. Chosen by display width, not
/// character count: a CJK path would otherwise overflow the field and hide its
/// tail.
fn window(path: &str, cursor: usize, width: usize) -> (String, usize) {
    let width = width.max(1);
    let characters: Vec<char> = path.chars().collect();
    let cursor = cursor.min(characters.len());

    // The caret past the end costs one column.
    let mut used = characters.get(cursor).and_then(|c| c.width()).unwrap_or(1);
    let mut first = cursor;
    for (index, character) in characters[..cursor].iter().enumerate().rev() {
        let next = used + character.width().unwrap_or(0);
        if next > width {
            break;
        }
        used = next;
        first = index;
    }

    let mut visible = String::new();
    let mut drawn = 0;
    for character in &characters[first..] {
        let next = drawn + character.width().unwrap_or(0);
        if next > width {
            break;
        }
        visible.push(*character);
        drawn = next;
    }

    (visible, cursor - first)
}

/// Focus fills it solid.
fn button(label: &str, focused: bool, theme: Theme) -> TextLine<'static> {
    let style = if focused {
        theme
            .style(Token::HintAction)
            .add_modifier(Modifier::REVERSED | Modifier::BOLD)
    } else {
        theme.style(Token::Ranged)
    };

    TextLine::from(Span::styled(format!("  {label}  "), style))
}

/// Converted through `Line::width()`, so wide characters count right. Clamped
/// to the inner rect.
fn set_caret(frame: &mut Frame, inner: Rect, row: u16, offset: u16, visible: &str, caret: usize) {
    let prefix: String = visible.chars().take(caret).collect();
    let taken = u16::try_from(TextLine::raw(prefix).width()).unwrap_or(inner.width);
    let cursor_x = (inner.x + offset + taken).min(inner.right().saturating_sub(1));

    frame.set_cursor_position((cursor_x, inner.y + row));
}

fn confirm_width(path: &str, frame_width: u16) -> u16 {
    let needed = u16::try_from(TextLine::raw(path).width() + 4).unwrap_or(u16::MAX);

    needed.max(CONFIRM_WIDTH).min(frame_width)
}

fn confirmation(frame: &mut Frame, view: &ExportView, theme: Theme) -> Option<Modal> {
    let width = confirm_width(&view.path, frame.area().width);
    let area = centred(frame.area(), width, CONFIRM_HEIGHT)?;

    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(theme.style(Token::Severity(lc_domain::style::Severity::Warning)))
        .title("File exists");
    let inner = block.inner(area);

    frame.render_widget(Clear, area);
    frame.buffer_mut().set_style(area, theme.ground());
    frame.render_widget(block, area);
    let overwrite = button("Yes, overwrite", true, theme);
    let buttons = vec![ButtonAt {
        area: Rect::new(
            inner.x,
            inner.y + 3,
            u16::try_from(overwrite.width())
                .unwrap_or(0)
                .min(inner.width),
            1,
        ),
        key: Key::Enter,
    }];
    frame.render_widget(
        Paragraph::new(vec![
            TextLine::from(view.path.as_str()),
            TextLine::from("already exists."),
            TextLine::default(),
            overwrite,
        ]),
        inner,
    );

    Some(Modal::new(area, buttons))
}

#[cfg(test)]
mod tests {
    use lc_domain::contents::{Subject, TableRef};
    use lc_domain::export::{ExportForm, Fetched};
    use lc_domain::overlay::Overlay;
    use lc_domain::state::State;
    use lc_domain::view::view_model;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::buffer::{Buffer, Cell};

    use crate::theme::Theme;

    fn drawn(width: u16, height: u16, form: ExportForm) -> Buffer {
        with_caret(width, height, form).0
    }

    fn with_caret(width: u16, height: u16, form: ExportForm) -> (Buffer, (u16, u16)) {
        let mut state = State::default();
        state.overlay = Some(Overlay::Export(form));
        let mut terminal = Terminal::new(TestBackend::new(width, height)).expect("a backend");

        terminal
            .draw(|frame| {
                crate::render::render(frame, &view_model(&state), Theme::default());
            })
            .expect("TestBackend always draws");

        let backend = terminal.backend();
        let position = backend.cursor_position();

        (backend.buffer().clone(), (position.x, position.y))
    }

    /// `left` characters back from the end, with `Tab` on the field so the
    /// caret is drawn.
    fn on_path(path: &str, left: usize) -> ExportForm {
        let mut form = form(Fetched::Some(200));

        let line = form.path_mut();
        while line.cursor() > 0 {
            line.delete_back();
        }
        for character in path.chars() {
            line.insert(character);
        }
        for _ in 0..left {
            line.left();
        }

        form
    }

    /// Skips the blank cell ratatui parks behind a double-width symbol.
    fn row(buffer: &Buffer, y: u16) -> String {
        let mut text = String::new();
        let mut x = 0;

        while let Some(cell) = buffer.cell((x, y)) {
            let symbol = cell.symbol();
            text.push_str(symbol);
            x += u16::try_from(unicode_width::UnicodeWidthStr::width(symbol).max(1))
                .unwrap_or(u16::MAX);
        }

        text
    }

    /// Centred: [`super::WIDTH`] wide, border one column, caption five.
    const FRAME_WIDTH: u16 = 100;
    const FIELD_X: u16 = (FRAME_WIDTH - super::WIDTH) / 2 + 1 + 5;

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
        assert!(drawn.contains("Export the first"));
        assert!(drawn.contains("Export the 200 visible rows"));
        assert!(drawn.contains("CSV"));
        assert!(drawn.contains("JSON"));
        assert!(drawn.contains("shop.orders.csv"));
        assert!(drawn.contains("Export"));
    }

    #[test]
    fn the_selected_radio_is_marked_and_the_other_is_not() {
        let drawn = text(&drawn(80, 24, form(Fetched::Some(200))));

        assert!(drawn.contains("◉ Export the whole table"));
        assert!(drawn.contains("○ Export the 200 visible rows"));
        assert!(drawn.contains("○ Export the first"));
    }

    #[test]
    fn a_result_both_scopes_write_whole_is_a_sentence_not_a_radio() {
        let drawn = text(&drawn(100, 24, form(Fetched::Everything(137))));

        assert!(drawn.contains("Export all 137 rows"));
        assert!(!drawn.contains("○ Export all"));
        assert!(!drawn.contains("◉ Export all"));
        assert!(!drawn.contains("Export the first"));
    }

    #[test]
    fn the_path_is_on_a_backdrop_at_rest_and_a_heavier_one_under_tab() {
        let theme = Theme::default();
        let bg = |focused| {
            super::path("a", 10, focused, theme).spans[1]
                .style
                .bg
                .expect("the field spends a ground")
        };

        assert_eq!(
            bg(true),
            theme.style(lc_domain::style::Token::Selected).bg.unwrap()
        );
        assert_eq!(
            bg(false),
            theme.style(lc_domain::style::Token::Ranged).bg.unwrap()
        );
        assert_eq!(
            super::path("a", 10, false, theme).spans[1].content,
            "a         "
        );
    }

    #[test]
    fn the_export_button_is_a_filled_block_and_names_no_key() {
        let line = super::button("Export", true, Theme::default());

        assert_eq!(line.spans.len(), 1);
        assert_eq!(line.spans[0].content.as_ref(), "  Export  ");
    }

    #[test]
    fn the_export_button_fills_solid_only_under_tab() {
        use ratatui::style::Modifier;

        let theme = Theme::default();

        assert_eq!(
            super::button("Export", true, theme).spans[0].style,
            theme
                .style(lc_domain::style::Token::HintAction)
                .add_modifier(Modifier::REVERSED | Modifier::BOLD)
        );
        assert_eq!(
            super::button("Export", false, theme).spans[0].style,
            theme.style(lc_domain::style::Token::Ranged)
        );
    }

    #[test]
    fn the_export_button_sits_on_the_form_s_last_row() {
        let buffer = drawn(80, 24, form(Fetched::Some(200)));
        let area = super::form_area(ratatui::layout::Rect::new(0, 0, 80, 24))
            .expect("the form fits an 80x24 frame");
        let last_inner_row = area.y + super::HEIGHT - 2;

        assert!(row(&buffer, last_inner_row).contains("  Export  "));
    }

    #[test]
    fn the_export_button_is_centred_in_the_form() {
        let buffer = drawn(100, 24, form(Fetched::Some(200)));
        let area = super::form_area(ratatui::layout::Rect::new(0, 0, 100, 24)).expect("fits");
        let line = row(&buffer, area.y + super::HEIGHT - 2);
        let inner = &line
            [line.find('│').expect("border") + '│'.len_utf8()..line.rfind('│').expect("border")];
        let left = inner.len() - inner.trim_start().len();
        let right = inner.len() - inner.trim_end().len();

        assert!(left.abs_diff(right) <= 1, "{left} left, {right} right");
    }

    #[test]
    fn the_confirmation_replaces_the_form() {
        let mut form = form(Fetched::Some(200));
        form.ask(lc_domain::export::Pending::Overwrite(
            Box::new(lc_domain::Source::Fetched {
                columns: Vec::new(),
                rows: Vec::new(),
            }),
            lc_domain::model::JobId::next(),
        ));

        let drawn = text(&drawn(80, 24, form));

        assert!(drawn.contains("File exists"));
        assert!(drawn.contains("Yes, overwrite"));
    }

    #[test]
    fn the_confirmation_widens_to_fit_a_long_path() {
        let mut form = form(Fetched::Some(200));
        let long = format!("/home/ike/{}shop.orders.csv", "deep/".repeat(6));
        let line = form.path_mut();
        while line.cursor() > 0 {
            line.delete_back();
        }
        for character in long.chars() {
            line.insert(character);
        }
        form.ask(lc_domain::export::Pending::Overwrite(
            Box::new(lc_domain::Source::Fetched {
                columns: Vec::new(),
                rows: Vec::new(),
            }),
            lc_domain::model::JobId::next(),
        ));

        let drawn = text(&drawn(100, 24, form));

        assert!(
            drawn.contains(&long),
            "the whole path is drawn, not clipped: {drawn:?}"
        );
    }

    #[test]
    fn a_frame_too_small_for_the_form_does_not_get_a_truncated_one() {
        let drawn = text(&drawn(20, 6, form(Fetched::Some(200))));

        assert!(!drawn.contains("Export the whole table"));
    }

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
    fn a_double_width_path_is_drawn_from_its_tail_with_the_caret_on_the_cursor() {
        let path = format!("{}終", "東".repeat(60));
        let (buffer, caret) = with_caret(FRAME_WIDTH, 24, on_path(&path, 0));
        let y = (0..buffer.area.height)
            .find(|y| row(&buffer, *y).contains('終'))
            .expect("the path field is drawn");
        let field = row(&buffer, y);

        assert!(field.contains(&format!("{}終", "東".repeat(37))));
        assert!(
            !field.contains(&"東".repeat(38)),
            "38 double-width characters and the caret are the most 77 columns hold"
        );
        assert_eq!(
            caret,
            (FIELD_X + 76, y),
            "the true column, not the right edge"
        );
    }

    #[test]
    fn a_mixed_width_path_windows_by_column_around_a_cursor_in_the_middle() {
        let path = format!("/home/ike/{}東京/report.csv", "deep/".repeat(20));
        let (buffer, caret) = with_caret(FRAME_WIDTH, 24, on_path(&path, 12));
        let y = (0..buffer.area.height)
            .find(|y| row(&buffer, *y).contains("東京"))
            .expect("the path field is drawn");
        let field = row(&buffer, y);

        assert!(field.contains("p/deep/東京"));
        assert!(!field.contains("/home/ike/"), "the head is scrolled off");
        assert!(!field.contains("report.csv"), "so is the tail");
        assert_eq!(caret, (FIELD_X + 75, y));
    }

    #[test]
    fn a_character_wider_than_the_whole_field_still_leaves_the_window() {
        assert_eq!(super::window("東", 1, 1), (String::new(), 0));
        assert_eq!(super::window("東b", 0, 1), (String::new(), 0));
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

    #[test]
    fn a_window_counts_characters_rather_than_bytes() {
        assert_eq!(super::window("aéb", 3, 10), ("aéb".to_owned(), 3));
    }

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

    fn find(buffer: &Buffer, needle: &str) -> (u16, u16) {
        let width = usize::from(buffer.area.width);
        let cells: Vec<&str> = buffer.content().iter().map(Cell::symbol).collect();
        let at = (0..cells.len())
            .find(|&at| cells[at..].concat().starts_with(needle))
            .unwrap_or_else(|| panic!("{needle:?} is on the frame"));

        (
            u16::try_from(at % width).expect("a column"),
            u16::try_from(at / width).expect("a row"),
        )
    }

    fn mapped(form: ExportForm) -> (Buffer, crate::hit::FrameMap) {
        let mut state = State::default();
        state.overlay = Some(Overlay::Export(form));
        let mut terminal = Terminal::new(TestBackend::new(100, 30)).expect("a backend");
        let mut map = crate::hit::FrameMap::default();
        terminal
            .draw(|frame| {
                map = crate::render::render(frame, &view_model(&state), Theme::default()).map;
            })
            .expect("TestBackend always draws");

        (terminal.backend().buffer().clone(), map)
    }

    #[test]
    fn every_element_of_the_form_resolves_a_press_to_itself() {
        use lc_domain::export::{Field, Format, Scope};
        use lc_domain::model::Key;
        use lc_domain::pointer::Pointer;

        let (buffer, map) = mapped(form(Fetched::Some(200)));

        let (x, y) = find(&buffer, "JSON");
        assert_eq!(
            map.press(x, y),
            Some(Pointer::Field(Field::Format(Format::Json)))
        );
        let (x, y) = find(&buffer, "Export the whole");
        assert_eq!(
            map.press(x, y),
            Some(Pointer::Field(Field::Scope(Scope::Whole)))
        );
        let (x, y) = find(&buffer, "Export the first");
        assert_eq!(
            map.press(x, y),
            Some(Pointer::Field(Field::Scope(Scope::First)))
        );
        let (x, y) = find(&buffer, "  Export  ");
        assert_eq!(map.press(x + 3, y), Some(Pointer::Hold(Key::Enter)));
        let (x, y) = find(&buffer, "Path ");
        assert!(
            matches!(
                map.press(x + 5, y),
                Some(Pointer::Field(Field::Path { cursor: 0 }))
            ),
            "the first column of the field is before the first character"
        );
        assert!(matches!(
            map.press(x + 5 + 3, y),
            Some(Pointer::Field(Field::Path { cursor: 3 }))
        ));
        assert_eq!(
            map.press(x + 3, y + 1),
            Some(Pointer::Modal),
            "the gap between rows is the block's"
        );
    }

    #[test]
    fn a_press_past_the_end_of_the_path_places_the_caret_at_the_end() {
        use lc_domain::export::Field;
        use lc_domain::pointer::Pointer;

        let (buffer, map) = mapped(form(Fetched::Some(200)));
        let (x, y) = find(&buffer, "Path ");
        let length = form(Fetched::Some(200)).path().text().chars().count();

        assert_eq!(
            map.press(x + 5 + u16::try_from(length).expect("short") + 2, y),
            Some(Pointer::Field(Field::Path { cursor: length }))
        );
    }
}

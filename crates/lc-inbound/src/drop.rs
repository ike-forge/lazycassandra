//! The drop confirmation, drawn over the panels
//! (docs/superpowers/specs/2026-08-31-drop-design.md §2).

use lc_domain::model::Key;
use lc_domain::style::{Segment, Severity, Token};
use lc_domain::view::{DropView, TypedName};
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Modifier;
use ratatui::text::{Line as TextLine, Span};
use ratatui::widgets::{Block, BorderType, Clear, Paragraph};

use crate::hit::{ButtonAt, Modal};
use crate::theme::Theme;

/// A floor, not a cap: [`width`] widens the block to fit the statement rather
/// than clip it. A statement the user cannot read whole is one they are
/// confirming blind, and unlike the export form's path field there is no
/// cursor here to scroll it around.
const MIN_WIDTH: u16 = 44;
/// Statement lines drawn before the rest are summarised away.
///
/// A batch of forty deletes is forty lines, which is a block taller than the
/// frame and a wall of near-identical text nobody reads. What answers the
/// question at that size is the count in the border, and the head of the
/// batch is what says the shape of it.
const MAX_CQL_LINES: usize = 8;
/// Everything the block draws that is not the statement: the two border
/// rows, a blank line above the statement and one below it, the warning or
/// the prompt, a blank line, and the keys.
const CHROME_HEIGHT: u16 = 7;
/// Two more rows on the typed spelling: the field, and the blank above it.
const TYPED_EXTRA: u16 = 2;

/// Whether `area` can hold the block at all.
///
/// The domain's gate on `d` and on closing a confirmation the frame shrank
/// past reads this rather than a size of its own, for the reason
/// `export::fits` gives: the dimensions are named once, beside the block they
/// measure.
///
/// Measured against the *largest* block either spelling can grow to, so the
/// gate cannot open a question that then fails to draw — a statement's line
/// count is not known until the target is, and the gate is asked before
/// there is one.
pub(crate) fn fits(area: Rect) -> bool {
    let tallest =
        CHROME_HEIGHT + TYPED_EXTRA + u16::try_from(MAX_CQL_LINES + 1).unwrap_or(u16::MAX);

    centred(area, MIN_WIDTH, tallest).is_some()
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

/// Wide enough for the widest line the block draws, plus the border and a
/// column of padding each side, and never wider than the frame.
fn width(view: &DropView, frame_width: u16) -> u16 {
    let longest = view
        .typed
        .iter()
        .map(|typed| prompt_line(typed).chars().count())
        .chain(statement(view).iter().map(|line| line.chars().count()))
        .max()
        .unwrap_or_default();
    let needed = u16::try_from(longest + 4).unwrap_or(u16::MAX);

    needed.max(MIN_WIDTH).min(frame_width)
}

/// The statement as the block draws it: whole where it fits in
/// [`MAX_CQL_LINES`], and otherwise its head, a line counting what was left
/// out, and its last line.
///
/// The last line is kept rather than cut off with the rest: a batch that
/// stops before `APPLY BATCH` is not a statement, and a preview that is not
/// one teaches the reader the wrong shape. Only `DELETE` lines are ever
/// elided, which is why the count reads as rows.
fn statement(view: &DropView) -> Vec<String> {
    if view.cql.len() <= MAX_CQL_LINES {
        return view.cql.clone();
    }

    let head = MAX_CQL_LINES - 2;
    let mut lines: Vec<String> = view.cql.iter().take(head).cloned().collect();
    lines.push(format!("  … and {} more", view.cql.len() - head - 1));
    lines.extend(view.cql.last().cloned());

    lines
}

/// The block's border text. A run with no token of its own keeps the block's
/// own style; the names carry the schema colours the domain gave them. Bold
/// is dropped for `export::title`'s reason: the border is bold already, and a
/// schema name is not emphasis.
pub(crate) fn title(segments: &[Segment], theme: Theme) -> TextLine<'static> {
    TextLine::from(
        segments
            .iter()
            .map(|segment| match segment.token {
                Some(token) => Span::styled(
                    segment.text.clone(),
                    theme.style(token).remove_modifier(Modifier::BOLD),
                ),
                None => Span::raw(segment.text.clone()),
            })
            .collect::<Vec<_>>(),
    )
}

fn prompt_line(typed: &TypedName) -> String {
    format!("Type {} to confirm:", typed.name)
}

pub(crate) fn render(
    frame: &mut Frame,
    view: &DropView,
    pressed: Option<Key>,
    theme: Theme,
) -> Option<Modal> {
    // Asked before the block is sized, so the two cannot disagree: [`width`]
    // clamps to the frame, which on a narrow one would otherwise draw a
    // block below the floor `fits` promised, and the height below varies with
    // the statement while the gate does not (drop design §1).
    if !fits(frame.area()) {
        return None;
    }

    let cql = statement(view);
    let height = CHROME_HEIGHT
        + u16::try_from(cql.len()).unwrap_or(u16::MAX)
        + if view.typed.is_some() { TYPED_EXTRA } else { 0 };
    let area = centred(frame.area(), width(view, frame.area().width), height)?;

    // The severity replaces the theme's border style, the one other place a
    // border says how bad the thing behind it is (`overlay.rs`).
    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(theme.style(Token::Severity(Severity::Danger)))
        .title(title(&view.title, theme));
    let inner = block.inner(area);

    frame.render_widget(Clear, area);
    frame.buffer_mut().set_style(area, theme.ground());
    frame.render_widget(block, area);

    let mut lines = vec![TextLine::default()];
    lines.extend(
        cql.iter()
            .map(|line| TextLine::from(Span::styled(line.clone(), theme.style(Token::Keyword)))),
    );
    lines.push(TextLine::default());

    let buttons = match &view.typed {
        None => {
            lines.push(TextLine::from(warning(view)));
            lines.push(TextLine::default());
            keys(
                &[
                    Button::new(Key::Char('y'), yes(view), true),
                    Button::new(Key::Char('n'), "Cancel", true),
                ],
                pressed,
                theme,
            )
        }
        Some(typed) => {
            lines.push(TextLine::from(prompt_line(typed)));
            lines.push(TextLine::default());
            lines.push(TextLine::from(Span::styled(
                typed.text.clone(),
                theme.style(Token::Selected),
            )));
            lines.push(TextLine::default());
            keys(
                &[
                    Button::new(
                        Key::Enter,
                        if view.rows.is_some() {
                            "Delete"
                        } else {
                            "Drop"
                        },
                        typed.confirmed,
                    ),
                    Button::new(Key::Escape, "Cancel", true),
                ],
                pressed,
                theme,
            )
        }
    };
    let row = Rect::new(
        inner.x,
        inner.y + u16::try_from(lines.len()).unwrap_or(u16::MAX),
        inner.width,
        1,
    );
    lines.push(buttons.line());

    frame.render_widget(Paragraph::new(lines), inner);

    if let Some(typed) = &view.typed {
        set_caret(frame, inner, view, typed);
    }

    Some(Modal::new(area, buttons.at(row)))
}

/// The `y` key's label. It names the verb, because the two verbs take
/// different things away and the border is the only other place that says
/// which.
const fn yes(view: &DropView) -> &'static str {
    match view.rows {
        Some(1) => "Yes, delete it",
        Some(_) => "Yes, delete them",
        None => "Yes, drop it",
    }
}

/// What the yes/no block says under the statement. The count is already in
/// the border; this is the line that says what answering costs.
fn warning(view: &DropView) -> String {
    match view.rows {
        Some(1) => "This row cannot be brought back.".to_owned(),
        Some(count) => format!("These {count} rows cannot be brought back."),
        None => "This cannot be undone.".to_owned(),
    }
}

/// Positions the caret in the name field, measuring the way
/// `export::set_caret` does: the cursor is a character offset, converted to a
/// column through `Line::width()` so a multi-byte or double-width character
/// before it is not counted as one column.
///
/// The row is counted rather than a constant: the statement above it is as
/// many lines as it is written on.
fn set_caret(frame: &mut Frame, inner: Rect, view: &DropView, typed: &TypedName) {
    let prefix: String = typed.text.chars().take(typed.cursor).collect();
    let taken = u16::try_from(TextLine::raw(prefix).width()).unwrap_or(inner.width);
    let cursor_x = (inner.x + taken).min(inner.right().saturating_sub(1));
    // A blank line, the statement, a blank line, the prompt, a blank line.
    let row = 4 + u16::try_from(statement(view).len()).unwrap_or(u16::MAX);

    frame.set_cursor_position((cursor_x, inner.y + row));
}

/// One button of a modal: the key that presses it, by click as by keyboard.
pub(crate) struct Button<'a> {
    pub(crate) key: Key,
    pub(crate) label: &'a str,
    pub(crate) live: bool,
}

impl<'a> Button<'a> {
    pub(crate) const fn new(key: Key, label: &'a str, live: bool) -> Self {
        Self { key, label, live }
    }

    fn glyph(&self) -> String {
        match self.key {
            Key::Char(character) => character.to_string(),
            Key::Enter => "⏎".to_owned(),
            Key::Escape => "esc".to_owned(),
            other => format!("{other:?}").to_lowercase(),
        }
    }
}

/// A row of [`Button`]s, drawn and measured from the same spans so a press
/// lands on what was painted.
pub(crate) struct Buttons {
    line: TextLine<'static>,
    /// Each live button's columns, from the left edge of the line.
    spans: Vec<(Key, u16, u16)>,
    width: u16,
}

impl Buttons {
    /// The row, right-aligned.
    pub(crate) fn line(&self) -> TextLine<'static> {
        self.line.clone().right_aligned()
    }

    /// Where each live button sits when the line is drawn across `row`.
    pub(crate) fn at(&self, row: Rect) -> Vec<ButtonAt> {
        let left = row.right().saturating_sub(self.width).max(row.x);

        self.spans
            .iter()
            .map(|&(key, start, end)| ButtonAt {
                area: Rect::new(left + start, row.y, end - start, 1),
                key,
            })
            .collect()
    }
}

/// The block's own keys as buttons: the key in the colour a shortcut always
/// wears and its label beside it, on the quiet band the export form's button
/// rests on. A key that would do nothing this frame recedes to `Faint` with
/// its label, an inert hint rather than a live one — the promise the bar's
/// greyed hints already keep (docs/shortcuts.md §2) — and is not pressable.
/// `pressed` lights one as solid as the export form's focused button. The last
/// button stops one column short of the border.
pub(crate) fn keys(buttons: &[Button], pressed: Option<Key>, theme: Theme) -> Buttons {
    let band = theme.style(Token::Ranged);
    let mut spans = Vec::new();
    let mut hits = Vec::new();
    let mut column = 0u16;

    for (index, button) in buttons.iter().enumerate() {
        if index > 0 {
            spans.push(Span::raw("  "));
            column += 2;
        }

        let (key_token, label_token) = if button.live {
            (Token::HintKey, Token::HintAction)
        } else {
            (Token::Faint, Token::Faint)
        };
        let lit = button.live && pressed == Some(button.key);
        let (pad, key_style, label_style) = if lit {
            let solid = theme
                .style(Token::HintAction)
                .add_modifier(Modifier::REVERSED | Modifier::BOLD);
            (solid, solid, solid)
        } else {
            (
                band,
                band.patch(theme.style(key_token)),
                band.patch(theme.style(label_token)),
            )
        };
        let glyph = button.glyph();
        let label = format!(" {} ", button.label);
        let width = u16::try_from(
            1 + TextLine::raw(glyph.as_str()).width() + TextLine::raw(label.as_str()).width(),
        )
        .unwrap_or(u16::MAX);

        // The pad is its own span: the key's underline must not run under it.
        spans.push(Span::styled(" ", pad));
        spans.push(Span::styled(glyph, key_style));
        spans.push(Span::styled(label, label_style));
        if button.live {
            hits.push((button.key, column, column + width));
        }
        column += width;
    }
    spans.push(Span::raw(" "));

    Buttons {
        line: TextLine::from(spans),
        spans: hits,
        width: column + 1,
    }
}

#[cfg(test)]
mod tests {
    use lc_domain::contents::TableRef;
    use lc_domain::drop::{DropPrompt, Target};
    use lc_domain::overlay::Overlay;
    use lc_domain::posture::Posture;
    use lc_domain::state::State;
    use lc_domain::view::view_model;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::buffer::{Buffer, Cell};

    use crate::theme::Theme;

    fn drawn(width: u16, height: u16, prompt: DropPrompt) -> Buffer {
        let mut state = State::default();
        state.overlay = Some(Overlay::Drop(prompt));
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

    fn table() -> DropPrompt {
        DropPrompt::opened(
            Target::Table(TableRef::new("shop", "orders")),
            Posture::Writable,
        )
    }

    fn keyspace() -> DropPrompt {
        DropPrompt::opened(Target::Keyspace("shop".to_owned()), Posture::Writable)
    }

    #[test]
    fn the_block_names_its_statement_and_its_keys() {
        let drawn = text(&drawn(80, 24, table()));

        assert!(drawn.contains("Drop table shop.orders"));
        assert!(
            drawn.contains("DROP TABLE \"shop\".\"orders\""),
            "the statement is shown before it runs"
        );
        assert!(drawn.contains("Yes, drop it"));
        assert!(drawn.contains("Cancel"));
    }

    #[test]
    fn a_keyspace_block_draws_the_name_field_rather_than_a_yes_key() {
        let drawn = text(&drawn(80, 24, keyspace()));

        assert!(drawn.contains("Drop keyspace shop"));
        assert!(drawn.contains("Type shop to confirm:"));
        assert!(
            !drawn.contains("Yes, drop it"),
            "a typed confirmation has no yes key"
        );
    }

    #[test]
    fn the_field_shows_what_has_been_typed() {
        let mut prompt = keyspace();
        for character in "sho".chars() {
            prompt
                .typed_mut()
                .expect("a typed confirmation")
                .insert(character);
        }

        assert!(text(&drawn(80, 24, prompt)).contains("sho"));
    }

    #[test]
    fn a_long_statement_widens_the_block_rather_than_being_clipped() {
        let long = "a".repeat(60);
        let prompt = DropPrompt::opened(
            Target::Table(TableRef::new(&long, "orders")),
            Posture::Writable,
        );

        assert!(
            text(&drawn(120, 24, prompt)).contains(&format!("DROP TABLE \"{long}\".\"orders\"")),
            "a statement confirmed blind is not confirmed"
        );
    }

    #[test]
    fn a_frame_too_small_for_the_block_draws_none_of_it() {
        assert!(!text(&drawn(40, 8, table())).contains("Drop table"));
    }

    /// `fits` is what the domain gates `d` on, so it has to agree with what
    /// `render` actually draws — the contract `export::fits` carries.
    #[test]
    fn fits_agrees_with_what_the_block_actually_draws() {
        for (width, height) in [(40, 24), (80, 8), (44, 10), (80, 24)] {
            let area = ratatui::layout::Rect::new(0, 0, width, height);
            let drew = text(&drawn(width, height, keyspace())).contains("Drop keyspace");

            assert_eq!(
                super::fits(area),
                drew,
                "fits and render disagree at {width}x{height}"
            );
        }
    }

    fn described() -> lc_domain::model::Table {
        use lc_domain::model::{
            ClusteringColumn, ClusteringOrder, Column, PrimaryKey, Table, TableOptions,
        };

        Table {
            name: "orders".to_owned(),
            primary_key: PrimaryKey {
                partition: vec![Column {
                    name: "id".to_owned(),
                    cql_type: "uuid".to_owned(),
                }],
                clustering: vec![ClusteringColumn {
                    column: Column {
                        name: "placed".to_owned(),
                        cql_type: "date".to_owned(),
                    },
                    order: ClusteringOrder::Ascending,
                }],
            },
            static_columns: Vec::new(),
            regular_columns: Vec::new(),
            indexes: Vec::new(),
            options: TableOptions::new(std::collections::BTreeMap::new()),
        }
    }

    fn deleting(count: usize) -> DropPrompt {
        use lc_domain::model::{CqlValue, Row};

        let columns = vec!["id".to_owned(), "placed".to_owned()];
        let rows: Vec<Row> = (0..count)
            .map(|index| Row {
                values: vec![
                    CqlValue::Uuid(format!("8f14e45f-000{index}")),
                    CqlValue::Text(format!("2026-01-0{}", index + 1)),
                ],
            })
            .collect();
        let target = Target::rows(
            &TableRef::new("shop", "orders"),
            &described(),
            &columns,
            rows.iter(),
        )
        .expect("every key column spells a literal");

        DropPrompt::opened(target, Posture::Writable)
    }

    #[test]
    fn a_delete_block_counts_the_rows_in_its_border_and_in_its_warning() {
        let drawn = text(&drawn(100, 26, deleting(3)));

        assert!(drawn.contains("Delete 3 rows from shop.orders"));
        assert!(drawn.contains("These 3 rows cannot be brought back."));
        assert!(drawn.contains("Yes, delete them"));
    }

    #[test]
    fn one_row_is_counted_in_the_singular_everywhere_it_is_counted() {
        let drawn = text(&drawn(100, 26, deleting(1)));

        assert!(drawn.contains("Delete 1 row from shop.orders"));
        assert!(drawn.contains("This row cannot be brought back."));
        assert!(drawn.contains("Yes, delete it"));
        assert!(
            !drawn.contains("BEGIN BATCH"),
            "a batch of one is a batch the user has to read past"
        );
    }

    #[test]
    fn a_delete_block_shows_the_statement_it_would_run() {
        let drawn = text(&drawn(100, 26, deleting(1)));

        assert!(drawn.contains(
            "DELETE FROM \"shop\".\"orders\" WHERE \"id\" = 8f14e45f-0000 AND \"placed\" = '2026-01-01'"
        ));
    }

    /// A batch too tall to draw whole keeps its head and its last line: one
    /// that stops before `APPLY BATCH` is not a statement, and a preview that
    /// is not one teaches the wrong shape.
    #[test]
    fn a_long_batch_is_elided_in_the_middle_and_says_how_much() {
        let drawn = text(&drawn(120, 30, deleting(9)));

        assert!(drawn.contains("BEGIN BATCH"));
        assert!(drawn.contains("… and 4 more"));
        assert!(drawn.contains("APPLY BATCH"));
    }

    /// Where `needle` starts on the frame, as a column and a row.
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

    #[test]
    fn a_press_resolves_to_a_button_the_block_or_the_world_outside() {
        use lc_domain::model::Key;
        use lc_domain::pointer::Pointer;

        let mut state = State::default();
        state.overlay = Some(Overlay::Drop(table()));
        let mut terminal = Terminal::new(TestBackend::new(80, 24)).expect("a backend");
        let mut map = crate::hit::FrameMap::default();
        terminal
            .draw(|frame| {
                map = crate::render::render(frame, &view_model(&state), Theme::default()).map;
            })
            .expect("TestBackend always draws");
        let buffer = terminal.backend().buffer();

        let (yes, row) = find(buffer, "Yes, drop it");
        let (cancel, _) = find(buffer, "Cancel");
        let (title, title_row) = find(buffer, "Drop table");

        assert_eq!(
            map.press(yes, row),
            Some(Pointer::Hold(Key::Char('y'))),
            "the label is the button"
        );
        assert_eq!(
            map.press(cancel + 5, row),
            Some(Pointer::Hold(Key::Char('n')))
        );
        assert_eq!(
            map.press(cancel - 3, row),
            Some(Pointer::Hold(Key::Char('n'))),
            "and so is the pad around it"
        );
        assert_eq!(
            map.press(cancel - 4, row),
            Some(Pointer::Modal),
            "the gap between them is the block's"
        );
        assert_eq!(map.press(title, title_row), Some(Pointer::Modal));
        assert!(matches!(map.press(0, 1), Some(Pointer::Click(_))));
    }

    #[test]
    fn a_held_button_is_drawn_solid_and_its_neighbour_is_not() {
        use lc_domain::model::Key;
        use lc_domain::pointer::Held;
        use ratatui::style::Modifier;

        let mut state = State::default();
        state.overlay = Some(Overlay::Drop(table()));
        state.held = Some(Held {
            key: Key::Char('y'),
            over: true,
        });
        let mut terminal = Terminal::new(TestBackend::new(80, 24)).expect("a backend");
        terminal
            .draw(|frame| {
                crate::render::render(frame, &view_model(&state), Theme::default());
            })
            .expect("TestBackend always draws");
        let buffer = terminal.backend().buffer();

        let (yes, row) = find(buffer, "Yes, drop it");
        let (cancel, _) = find(buffer, "Cancel");

        assert!(
            buffer[(yes, row)]
                .modifier
                .contains(Modifier::REVERSED | Modifier::BOLD)
        );
        assert!(!buffer[(cancel, row)].modifier.contains(Modifier::REVERSED));
    }
}

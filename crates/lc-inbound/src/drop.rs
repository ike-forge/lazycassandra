//! The drop confirmation, drawn over the panels
//! (docs/superpowers/specs/2026-08-31-drop-design.md §2).

use lc_domain::style::{Segment, Severity, Token};
use lc_domain::view::{DropView, TypedName};
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Modifier;
use ratatui::text::{Line as TextLine, Span};
use ratatui::widgets::{Block, BorderType, Clear, Paragraph};

use crate::theme::Theme;

/// A floor, not a cap: [`width`] widens the block to fit the statement on one
/// line rather than clip it. A `DROP` the user cannot read whole is a `DROP`
/// they are confirming blind, and unlike the export form's path field there
/// is no cursor here to scroll it around.
const MIN_WIDTH: u16 = 44;
/// The yes/no block: a blank line, the statement, a blank line, the warning, a
/// blank line, the keys — six content rows inside the border.
const YES_NO_HEIGHT: u16 = 8;
/// Two more rows: the field, and the blank line above it.
const TYPED_HEIGHT: u16 = 10;

/// Whether `area` can hold the block at all.
///
/// The domain's gate on `d` and on closing a confirmation the frame shrank
/// past reads this rather than a size of its own, for the reason
/// `export::fits` gives: the dimensions are named once, beside the block they
/// measure. Measured against the taller of the two spellings, so the gate
/// cannot open a question that grows past the frame the moment it is asked.
pub(crate) fn fits(area: Rect) -> bool {
    centred(area, MIN_WIDTH, TYPED_HEIGHT).is_some()
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
        .map(|typed| prompt_line(typed).len())
        .chain(std::iter::once(view.cql.chars().count()))
        .max()
        .unwrap_or_default();
    let needed = u16::try_from(longest + 4).unwrap_or(u16::MAX);

    needed.max(MIN_WIDTH).min(frame_width)
}

/// The block's border text. A run with no token of its own keeps the block's
/// own style; the names carry the schema colours the domain gave them. Bold
/// is dropped for `export::title`'s reason: the border is bold already, and a
/// schema name is not emphasis.
fn title(segments: &[Segment], theme: Theme) -> TextLine<'static> {
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

pub(crate) fn render(frame: &mut Frame, view: &DropView, theme: Theme) {
    // Asked before the block is sized, so the two cannot disagree: [`width`]
    // clamps to the frame, which on a narrow one would otherwise draw a
    // block below the floor `fits` promised (drop design §1).
    if !fits(frame.area()) {
        return;
    }

    let height = if view.typed.is_some() {
        TYPED_HEIGHT
    } else {
        YES_NO_HEIGHT
    };
    let Some(area) = centred(frame.area(), width(view, frame.area().width), height) else {
        return;
    };

    // The severity replaces the theme's border style, the one other place a
    // border says how bad the thing behind it is (`overlay.rs`).
    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(theme.style(Token::Severity(Severity::Danger)))
        .title(title(&view.title, theme));
    let inner = block.inner(area);

    frame.render_widget(Clear, area);
    frame.buffer_mut().set_style(area, theme.overlay_ground());
    frame.render_widget(block, area);

    let mut lines = vec![
        TextLine::default(),
        TextLine::from(Span::styled(view.cql.clone(), theme.style(Token::Keyword))),
        TextLine::default(),
    ];

    match &view.typed {
        None => {
            lines.push(TextLine::from("This cannot be undone."));
            lines.push(TextLine::default());
            lines.push(keys(
                &[("y", "Yes, drop it", true), ("n", "Cancel", true)],
                theme,
            ));
        }
        Some(typed) => {
            lines.push(TextLine::from(prompt_line(typed)));
            lines.push(TextLine::default());
            lines.push(TextLine::from(Span::styled(
                typed.text.clone(),
                theme.style(Token::Selected),
            )));
            lines.push(TextLine::default());
            lines.push(keys(
                &[("⏎", "Drop", typed.confirmed), ("esc", "Cancel", true)],
                theme,
            ));
        }
    }

    frame.render_widget(Paragraph::new(lines), inner);

    if let Some(typed) = &view.typed {
        set_caret(frame, inner, typed);
    }
}

/// The row of rows the field is drawn on, counted from the top of `inner`: a
/// blank line, the statement, a blank line, the prompt, a blank line.
const FIELD_ROW: u16 = 5;

/// Positions the caret in the name field, measuring the way
/// `export::set_caret` does: the cursor is a character offset, converted to a
/// column through `Line::width()` so a multi-byte or double-width character
/// before it is not counted as one column.
fn set_caret(frame: &mut Frame, inner: Rect, typed: &TypedName) {
    let prefix: String = typed.text.chars().take(typed.cursor).collect();
    let taken = u16::try_from(TextLine::raw(prefix).width()).unwrap_or(inner.width);
    let cursor_x = (inner.x + taken).min(inner.right().saturating_sub(1));

    frame.set_cursor_position((cursor_x, inner.y + FIELD_ROW));
}

/// The block's own keys, drawn the way a hint bar draws one
/// (`hints::Segment::spans`): the key in the colour a shortcut always wears,
/// the label beside it dim. A key that would do nothing this frame recedes to
/// `Faint` with its label, an inert hint rather than a live one — the promise
/// the bar's greyed hints already keep (docs/shortcuts.md §2).
fn keys(pairs: &[(&str, &str, bool)], theme: Theme) -> TextLine<'static> {
    let mut spans = Vec::new();

    for (index, (key, label, live)) in pairs.iter().enumerate() {
        if index > 0 {
            spans.push(Span::raw("    "));
        }

        let (key_token, label_token) = if *live {
            (Token::HintKey, Token::HintAction)
        } else {
            (Token::Faint, Token::Faint)
        };
        spans.push(Span::styled(format!("{key} "), theme.style(key_token)));
        spans.push(Span::styled((*label).to_owned(), theme.style(label_token)));
    }

    TextLine::from(spans).right_aligned()
}

#[cfg(test)]
mod tests {
    use lc_domain::contents::TableRef;
    use lc_domain::drop::{DropPrompt, Target};
    use lc_domain::posture::Posture;
    use lc_domain::state::{Overlay, State};
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
}

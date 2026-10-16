//! The Queries panel: the buffer and the completion popup, which must move
//! together. [`Editor`] knows no schema.

use crate::cql::{Completion, Span, statements};
use crate::editor::{Editor, Position};
use crate::model::Key;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refresh {
    /// Only `update` can reach the schema to offer again.
    Recompute,
    Close,
    Hold,
}

/// `end` is one past the last character; a span always holds a piece, so the
/// band is never empty.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Highlight {
    pub start: Position,
    pub end: Position,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum QueriesTab {
    #[default]
    Cql,
    Logs,
}

impl QueriesTab {
    pub const ALL: [Self; 2] = [Self::Cql, Self::Logs];

    #[must_use]
    pub const fn next(self) -> Self {
        match self {
            Self::Cql => Self::Logs,
            Self::Logs => Self::Cql,
        }
    }

    #[must_use]
    pub const fn previous(self) -> Self {
        match self {
            Self::Cql => Self::Logs,
            Self::Logs => Self::Cql,
        }
    }

    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Cql => "Queries",
            Self::Logs => "Logs",
        }
    }

    #[must_use]
    pub const fn short_label(self) -> &'static str {
        self.label()
    }

    /// Logs needs its own: `4` opens the buffer, which then takes a second `4`
    /// as text.
    #[must_use]
    pub const fn number(self) -> char {
        match self {
            Self::Cql => '4',
            Self::Logs => '5',
        }
    }

    #[must_use]
    pub fn from_number(key: char) -> Option<Self> {
        Self::ALL.into_iter().find(|tab| tab.number() == key)
    }
}

/// `4` and a click arrive editing; `Tab` and `Esc` leave the tab browsing,
/// where global keys keep their meaning.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum CqlMode {
    #[default]
    Browsing,
    Editing,
}

/// An editing Logs tab is unrepresentable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Shown {
    Cql(CqlMode),
    Logs,
}

impl Default for Shown {
    fn default() -> Self {
        Self::Cql(CqlMode::default())
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct QueriesPanel {
    buffer: Editor,
    completion: Option<Completion>,
    shown: Shown,
}

impl QueriesPanel {
    #[must_use]
    pub fn lines(&self) -> &[String] {
        self.buffer.lines()
    }

    #[must_use]
    pub const fn tab(&self) -> QueriesTab {
        match self.shown {
            Shown::Cql(_) => QueriesTab::Cql,
            Shown::Logs => QueriesTab::Logs,
        }
    }

    #[must_use]
    pub const fn mode(&self) -> Option<CqlMode> {
        match self.shown {
            Shown::Cql(mode) => Some(mode),
            Shown::Logs => None,
        }
    }

    #[must_use]
    pub const fn is_editing(&self) -> bool {
        matches!(self.shown, Shown::Cql(CqlMode::Editing))
    }

    /// Re-showing the drawn tab keeps an open buffer open.
    pub fn show(&mut self, tab: QueriesTab) {
        if self.tab() == tab {
            return;
        }

        self.stop_editing();
        self.shown = match tab {
            QueriesTab::Cql => Shown::Cql(CqlMode::Browsing),
            QueriesTab::Logs => Shown::Logs,
        };
    }

    pub fn start_editing(&mut self) {
        if let Shown::Cql(mode) = &mut self.shown {
            *mode = CqlMode::Editing;
        }
    }

    /// The popup goes too: its caret is no longer drawn.
    pub fn stop_editing(&mut self) {
        if let Shown::Cql(mode) = &mut self.shown {
            *mode = CqlMode::Browsing;
        }
        self.completion = None;
    }

    #[must_use]
    pub fn cursor(&self) -> Position {
        self.buffer.cursor()
    }

    #[must_use]
    pub fn text(&self) -> String {
        self.buffer.text()
    }

    #[must_use]
    pub const fn buffer(&self) -> &Editor {
        &self.buffer
    }

    #[must_use]
    pub const fn completion(&self) -> Option<&Completion> {
        self.completion.as_ref()
    }

    /// `None` on a buffer with no statement.
    #[must_use]
    pub fn band(&self) -> Option<Highlight> {
        let text = self.buffer.text();
        let span = self.span(&text)?;

        Some(Highlight {
            start: self.buffer.position_of(span.start),
            end: self.buffer.position_of(span.end),
        })
    }

    #[must_use]
    pub fn statement(&self) -> Option<String> {
        let text = self.buffer.text();

        Some(self.span(&text)?.of(&text))
    }

    /// Stops at either end, like any list cursor.
    pub fn step_statement(&mut self, forwards: bool) {
        let text = self.buffer.text();
        let spans = statements(&text);
        let Some(at) = holding(&spans, self.buffer.offset()) else {
            return;
        };

        let target = if forwards { at + 1 } else { at.wrapping_sub(1) };
        self.go_to(&spans, target);
    }

    pub fn edge_statement(&mut self, last: bool) {
        let text = self.buffer.text();
        let spans = statements(&text);
        let target = if last { spans.len().wrapping_sub(1) } else { 0 };

        self.go_to(&spans, target);
    }

    fn span(&self, text: &str) -> Option<Span> {
        let spans = statements(text);

        spans.get(holding(&spans, self.buffer.offset())?).copied()
    }

    fn go_to(&mut self, spans: &[Span], index: usize) {
        let Some(span) = spans.get(index) else {
            return;
        };

        self.buffer.go_to_offset(span.start);
        self.completion = None;
    }

    /// The popup described text that is gone.
    pub fn replace(&mut self, text: &str) {
        self.buffer.replace(text);
        self.completion = None;
    }

    pub fn append(&mut self, text: &str) {
        let current = self.text();

        if current.trim().is_empty() {
            self.replace(text);
        } else {
            self.replace(&format!("{}\n\n{text}", current.trim_end()));
        }
    }

    /// `false` when nothing changed, so the press costs no frame.
    pub fn format(&mut self) -> bool {
        let laid_out = crate::cql::format(&self.buffer.text());
        if laid_out == self.buffer.text() {
            return false;
        }

        self.replace(&laid_out);
        true
    }

    pub fn offer(&mut self, completion: Option<Completion>) {
        self.completion = completion;
    }

    pub fn step(&mut self, forwards: bool) -> bool {
        let Some(completion) = self.completion.as_mut() else {
            return false;
        };

        if forwards {
            completion.next();
        } else {
            completion.previous();
        }

        true
    }

    /// `None` where the key keeps its global meaning, and for every key while
    /// the buffer is closed. Exhaustive over [`Key`].
    pub fn edit(&mut self, key: Key, height: usize) -> Option<Refresh> {
        if !self.is_editing() {
            return None;
        }

        // With a popup up these five are its keys; otherwise none is claimed.
        // `Enter` accepts too, since a template's caret lands where a name, not
        // a newline, is wanted. `←`/`→` stay motions.
        if let Some(completion) = self.completion.take() {
            match key {
                Key::Tab | Key::Enter => {
                    completion.accept(&mut self.buffer);
                    return Some(Refresh::Close);
                }
                Key::Escape => return Some(Refresh::Close),
                Key::Up | Key::Down => {
                    self.completion = Some(completion);
                    self.step(key == Key::Down);
                    return Some(Refresh::Hold);
                }
                _ => self.completion = Some(completion),
            }
        }

        let buffer = &mut self.buffer;

        match key {
            Key::Char(character) => buffer.insert(character),
            Key::Backspace => buffer.backspace(),
            Key::Delete => buffer.delete(),
            Key::Enter => buffer.insert_newline(),
            Key::Left => return Some(self.moved(Editor::left)),
            Key::Right => return Some(self.moved(Editor::right)),
            Key::Up => return Some(self.moved(Editor::up)),
            Key::Down => return Some(self.moved(Editor::down)),
            Key::Home => return Some(self.moved(Editor::line_start)),
            Key::End => return Some(self.moved(Editor::line_end)),
            Key::PageUp => return Some(self.paged(height, Editor::page_up)),
            Key::PageDown => return Some(self.paged(height, Editor::page_down)),
            Key::Escape | Key::Tab | Key::BackTab | Key::Insert | Key::F(_) => return None,
        }

        Some(Refresh::Recompute)
    }

    /// Leaves the word the offers were made against.
    fn moved(&mut self, motion: impl Fn(&mut Editor)) -> Refresh {
        motion(&mut self.buffer);
        self.completion = None;

        Refresh::Close
    }

    fn paged(&mut self, height: usize, motion: impl Fn(&mut Editor, usize)) -> Refresh {
        motion(&mut self.buffer, height);
        self.completion = None;

        Refresh::Close
    }
}

/// Between statements, the last one beginning before `offset`, so a cursor past
/// a `;` names the statement it closed.
fn holding(spans: &[Span], offset: usize) -> Option<usize> {
    if spans.is_empty() {
        return None;
    }

    Some(
        spans
            .iter()
            .rposition(|span| span.start <= offset)
            .unwrap_or(0),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cql::suggest;
    use crate::model::Schema;
    use crate::tree::tests_support::schema_of;

    fn schema() -> Schema {
        schema_of(&[("shop", &["orders"])])
    }

    #[test]
    fn a_panel_opens_on_the_buffer_rather_than_the_trail() {
        assert_eq!(QueriesPanel::default().tab(), QueriesTab::Cql);
    }

    #[test]
    fn walking_the_tabs_wraps_and_stepping_back_undoes_it() {
        for tab in QueriesTab::ALL {
            assert_eq!(tab.next().previous(), tab);
        }
    }

    #[test]
    fn showing_a_tab_draws_it() {
        let mut panel = QueriesPanel::default();

        panel.show(QueriesTab::Logs);

        assert_eq!(panel.tab(), QueriesTab::Logs);
    }

    #[test]
    fn a_panel_opens_browsing_and_claims_no_key_until_opened() {
        let mut panel = QueriesPanel::default();

        assert_eq!(panel.mode(), Some(CqlMode::Browsing));
        assert_eq!(panel.edit(Key::Char('4'), 1), None);
        assert_eq!(panel.text(), "");

        panel.start_editing();

        assert_eq!(panel.edit(Key::Char('4'), 1), Some(Refresh::Recompute));
        assert_eq!(panel.text(), "4");
    }

    #[test]
    fn the_logs_tab_cannot_be_opened_for_typing() {
        let mut panel = QueriesPanel::default();
        panel.show(QueriesTab::Logs);

        panel.start_editing();

        assert_eq!(panel.mode(), None);
        assert!(!panel.is_editing());
    }

    #[test]
    fn leaving_the_cql_tab_closes_the_buffer_and_coming_back_does_not_reopen_it() {
        let mut panel = QueriesPanel::default();
        panel.start_editing();

        panel.show(QueriesTab::Logs);
        panel.show(QueriesTab::Cql);

        assert_eq!(panel.mode(), Some(CqlMode::Browsing));
    }

    #[test]
    fn showing_the_cql_tab_again_keeps_the_buffer_open() {
        let mut panel = QueriesPanel::default();
        panel.start_editing();

        panel.show(QueriesTab::Cql);

        assert!(panel.is_editing());
    }

    #[test]
    fn closing_the_buffer_takes_the_popup_down() {
        let mut panel = typed("se");

        panel.stop_editing();

        assert!(panel.completion().is_none());
    }

    fn opened() -> QueriesPanel {
        let mut panel = QueriesPanel::default();
        panel.start_editing();
        panel
    }

    /// The loop `update` runs for real.
    fn typed(text: &str) -> QueriesPanel {
        let mut panel = opened();
        let schema = schema();

        for character in text.chars() {
            let refresh = panel.edit(Key::Char(character), 1);
            assert_eq!(refresh, Some(Refresh::Recompute));
            panel.offer(suggest(panel.buffer(), Some(&schema), Some("shop")));
        }

        panel
    }

    #[test]
    fn typing_a_prefix_raises_a_popup_and_typing_past_it_takes_it_down() {
        assert!(typed("se").completion().is_some());
        assert!(
            typed("select").completion().is_none(),
            "the only offer would be the word already typed"
        );
    }

    #[test]
    fn tab_accepts_what_is_offered_and_takes_the_popup_down() {
        let mut panel = typed("se");

        assert_eq!(panel.edit(Key::Tab, 1), Some(Refresh::Close));

        assert_eq!(panel.text(), "SELECT ");
        assert!(panel.completion().is_none());
    }

    #[test]
    fn enter_accepts_what_is_offered_and_splits_the_line_without_a_popup() {
        let mut panel = typed("se");

        assert_eq!(panel.edit(Key::Enter, 1), Some(Refresh::Close));
        assert_eq!(panel.text(), "SELECT ");
        assert!(panel.completion().is_none());

        assert_eq!(panel.edit(Key::Enter, 1), Some(Refresh::Recompute));
        assert_eq!(
            panel.text(),
            "SELECT \n",
            "with the popup down, enter is a line break again"
        );
    }

    #[test]
    fn tab_with_no_popup_up_is_not_the_editors_to_claim() {
        let mut panel = opened();

        assert_eq!(panel.edit(Key::Tab, 1), None, "tab still moves focus");
    }

    #[test]
    fn escape_takes_the_popup_down_before_it_could_leave_the_panel() {
        let mut panel = typed("se");

        assert_eq!(panel.edit(Key::Escape, 1), Some(Refresh::Close));
        assert!(panel.completion().is_none());

        assert_eq!(
            panel.edit(Key::Escape, 1),
            None,
            "with the popup down, escape closes the buffer again"
        );
    }

    #[test]
    fn a_motion_takes_the_popup_down() {
        let mut panel = typed("se");

        assert_eq!(panel.edit(Key::Left, 1), Some(Refresh::Close));

        assert!(panel.completion().is_none());
    }

    #[test]
    fn the_arrows_walk_the_popup_rather_than_the_buffer() {
        let mut panel = typed("s");

        assert_eq!(panel.edit(Key::Down, 1), Some(Refresh::Hold));
        assert_eq!(panel.completion().expect("still up").selected(), 1);

        assert_eq!(panel.edit(Key::Up, 1), Some(Refresh::Hold));
        assert_eq!(panel.completion().expect("still up").selected(), 0);
    }

    #[test]
    fn walking_the_popup_leaves_the_cursor_where_it_is() {
        let mut panel = opened();
        panel.replace("SELECT *\nFROM shop.");
        assert_eq!(panel.edit(Key::Char('o'), 1), Some(Refresh::Recompute));
        panel.offer(suggest(panel.buffer(), Some(&schema()), Some("shop")));
        let cursor = panel.cursor();

        assert_eq!(panel.edit(Key::Up, 1), Some(Refresh::Hold));

        assert_eq!(panel.cursor(), cursor, "the buffer never saw the key");
        assert!(panel.completion().is_some());
    }

    #[test]
    fn an_arrow_with_no_popup_up_is_a_motion_again() {
        let mut panel = opened();
        panel.replace("SELECT *\nFROM shop.orders");

        assert_eq!(panel.edit(Key::Up, 1), Some(Refresh::Close));

        assert_eq!(panel.cursor().line, 0);
    }

    #[test]
    fn stepping_without_a_popup_does_nothing() {
        let mut panel = QueriesPanel::default();

        assert!(!panel.step(true));
    }

    fn stepped(text: &str) -> Vec<String> {
        let mut panel = QueriesPanel::default();
        panel.replace(text);
        panel.edge_statement(false);

        let mut named = Vec::new();
        loop {
            let statement = panel.statement().expect("the buffer holds statements");
            if named.last() == Some(&statement) {
                return named;
            }
            named.push(statement);
            panel.step_statement(true);
        }
    }

    #[test]
    fn an_empty_buffer_bands_and_runs_nothing() {
        let panel = QueriesPanel::default();

        assert_eq!(panel.band(), None);
        assert_eq!(panel.statement(), None);
    }

    #[test]
    fn a_buffer_of_whitespace_bands_and_runs_nothing() {
        let mut panel = QueriesPanel::default();
        panel.replace("\n  \n");

        assert_eq!(panel.band(), None);
        assert_eq!(panel.statement(), None);
    }

    #[test]
    fn the_band_covers_the_statement_and_not_the_whitespace_around_it() {
        let mut panel = QueriesPanel::default();
        panel.replace("  SELECT 1;  ");
        panel.edge_statement(false);

        assert_eq!(
            panel.band(),
            Some(Highlight {
                start: Position { line: 0, column: 2 },
                end: Position {
                    line: 0,
                    column: 11
                },
            })
        );
    }

    #[test]
    fn the_band_spans_the_lines_a_statement_is_laid_out_over() {
        let mut panel = QueriesPanel::default();
        panel.replace("SELECT *\nFROM shop.orders;\nSELECT 2");
        panel.edge_statement(false);

        assert_eq!(
            panel.band(),
            Some(Highlight {
                start: Position { line: 0, column: 0 },
                end: Position {
                    line: 1,
                    column: 17
                },
            })
        );
    }

    #[test]
    fn stepping_walks_the_statements_in_the_order_they_are_written() {
        assert_eq!(
            stepped("SELECT 1;\nSELECT 2;\nSELECT 3"),
            ["SELECT 1;", "SELECT 2;", "SELECT 3"]
        );
    }

    #[test]
    fn a_step_off_either_end_stays_on_the_statement_it_was_on() {
        let mut panel = QueriesPanel::default();
        panel.replace("SELECT 1;\nSELECT 2");

        panel.edge_statement(false);
        panel.step_statement(false);
        assert_eq!(panel.statement().as_deref(), Some("SELECT 1;"));

        panel.edge_statement(true);
        panel.step_statement(true);
        assert_eq!(panel.statement().as_deref(), Some("SELECT 2"));
    }

    #[test]
    fn a_batch_is_one_statement_to_step_over_and_to_run() {
        assert_eq!(
            stepped("BEGIN BATCH\nINSERT INTO a (id) VALUES (1);\nAPPLY BATCH;\nSELECT * FROM a"),
            [
                "BEGIN BATCH\nINSERT INTO a (id) VALUES (1);\nAPPLY BATCH;",
                "SELECT * FROM a"
            ]
        );
    }

    /// `replace` leaves the cursor past the last `;`.
    #[test]
    fn a_cursor_past_the_last_semicolon_still_names_the_statement_it_closed() {
        let mut panel = QueriesPanel::default();
        panel.replace("SELECT 1;\nSELECT 2;\n");

        assert_eq!(panel.statement().as_deref(), Some("SELECT 2;"));
    }

    #[test]
    fn a_step_that_moves_takes_the_popup_down() {
        let mut panel = typed("SELECT 1;se");
        assert!(panel.completion().is_some());

        panel.step_statement(false);

        assert!(panel.completion().is_none());
    }

    #[test]
    fn the_band_follows_the_caret_while_the_buffer_is_typed_into() {
        let mut panel = opened();
        panel.replace("SELECT 1;\nSELECT 2");
        panel.edge_statement(false);
        assert_eq!(panel.statement().as_deref(), Some("SELECT 1;"));

        panel.edit(Key::Down, 1);
        panel.edit(Key::End, 1);

        assert_eq!(
            panel.statement().as_deref(),
            Some("SELECT 2"),
            "an edit moves the band, so `ctrl-r` follows the caret"
        );
    }

    #[test]
    fn formatting_an_already_formatted_buffer_changes_nothing() {
        let mut panel = QueriesPanel::default();
        panel.replace("SELECT *\nFROM t");

        assert!(
            !panel.format(),
            "no frame is owed for a press that did nothing"
        );
    }

    #[test]
    fn formatting_lays_the_buffer_out_and_leaves_the_cursor_in_it() {
        let mut panel = QueriesPanel::default();
        panel.replace("select * from shop.orders where id = 7");

        assert!(panel.format());

        assert_eq!(panel.text(), "SELECT *\nFROM shop.orders\nWHERE id = 7");
        assert_eq!(
            panel.cursor().line,
            2,
            "the cursor is inside what was written"
        );
    }
}

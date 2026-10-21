//! Resolving a pointer position against the frame it happened in, assembled
//! from what did the drawing: a second layout description drifts silently.

use lc_domain::export::Field;
use lc_domain::panel::Panel;
use lc_domain::pointer::{Click, Mark, Pointer, Press};
use ratatui::layout::{Position, Rect};
use unicode_width::UnicodeWidthChar as _;

use crate::body::{Body, Unit};

/// Half-open, in frame coordinates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Band<T> {
    pub(crate) start: u16,
    pub(crate) end: u16,
    pub(crate) names: T,
}

impl<T: Copy> Band<T> {
    const fn holds(&self, x: u16) -> bool {
        x >= self.start && x < self.end
    }

    fn at(bands: &[Self], x: u16) -> Option<T> {
        bands
            .iter()
            .find(|band| band.holds(x))
            .map(|band| band.names)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct MarkAt {
    pub(crate) column: usize,
    pub(crate) mark: Mark,
}

/// The default is an empty frame, which nothing can be clicked in.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct FrameMap {
    /// In [`Panel::ALL`] order.
    panels: [PanelHit; 4],
    /// Only when it answers presses of its own.
    modal: Option<Modal>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Modal {
    pub(crate) area: Rect,
    pub(crate) buttons: Vec<ButtonAt>,
    pub(crate) fields: Vec<FieldAt>,
    pub(crate) list: Option<Listed>,
}

impl Modal {
    pub(crate) const fn new(area: Rect, buttons: Vec<ButtonAt>) -> Self {
        Self {
            area,
            buttons,
            fields: Vec::new(),
            list: None,
        }
    }

    pub(crate) fn with_fields(self, fields: Vec<FieldAt>) -> Self {
        Self { fields, ..self }
    }

    pub(crate) fn with_list(self, list: Option<Listed>) -> Self {
        Self { list, ..self }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FieldAt {
    pub(crate) area: Rect,
    pub(crate) target: Target,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Target {
    Fixed(Field),
    Text(TextAt),
}

/// Enough to name the character under a column without asking the renderer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TextAt {
    pub(crate) text: String,
    /// Character index of the first one drawn.
    pub(crate) first: usize,
    pub(crate) rows: bool,
}

impl Target {
    fn at(&self, offset: u16) -> Field {
        match self {
            Self::Fixed(field) => *field,
            Self::Text(text) => {
                let cursor = text.cursor_at(offset);

                if text.rows {
                    Field::Rows { cursor }
                } else {
                    Field::Path { cursor }
                }
            }
        }
    }
}

impl TextAt {
    /// Before the character under the column, or at the end past it.
    fn cursor_at(&self, offset: u16) -> usize {
        let mut used = 0u16;

        for (index, character) in self.text.chars().enumerate().skip(self.first) {
            let width = u16::try_from(character.width().unwrap_or(0)).unwrap_or(0);
            if offset < used + width {
                return index;
            }
            used += width;
        }

        self.text.chars().count()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Listed {
    pub(crate) area: Rect,
    pub(crate) first: usize,
    pub(crate) len: usize,
}

impl Listed {
    fn row_at(&self, position: Position) -> Option<usize> {
        if !self.area.contains(position) {
            return None;
        }
        let row = self.first + usize::from(position.y - self.area.y);

        (row < self.len).then_some(row)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ButtonAt {
    pub(crate) area: Rect,
    pub(crate) press: Press,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct PanelHit {
    pub(crate) panel: Panel,
    pub(crate) area: Rect,
    pub(crate) inner: Rect,
    pub(crate) tabs: Vec<Band<usize>>,
    pub(crate) columns: Vec<Band<usize>>,
    pub(crate) marks: Vec<Band<MarkAt>>,
    pub(crate) rows: Option<RowStrip>,
}

/// A list is one row per line; Ddl wraps one row over several, and has no
/// `header` to ask about.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum RowStrip {
    Rows {
        /// Below the body's own chrome, such as a grid's header.
        top: u16,
        first: usize,
        /// Makes the blank below a short list chrome.
        len: usize,
    },
    Lines {
        top: u16,
        first: usize,
        /// Over the whole body, not only what is in view.
        rows: Vec<Option<usize>>,
    },
}

impl FrameMap {
    pub(crate) const fn new(panels: [PanelHit; 4]) -> Self {
        Self {
            panels,
            modal: None,
        }
    }

    pub(crate) fn with_modal(self, modal: Option<Modal>) -> Self {
        Self { modal, ..self }
    }

    /// With a modal up, a miss on it falls through to [`Self::hit`], which the
    /// domain reads as a dismissing click.
    pub(crate) fn press(&self, column: u16, row: u16) -> Option<Pointer> {
        let position = Position::new(column, row);

        if let Some(modal) = self.modal.as_ref().filter(|m| m.area.contains(position)) {
            if let Some(button) = modal
                .buttons
                .iter()
                .find(|button| button.area.contains(position))
            {
                return Some(Pointer::Hold(button.press));
            }
            if let Some(field) = modal
                .fields
                .iter()
                .find(|field| field.area.contains(position))
            {
                return Some(Pointer::Field(field.target.at(column - field.area.x)));
            }

            return Some(
                modal
                    .list
                    .and_then(|list| list.row_at(position))
                    .map_or(Pointer::Modal, Pointer::Entry),
            );
        }

        self.hit(column, row).map(Pointer::Click)
    }

    /// `None` with no modal up.
    pub(crate) fn over(
        &self,
        column: u16,
        row: u16,
        gesture: fn(Option<Press>) -> Pointer,
    ) -> Option<Pointer> {
        let position = Position::new(column, row);
        let modal = self.modal.as_ref()?;

        Some(gesture(
            modal
                .buttons
                .iter()
                .find(|button| button.area.contains(position))
                .map(|button| button.press),
        ))
    }

    /// `None` outside every panel. `pub(crate)` so `to_pointer` is the single
    /// route.
    #[must_use]
    pub(crate) fn hit(&self, column: u16, row: u16) -> Option<Click> {
        let position = Position::new(column, row);

        self.panels
            .iter()
            .find(|hit| hit.area.contains(position))
            .map(|hit| hit.click_at(position))
    }
}

impl PanelHit {
    pub(crate) fn new(
        panel: Panel,
        area: Rect,
        inner: Rect,
        tabs: Vec<Band<usize>>,
        body: Option<&Body>,
    ) -> Self {
        Self {
            panel,
            area,
            inner,
            tabs,
            columns: body.map(|body| body.bands.clone()).unwrap_or_default(),
            marks: body.map(|body| body.marks.clone()).unwrap_or_default(),
            rows: RowStrip::of(inner, body),
        }
    }

    fn click_at(&self, position: Position) -> Click {
        let chrome = Click::Panel(self.panel);

        // Tabs sit on the top border, so they are asked before the chrome
        // fall-through.
        if position.y == self.area.y
            && let Some(tab) = Band::at(&self.tabs, position.x)
        {
            return Click::Tab {
                panel: self.panel,
                tab,
            };
        }

        if !self.inner.contains(position) {
            return chrome;
        }

        let column = Band::at(&self.columns, position.x);

        match &self.rows {
            Some(RowStrip::Rows { top, first, len }) => {
                // A grid heading names a column and no row.
                if position.y < *top {
                    // Marks first: a press on one is `s`/`f`; elsewhere in the
                    // heading it is `h`/`l`.
                    if let Some(at) = Band::at(&self.marks, position.x) {
                        return Click::Mark {
                            panel: self.panel,
                            column: at.column,
                            mark: at.mark,
                        };
                    }

                    return match column {
                        Some(column) => Click::Column {
                            panel: self.panel,
                            column,
                        },
                        None => chrome,
                    };
                }

                let row = first + usize::from(position.y - top);

                if row >= *len {
                    return chrome;
                }

                Click::Row {
                    panel: self.panel,
                    row,
                    column,
                }
            }
            Some(RowStrip::Lines { top, first, rows }) => {
                if position.y < *top {
                    return chrome;
                }

                let line = first + usize::from(position.y - top);

                match rows.get(line).copied().flatten() {
                    Some(row) => Click::Row {
                        panel: self.panel,
                        row,
                        column: None,
                    },
                    // A blank between groups, a heading, or past a short body.
                    None => chrome,
                }
            }
            None => chrome,
        }
    }
}

impl RowStrip {
    /// `None` for a blank panel, a notice, or the editor.
    fn of(inner: Rect, body: Option<&Body>) -> Option<Self> {
        let body = body?;

        match &body.unit {
            Unit::Rows { header } => Some(Self::Rows {
                top: inner.y.saturating_add(*header),
                first: body.extent.position,
                len: body.extent.total,
            }),
            Unit::Lines { rows } if rows.is_empty() => None,
            Unit::Lines { rows } => Some(Self::Lines {
                top: inner.y,
                first: body.extent.position,
                rows: rows.clone(),
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::layout::Margin;

    fn inner(area: Rect) -> Rect {
        area.inner(Margin {
            horizontal: 1,
            vertical: 1,
        })
    }

    fn listed(panel: Panel, area: Rect, header: u16, first: usize, len: usize) -> PanelHit {
        let inner = inner(area);

        PanelHit {
            panel,
            area,
            inner,
            tabs: Vec::new(),
            columns: Vec::new(),
            marks: Vec::new(),
            rows: Some(RowStrip::Rows {
                top: inner.y + header,
                first,
                len,
            }),
        }
    }

    fn wrapped(panel: Panel, area: Rect, first: usize, rows: &[Option<usize>]) -> PanelHit {
        let inner = inner(area);

        PanelHit {
            panel,
            area,
            inner,
            tabs: Vec::new(),
            columns: Vec::new(),
            marks: Vec::new(),
            rows: Some(RowStrip::Lines {
                top: inner.y,
                first,
                rows: rows.to_vec(),
            }),
        }
    }

    fn plain(panel: Panel, area: Rect) -> PanelHit {
        PanelHit {
            panel,
            area,
            inner: inner(area),
            tabs: Vec::new(),
            columns: Vec::new(),
            marks: Vec::new(),
            rows: None,
        }
    }

    fn gridded(panel: Panel, area: Rect, len: usize, x: u16, widths: &[u16]) -> PanelHit {
        let inner = inner(area);
        let mut start = x;

        let columns = widths
            .iter()
            .enumerate()
            .map(|(index, width)| {
                let band = Band {
                    start,
                    end: start + width,
                    names: index,
                };
                start = band.end + 1;

                band
            })
            .collect();

        PanelHit {
            panel,
            area,
            inner,
            tabs: Vec::new(),
            columns,
            marks: Vec::new(),
            rows: Some(RowStrip::Rows {
                top: inner.y + 1,
                first: 0,
                len,
            }),
        }
    }

    /// Tables down the left, Contents down the right, 40×12 with the hint bar
    /// on the bottom row.
    fn map(tables: PanelHit) -> FrameMap {
        FrameMap::new([
            tables,
            plain(Panel::Ddl, Rect::new(0, 7, 16, 4)),
            plain(Panel::Contents, Rect::new(16, 0, 24, 8)),
            plain(Panel::Queries, Rect::new(16, 8, 24, 3)),
        ])
    }

    #[test]
    fn a_press_inside_the_rows_resolves_to_the_row_under_it() {
        let map = map(listed(Panel::Tables, Rect::new(0, 0, 16, 7), 0, 0, 3));

        assert_eq!(
            map.hit(2, 1),
            Some(Click::Row {
                panel: Panel::Tables,
                row: 0,
                column: None
            })
        );
        assert_eq!(
            map.hit(2, 3),
            Some(Click::Row {
                panel: Panel::Tables,
                row: 2,
                column: None
            })
        );
    }

    #[test]
    fn a_scrolled_panel_resolves_against_the_row_at_its_top() {
        let map = map(listed(Panel::Tables, Rect::new(0, 0, 16, 7), 0, 15, 40));

        assert_eq!(
            map.hit(2, 1),
            Some(Click::Row {
                panel: Panel::Tables,
                row: 15,
                column: None
            }),
            "the offset is what the row index counts from"
        );
    }

    #[test]
    fn a_header_row_over_no_column_is_chrome_rather_than_the_first_row() {
        let map = map(listed(Panel::Tables, Rect::new(0, 0, 16, 7), 1, 0, 3));

        assert_eq!(map.hit(2, 1), Some(Click::Panel(Panel::Tables)));
        assert_eq!(
            map.hit(2, 2),
            Some(Click::Row {
                panel: Panel::Tables,
                row: 0,
                column: None
            })
        );
    }

    #[test]
    fn a_press_past_the_last_row_is_chrome() {
        let map = map(listed(Panel::Tables, Rect::new(0, 0, 16, 7), 0, 0, 2));

        assert_eq!(
            map.hit(2, 3),
            Some(Click::Panel(Panel::Tables)),
            "the blank space below a short list is not row 2"
        );
    }

    #[test]
    fn a_press_on_the_border_is_chrome() {
        let map = map(listed(Panel::Tables, Rect::new(0, 0, 16, 7), 0, 0, 5));

        assert_eq!(map.hit(0, 0), Some(Click::Panel(Panel::Tables)), "corner");
        assert_eq!(
            map.hit(3, 0),
            Some(Click::Panel(Panel::Tables)),
            "top border away from any tab"
        );
        assert_eq!(
            map.hit(15, 2),
            Some(Click::Panel(Panel::Tables)),
            "the right border sits beside a row, not on one"
        );
        assert_eq!(map.hit(2, 6), Some(Click::Panel(Panel::Tables)), "bottom");
    }

    #[test]
    fn a_press_on_a_body_with_no_rows_is_chrome() {
        let map = map(plain(Panel::Tables, Rect::new(0, 0, 16, 7)));

        assert_eq!(map.hit(2, 2), Some(Click::Panel(Panel::Tables)));
    }

    #[test]
    fn a_press_outside_every_panel_hits_nothing() {
        let map = map(listed(Panel::Tables, Rect::new(0, 0, 16, 7), 0, 0, 5));

        assert_eq!(map.hit(2, 11), None, "the hint bar is not a panel");
    }

    #[test]
    fn a_press_on_a_tab_names_its_place_in_the_strip() {
        let mut ddl = plain(Panel::Ddl, Rect::new(0, 7, 16, 4));
        ddl.tabs = vec![
            Band {
                start: 2,
                end: 5,
                names: 0,
            },
            Band {
                start: 8,
                end: 15,
                names: 1,
            },
        ];
        let map = FrameMap::new([
            plain(Panel::Tables, Rect::new(0, 0, 16, 7)),
            ddl,
            plain(Panel::Contents, Rect::new(16, 0, 24, 8)),
            plain(Panel::Queries, Rect::new(16, 8, 24, 3)),
        ]);

        assert_eq!(
            map.hit(3, 7),
            Some(Click::Tab {
                panel: Panel::Ddl,
                tab: 0
            })
        );
        assert_eq!(
            map.hit(9, 7),
            Some(Click::Tab {
                panel: Panel::Ddl,
                tab: 1
            })
        );
        assert_eq!(
            map.hit(6, 7),
            Some(Click::Panel(Panel::Ddl)),
            "the separator between two tabs is neither of them"
        );
        assert_eq!(
            map.hit(3, 8),
            Some(Click::Panel(Panel::Ddl)),
            "a strip lives on the top border and nowhere else"
        );
    }

    #[test]
    fn a_press_on_a_wrapped_body_names_the_row_the_line_came_from() {
        // Row 0 wrapped to two lines, then a blank, then row 1 on one.
        let rows = [Some(0), Some(0), None, Some(1)];
        let map = map(wrapped(Panel::Tables, Rect::new(0, 0, 16, 7), 0, &rows));

        for (line, row) in [(0, 0), (1, 0), (3, 1)] {
            assert_eq!(
                map.hit(2, 1 + line),
                Some(Click::Row {
                    panel: Panel::Tables,
                    row,
                    column: None
                }),
                "line {line} was wrapped from row {row}"
            );
        }

        assert_eq!(
            map.hit(2, 3),
            Some(Click::Panel(Panel::Tables)),
            "a blank between two groups belongs to no row"
        );
    }

    #[test]
    fn a_scrolled_wrapped_body_resolves_against_the_line_at_its_top() {
        let rows = [Some(0), Some(0), Some(1), Some(2), Some(2)];
        let map = map(wrapped(Panel::Tables, Rect::new(0, 0, 16, 7), 2, &rows));

        assert_eq!(
            map.hit(2, 1),
            Some(Click::Row {
                panel: Panel::Tables,
                row: 1,
                column: None
            }),
            "the offset is what the line index counts from"
        );
        assert_eq!(
            map.hit(2, 5),
            Some(Click::Panel(Panel::Tables)),
            "past the last line is blank space, not a row"
        );
    }

    /// Bar and gutter at x=17, spacing at x=18, then two four-wide columns.
    fn grid_map() -> FrameMap {
        FrameMap::new([
            plain(Panel::Tables, Rect::new(0, 0, 16, 7)),
            plain(Panel::Ddl, Rect::new(0, 7, 16, 4)),
            gridded(Panel::Contents, Rect::new(16, 0, 24, 8), 2, 19, &[4, 4]),
            plain(Panel::Queries, Rect::new(16, 8, 24, 3)),
        ])
    }

    #[test]
    fn a_press_in_a_grid_names_the_row_and_the_column_it_landed_in() {
        let map = grid_map();

        assert_eq!(
            map.hit(20, 2),
            Some(Click::Row {
                panel: Panel::Contents,
                row: 0,
                column: Some(0)
            })
        );
        assert_eq!(
            map.hit(25, 3),
            Some(Click::Row {
                panel: Panel::Contents,
                row: 1,
                column: Some(1)
            })
        );
    }

    #[test]
    fn a_press_on_the_gutter_names_a_row_and_no_column() {
        let map = grid_map();

        assert_eq!(
            map.hit(17, 2),
            Some(Click::Row {
                panel: Panel::Contents,
                row: 0,
                column: None
            }),
            "the bar and the row number are not a column of the page"
        );
    }

    fn marked_map() -> FrameMap {
        let mut hit = gridded(Panel::Contents, Rect::new(16, 0, 24, 8), 2, 19, &[4, 4]);
        hit.marks = [(21, Mark::Sort), (22, Mark::Filter)]
            .into_iter()
            .map(|(start, mark)| Band {
                start,
                end: start + 1,
                names: MarkAt { column: 0, mark },
            })
            .collect();

        FrameMap::new([
            plain(Panel::Tables, Rect::new(0, 0, 16, 7)),
            plain(Panel::Ddl, Rect::new(0, 7, 16, 4)),
            hit,
            plain(Panel::Queries, Rect::new(16, 8, 24, 3)),
        ])
    }

    #[test]
    fn a_press_on_a_heading_mark_is_the_key_that_mark_stands_for() {
        let map = marked_map();

        assert_eq!(
            map.hit(21, 1),
            Some(Click::Mark {
                panel: Panel::Contents,
                column: 0,
                mark: Mark::Sort
            })
        );
        assert_eq!(
            map.hit(22, 1),
            Some(Click::Mark {
                panel: Panel::Contents,
                column: 0,
                mark: Mark::Filter
            })
        );
        assert_eq!(
            map.hit(19, 1),
            Some(Click::Column {
                panel: Panel::Contents,
                column: 0
            }),
            "the name beside them is still the column, not a mark"
        );
        assert_eq!(
            map.hit(21, 2),
            Some(Click::Row {
                panel: Panel::Contents,
                row: 0,
                column: Some(0)
            }),
            "and a mark's column below the heading is a row like any other"
        );
    }

    #[test]
    fn a_press_on_a_heading_cell_names_its_column_and_no_row() {
        let map = grid_map();

        assert_eq!(
            map.hit(20, 1),
            Some(Click::Column {
                panel: Panel::Contents,
                column: 0
            })
        );
        assert_eq!(
            map.hit(25, 1),
            Some(Click::Column {
                panel: Panel::Contents,
                column: 1
            })
        );
        assert_eq!(
            map.hit(17, 1),
            Some(Click::Panel(Panel::Contents)),
            "the blank over the gutter names no column"
        );
    }

    #[test]
    fn a_press_right_of_the_last_column_names_no_column() {
        let map = grid_map();

        assert_eq!(
            map.hit(30, 2),
            Some(Click::Row {
                panel: Panel::Contents,
                row: 0,
                column: None
            }),
            "the grid runs out before the panel does"
        );
        assert_eq!(map.hit(30, 1), Some(Click::Panel(Panel::Contents)));
    }

    #[test]
    fn an_empty_map_is_not_clickable() {
        assert_eq!(FrameMap::default().hit(0, 0), None);
        assert_eq!(FrameMap::default().hit(20, 5), None);
    }
}

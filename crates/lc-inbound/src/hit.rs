//! Resolving a pointer position against the frame it happened in.
//!
//! Assembled from the values that did the drawing — the `Rect` a panel was
//! given and the `Extent` its body returned — rather than computed a second
//! time. A second description of the layout drifts, and drift here is silent:
//! presses landing on the wrong row
//! (docs/superpowers/specs/2026-08-09-mouse-input-design.md §4).

use lc_domain::export::Field;
use lc_domain::model::Key;
use lc_domain::panel::Panel;
use lc_domain::pointer::{Click, Mark, Pointer};
use ratatui::layout::{Position, Rect};
use unicode_width::UnicodeWidthChar as _;

use crate::body::{Body, Unit};

/// A run of screen columns, and what was drawn across it.
///
/// Half-open — `start..end`, in absolute frame coordinates — and generic in
/// what it names, because a grid's columns are named by index and a title
/// strip's tabs by their place in the strip.
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

    /// What the band holding `x` names, if one does.
    fn at(bands: &[Self], x: u16) -> Option<T> {
        bands
            .iter()
            .find(|band| band.holds(x))
            .map(|band| band.names)
    }
}

/// One of a heading's marks, and the page column it belongs to.
///
/// What a [`Band`] over the two mark cells names, so resolving a press there
/// needs no second description of which column it sat over
/// (docs/superpowers/specs/2026-08-31-grid-sort-filter-design.md §5).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct MarkAt {
    pub(crate) column: usize,
    pub(crate) mark: Mark,
}

/// Where each panel's content ended up in the frame just drawn.
///
/// The default is a frame with nothing in it, which nothing can be clicked in
/// — what a frame below the minimum size reports.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct FrameMap {
    /// In [`Panel::ALL`] order.
    panels: [PanelHit; 4],
    /// The modal standing over them, when it answers presses of its own.
    modal: Option<Modal>,
}

/// A modal's block and what is drawn in it, as a press sees them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Modal {
    pub(crate) area: Rect,
    pub(crate) buttons: Vec<ButtonAt>,
    /// What a press takes effect on, off the buttons: a form's fields.
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

/// A form field's columns, and what a press in them chooses.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FieldAt {
    pub(crate) area: Rect,
    pub(crate) target: Target,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Target {
    Fixed(Field),
    /// A text field, which a press also places the caret in.
    Text(TextAt),
}

/// The text a field drew, and where its window began: enough to say which
/// character a column was over without asking the renderer again.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TextAt {
    pub(crate) text: String,
    /// Character index of the first one drawn.
    pub(crate) first: usize,
    pub(crate) rows: bool,
}

impl Target {
    /// The field a press `offset` columns into the area chose.
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
    /// The caret goes before the character under the column, or at the end
    /// of the text when the press is past it.
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

/// A list drawn one row to a line down `area`, `first` at its top.
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

/// One button, and the key pressing it stands for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ButtonAt {
    pub(crate) area: Rect,
    pub(crate) key: Key,
}

/// One panel, as a press sees it.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct PanelHit {
    pub(crate) panel: Panel,
    /// The whole block, borders included.
    pub(crate) area: Rect,
    /// The body, inside the borders.
    pub(crate) inner: Rect,
    /// The tabs on the top border, each naming its place in the strip.
    pub(crate) tabs: Vec<Band<usize>>,
    /// The columns of a grid body, each naming its index in the page.
    pub(crate) columns: Vec<Band<usize>>,
    /// The two marks each drawn heading carries, inside those columns.
    pub(crate) marks: Vec<Band<MarkAt>>,
    pub(crate) rows: Option<RowStrip>,
}

/// The band of a body that drew rows, and how to name the one at a given
/// screen row.
///
/// Two variants because the two regimes cannot be told apart by a count: a
/// list stands one row to a line, and the Ddl body wraps one row across
/// several. A `header` beside a line map would let a caller ask a body that
/// wraps how many lines its chrome takes, which is not a question it has an
/// answer to (conventions.md §1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum RowStrip {
    /// One screen row per row.
    Rows {
        /// Screen row of the first row drawn — below whatever chrome the body
        /// spends above it, such as a grid's header.
        top: u16,
        /// The row index drawn at `top`.
        first: usize,
        /// How many rows exist, which is what makes the blank space below a
        /// short list chrome rather than a row.
        len: usize,
    },
    /// Wrapped lines, each naming the row it came from.
    Lines {
        /// Screen row of the first line drawn.
        top: u16,
        /// The line index drawn at `top`.
        first: usize,
        /// The row each line belongs to, over the whole body — not only the
        /// lines scrolled into view.
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

    /// What a press at `column`, `row` means: a modal's button, a row of its
    /// list, the rest of its block (which answers nothing), or else whatever [`Self::hit`] finds
    /// beneath — which, with a modal up, the domain reads as the click outside
    /// it that dismisses it.
    pub(crate) fn press(&self, column: u16, row: u16) -> Option<Pointer> {
        let position = Position::new(column, row);

        if let Some(modal) = self.modal.as_ref().filter(|m| m.area.contains(position)) {
            if let Some(button) = modal
                .buttons
                .iter()
                .find(|button| button.area.contains(position))
            {
                return Some(Pointer::Hold(button.key));
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

    /// `gesture` over the modal button under `column`, `row` — or over none,
    /// when it is off them. `None` outright with no modal up, where a release
    /// or a drag means nothing.
    pub(crate) fn over(
        &self,
        column: u16,
        row: u16,
        gesture: fn(Option<Key>) -> Pointer,
    ) -> Option<Pointer> {
        let position = Position::new(column, row);
        let modal = self.modal.as_ref()?;

        Some(gesture(
            modal
                .buttons
                .iter()
                .find(|button| button.area.contains(position))
                .map(|button| button.key),
        ))
    }

    /// What a press at `column`, `row` resolves to, or `None` when it landed
    /// outside every panel — the hint bar, or a frame too small to draw panels
    /// in.
    ///
    /// `pub(crate)`: `to_pointer` is the only caller outside this module's own
    /// tests, and keeping resolution behind it is what makes "`lc-inbound`
    /// resolves" (design §3) a single route rather than a convention.
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

        // The strip sits on the top border, which is chrome everywhere else
        // along its length — so the tabs are asked before the fall-through, not
        // after it.
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
                // Above the first row and still inside the body is a grid's
                // heading: it names a column and no row, which is the `h`/`l`
                // binding rather than a sort nobody asked for (design §1).
                if position.y < *top {
                    // The marks are asked before the column they sit inside,
                    // the same order the tab strip is asked before the border
                    // it sits on: a press on a mark is the `s`/`f` binding,
                    // and a press anywhere else in the heading is still the
                    // `h`/`l` one
                    // (docs/superpowers/specs/2026-08-31-grid-sort-filter-design.md §5).
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
                    // A blank between two groups, a section heading, or a line
                    // past the end of a body shorter than its panel.
                    None => chrome,
                }
            }
            None => chrome,
        }
    }
}

impl RowStrip {
    /// `None` when the body drew nothing a press can land on: a blank panel, a
    /// notice, or the Queries editor.
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

    /// A bordered panel at `area` drawing `len` rows from `first`, with
    /// `header` rows of its own chrome above the first one.
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

    /// A bordered panel at `area` whose body wraps its rows over `rows` lines,
    /// drawn from line `first`.
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

    /// A bordered panel at `area` whose body no press can address.
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

    /// `panel` as a grid: `header` above the rows, and `widths` columns laid
    /// out from `x` with one spacing column between them.
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

    /// Tables down the left, Contents down the right, in a 40×12 frame whose
    /// bottom row belongs to the hint bar.
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

    /// Contents at 24 columns from x=16: the bar and gutter take x=17, the
    /// spacing x=18, then two four-wide columns.
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

    /// [`grid_map`] with the two marks of its first column laid over the
    /// last two cells of that heading, as [`crate::body::mark_bands`] places
    /// them.
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

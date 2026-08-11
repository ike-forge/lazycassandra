//! Resolving a pointer position against the frame it happened in.
//!
//! Assembled from the values that did the drawing — the `Rect` a panel was
//! given and the `Extent` its body returned — rather than computed a second
//! time. A second description of the layout drifts, and drift here is silent:
//! presses landing on the wrong row
//! (docs/superpowers/specs/2026-08-09-mouse-input-design.md §4).

use lc_domain::pointer::Click;
use lc_domain::state::Panel;
use ratatui::layout::{Position, Rect};

use crate::body::{Body, Unit};

/// Where each panel's content ended up in the frame just drawn.
///
/// The default is a frame with nothing in it, which nothing can be clicked in
/// — what a frame below the minimum size reports.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct FrameMap {
    /// In [`Panel::ALL`] order.
    panels: [PanelHit; 4],
}

/// One panel, as a press sees it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct PanelHit {
    pub(crate) panel: Panel,
    /// The whole block, borders included.
    pub(crate) area: Rect,
    /// The body, inside the borders.
    pub(crate) inner: Rect,
    pub(crate) rows: Option<RowStrip>,
}

/// The band of a body that drew rows, and which row sat at its top.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct RowStrip {
    /// Screen row of the first row drawn — below whatever chrome the body
    /// spends above it, such as a grid's header.
    pub(crate) top: u16,
    /// The row index drawn at [`RowStrip::top`].
    pub(crate) first: usize,
    /// How many rows exist, which is what makes the blank space below a short
    /// list chrome rather than a row.
    pub(crate) len: usize,
}

impl FrameMap {
    pub(crate) const fn new(panels: [PanelHit; 4]) -> Self {
        Self { panels }
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
    pub(crate) fn new(panel: Panel, area: Rect, inner: Rect, body: Option<&Body>) -> Self {
        Self {
            panel,
            area,
            inner,
            rows: RowStrip::of(inner, body),
        }
    }

    fn click_at(&self, position: Position) -> Click {
        let chrome = Click::Panel(self.panel);

        if !self.inner.contains(position) {
            return chrome;
        }

        let Some(strip) = self.rows else {
            return chrome;
        };

        if position.y < strip.top {
            return chrome;
        }

        let row = strip.first + usize::from(position.y - strip.top);

        if row >= strip.len {
            return chrome;
        }

        Click::Row {
            panel: self.panel,
            row,
        }
    }
}

impl RowStrip {
    /// `None` when the body drew nothing a press can land on: a blank panel, a
    /// notice, the Ddl body's wrapped lines, or the Queries editor.
    fn of(inner: Rect, body: Option<&Body>) -> Option<Self> {
        let body = body?;
        let Unit::Rows { header } = body.unit else {
            return None;
        };

        Some(Self {
            top: inner.y.saturating_add(header),
            first: body.extent.position,
            len: body.extent.total,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::layout::Margin;

    /// A bordered panel at `area` drawing `len` rows from `first`, with
    /// `header` rows of its own chrome above the first one.
    fn listed(panel: Panel, area: Rect, header: u16, first: usize, len: usize) -> PanelHit {
        let inner = area.inner(Margin {
            horizontal: 1,
            vertical: 1,
        });

        PanelHit {
            panel,
            area,
            inner,
            rows: Some(RowStrip {
                top: inner.y + header,
                first,
                len,
            }),
        }
    }

    /// A bordered panel at `area` whose body no press can address.
    fn plain(panel: Panel, area: Rect) -> PanelHit {
        PanelHit {
            panel,
            area,
            inner: area.inner(Margin {
                horizontal: 1,
                vertical: 1,
            }),
            rows: None,
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
                row: 0
            })
        );
        assert_eq!(
            map.hit(2, 3),
            Some(Click::Row {
                panel: Panel::Tables,
                row: 2
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
                row: 15
            }),
            "the offset is what the row index counts from"
        );
    }

    #[test]
    fn a_header_row_is_chrome_rather_than_the_first_row() {
        let map = map(listed(Panel::Tables, Rect::new(0, 0, 16, 7), 1, 0, 3));

        assert_eq!(map.hit(2, 1), Some(Click::Panel(Panel::Tables)));
        assert_eq!(
            map.hit(2, 2),
            Some(Click::Row {
                panel: Panel::Tables,
                row: 0
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
        assert_eq!(map.hit(3, 0), Some(Click::Panel(Panel::Tables)), "title");
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
    fn an_empty_map_is_not_clickable() {
        assert_eq!(FrameMap::default().hit(0, 0), None);
        assert_eq!(FrameMap::default().hit(20, 5), None);
    }
}

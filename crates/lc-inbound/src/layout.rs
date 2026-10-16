//! Panel geometry, from docs/ui-draft.png, except that Ddl takes 60% of the
//! sidebar for its tabs. Queries opens to [`QUERIES_FOCUS_PERCENT`] when
//! focused and otherwise hugs its buffer; Contents takes the rest.

use lc_domain::panel::Panel;
use lc_domain::view::{PanelBody, ViewModel};
use ratatui::layout::Rect;

const SIDEBAR_PERCENT: u16 = 25;
const DDL_PERCENT: u16 = 60;
const QUERIES_FOCUS_PERCENT: u16 = 60;
/// Past this the editor scrolls rather than eat the results.
const QUERIES_HUG_MAX_LINES: u16 = 10;
const QUERIES_CLOSED_H: u16 = 2;

/// Below this a block shows nothing.
const PANEL_MIN_H: u16 = 3;
const SIDEBAR_MIN_W: u16 = 16;
/// What the main column must keep before the sidebar may exist.
const MAIN_RESERVE_W: u16 = 24;
const BAR_H: u16 = 1;

/// Both columns and both rows on their floors, plus the hint bar.
pub(crate) const MIN_W: u16 = SIDEBAR_MIN_W + MAIN_RESERVE_W;
pub(crate) const MIN_H: u16 = 2 * PANEL_MIN_H + BAR_H;

/// An enum, so no path draws panels into a frame too small for them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shell {
    Panels(PanelRects),
    TooSmall(Rect),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PanelRects {
    tables: Rect,
    ddl: Rect,
    contents: Rect,
    queries: Rect,
    bar: Rect,
}

impl PanelRects {
    /// Always clears the panel's floor.
    #[must_use]
    pub const fn get(&self, panel: Panel) -> Rect {
        match panel {
            Panel::Tables => self.tables,
            Panel::Ddl => self.ddl,
            Panel::Contents => self.contents,
            Panel::Queries => self.queries,
        }
    }

    /// Produced with the panels, so nobody draws the bar over one of their
    /// rows.
    #[must_use]
    pub const fn bar(&self) -> Rect {
        self.bar
    }
}

/// The one panel whose height depends on state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QueriesHeight {
    Expanded,
    Hug(u16),
    Closed,
}

/// Buffer lines: the editor does not wrap, so no width is needed.
#[must_use]
pub fn queries_height(view: &ViewModel) -> QueriesHeight {
    if view.focus == Panel::Queries {
        return QueriesHeight::Expanded;
    }

    let lines = view
        .panels
        .iter()
        .find(|panel| panel.panel == Panel::Queries)
        .and_then(|panel| match &panel.body {
            PanelBody::Editor { lines, .. } => Some(lines.as_slice()),
            _ => None,
        })
        .unwrap_or_default();

    if lines.iter().all(Vec::is_empty) {
        return QueriesHeight::Closed;
    }

    QueriesHeight::Hug(u16::try_from(lines.len()).unwrap_or(u16::MAX))
}

#[must_use]
pub fn layout(area: Rect, queries: QueriesHeight) -> Shell {
    if area.width < MIN_W || area.height < MIN_H {
        return Shell::TooSmall(area);
    }

    let panels_h = area.height - BAR_H;
    let (main_w, side_w) = split(area.width, SIDEBAR_PERCENT, MAIN_RESERVE_W, SIDEBAR_MIN_W);
    let (tables_h, ddl_h) = split(panels_h, DDL_PERCENT, PANEL_MIN_H, PANEL_MIN_H);
    let queries_h = queries_rows(queries, panels_h);
    let contents_h = panels_h - queries_h;

    let main_x = area.x + side_w;

    Shell::Panels(PanelRects {
        tables: Rect::new(area.x, area.y, side_w, tables_h),
        ddl: Rect::new(area.x, area.y + tables_h, side_w, ddl_h),
        contents: Rect::new(main_x, area.y, main_w, contents_h),
        queries: Rect::new(main_x, area.y + contents_h, main_w, queries_h),
        bar: Rect::new(area.x, area.y + panels_h, area.width, BAR_H),
    })
}

/// Never so much that Contents drops below its floor.
fn queries_rows(queries: QueriesHeight, panels_h: u16) -> u16 {
    let want = match queries {
        QueriesHeight::Expanded => percent(panels_h, QUERIES_FOCUS_PERCENT).max(PANEL_MIN_H),
        QueriesHeight::Hug(lines) => lines.min(QUERIES_HUG_MAX_LINES) + QUERIES_CLOSED_H,
        QueriesHeight::Closed => QUERIES_CLOSED_H,
    };

    want.min(panels_h - PANEL_MIN_H)
}

/// The minor part is lifted to its floor, or dropped to zero to keep the major
/// above its own.
fn split(total: u16, minor_percent: u16, major_floor: u16, minor_floor: u16) -> (u16, u16) {
    let share = percent(total, minor_percent);
    let cap = total.saturating_sub(major_floor);
    // `clamp` panics on crossed bounds; this branch prevents them.
    let minor = if cap < minor_floor {
        0
    } else {
        share.clamp(minor_floor, cap)
    };

    (total - minor, minor)
}

fn percent(total: u16, percent: u16) -> u16 {
    // The `u16` product can overflow at sizes the type admits.
    let share = u32::from(total) * u32::from(percent) / 100;
    u16::try_from(share).unwrap_or(u16::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;
    use lc_domain::model::Key;
    use lc_domain::{State, view_model};

    fn rects(width: u16, height: u16) -> PanelRects {
        panels(Rect::new(0, 0, width, height))
    }

    fn panels(area: Rect) -> PanelRects {
        sized(area, QueriesHeight::Closed)
    }

    fn sized(area: Rect, queries: QueriesHeight) -> PanelRects {
        match layout(area, queries) {
            Shell::Panels(rects) => rects,
            Shell::TooSmall(area) => panic!("{area:?} is below {MIN_W}x{MIN_H}"),
        }
    }

    fn queries(width: u16, height: u16, want: QueriesHeight) -> Rect {
        sized(Rect::new(0, 0, width, height), want).get(Panel::Queries)
    }

    #[test]
    fn the_draft_proportions_hold_when_there_is_room() {
        let r = rects(100, 100);

        assert_eq!(r.get(Panel::Tables), Rect::new(0, 0, 25, 40));
        assert_eq!(r.get(Panel::Ddl), Rect::new(0, 40, 25, 59));
        assert_eq!(r.get(Panel::Contents), Rect::new(25, 0, 75, 97));
        assert_eq!(r.get(Panel::Queries), Rect::new(25, 97, 75, 2));
    }

    #[test]
    fn the_layout_is_offset_by_the_area_origin() {
        let r = panels(Rect::new(7, 3, 100, 100));

        assert_eq!(r.get(Panel::Tables), Rect::new(7, 3, 25, 40));
        assert_eq!(r.get(Panel::Queries), Rect::new(32, 100, 75, 2));
    }

    #[test]
    fn a_closed_queries_panel_is_its_borders_and_contents_takes_the_rest() {
        let r = sized(Rect::new(0, 0, 80, 24), QueriesHeight::Closed);

        assert_eq!(r.get(Panel::Contents), Rect::new(20, 0, 60, 21));
        assert_eq!(r.get(Panel::Queries), Rect::new(20, 21, 60, 2));
    }

    /// Returning `Closed` for every unfocused panel would otherwise pass.
    #[test]
    fn an_unfocused_queries_panel_hugs_the_buffer_it_holds() {
        let mut state = State::default();
        state.queries.start_editing();
        for key in [
            Key::Char('a'),
            Key::Enter,
            Key::Char('b'),
            Key::Enter,
            Key::Char('c'),
        ] {
            state.queries.edit(key, 1);
        }
        state.focus_on(Panel::Contents);

        let view = view_model(&state);

        assert_eq!(queries_height(&view), QueriesHeight::Hug(3));
        assert_eq!(
            queries(80, 24, queries_height(&view)).height,
            5,
            "three buffer lines and the two borders around them"
        );
    }

    #[test]
    fn a_hugging_queries_panel_wraps_its_lines_exactly() {
        // Four lines and two borders.
        assert_eq!(queries(80, 24, QueriesHeight::Hug(4)).height, 6);
        assert_eq!(queries(80, 24, QueriesHeight::Hug(1)).height, 3);
    }

    #[test]
    fn a_hugging_queries_panel_stops_growing_at_its_line_cap() {
        assert_eq!(
            queries(80, 60, QueriesHeight::Hug(40)).height,
            QUERIES_HUG_MAX_LINES + QUERIES_CLOSED_H,
            "a long buffer scrolls rather than take the frame"
        );
    }

    #[test]
    fn a_focused_queries_panel_takes_its_share_whatever_the_buffer_holds() {
        // 60% of 23 rows is 13.
        assert_eq!(queries(80, 24, QueriesHeight::Expanded).height, 13);
        assert_eq!(queries(100, 100, QueriesHeight::Expanded).height, 59);
    }

    #[test]
    fn queries_never_pushes_contents_below_its_floor() {
        for want in [
            QueriesHeight::Expanded,
            QueriesHeight::Hug(40),
            QueriesHeight::Hug(4),
            QueriesHeight::Closed,
        ] {
            let r = sized(Rect::new(0, 0, MIN_W, MIN_H), want);

            assert!(
                r.get(Panel::Contents).height >= PANEL_MIN_H,
                "{want:?} left Contents {} rows",
                r.get(Panel::Contents).height
            );
        }
    }

    #[test]
    fn the_bar_takes_the_bottom_row_and_the_panels_stop_above_it() {
        let r = rects(80, 24);

        assert_eq!(r.bar(), Rect::new(0, 23, 80, 1));
        for panel in Panel::ALL {
            assert!(
                !r.get(panel).intersects(r.bar()),
                "{panel:?} overlaps the bar"
            );
        }
    }

    #[test]
    fn the_bar_is_offset_by_the_area_origin_like_everything_else() {
        let r = panels(Rect::new(7, 3, 100, 100));

        assert_eq!(r.bar(), Rect::new(7, 102, 100, 1));
    }

    #[test]
    fn the_sidebar_is_lifted_to_its_floor_on_a_narrow_terminal() {
        // 25% of 40 is 10.
        let r = rects(40, 24);

        assert_eq!(r.get(Panel::Tables).width, 16);
        assert_eq!(r.get(Panel::Contents).width, 24);
    }

    #[test]
    fn both_floors_still_fit_at_seven_rows() {
        let r = sized(Rect::new(0, 0, 80, 7), QueriesHeight::Expanded);

        assert_eq!(r.get(Panel::Contents).height, 3);
        assert_eq!(r.get(Panel::Queries).height, 3);
    }

    #[test]
    fn split_collapses_the_minor_part_rather_than_shrink_it_below_its_floor() {
        // Unreachable through `layout`, but `split` stays total.
        assert_eq!(
            split(4, DDL_PERCENT, PANEL_MIN_H, PANEL_MIN_H),
            (4, 0),
            "a one-row bordered panel looks present and shows nothing"
        );
        assert_eq!(
            split(20, SIDEBAR_PERCENT, MAIN_RESERVE_W, SIDEBAR_MIN_W),
            (20, 0),
            "the sidebar goes before the main column does"
        );
    }

    #[test]
    fn split_lifts_the_minor_part_to_its_floor() {
        assert_eq!(split(6, DDL_PERCENT, PANEL_MIN_H, PANEL_MIN_H), (3, 3));
    }

    #[test]
    fn the_panels_tile_the_area_exactly() {
        let sizes = [
            (MIN_W, MIN_H),
            (40, 12),
            (40, 24),
            (80, 24),
            (100, 100),
            (u16::MAX, u16::MAX),
        ];

        for (width, height) in sizes {
            for want in [
                QueriesHeight::Expanded,
                QueriesHeight::Hug(40),
                QueriesHeight::Hug(2),
                QueriesHeight::Closed,
            ] {
                let area = Rect::new(0, 0, width, height);
                let r = sized(area, want);

                let regions: Vec<Rect> = Panel::ALL
                    .iter()
                    .map(|&panel| r.get(panel))
                    .chain(std::iter::once(r.bar()))
                    .collect();

                let covered: u32 = regions.iter().copied().map(Rect::area).sum();
                assert_eq!(
                    covered,
                    area.area(),
                    "the panels and the bar must cover {width}x{height} with no gap"
                );

                for (index, &first) in regions.iter().enumerate() {
                    for &second in &regions[index + 1..] {
                        assert!(
                            first.is_empty() || second.is_empty() || !first.intersects(second),
                            "two regions overlap at {width}x{height}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn a_frame_below_the_minimum_gets_no_panels() {
        for (width, height) in [(39, 6), (40, 6), (40, 5), (39, 5), (1, 1), (0, 0)] {
            let area = Rect::new(0, 0, width, height);

            assert_eq!(
                layout(area, QueriesHeight::Closed),
                Shell::TooSmall(area),
                "{width}x{height} is below {MIN_W}x{MIN_H}"
            );
        }
    }

    #[test]
    fn the_minimum_grew_by_the_bar_it_now_has_to_hold() {
        assert_eq!((MIN_W, MIN_H), (40, 7));

        let r = sized(Rect::new(0, 0, MIN_W, MIN_H), QueriesHeight::Expanded);
        for panel in Panel::ALL {
            assert!(
                !r.get(panel).is_empty(),
                "{panel:?} is empty at the minimum"
            );
        }
        assert_eq!(r.bar().height, 1);
    }
}

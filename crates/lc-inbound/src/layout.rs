//! Panel geometry.
//!
//! Proportions come from docs/ui-draft.png; the floors and the collapse rule
//! from docs/superpowers/specs/2026-08-08-four-panel-layout-design.md §3.

use lc_domain::state::Panel;
use ratatui::layout::Rect;

const SIDEBAR_PERCENT: u16 = 25;
const DDL_PERCENT: u16 = 40;
const QUERIES_PERCENT: u16 = 10;

/// Two borders, the blank row every panel opens and closes with, and one line
/// of content. Below this a block shows nothing.
const PANEL_MIN_H: u16 = 5;
const SIDEBAR_MIN_W: u16 = 16;
/// `split`'s `major_floor` for the root split: how much the main column must
/// be able to keep before the sidebar is allowed to exist at all.
const MAIN_RESERVE_W: u16 = 24;
/// The hint bar's row, off the bottom of the frame.
const BAR_H: u16 = 1;

/// The smallest frame the shell is drawn into: both columns on their width
/// floors, one panel above another on its height floor in each, and the hint
/// bar's row.
pub(crate) const MIN_W: u16 = SIDEBAR_MIN_W + MAIN_RESERVE_W;
pub(crate) const MIN_H: u16 = 2 * PANEL_MIN_H + BAR_H;

/// What a frame is big enough to be.
///
/// An enum rather than a `PanelRects` plus a separate `fits` predicate, so
/// there is no path on which a caller draws a panel into a frame too small to
/// hold one (conventions.md §1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shell {
    Panels(PanelRects),
    /// The frame, which fell short of `MIN_W` × `MIN_H`.
    TooSmall(Rect),
}

/// Where each panel is drawn this frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PanelRects {
    tables: Rect,
    ddl: Rect,
    contents: Rect,
    queries: Rect,
    bar: Rect,
}

impl PanelRects {
    /// The area assigned to `panel`, which always clears that panel's floor.
    #[must_use]
    pub const fn get(&self, panel: Panel) -> Rect {
        match panel {
            Panel::Tables => self.tables,
            Panel::Ddl => self.ddl,
            Panel::Contents => self.contents,
            Panel::Queries => self.queries,
        }
    }

    /// The bottom row, which belongs to the hint bar.
    ///
    /// Produced with the panels rather than derived by the caller, so no
    /// caller can lay the panels out against the whole frame and then draw
    /// the bar over one of their rows.
    #[must_use]
    pub const fn bar(&self) -> Rect {
        self.bar
    }
}

/// Divide `area` into the four panels of docs/ui-draft.png, or refuse it.
#[must_use]
pub fn layout(area: Rect) -> Shell {
    if area.width < MIN_W || area.height < MIN_H {
        return Shell::TooSmall(area);
    }

    let panels_h = area.height - BAR_H;
    let (main_w, side_w) = split(area.width, SIDEBAR_PERCENT, MAIN_RESERVE_W, SIDEBAR_MIN_W);
    let (tables_h, ddl_h) = split(panels_h, DDL_PERCENT, PANEL_MIN_H, PANEL_MIN_H);
    let (contents_h, queries_h) = split(panels_h, QUERIES_PERCENT, PANEL_MIN_H, PANEL_MIN_H);

    let main_x = area.x + side_w;

    Shell::Panels(PanelRects {
        tables: Rect::new(area.x, area.y, side_w, tables_h),
        ddl: Rect::new(area.x, area.y + tables_h, side_w, ddl_h),
        contents: Rect::new(main_x, area.y, main_w, contents_h),
        queries: Rect::new(main_x, area.y + contents_h, main_w, queries_h),
        bar: Rect::new(area.x, area.y + panels_h, area.width, BAR_H),
    })
}

/// Split `total` into a major and a minor part, in that order.
///
/// The minor part takes `minor_percent`, lifted to `minor_floor` when the
/// percentage falls short of it, and dropped to zero when honouring it would
/// push the major part below `major_floor`.
fn split(total: u16, minor_percent: u16, major_floor: u16, minor_floor: u16) -> (u16, u16) {
    // u16 arithmetic overflows here at terminal widths that cannot exist but
    // that the parameter type admits.
    let share = u32::from(total) * u32::from(minor_percent) / 100;
    let share = u16::try_from(share).unwrap_or(u16::MAX);

    let cap = total.saturating_sub(major_floor);
    // Not defensive: `clamp` panics on crossed bounds, and this branch is what
    // guarantees `minor_floor <= cap` below.
    let minor = if cap < minor_floor {
        0
    } else {
        share.clamp(minor_floor, cap)
    };

    (total - minor, minor)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rects(width: u16, height: u16) -> PanelRects {
        panels(Rect::new(0, 0, width, height))
    }

    fn panels(area: Rect) -> PanelRects {
        match layout(area) {
            Shell::Panels(rects) => rects,
            Shell::TooSmall(area) => panic!("{area:?} is below {MIN_W}x{MIN_H}"),
        }
    }

    #[test]
    fn the_draft_proportions_hold_when_there_is_room() {
        let r = rects(100, 100);

        assert_eq!(r.get(Panel::Tables), Rect::new(0, 0, 25, 60));
        assert_eq!(r.get(Panel::Ddl), Rect::new(0, 60, 25, 39));
        assert_eq!(r.get(Panel::Contents), Rect::new(25, 0, 75, 90));
        assert_eq!(r.get(Panel::Queries), Rect::new(25, 90, 75, 9));
    }

    #[test]
    fn the_layout_is_offset_by_the_area_origin() {
        let r = panels(Rect::new(7, 3, 100, 100));

        assert_eq!(r.get(Panel::Tables), Rect::new(7, 3, 25, 60));
        assert_eq!(r.get(Panel::Queries), Rect::new(32, 93, 75, 9));
    }

    #[test]
    fn queries_is_lifted_to_its_floor_on_a_short_terminal() {
        // 10% of the 23 rows left after the bar is 2: two borders and no
        // content.
        let r = rects(80, 24);

        assert_eq!(r.get(Panel::Contents), Rect::new(20, 0, 60, 18));
        assert_eq!(r.get(Panel::Queries), Rect::new(20, 18, 60, 5));
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
        // 25% of 40 columns is 10.
        let r = rects(40, 24);

        assert_eq!(r.get(Panel::Tables).width, 16);
        assert_eq!(r.get(Panel::Contents).width, 24);
    }

    #[test]
    fn both_floors_still_fit_at_eleven_rows() {
        let r = rects(80, 11);

        assert_eq!(r.get(Panel::Contents).height, 5);
        assert_eq!(r.get(Panel::Queries).height, 5);
    }

    #[test]
    fn split_collapses_the_minor_part_rather_than_shrink_it_below_its_floor() {
        // Unreachable through `layout`, which now refuses these sizes. `split`
        // stays total over them, so it is tested where it is total
        // (minimum terminal size design §5).
        assert_eq!(
            split(5, QUERIES_PERCENT, PANEL_MIN_H, PANEL_MIN_H),
            (5, 0),
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
        assert_eq!(
            split(24, QUERIES_PERCENT, PANEL_MIN_H, PANEL_MIN_H),
            (19, 5)
        );
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
            let area = Rect::new(0, 0, width, height);
            let r = panels(area);

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

    #[test]
    fn a_frame_below_the_minimum_gets_no_panels() {
        for (width, height) in [(39, 6), (40, 6), (40, 5), (39, 5), (1, 1), (0, 0)] {
            let area = Rect::new(0, 0, width, height);

            assert_eq!(
                layout(area),
                Shell::TooSmall(area),
                "{width}x{height} is below {MIN_W}x{MIN_H}"
            );
        }
    }

    #[test]
    fn the_minimum_grew_by_the_bar_it_now_has_to_hold() {
        assert_eq!((MIN_W, MIN_H), (40, 11));

        let r = rects(MIN_W, MIN_H);
        for panel in Panel::ALL {
            assert!(
                !r.get(panel).is_empty(),
                "{panel:?} is empty at the minimum"
            );
        }
        assert_eq!(r.bar().height, 1);
    }
}

//! Panel geometry.
//!
//! Proportions come from doc/ui-draft.png; the floors and the collapse rule
//! from doc/specs/2026-08-08-four-panel-layout-design.md §3.

use lc_domain::state::Panel;
use ratatui::layout::Rect;

const SIDEBAR_PERCENT: u16 = 25;
const DDL_PERCENT: u16 = 40;
const QUERIES_PERCENT: u16 = 10;

/// Two borders and one line of content. Below this a block shows nothing.
const PANEL_MIN_H: u16 = 3;
const SIDEBAR_MIN_W: u16 = 16;
/// `split`'s `major_floor` for the root split: how much the main column must
/// be able to keep before the sidebar is allowed to exist at all.
const MAIN_RESERVE_W: u16 = 24;

/// The smallest frame the shell is drawn into: both columns on their width
/// floors, and one panel above another on its height floor in each.
pub(crate) const MIN_W: u16 = SIDEBAR_MIN_W + MAIN_RESERVE_W;
pub(crate) const MIN_H: u16 = 2 * PANEL_MIN_H;

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
}

impl PanelRects {
    /// The area assigned to `panel`, which may be empty when the terminal is
    /// too small to hold it.
    #[must_use]
    pub const fn get(&self, panel: Panel) -> Rect {
        match panel {
            Panel::Tables => self.tables,
            Panel::Ddl => self.ddl,
            Panel::Contents => self.contents,
            Panel::Queries => self.queries,
        }
    }
}

/// Divide `area` into the four panels of doc/ui-draft.png, or refuse it.
#[must_use]
pub fn layout(area: Rect) -> Shell {
    if area.width < MIN_W || area.height < MIN_H {
        return Shell::TooSmall(area);
    }

    let (main_w, side_w) = split(area.width, SIDEBAR_PERCENT, MAIN_RESERVE_W, SIDEBAR_MIN_W);
    let (tables_h, ddl_h) = split(area.height, DDL_PERCENT, PANEL_MIN_H, PANEL_MIN_H);
    let (contents_h, queries_h) = split(area.height, QUERIES_PERCENT, PANEL_MIN_H, PANEL_MIN_H);

    let main_x = area.x + side_w;

    Shell::Panels(PanelRects {
        tables: Rect::new(area.x, area.y, side_w, tables_h),
        ddl: Rect::new(area.x, area.y + tables_h, side_w, ddl_h),
        contents: Rect::new(main_x, area.y, main_w, contents_h),
        queries: Rect::new(main_x, area.y + contents_h, main_w, queries_h),
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
        assert_eq!(r.get(Panel::Ddl), Rect::new(0, 60, 25, 40));
        assert_eq!(r.get(Panel::Contents), Rect::new(25, 0, 75, 90));
        assert_eq!(r.get(Panel::Queries), Rect::new(25, 90, 75, 10));
    }

    #[test]
    fn the_layout_is_offset_by_the_area_origin() {
        let r = panels(Rect::new(7, 3, 100, 100));

        assert_eq!(r.get(Panel::Tables), Rect::new(7, 3, 25, 60));
        assert_eq!(r.get(Panel::Queries), Rect::new(32, 93, 75, 10));
    }

    #[test]
    fn queries_is_lifted_to_its_floor_on_a_short_terminal() {
        // 10% of 24 rows is 2: two borders and no content.
        let r = rects(80, 24);

        assert_eq!(r.get(Panel::Contents), Rect::new(20, 0, 60, 21));
        assert_eq!(r.get(Panel::Queries), Rect::new(20, 21, 60, 3));
    }

    #[test]
    fn the_sidebar_is_lifted_to_its_floor_on_a_narrow_terminal() {
        // 25% of 40 columns is 10.
        let r = rects(40, 24);

        assert_eq!(r.get(Panel::Tables).width, 16);
        assert_eq!(r.get(Panel::Contents).width, 24);
    }

    #[test]
    fn both_floors_still_fit_at_six_rows() {
        let r = rects(80, 6);

        assert_eq!(r.get(Panel::Contents).height, 3);
        assert_eq!(r.get(Panel::Queries).height, 3);
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
            (21, 3)
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

            let covered: u32 = Panel::ALL.iter().map(|&panel| r.get(panel).area()).sum();
            assert_eq!(
                covered,
                area.area(),
                "panels must cover {width}x{height} with no gap"
            );

            for (index, &first) in Panel::ALL.iter().enumerate() {
                for &second in &Panel::ALL[index + 1..] {
                    let (a, b) = (r.get(first), r.get(second));
                    assert!(
                        a.is_empty() || b.is_empty() || !a.intersects(b),
                        "{first:?} and {second:?} overlap at {width}x{height}"
                    );
                }
            }
        }
    }

    #[test]
    fn a_frame_below_the_minimum_gets_no_panels() {
        for (width, height) in [(39, 6), (40, 5), (39, 5), (1, 1), (0, 0)] {
            let area = Rect::new(0, 0, width, height);

            assert_eq!(
                layout(area),
                Shell::TooSmall(area),
                "{width}x{height} is below {MIN_W}x{MIN_H}"
            );
        }
    }

    #[test]
    fn the_minimum_is_the_size_at_which_every_panel_still_fits() {
        let r = rects(MIN_W, MIN_H);

        for panel in Panel::ALL {
            assert!(
                !r.get(panel).is_empty(),
                "{panel:?} is empty at the minimum, so the minimum is wrong"
            );
        }
    }

    #[test]
    fn the_minimum_is_derived_from_the_floors_rather_than_chosen() {
        assert_eq!((MIN_W, MIN_H), (40, 6));
    }
}

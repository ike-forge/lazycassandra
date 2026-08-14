//! Drawing a frame.

use lc_domain::ViewModel;
use lc_domain::state::{Geometry, Panel, Span as DdlSpan};
use lc_domain::style::Token;
use lc_domain::view::{OverlayView, PanelTitle, Tab};
use ratatui::Frame;
use ratatui::layout::{Margin, Rect};
use ratatui::style::Style;
use ratatui::symbols;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Scrollbar, ScrollbarOrientation, ScrollbarState};

use crate::body::Extent;
use crate::hit::{Band, FrameMap, PanelHit};
use crate::layout::{Shell, layout};
use crate::theme::Theme;

/// Half a cell, flush with the panel's outer edge: the thumb marks a position
/// without reading as a bar of its own beside the border it sits on.
const THUMB: &str = "▐";

/// What the frame just drawn turned out to be.
///
/// `geometry` travels into the domain as `Msg::Measured`, so it is guarded
/// against closing a redraw–measure–redraw loop (architecture.md §3). `map`
/// re-enters nothing — nobody reads it until a human moves a mouse — so it is
/// free to depend on the offsets `Geometry` may not
/// (docs/superpowers/specs/2026-08-09-mouse-input-design.md §4).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Rendered {
    pub geometry: Geometry,
    pub map: FrameMap,
}

/// Draw one titled block per panel, and report what the frame turned out to
/// be.
///
/// The return value is the domain's only way to learn how tall a panel is and
/// how many lines the Ddl body wrapped to
/// (docs/superpowers/specs/2026-08-09-panel-scrolling-design.md §3).
pub fn render(frame: &mut Frame, view: &ViewModel, theme: Theme) -> Rendered {
    let ground = theme.ground();
    if ground != Style::new() {
        let area = frame.area();
        frame.buffer_mut().set_style(area, ground);
    }

    let command_rows = crate::palette::visible_rows(frame.area(), lc_domain::commands::row_count());
    let rects = match layout(frame.area()) {
        Shell::Panels(rects) => rects,
        Shell::TooSmall(area) => {
            crate::too_small::render(frame, area);
            return Rendered::default();
        }
    };
    let mut heights = [0; 4];
    let mut ddl_lines = 0;
    let mut ddl_selection = DdlSpan::default();
    let mut grid_width = 0;
    let mut hits: [PanelHit; 4] = std::array::from_fn(|_| PanelHit::default());

    for (index, panel) in view.panels.iter().enumerate() {
        let area = rects.get(panel.panel);

        let is_focused_panel = panel.panel == view.focus;
        let (block, border, tabs) = panel_block(panel, area, is_focused_panel, theme);

        let inner = block.inner(area);
        frame.render_widget(block, area);

        // Every body kind, not only a grid: it is the panel's width, which is
        // what the domain needs in order to decide what would fit once a page
        // arrives (docs/superpowers/specs/2026-08-10-table-display-design.md §6).
        if panel.panel == Panel::Contents {
            grid_width = usize::from(inner.width);
        }

        // An overlay owns the frame: nothing beneath it shows a cursor, or it
        // blinks over the modal at a position that means nothing there.
        let focused = panel.panel == view.focus && view.overlay.is_none();

        let body = crate::body::render(frame, inner, &panel.body, focused, theme);

        match &body {
            Some(drawn) => {
                heights[index] = drawn.extent.visible;
                if panel.panel == Panel::Ddl {
                    ddl_lines = drawn.extent.total;
                    ddl_selection = drawn.selection;
                }
                if drawn.extent.overflows() {
                    scrollbar(frame, area, &drawn.extent, border, theme);
                }
                if let Some(columns) = &drawn.columns
                    && columns.overflows()
                {
                    horizontal_scrollbar(frame, area, columns, border, theme);
                }
            }
            // `Blank` and `Notice` do not scroll, so what such a panel can
            // show is simply the room it was given.
            None => heights[index] = usize::from(inner.height),
        }

        hits[index] = PanelHit::new(panel.panel, area, inner, tabs, body.as_ref());
    }

    let bar = rects.bar();
    let (loader, hint_bar) = bar_split(bar);
    frame.render_widget(crate::loader::line(view.activity, theme), loader);
    frame.render_widget(
        crate::hints::line(&view.hints, hint_bar.width, theme),
        hint_bar,
    );

    match &view.overlay {
        Some(OverlayView::Message(message)) => crate::overlay::render(frame, message, theme),
        Some(OverlayView::Commands { rows, selected }) => {
            crate::palette::render(frame, rows, *selected, theme);
        }
        // Drawn on the hint bar row, not here: a prompt is a text field, not
        // a modal over the panels (design §5.2).
        Some(OverlayView::Search { query }) => {
            render_prompt(frame, hint_bar, query, view, theme);
        }
        Some(OverlayView::Themes { labels, selected }) => {
            crate::themes::render(frame, labels, *selected, theme);
        }
        None => {}
    }

    Rendered {
        geometry: Geometry::new(heights, ddl_lines, command_rows)
            .with_ddl_selection(ddl_selection)
            .with_grid_width(grid_width),
        map: FrameMap::new(hits),
    }
}

/// The bottom row, split into the loader's cells and what the hint bar is
/// left with.
///
/// The loader takes its width at every frame size rather than hiding on a
/// narrow one: the bar already knows how to give up hints in value order, and
/// trading a hint the user can read for one they cannot see is not a trade
/// (design §4.4).
fn bar_split(bar: Rect) -> (Rect, Rect) {
    let taken = crate::loader::WIDTH + crate::loader::GAP;

    (
        Rect {
            width: crate::loader::WIDTH,
            ..bar
        },
        Rect {
            x: bar.x + taken,
            width: bar.width.saturating_sub(taken),
            ..bar
        },
    )
}

/// `rect` split at `taken` columns from its left edge, clamped to what `rect`
/// actually has — the shape `bar_split` uses for the loader, reused here for
/// the query so a query wider than the bar cannot push the split past the
/// rect it is dividing.
fn split_left(rect: Rect, taken: u16) -> (Rect, Rect) {
    let taken = taken.min(rect.width);

    (
        Rect {
            width: taken,
            ..rect
        },
        Rect {
            x: rect.x + taken,
            width: rect.width - taken,
            ..rect
        },
    )
}

/// The bordered block a panel is drawn in, the border style the scrollbars
/// that hang off it share, and where each tab of its title landed.
fn panel_block(
    panel: &lc_domain::view::PanelView,
    area: Rect,
    is_focused_panel: bool,
    theme: Theme,
) -> (Block<'_>, Style, Vec<Band<char>>) {
    let border = if is_focused_panel {
        theme.border_style().patch(theme.style(Token::Focused))
    } else {
        theme.border_style()
    };
    let (tabs, form) = drawn_tabs(&panel.title, area.width);
    let strip = strip(&tabs, form);
    // Right-aligned on the top border, and dropped whole when the border
    // cannot hold both: a truncated term is a lie about what is applied
    // (design §5.1). Drawn on every panel that has one, focused or not — the
    // marker is the only place a filter that survives a focus change still
    // says so.
    let marker = panel
        .filter
        .as_ref()
        .map(|term| format!("/{term}"))
        .filter(|marker| marker_fits(area.width, &strip, marker));
    let (lead, trail) = insets(area.width, &strip, marker.as_deref());

    let mut block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(border)
        .title(panel_title(
            &tabs,
            form,
            is_focused_panel,
            theme,
            border,
            lead,
        ));

    if let Some(marker) = marker {
        block = block.title(
            Line::from(vec![
                Span::styled(
                    format!("{TITLE_PAD}{marker}{TITLE_PAD}"),
                    theme.style(Token::Muted),
                ),
                inset(trail, border),
            ])
            .right_aligned(),
        );
    }

    (block, border, tab_bands(&tabs, form, lead, area))
}

/// Where each tab of a title strip landed on the top border.
///
/// Walked in step with [`panel_title`], off the same `tabs`, `form` and `lead`
/// — the strip is measured where it is built, rather than measured a second
/// time from the panel's state, for the reason the row map is
/// (docs/superpowers/specs/2026-08-09-mouse-input-design.md §4).
///
/// Clipped at the right corner: a strip too wide for its border is drawn short
/// by `ratatui`, and a band running past what was drawn would claim border the
/// user can see is blank.
fn tab_bands(tabs: &[&Tab], form: Strip, lead: usize, area: Rect) -> Vec<Band<char>> {
    let limit = area.right().saturating_sub(1);
    let mut start = area
        .x
        .saturating_add(1)
        .saturating_add(columns(lead + TITLE_PAD.chars().count()));

    let mut bands = Vec::new();

    for (index, tab) in tabs.iter().enumerate() {
        if index > 0 {
            start = start.saturating_add(columns(lc_domain::view::TAB_SEPARATOR.chars().count()));
        }

        let width = columns(match form {
            Strip::Short => tab.short().chars().count(),
            Strip::Full | Strip::Active => tab.text().chars().count(),
        });
        let end = start.saturating_add(width).min(limit);

        if let Some(shortcut) = tab.shortcut()
            && start < end
        {
            bands.push(Band {
                start,
                end,
                names: shortcut,
            });
        }

        start = start.saturating_add(width);
    }

    bands
}

/// A count of characters as a count of terminal columns. Every tab name is
/// ASCII, and one wider than a `u16` cannot have been drawn.
fn columns(characters: usize) -> u16 {
    u16::try_from(characters).unwrap_or(u16::MAX)
}

/// The blank column between a title and the border either side of it.
///
/// `ratatui` gives a title none, so a name grows out of the corner it touches.
const TITLE_PAD: &str = " ";

/// The columns of drawn border a title is pushed along by, where the border
/// has them to spare.
///
/// Border glyphs rather than blanks: the gap is meant to read as the panel's
/// frame running on, not as a margin cut out of it
/// (docs/superpowers/specs/2026-08-12-ddl-tabs-design.md §2). It is a ceiling
/// and not a cost, because the names come first — a sidebar on its 16-column
/// floor spends what it has left over on the inset and no more, rather than
/// shortening a tab to buy one.
const TITLE_INSET: usize = 2;

/// The horizontal glyph of the border the panels are drawn with — `ratatui`
/// puts it under a title, and a title that pads with it puts it back.
const BORDER_LINE: char = '─';

/// What each end of a `width`-column border has spare for an inset once the
/// title strip, a marker if one is drawn, their padding and the corners are
/// paid for.
///
/// The left end is served first: a title pushed off its corner while the
/// marker still sat on its own would read as two different alignments on one
/// border.
fn insets(width: u16, title: &str, marker: Option<&str>) -> (usize, usize) {
    let marker_width = marker.map_or(0, |marker| {
        marker.chars().count() + 2 * TITLE_PAD.chars().count()
    });
    let spent = title.chars().count() + 2 * TITLE_PAD.chars().count() + marker_width + 2;
    let spare = usize::from(width).saturating_sub(spent);
    let lead = TITLE_INSET.min(spare);

    (lead, TITLE_INSET.min(spare - lead))
}

/// `columns` of border, in the style the block draws its own with.
fn inset(columns: usize, border: Style) -> Span<'static> {
    Span::styled(BORDER_LINE.to_string().repeat(columns), border)
}

/// The shortcut span keeps its own `Token::Shortcut` style regardless of
/// focus. The active tab takes `Token::Focused`'s colour when its panel is
/// focused, matching the border, and drops one tier to `Token::Muted` when it
/// is not — an unfocused panel's active tab still names what it is showing,
/// but not as loudly as body text. An inactive tab sits a further tier down
/// at `Token::Faint`, whether or not its panel has focus: it names something
/// the panel is not showing.
fn panel_title<'a>(
    tabs: &[&'a Tab],
    form: Strip,
    is_focused_panel: bool,
    theme: Theme,
    border: Style,
    lead: usize,
) -> Line<'a> {
    let shortcut = theme.style(Token::Shortcut);
    let separator = theme.style(Token::Faint);
    let inactive = theme.style(Token::Faint);
    let active = if is_focused_panel {
        theme.style(Token::Focused)
    } else {
        theme.style(Token::Muted)
    };

    let mut spans = vec![inset(lead, border), Span::raw(TITLE_PAD)];

    for (index, tab) in tabs.iter().enumerate() {
        if index > 0 {
            spans.push(Span::styled(lc_domain::view::TAB_SEPARATOR, separator));
        }

        let style = if tab.active { active } else { inactive };
        let (before, mark, after) = match form {
            Strip::Short => tab.split_short(),
            Strip::Full | Strip::Active => tab.split(),
        };

        spans.push(Span::styled(before, style));
        // The active tab's mark spends no colour of its own, so it inherits
        // whatever the border or the focus style put under it; an inactive
        // tab's is faint with the rest of that tab, or the strip would mark a
        // tab the panel is not showing more strongly than the one it is.
        spans.push(Span::styled(mark, style.patch(shortcut)));
        spans.push(Span::styled(after, style));
    }

    spans.push(Span::raw(TITLE_PAD));
    Line::from(spans)
}

/// How much of a panel's title strip its border turned out to hold.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Strip {
    /// Every tab, under its full name.
    Full,
    /// Every tab, under the short names.
    Short,
    /// The tab being drawn, alone and whole.
    Active,
}

/// The tabs a `width`-column border can hold, and the names to draw them
/// under.
///
/// Three rungs, each giving up less than clipping would: a clipped strip says
/// the panel has a tab named `Table col`, which is a name no key reaches
/// (docs/superpowers/specs/2026-08-12-ddl-tabs-design.md §4). Every tab stays
/// visible for as long as it can, because a tab nobody can see is one nobody
/// presses `a` for.
fn drawn_tabs(title: &PanelTitle, width: u16) -> (Vec<&Tab>, Strip) {
    let all: Vec<&Tab> = title.tabs().iter().collect();
    let holds =
        |text: &str| text.chars().count() + 2 * TITLE_PAD.chars().count() + 2 <= usize::from(width);

    for form in [Strip::Full, Strip::Short] {
        if holds(&strip(&all, form)) {
            return (all, form);
        }
    }

    (
        all.into_iter().filter(|tab| tab.active).collect(),
        Strip::Active,
    )
}

/// What `tabs` reads as on the border, separators included.
fn strip(tabs: &[&Tab], form: Strip) -> String {
    tabs.iter()
        .map(|tab| match form {
            Strip::Short => tab.short(),
            Strip::Full | Strip::Active => tab.text(),
        })
        .collect::<Vec<_>>()
        .join(lc_domain::view::TAB_SEPARATOR)
}

/// Both titles, the two corners, and the padding either side of each. The
/// inset is not counted: it is decoration, and a marker names what the panel
/// is actually showing.
fn marker_fits(width: u16, title: &str, marker: &str) -> bool {
    let padding = 4 * TITLE_PAD.chars().count();
    let needed = title.chars().count() + marker.chars().count() + 2 + padding;
    usize::from(width) >= needed
}

/// The query at the left of the bar row, `⏎ apply` / `esc cancel` at the
/// right of what it leaves — composed rather than drawn over the hints, which
/// `render` already put on `hint_bar` before this runs.
fn render_prompt(frame: &mut Frame, hint_bar: Rect, query: &str, view: &ViewModel, theme: Theme) {
    let line = Line::styled(format!(" /{query}"), theme.style(Token::HintAction));
    let taken = u16::try_from(line.width()).unwrap_or(hint_bar.width);
    let (left, right) = split_left(hint_bar, taken);

    frame.render_widget(line, left);
    frame.render_widget(crate::hints::line(&view.hints, right.width, theme), right);
    // `split_left` already clamps `left` to the bar's own bounds; a query
    // wide enough to fill it would otherwise place the cursor one column
    // past the frame's right edge.
    let cursor_x = (left.x + left.width).min(hint_bar.right().saturating_sub(1));
    frame.set_cursor_position((cursor_x, left.y));
}

/// Over the block's right border, so it spends no content width — and only
/// when something is hidden, so its absence says nothing is.
///
/// The track is the border glyph at `Token::Faint`, so it reads as part of the
/// border
/// rather than a second bar beside it; the thumb takes the border's own
/// (possibly focus-patched) style, so the scroll position — not the
/// unscrolled rest of the column — carries the stronger colour
/// (2026-08-11-panel-focus-contrast-design.md). `Scrollbar`'s default set is
/// `DOUBLE_VERTICAL`, whose `║` is what makes an unstyled scrollbar read as a
/// second border beside the first (design §3).
///
/// `ScrollbarState` normalises the thumb against `content_length - 1 +
/// viewport_length`, a model whose `position` is a selection index running over
/// `0..content_length`. Ours is a top-of-viewport offset, which stops at
/// `total - visible` — so that is the content length it is given, and
/// `viewport_content_length` is left alone: it is for a viewport item spanning
/// several track rows, not for this. Handing it `total` instead leaves the
/// thumb short of the bottom when the last row is already on screen.
///
/// Only called when the extent overflows, so the length below is at least one.
///
/// `pub(crate)` for the palette, which scrolls the same way and must not grow
/// a second copy of the arithmetic above.
pub(crate) fn scrollbar(
    frame: &mut Frame,
    area: Rect,
    extent: &Extent,
    border: Style,
    theme: Theme,
) {
    bar(
        frame,
        area,
        extent,
        ScrollbarOrientation::VerticalRight,
        border,
        theme,
    );
}

/// Over the bottom border, mirroring [`scrollbar`] on the right one, and only
/// when a column is off screen — so its absence says nothing is hidden
/// (docs/superpowers/specs/2026-08-10-table-display-design.md §9).
fn horizontal_scrollbar(
    frame: &mut Frame,
    area: Rect,
    extent: &Extent,
    border: Style,
    theme: Theme,
) {
    bar(
        frame,
        area,
        extent,
        ScrollbarOrientation::HorizontalBottom,
        border,
        theme,
    );
}

fn bar(
    frame: &mut Frame,
    area: Rect,
    extent: &Extent,
    orientation: ScrollbarOrientation,
    border: Style,
    theme: Theme,
) {
    // The thumb is parameterised with the track: `▐` is a full-height glyph,
    // which beside a `─` sitting at mid-height reads as a row of vertical bars
    // rather than as a thicker stretch of the same line.
    let (track, thumb, margin) = match orientation {
        ScrollbarOrientation::HorizontalBottom | ScrollbarOrientation::HorizontalTop => (
            symbols::line::HORIZONTAL,
            symbols::line::THICK_HORIZONTAL,
            Margin {
                horizontal: 1,
                vertical: 0,
            },
        ),
        _ => (
            symbols::line::VERTICAL,
            THUMB,
            Margin {
                horizontal: 0,
                vertical: 1,
            },
        ),
    };
    let mut state =
        ScrollbarState::new(extent.total.saturating_sub(extent.visible)).position(extent.position);

    frame.render_stateful_widget(
        // No arrows: they would land on the block's corners.
        Scrollbar::new(orientation)
            .begin_symbol(None)
            .end_symbol(None)
            .track_symbol(Some(track))
            .track_style(theme.style(Token::Faint))
            .thumb_symbol(thumb)
            .thumb_style(border),
        area.inner(margin),
        &mut state,
    );
}

#[cfg(test)]
mod tests {
    use crate::body::Marked;
    use lc_domain::commands::CommandsPanel;
    use lc_domain::contents::{SelectionMode, Subject, TableRef};
    use lc_domain::ddl::DdlTab;
    use lc_domain::model::{
        ClusteringColumn, ClusteringOrder, Column, ConnId, CqlValue, DbError, Keyspace, Page,
        PrimaryKey, QueryId, Replication, Schema, Table, TableOptions,
    };
    use lc_domain::pointer::Click;
    use lc_domain::posture::Posture;
    use lc_domain::search::{Filter, Filtered, Prompt};
    use lc_domain::state::{
        Connection, ConnectionState, Flash, Geometry, Message, Overlay, Panel, Range, SchemaState,
        Selection,
    };
    use lc_domain::style::Severity;
    use lc_domain::view::PanelBody;
    use lc_domain::{Ported, ThemeChoice};
    use lc_domain::{State, view_model};
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::buffer::{Buffer, Cell};
    use ratatui::style::{Color, Modifier};
    use std::collections::BTreeMap;

    use super::*;

    fn draw_focused(width: u16, height: u16, focus: Panel) -> Buffer {
        let state = State {
            focus,
            ..State::default()
        };
        let mut terminal =
            Terminal::new(TestBackend::new(width, height)).expect("TestBackend always initialises");

        terminal
            .draw(|frame| {
                render(frame, &view_model(&state), Theme::default());
            })
            .expect("TestBackend always draws");

        terminal.backend().buffer().clone()
    }

    fn draw(width: u16, height: u16) -> Buffer {
        draw_focused(width, height, Panel::default())
    }

    fn row(buffer: &Buffer, y: u16) -> String {
        (0..buffer.area.width)
            .filter_map(|x| buffer.cell((x, y)))
            .map(Cell::symbol)
            .collect()
    }

    #[test]
    fn a_flavour_paints_the_field_it_is_drawn_on() {
        let mut terminal =
            Terminal::new(TestBackend::new(80, 24)).expect("TestBackend always initialises");
        let state = State::default();

        terminal
            .draw(|frame| {
                render(
                    frame,
                    &view_model(&state),
                    Theme::of(ThemeChoice::Ported(Ported::CatppuccinMocha)),
                );
            })
            .expect("TestBackend always draws");

        let buffer = terminal.backend().buffer();
        let ground = buffer[(0, 0)].bg;

        assert_eq!(
            ground,
            Color::Rgb(0x1e, 0x1e, 0x2e),
            "a flavour owns the whole frame, corners included"
        );
    }

    #[test]
    fn focus_wins_over_a_flavours_own_panel_border() {
        let theme = Theme::of(ThemeChoice::Ported(Ported::CatppuccinMocha));
        let mut terminal =
            Terminal::new(TestBackend::new(40, 12)).expect("TestBackend always initialises");
        let state = State {
            focus: Panel::Tables,
            ..State::default()
        };

        terminal
            .draw(|frame| {
                render(frame, &view_model(&state), theme);
            })
            .expect("TestBackend always draws");

        let buffer = terminal.backend().buffer();
        let corner_fg = buffer[(0, 0)].fg;

        assert_eq!(
            corner_fg,
            theme.palette().focus,
            "focus's colour patches on top of panel_border's, exactly as \
             border_style()'s doc comment claims"
        );
    }

    #[test]
    fn a_flavour_paints_the_field_even_when_the_frame_is_too_small_for_a_panel() {
        // Below layout::MIN_W x MIN_H (40x7), `render` takes the `TooSmall`
        // arm and returns before the panel loop — the path the ground paint
        // must cover too, or a flavour leaves a hole exactly where the notice
        // is drawn.
        let mut terminal =
            Terminal::new(TestBackend::new(30, 6)).expect("TestBackend always initialises");
        let state = State::default();

        terminal
            .draw(|frame| {
                render(
                    frame,
                    &view_model(&state),
                    Theme::of(ThemeChoice::Ported(Ported::CatppuccinMocha)),
                );
            })
            .expect("TestBackend always draws");

        let buffer = terminal.backend().buffer();

        assert!(
            buffer.content().iter().all(|cell| cell.bg != Color::Reset),
            "a cell the flavour missed shows the terminal through it, notice included"
        );
    }

    #[test]
    fn the_default_theme_paints_the_field_too() {
        let mut terminal =
            Terminal::new(TestBackend::new(80, 24)).expect("TestBackend always initialises");
        let state = State::default();

        terminal
            .draw(|frame| {
                render(frame, &view_model(&state), Theme::default());
            })
            .expect("TestBackend always draws");

        assert!(
            terminal
                .backend()
                .buffer()
                .content()
                .iter()
                .all(|cell| cell.bg != Color::Reset),
            "the default theme declares its own base, corners included \
             (docs/superpowers/specs/2026-08-13-colour-palettes-design.md §1)"
        );
    }

    #[test]
    fn the_four_panels_are_drawn_where_the_draft_puts_them() {
        // At 40x12 the sidebar sits on its 16-column floor, Tables takes 5
        // rows to Ddl's 6, and Queries sits on its 3-row floor.
        let buffer = draw(40, 12);

        assert_eq!(row(&buffer, 0), "╭── Tables ────╮╭── Table contents ────╮");
        assert_eq!(row(&buffer, 1), "│Not connected ││Press Enter on a table│");
        assert_eq!(row(&buffer, 4), "╰──────────────╯│                      │");
        // Nothing is selected, so the panel offers no Columns tab and the
        // strip is the one name.
        assert_eq!(row(&buffer, 5), "╭── DDL ───────╮│                      │");
        assert_eq!(row(&buffer, 7), "│              │╰──────────────────────╯");
        assert_eq!(row(&buffer, 8), "│              │╭── Queries ───────────╮");
        assert_eq!(row(&buffer, 10), "╰──────────────╯╰──────────────────────╯");
    }

    #[test]
    fn the_hint_bar_sits_on_the_bottom_row() {
        let buffer = draw(80, 24);

        assert_eq!(
            row(&buffer, 23).trim_end(),
            "■⬝⬝⬝⬝⬝⬝⬝  ⏎ open | / search | v select | tab panel | q quit",
            "the loader takes the leading cells and the hints follow it"
        );
        assert!(
            !row(&buffer, 22).contains("q quit"),
            "the bar owns one row and no more"
        );
    }

    #[test]
    fn the_loader_rests_until_something_is_outstanding() {
        // `State::default` is disconnected with nothing in flight.
        let resting = draw(80, 24);
        let mut state = State {
            connection: ConnectionState::Connecting(ConnId::next()),
            ..State::default()
        };
        state.loader.advance(true);

        let working = draw_state(80, 24, &state);

        assert_ne!(
            row(&resting, 23),
            row(&working, 23),
            "a tick during a connection attempt has to show on the bar"
        );
    }

    #[test]
    fn the_bar_describes_the_open_palette_rather_than_the_panel_under_it() {
        // The sample design §7 shows, checked against what the renderer
        // actually draws rather than being correct by construction.
        //
        // Sliced past the loader, which owns the head of every bar and is not
        // what this test is about (visual polish design §4.4).
        let buffer = with_palette(80, 24);
        let taken = usize::from(crate::loader::WIDTH + crate::loader::GAP);
        let drawn = row(&buffer, 23);
        let hints: String = drawn.chars().skip(taken).collect();

        assert_eq!(hints.trim_end(), " ?/esc close | q quit");
    }

    #[test]
    fn the_bar_says_what_the_focused_panel_does() {
        let bars: Vec<String> = Panel::ALL
            .iter()
            .map(|&panel| row(&draw_focused(80, 24, panel), 23).trim_end().to_owned())
            .collect();

        assert!(bars[0].contains("⏎ open"), "Tables opens: {:?}", bars[0]);
        assert!(bars[1].contains("/ search"), "Ddl filters: {:?}", bars[1]);
        assert!(
            bars[2].contains("/ search"),
            "Contents filters: {:?}",
            bars[2]
        );
        assert!(
            bars[3].contains("⏎ newline"),
            "Queries takes text: {:?}",
            bars[3]
        );
    }

    #[test]
    fn a_narrow_frame_drops_hints_rather_than_the_bar() {
        let buffer = draw(40, 12);

        assert_eq!(
            row(&buffer, 11).trim_end(),
            "■⬝⬝⬝⬝⬝⬝⬝  ⏎ open | tab panel | q quit",
            "the cells the loader took come out of the hints, lowest value first"
        );
    }

    #[test]
    fn only_the_focused_panel_is_bold() {
        let buffer = draw(40, 12);
        let bold = |x, y| {
            buffer
                .cell((x, y))
                .expect("corner is in bounds")
                .modifier
                .contains(Modifier::BOLD)
        };

        // Top-left corner of each panel; Tables is focused by default.
        assert!(bold(0, 0), "Tables is focused");
        assert!(!bold(16, 0), "Contents is not");
        assert!(!bold(0, 5), "Ddl is not");
        assert!(!bold(16, 8), "Queries is not");
    }

    #[test]
    fn only_the_focused_panel_wears_the_focus_colour() {
        let buffer = draw(40, 12);
        let focus_fg = |x, y| buffer.cell((x, y)).expect("corner is in bounds").fg;
        let focus = Theme::default().palette().focus;

        // Top-left corner of each panel; Tables is focused by default.
        assert_eq!(focus_fg(0, 0), focus, "Tables' border is focus-coloured");
        assert_ne!(focus_fg(16, 0), focus, "Contents' is not");
        assert_ne!(focus_fg(0, 7), focus, "Ddl's is not");
        assert_ne!(focus_fg(16, 8), focus, "Queries' is not");
    }

    #[test]
    fn the_focused_panels_plain_title_text_wears_the_focus_colour() {
        // Row 0 is "╭Tables────────╮╭Table contents────────╮": Tables'
        // shortcut 'T' sits at x=1 (its own colour), so x=2 is
        // the first cell of its plain "ables" — and Contents is not focused,
        // so x=18, inside its plain "able " before the 'c' shortcut at
        // x=23, must stay uncoloured.
        let buffer = draw(40, 12);
        let fg = |x, y| buffer.cell((x, y)).expect("cell is in bounds").fg;
        let focus = Theme::default().palette().focus;

        assert_eq!(
            fg(2, 0),
            focus,
            "Tables' plain title text is focus-coloured"
        );
        assert_ne!(fg(18, 0), focus, "Contents' plain title text is not");
    }

    #[test]
    fn focus_follows_the_state_rather_than_the_draw_order() {
        // Contents is drawn third, so bolding it cannot be positional luck.
        let buffer = draw_focused(40, 12, Panel::Contents);
        let bold = |x, y| {
            buffer
                .cell((x, y))
                .expect("corner is in bounds")
                .modifier
                .contains(Modifier::BOLD)
        };

        assert!(bold(16, 0), "Contents is focused");
        assert!(!bold(0, 0), "Tables is not");
        assert!(!bold(0, 5), "Ddl is not");
        assert!(!bold(16, 8), "Queries is not");
    }

    #[test]
    fn each_panel_colours_the_key_that_focuses_it() {
        let buffer = draw(40, 12);
        let marked = |x, y| {
            buffer.cell((x, y)).expect("cell is in bounds").fg
                == Theme::default().palette().shortcut
        };

        // "╭── Tables" and "╭── Table contents" on row 0, "╭── DDL" on row 5,
        // "╭── Queries" on row 8 — a corner, the inset and `TITLE_PAD` precede
        // each name.
        assert!(marked(5, 0), "Tables marks its a");
        assert!(marked(22, 0), "Table contents marks its b");
        assert!(marked(4, 5), "DDL marks its D");
        assert!(marked(21, 8), "Queries marks its u");
    }

    #[test]
    fn nothing_but_the_shortcut_wears_the_shortcut_colour_in_a_title() {
        let buffer = draw(40, 12);
        let marked = |x, y| {
            buffer.cell((x, y)).expect("cell is in bounds").fg
                == Theme::default().palette().shortcut
        };

        for x in [0, 1, 3, 4, 9] {
            assert!(!marked(x, 0), "column {x} of row 0 is not the mark");
        }
        assert!(!marked(5, 7), "only the first D of DDL is its mark");
    }

    #[test]
    fn a_frame_below_the_minimum_draws_the_notice_and_nothing_else() {
        for (width, height) in [(39, 6), (40, 6), (40, 5), (39, 5)] {
            let buffer = draw(width, height);
            let text: String = (0..height).map(|y| row(&buffer, y)).collect();

            assert!(
                text.contains("need 40×7"),
                "{width}x{height} must say what it needs, got {text:?}"
            );
            assert!(
                !text.contains('╭'),
                "{width}x{height} must draw no panel, got {text:?}"
            );
        }
    }

    #[test]
    fn the_notice_names_the_frame_it_was_given() {
        let buffer = draw(24, 5);
        let text: String = (0..5).map(|y| row(&buffer, y)).collect();

        assert!(text.contains("have 24×5"), "got {text:?}");
    }

    #[test]
    fn the_notice_drops_its_least_useful_line_first() {
        let lines = |height| {
            let buffer = draw(30, height);
            (0..height)
                .map(|y| row(&buffer, y).trim().to_owned())
                .filter(|line| !line.is_empty())
                .collect::<Vec<_>>()
        };

        assert_eq!(lines(3), ["Too small", "need 40×7", "have 30×3"]);
        assert_eq!(lines(2), ["need 40×7", "have 30×2"]);
        assert_eq!(lines(1), ["need 40×7"]);
    }

    #[test]
    fn nothing_is_drawn_over_the_notice() {
        let state = State {
            overlay: Some(Overlay::Message(Message {
                title: "Cannot connect".to_owned(),
                message: "no route to host".to_owned(),
                severity: Severity::Danger,
            })),
            ..State::default()
        };
        let mut terminal =
            Terminal::new(TestBackend::new(39, 6)).expect("TestBackend always initialises");

        terminal
            .draw(|frame| {
                render(frame, &view_model(&state), Theme::default());
            })
            .expect("TestBackend always draws");

        let buffer = terminal.backend().buffer().clone();
        let text: String = (0..6).map(|y| row(&buffer, y)).collect();

        assert!(
            !text.contains("Cannot connect"),
            "an overlay has nothing to sit over, got {text:?}"
        );
    }

    #[test]
    fn a_zero_row_frame_draws_nothing_and_does_not_panic() {
        let buffer = draw(30, 0);

        assert_eq!(buffer.area.height, 0);
    }

    #[test]
    fn a_frame_below_the_minimum_measures_nothing() {
        let state = State::default();
        let mut terminal =
            Terminal::new(TestBackend::new(39, 6)).expect("TestBackend always initialises");
        let mut geometry = Geometry::default();

        terminal
            .draw(|frame| {
                geometry = render(frame, &view_model(&state), Theme::default()).geometry;
            })
            .expect("TestBackend always draws");

        assert_eq!(geometry, Geometry::default());
    }

    fn table(name: &str) -> Table {
        Table {
            name: name.to_owned(),
            primary_key: PrimaryKey {
                partition: vec![column("id", "uuid")],
                clustering: Vec::new(),
            },
            static_columns: Vec::new(),
            regular_columns: Vec::new(),
            options: TableOptions::default(),
        }
    }

    fn column(name: &str, cql_type: &str) -> Column {
        Column {
            name: name.to_owned(),
            cql_type: cql_type.to_owned(),
        }
    }

    fn schema_of(names: &[&str]) -> Schema {
        tabled_schema(&names.iter().map(|n| (*n, &[][..])).collect::<Vec<_>>())
    }

    fn tabled_schema(keyspaces: &[(&str, &[&str])]) -> Schema {
        Schema {
            keyspaces: keyspaces
                .iter()
                .map(|(name, tables)| {
                    (
                        (*name).to_owned(),
                        Keyspace {
                            name: (*name).to_owned(),
                            replication: Replication::SimpleStrategy { factor: 1 },
                            tables: tables
                                .iter()
                                .map(|t| ((*t).to_owned(), table(t)))
                                .collect::<BTreeMap<_, _>>(),
                        },
                    )
                })
                .collect(),
        }
    }

    /// Rows of body the Tables panel has at 40x12, where these tests draw it.
    const TABLES_ROWS: usize = 3;

    fn browsing(names: &[&str], selected: usize) -> State {
        let mut state = State {
            connection: ConnectionState::Connected(Connection {
                id: ConnId::next(),
                posture: Posture::ReadOnly,
                schema: SchemaState::Loaded(schema_of(names)),
            }),
            ..State::default()
        };
        state.tables.cursor.go_to(selected, names.len());
        // Set with the cursor for the reason `select_contents` sets it: half a
        // state is not a state (list viewport design §6).
        state.tables.scroll.follow(selected, TABLES_ROWS);
        state
    }

    fn draw_state(width: u16, height: u16, state: &State) -> Buffer {
        draw_state_with(width, height, state, Theme::default())
    }

    fn draw_state_with(width: u16, height: u16, state: &State, theme: Theme) -> Buffer {
        let mut terminal =
            Terminal::new(TestBackend::new(width, height)).expect("TestBackend always initialises");

        terminal
            .draw(|frame| {
                render(frame, &view_model(state), theme);
            })
            .expect("TestBackend always draws");

        terminal.backend().buffer().clone()
    }

    #[test]
    fn keyspaces_are_listed_inside_the_tables_panel() {
        let buffer = draw_state(40, 12, &browsing(&["shop", "typezoo", "system"], 0));

        assert_eq!(
            row(&buffer, 1),
            "│▌\u{f084} shop       ││Press Enter on a table│"
        );
        assert_eq!(
            row(&buffer, 2),
            "│ \u{f084} typezoo    ││                      │"
        );
        assert_eq!(
            row(&buffer, 3),
            "│ \u{f084} system     ││                      │"
        );
    }

    #[test]
    fn a_tree_row_wears_the_icon_of_what_it_names() {
        let mut state = State {
            connection: ConnectionState::Connected(Connection {
                id: ConnId::next(),
                posture: Posture::ReadOnly,
                schema: SchemaState::Loaded(tabled_schema(&[("shop", &["orders"])])),
            }),
            ..State::default()
        };
        let schema = state.schema().cloned().expect("loaded");
        state.tables.open(&schema);

        let buffer = draw_state(40, 12, &state);

        assert!(
            row(&buffer, 1).contains(&format!("{}shop", crate::icons::KEYSPACE)),
            "the keyspace icon precedes the keyspace"
        );
        assert!(
            row(&buffer, 2).contains(&format!("{}orders", crate::icons::TABLE)),
            "the table icon precedes the table"
        );
    }

    #[test]
    fn the_ddl_identity_rows_wear_the_tree_icons_and_each_heading_its_own() {
        // Wide enough that every label has room for its icon; the panel that
        // has not is the subject of its own test.
        let buffer = draw_state(160, 41, &describing(orders()));
        let rows = ddl_rows(&buffer);

        assert!(
            rows[0].starts_with(&format!("▌{}Keyspace", crate::icons::KEYSPACE)),
            "the panel names its keyspace as the tree does, got {:?}",
            rows[0]
        );
        assert!(
            rows[1].starts_with(&format!(" {}Table", crate::icons::TABLE)),
            "and its table likewise, got {:?}",
            rows[1]
        );
        assert!(
            rows.iter()
                .any(|line| *line == format!("{}Partition Key", crate::icons::KEY)),
            "got {rows:?}"
        );
        assert!(
            rows.iter()
                .any(|line| *line == format!("{}Options", crate::icons::SETTINGS)),
            "and a settings section the cog, got {rows:?}"
        );
    }

    #[test]
    fn a_name_with_no_room_beside_its_label_wraps_rather_than_abbreviating() {
        // The Ddl body is 23 columns at this width, and the label spends ten
        // of them, so the table's name cannot share the line it labels.
        let buffer = draw_state(104, 41, &listing(orders()));

        assert_eq!(
            ddl_rows(&buffer)[..3],
            [
                // The cursor opens on the first row, which is where the bar is.
                format!("▌{}Keyspace         shop", crate::icons::KEYSPACE),
                format!(" {}Table", crate::icons::TABLE),
                "     orders_by_customer".to_owned(),
            ]
        );
    }

    #[test]
    fn an_unfolded_keyspaces_tables_are_indented_beneath_it() {
        let mut state = State {
            connection: ConnectionState::Connected(Connection {
                id: ConnId::next(),
                posture: Posture::ReadOnly,
                schema: SchemaState::Loaded(tabled_schema(&[
                    ("shop", &["orders", "users"]),
                    ("typezoo", &[]),
                ])),
            }),
            ..State::default()
        };
        let schema = state.schema().cloned().expect("loaded");
        state.tables.open(&schema);

        let buffer = draw_state(40, 12, &state);

        // Four rows of tree in three rows of panel, so the border carries the
        // scrollbar and `typezoo` is below the fold.
        assert_eq!(
            row(&buffer, 1),
            "│▌\u{f084} shop       ▐│Press Enter on a table│"
        );
        assert_eq!(
            row(&buffer, 2),
            "│ ├─ \u{f0ce} orders  ▐│                      │"
        );
        assert_eq!(
            row(&buffer, 3),
            "│ └─ \u{f0ce} users   ▐│                      │"
        );
    }

    #[test]
    fn the_clusters_own_keyspaces_are_dimmed() {
        let buffer = draw_state(40, 12, &browsing(&["shop", "system"], 0));
        // Column 4: the name, past the border, the gutter bar and the icon it
        // opens with. The icon carries its own colour (design §8), which is
        // not what this is about.
        let fg = |y: u16| buffer.cell((4, y)).expect("inside the panel").fg;
        let faint = Theme::default()
            .style(Token::Faint)
            .fg
            .expect("faint spends a colour");

        assert_ne!(fg(1), faint, "shop is the user's");
        assert_eq!(fg(2), faint, "system is the cluster's");
    }

    #[test]
    fn a_dimmed_rows_icon_keeps_the_colour_of_what_it_names() {
        let buffer = draw_state(40, 12, &browsing(&["shop", "system"], 0));
        // Column 2: a keyspace opens right after the border and the gutter
        // bar.
        let icon = |y: u16| buffer.cell((2, y)).expect("inside the panel").fg;
        let theme = Theme::default();
        let faint = theme.style(Token::Faint).fg.expect("faint spends a colour");
        let keyspace = theme
            .style(Token::Keyspace)
            .fg
            .expect("a keyspace spends a colour");

        assert_eq!(icon(2), keyspace, "the icon says what the row is");
        assert_eq!(
            buffer.cell((4, 2)).expect("inside the panel").fg,
            faint,
            "the name it precedes stays the cluster's"
        );
    }

    #[test]
    fn the_selected_row_is_the_one_the_cursor_is_on() {
        let buffer = draw_state(40, 12, &browsing(&["shop", "typezoo", "system"], 1));
        let highlight = Theme::default()
            .style(Token::Selected)
            .bg
            .expect("the cursor row spends a background");
        let selected = |y: u16| buffer.cell((1, y)).expect("inside the panel").bg == highlight;

        assert!(!selected(1));
        assert!(selected(2), "typezoo is selected");
        assert!(!selected(3));
    }

    #[test]
    fn a_ranged_tree_row_carries_the_bar_and_the_band() {
        let mut state = browsing(&["shop", "typezoo", "system"], 0);
        state.tables.selection = Selection::held(0, 1);

        let buffer = draw_state(40, 12, &state);

        assert_eq!(
            row(&buffer, 2).chars().nth(1),
            Some('▌'),
            "row 1 carries the bar though the cursor is on row 0"
        );
        assert_ne!(
            buffer.cell((1, 2)).expect("inside the panel").bg,
            buffer.cell((1, 3)).expect("inside the panel").bg,
            "row 1 is inside the range and banded, row 2 is not"
        );
        assert_ne!(
            buffer.cell((1, 1)).expect("inside the panel").bg,
            buffer.cell((1, 2)).expect("inside the panel").bg,
            "row 0 is the cursor row, also inside the range, and reads apart from row 1"
        );
    }

    #[test]
    fn a_panel_with_nothing_to_list_says_what_it_is_waiting_for() {
        let state = State {
            connection: ConnectionState::Connecting(ConnId::next()),
            ..State::default()
        };

        let buffer = draw_state(40, 12, &state);

        assert_eq!(row(&buffer, 1), "│Connecting…   ││Press Enter on a table│");
    }

    #[test]
    fn a_failure_notice_is_drawn_in_the_danger_style_rather_than_dimmed() {
        let state = State {
            connection: ConnectionState::Failed(DbError::Timeout),
            ..State::default()
        };

        let buffer = draw_state(40, 12, &state);
        let cell = buffer.cell((1, 1)).expect("inside the panel");
        let danger = Theme::default()
            .style(Token::Severity(Severity::Danger))
            .fg
            .expect("danger spends a colour");

        assert_eq!(
            cell.fg, danger,
            "a failure notice carries the danger token, not the muted one waiting states use"
        );
        assert!(
            !cell.modifier.contains(Modifier::DIM),
            "a failure must read as urgent, not as background noise"
        );
    }

    /// A state showing `page` for `shop.orders`, whose partition key is `id`.
    fn showing(page: Page) -> State {
        let mut state = State {
            connection: ConnectionState::Connected(Connection {
                id: ConnId::next(),
                posture: Posture::ReadOnly,
                schema: SchemaState::Loaded(tabled_schema(&[("shop", &["orders"])])),
            }),
            focus: Panel::Contents,
            ..State::default()
        };
        let query = QueryId::next();
        state
            .contents
            .requested(query, Subject::Table(TableRef::new("shop", "orders")));
        state.contents.page_ready(query, page);
        state
    }

    fn page_of(columns: &[&str], rows: &[&[CqlValue]]) -> Page {
        Page {
            columns: columns.iter().map(|name| (*name).to_owned()).collect(),
            rows: rows
                .iter()
                .map(|values| lc_domain::model::Row {
                    values: values.to_vec(),
                })
                .collect(),
            next: None,
        }
    }

    fn text(value: &str) -> CqlValue {
        CqlValue::Text(value.to_owned())
    }

    /// Places the cursor and the viewport that belongs with it, as `update`
    /// does — a fixture that sets one without the other describes a state the
    /// application cannot be in
    /// (docs/superpowers/specs/2026-08-09-list-viewport-design.md §6).
    fn select_contents(state: &mut State, index: usize, height: usize) {
        let len = state.contents.row_count();
        state.contents.cursor.go_to(index, len);
        state.contents.scroll.follow(index, height);
    }

    #[test]
    fn a_stored_offset_decides_which_rows_are_drawn() {
        let labels: Vec<String> = (0..20).map(|i| format!("row{i:02}")).collect();
        let values: Vec<Vec<CqlValue>> = labels.iter().map(|l| vec![text(l)]).collect();
        let rows: Vec<&[CqlValue]> = values.iter().map(Vec::as_slice).collect();
        let mut state = showing(page_of(&["id"], &rows));
        // The selection stays on row 0; only the viewport moves, which is what
        // no test could express before the offset was state.
        state.contents.scroll.follow(12, 1);

        let buffer = draw_state(60, 12, &state);
        let drawn: Vec<String> = (2..7).map(|y| row(&buffer, y)).collect();

        assert!(
            drawn.iter().any(|line| line.contains("row12")),
            "the stored offset decides the top row, got {drawn:?}"
        );
        assert!(
            !drawn.iter().any(|line| line.contains("row00")),
            "the selected row is scrolled away, got {drawn:?}"
        );
    }

    #[test]
    fn a_selection_scrolled_out_of_view_is_not_highlighted() {
        let labels: Vec<String> = (0..20).map(|i| format!("row{i:02}")).collect();
        let values: Vec<Vec<CqlValue>> = labels.iter().map(|l| vec![text(l)]).collect();
        let rows: Vec<&[CqlValue]> = values.iter().map(Vec::as_slice).collect();
        let mut state = showing(page_of(&["id"], &rows));
        state.contents.mode = SelectionMode::Cell;
        state.contents.scroll.follow(12, 1);

        let highlight = Theme::default()
            .style(Token::Crosshair)
            .bg
            .expect("the cell mode's crosshair spends a background");

        let buffer = draw_state(60, 12, &state);
        let selected = |y: u16| buffer.cell((17, y)).expect("inside the panel").bg == highlight;

        for y in 2..7 {
            assert!(
                !selected(y),
                "row 0 is off screen, so nothing drawn is the selection"
            );
        }

        // A control: the same panel, drawn with the selection inside the
        // viewport, does highlight it — so the absence above is the rule at
        // work rather than a fixture that stopped drawing rows.
        let mut on_screen = showing(page_of(&["id"], &rows));
        on_screen.contents.mode = SelectionMode::Cell;
        on_screen.contents.cursor.go_to(12, 20);
        on_screen.contents.scroll.follow(12, 5);
        let buffer = draw_state(60, 12, &on_screen);
        let highlighted =
            (2..7).any(|y| buffer.cell((17, y)).expect("inside the panel").bg == highlight);

        assert!(
            highlighted,
            "a selection on screen is drawn as the selection"
        );
    }

    #[test]
    fn a_tree_selection_scrolled_out_of_view_is_not_highlighted() {
        // The tree's half of the rule in `a_selection_scrolled_out_of_view_is_
        // not_highlighted`. Without it, handing ratatui a `Some(selected)` it
        // cannot see would walk the offset back to the selection and the wheel
        // would scroll Tables nowhere, with every other test still green
        // (list viewport design §4).
        let names: Vec<String> = (0..12).map(|i| format!("ks{i:02}")).collect();
        let refs: Vec<&str> = names.iter().map(String::as_str).collect();
        let mut state = browsing(&refs, 0);
        state.tables.scroll.follow(11, 1);

        let highlight = Theme::default()
            .style(Token::Selected)
            .bg
            .expect("the cursor row spends a background");
        let (buffer, _map) = draw_mapped(40, 12, &state);
        let drawn: Vec<String> = (1..6).map(|y| row(&buffer, y)).collect();
        let selected = |y: u16| buffer.cell((1, y)).expect("inside the panel").bg == highlight;

        assert!(
            drawn.iter().any(|line| line.contains("ks11")),
            "the stored offset decides the top row, got {drawn:?}"
        );
        for y in 1..6 {
            assert!(
                !selected(y),
                "ks00 is off screen, so nothing drawn is the selection"
            );
        }
    }

    #[test]
    fn a_page_is_drawn_as_a_header_over_its_rows() {
        let state = showing(page_of(
            &["id", "email"],
            &[
                &[CqlValue::Int(1), text("ada@example.com")],
                &[CqlValue::Int(2), text("bob@example.com")],
            ],
        ));

        let buffer = draw_state(60, 12, &state);

        assert_eq!(
            row(&buffer, 0),
            "╭── Tables ────╮╭── Table contents — shop.orders (2 rows) ─╮"
        );
        assert_eq!(
            row(&buffer, 1),
            "│▌\u{f084} shop       ││   id email                               │",
            "the header leaves the bar+gutter block blank"
        );
        assert_eq!(
            row(&buffer, 2),
            "│              ││▌1 1  ada@example.com                     │"
        );
        assert_eq!(
            row(&buffer, 3),
            "│              ││ 2 2  bob@example.com                     │"
        );
    }

    #[test]
    fn a_column_wider_than_the_cap_is_ellipsised_rather_than_clipped() {
        // A uuid is 36 columns; the cap is 24.
        let state = showing(page_of(
            &["id"],
            &[&[text("8f14e45f-ceea-467a-9c2b-8c4d2a1e9f01")]],
        ));

        let buffer = draw_state(60, 12, &state);

        assert_eq!(
            row(&buffer, 2),
            "│              ││▌1 8f14e45f-ceea-467a-9c2b…               │",
            "a silently clipped value looks like a shorter value"
        );
    }

    #[test]
    fn a_table_too_wide_for_the_panel_drops_columns_rather_than_squeezing_them() {
        // Four uuid-wide columns want 4×24 + spacing = 99 in a 66-column
        // panel. `Constraint::Length` is a request, not a floor: handed all
        // four, ratatui shrinks every one to fit and truncates each value with
        // no ellipsis to say it did.
        let wide = "8f14e45f-ceea-467a-9c2b-8c4d2a1e9f01";
        let state = showing(page_of(
            &["a", "b", "c", "d"],
            &[&[text(wide), text(wide), text(wide), text(wide)]],
        ));

        let buffer = draw_state(90, 12, &state);

        assert_eq!(
            row(&buffer, 1),
            "│▌\u{f084} shop             ││   a                        b                                     │",
            "the two columns that do not fit are not drawn at all"
        );
        assert_eq!(
            row(&buffer, 2),
            "│                    ││▌1 8f14e45f-ceea-467a-9c2b… 8f14e45f-ceea-467a-9c2b…              │",
            "and the two that do keep their full width and their ellipsis"
        );
    }

    #[test]
    fn the_key_columns_are_drawn_in_the_styles_that_carry_the_physical_shape() {
        let mut state = showing(page_of(
            &["id", "email", "note"],
            &[&[CqlValue::Int(1), text("a"), text("b")]],
        ));
        // A third column for the cursor to sit on: in the default cell mode
        // the crosshair washes the heading of the column the cursor is in, and
        // the terminal-default theme flattens the text under its wash — so the
        // two headings under test have to be ones the cursor is not on.
        state.contents.column.go_to(2, 3);

        let buffer = draw_state(60, 12, &state);
        let header = |x: u16| buffer.cell((x, 1)).expect("inside the panel");
        let partition = Theme::default()
            .style(Token::PartitionKey)
            .fg
            .expect("the partition key spends a colour");

        // Column 16 is the Contents panel's left border. The bar and the
        // gutter share one two-wide block (17-18), then one spacing column
        // (19), so the first heading starts at 20.
        assert_eq!(header(20).fg, partition, "id is the partition key");
        assert!(header(20).modifier.contains(Modifier::BOLD));
        assert_eq!(
            header(23).fg,
            Theme::default()
                .style(Token::RegularColumn)
                .fg
                .expect("a regular column reads as body text"),
            "a regular column reads as body text rather than as a key"
        );
    }

    #[test]
    fn a_matched_cell_reads_apart_from_an_unmatched_one_on_and_off_the_selected_row() {
        // Row 0 (not selected) and row 1 (the cursor's row) each hold one
        // cell the filter matches and one it does not — the composed case
        // is row 1, where `Token::Match` is drawn under `Token::Selected`'s
        // background bar and still has to be tellable apart from the id
        // beside it.
        let mut state = showing(page_of(
            &["id", "email"],
            &[
                &[CqlValue::Int(1), text("ali@example.com")],
                &[CqlValue::Int(2), text("alice@example.com")],
            ],
        ));
        state.contents.cursor.go_to(1, 2);
        state.contents.scroll.follow(1, 5);
        state.apply_filter(Filtered::Contents, Filter::new("ali"));

        let buffer = draw_state(60, 12, &state);
        let style = |x: u16, y: u16| {
            let cell = buffer.cell((x, y)).expect("inside the panel");
            (cell.fg, cell.bg, cell.modifier)
        };
        // Bar, then gutter, then `id` (one digit, the cursor's cell at column
        // 0), then `email` (matched): 21 lands on the digit, 24 on `email`'s
        // first character.
        let unmatched_id = 21;
        let matched_email = 24;

        assert_ne!(
            style(matched_email, 2),
            style(unmatched_id, 2),
            "row 0 is not selected: its matched email must still read apart from its unmatched id"
        );
        assert!(
            buffer
                .cell((matched_email, 3))
                .expect("inside the panel")
                .modifier
                .contains(Modifier::BOLD)
                && !buffer
                    .cell((unmatched_id, 3))
                    .expect("inside the panel")
                    .modifier
                    .contains(Modifier::BOLD),
            "row 1 is the cursor's row: its highlight text flattens both cells' colours, so \
             `BOLD` is what still tells the matched cell apart from the unmatched one \
             (the unmatched id is also the cell cursor, so it carries its own modifiers \
             instead — never `BOLD`)"
        );
    }

    #[test]
    fn a_null_is_faint_so_it_reads_apart_from_the_word() {
        // Row 1 (not the cursor's row) so the row's own colours are not
        // flattened under `Token::Selected`'s highlight text.
        let mut state = showing(page_of(
            &["id", "email"],
            &[
                &[CqlValue::Int(1), text("a")],
                &[CqlValue::Int(2), CqlValue::Null],
            ],
        ));
        state.contents.cursor.go_to(0, 2);

        let buffer = draw_state(60, 12, &state);
        let cell = |x: u16| buffer.cell((x, 3)).expect("inside the panel");
        let faint = Theme::default()
            .style(Token::Faint)
            .fg
            .expect("faint spends a colour");
        let muted = Theme::default()
            .style(Token::Muted)
            .fg
            .expect("muted spends a colour");

        assert_eq!(cell(24).fg, faint, "the null is faint");
        assert_eq!(
            cell(18).fg,
            muted,
            "the gutter digit shares the bar+gutter block's column and is muted, not faint"
        );
        assert_ne!(
            cell(21).fg,
            faint,
            "the value beside it is not — column 21 is inside the id value's own \
             column, not the gutter, and is not what this is about"
        );
    }

    #[test]
    fn a_matched_null_still_carries_the_match_colour() {
        // The ramp carries the tier now, not a modifier (design §4.1), so a
        // null's own token (`Token::Faint`) has no modifier for
        // `Style::patch` to carry through `Token::Match` overwriting `fg`. A
        // matched null and a matched word both end up drawn as a plain
        // match, indistinguishable from each other; telling a matched null
        // apart from a matched word again is future work, not this token's
        // job.
        let mut state = showing(page_of(
            &["id", "value", "note"],
            &[&[CqlValue::Int(1), CqlValue::Null, text("null")]],
        ));
        state.apply_filter(Filtered::Contents, Filter::new("null"));

        let buffer = draw_state(70, 12, &state);
        // Shifted right by the bar, the row-number gutter and their spacing:
        // one digit for a one-row page, plus one column each (table display
        // design §4, design §4.2).
        let cell = |x: u16| buffer.cell((x, 2)).expect("inside the panel");
        let null_cell = cell(25);
        let word_cell = cell(31);

        assert_eq!(
            null_cell.fg, word_cell.fg,
            "both are matches, so they carry the same match colour"
        );
        assert!(
            !null_cell.modifier.contains(Modifier::DIM)
                && !word_cell.modifier.contains(Modifier::DIM),
            "no token recedes with a modifier now that the ramp carries the tier"
        );
    }

    #[test]
    fn the_selected_row_of_the_grid_is_the_one_the_cursor_is_on() {
        let mut state = showing(page_of(
            &["id"],
            &[&[CqlValue::Int(1)], &[CqlValue::Int(2)]],
        ));
        state.contents.mode = SelectionMode::Cell;
        state.contents.cursor.down(2);

        let highlight = Theme::default()
            .style(Token::Crosshair)
            .bg
            .expect("the cell mode's crosshair spends a background");
        let buffer = draw_state(60, 12, &state);
        let selected = |y: u16| buffer.cell((17, y)).expect("inside the panel").bg == highlight;

        assert!(!selected(1), "the header is never the selection");
        assert!(!selected(2), "nor is the first row");
        assert!(selected(3), "the second row is");
    }

    #[test]
    fn row_mode_lights_the_whole_row_and_no_column() {
        let mut state = showing(page_of(
            &["id", "name"],
            &[
                &[CqlValue::Int(1), text("alice")],
                &[CqlValue::Int(2), text("bob")],
            ],
        ));
        state.contents.mode = SelectionMode::Row;
        state.contents.cursor.down(2);
        // The column cursor is still somewhere, so "no column" is a claim about
        // what row mode draws rather than about where the cursor happens to be.
        state.contents.column.go_to(1, 2);

        let theme = Theme::default();
        let selected = theme
            .style(Token::Selected)
            .bg
            .expect("the cursor row spends a background");
        let buffer = draw_state(60, 12, &state);
        let area = block_area(&buffer, "Table contents");
        let bg = |x: u16, y: u16| buffer.cell((x, y)).expect("inside the panel").bg;
        let name = area.x + 1 + 6;

        assert_eq!(bg(17, 3), selected, "the row the cursor is on is lit");
        assert_ne!(
            bg(name, 2),
            selected,
            "and the cursor's column off that row is not — row mode lights no column"
        );
        assert_ne!(
            bg(17, 2),
            selected,
            "and the row above it is not — row mode lights one row"
        );
        assert_ne!(
            bg(17, 1),
            selected,
            "the header is never part of the selection in row mode"
        );
    }

    #[test]
    fn column_mode_lights_the_column_and_its_heading() {
        let mut state = showing(page_of(
            &["id", "name"],
            &[
                &[CqlValue::Int(1), text("alice")],
                &[CqlValue::Int(2), text("bob")],
            ],
        ));
        state.contents.mode = SelectionMode::Column;
        state.contents.column.go_to(1, 2);

        let theme = Theme::default();
        let selected = theme
            .style(Token::Selected)
            .bg
            .expect("the cursor column spends a background");
        let buffer = draw_state(60, 12, &state);
        let area = block_area(&buffer, "Table contents");
        let bg = |x: u16, y: u16| buffer.cell((x, y)).expect("inside the panel").bg;
        // The bar and the gutter share one widget column, two wide for a
        // two-row page, then one column of spacing, then `id` (two wide), then
        // one more spacing column: the second column's first cell is six in
        // from the panel's inner edge.
        let name = area.x + 1 + 6;

        assert_eq!(bg(name, 2), selected, "every row of the column is lit");
        assert_eq!(bg(name, 3), selected, "including the last one");
        assert_eq!(
            bg(name, 1),
            selected,
            "and its heading, which ratatui leaves to us (design §4)"
        );
        assert_ne!(
            bg(area.x + 1 + 3, 2),
            selected,
            "the row the cursor is on is not — column mode lights one column"
        );
    }

    /// A yank spends the selection that named the rows, so the flash is the
    /// only thing left saying what was copied — and it has to be louder than
    /// the band that was standing there a frame ago, or the copy reads as the
    /// selection simply vanishing (visual mode design §6.1).
    #[test]
    fn a_yanks_afterglow_is_louder_than_the_range_it_replaces() {
        let mut ranged = showing(sized_page(2, 4));
        ranged.contents.mode = SelectionMode::Row;
        ranged.contents.cursor.go_to(3, 4);
        ranged.contents.rows = Selection::held(0, 1);

        let mut flashed = showing(sized_page(2, 4));
        flashed.contents.mode = SelectionMode::Row;
        flashed.contents.cursor.go_to(3, 4);
        flashed.flash = Some(Flash::lit(Panel::Contents, Some(Range::new(0, 1)), None));

        let bg = |state: &State, row: u16| {
            let buffer = draw_state(60, 12, state);
            let area = block_area(&buffer, "Table contents");
            buffer
                .cell((area.x + 4, area.y + 2 + row))
                .expect("inside the panel")
                .bg
        };
        let palette = Theme::default().palette();

        assert_eq!(bg(&ranged, 0), palette.ranged);
        assert_eq!(
            bg(&flashed, 0),
            palette.selection_bg,
            "the afterglow takes the loudest band, which is the cursor row's"
        );
        assert_eq!(
            bg(&flashed, 2),
            bg(&ranged, 2),
            "and it lights only the rows the yank took"
        );
    }

    /// The flash reaches the panel whose `y` lit it and no other.
    #[test]
    fn a_yank_in_the_tree_lights_the_tree_alone() {
        let mut state = showing(sized_page(2, 4));
        state.flash = Some(Flash::lit(Panel::Tables, Some(Range::new(0, 0)), None));

        let buffer = draw_state(60, 12, &state);
        let palette = Theme::default().palette();
        let tree = block_area(&buffer, "Tables");
        let grid = block_area(&buffer, "Table contents");

        assert_eq!(
            buffer
                .cell((tree.x + 2, tree.y + 1))
                .expect("inside the panel")
                .bg,
            palette.selection_bg
        );
        // Row 2, not the grid's own row 0: that one carries the cursor band in
        // this palette's loudest colour whether or not anything flashed, and
        // would pass this assertion for the wrong reason.
        assert_ne!(
            buffer
                .cell((grid.x + 4, grid.y + 4))
                .expect("inside the panel")
                .bg,
            palette.selection_bg,
            "a flash lit over the tree reaches no other panel"
        );
    }

    #[test]
    fn every_row_of_a_range_is_banded_and_the_cursor_row_reads_apart_from_them() {
        let mut state = showing(sized_page(2, 4));
        state.contents.mode = SelectionMode::Row;
        state.contents.cursor.down(4);
        state.contents.rows = Selection::held(0, 2);

        let buffer = draw_state(60, 12, &state);
        let area = block_area(&buffer, "Table contents");
        // The bar+gutter block for a 4-row page is two columns wide, then one
        // of spacing: the first data column starts four in from the inner
        // edge, and each further column is three cells wide.
        let bg = |row: u16, column: u16| {
            buffer
                .cell((area.x + 4 + column * 3, area.y + 2 + row))
                .expect("inside the panel")
                .bg
        };

        let banded = bg(0, 0);
        let cursor = bg(1, 0);
        let plain = bg(3, 0);

        assert_ne!(banded, plain, "a row in the range is banded");
        assert_ne!(
            banded, cursor,
            "and the cursor row still reads apart from it"
        );
        assert_ne!(
            bg(0, 1),
            bg(3, 1),
            "a second column on the banded row is banded too, not just the first"
        );
    }

    /// A band drawn cell by cell breaks at every column spacing, so a held
    /// range reads as one stripe per column instead of one selection.
    #[test]
    fn a_row_range_bands_the_spacing_between_its_columns_too() {
        let mut state = showing(sized_page(2, 4));
        state.contents.mode = SelectionMode::Row;
        state.contents.cursor.down(4);
        state.contents.rows = Selection::held(0, 2);

        let buffer = draw_state(60, 12, &state);
        let area = block_area(&buffer, "Table contents");
        let bg = |row: u16, x: u16| {
            buffer
                .cell((area.x + x, area.y + 2 + row))
                .expect("inside the panel")
                .bg
        };

        // A two-wide value column starts four in from the inner edge, so the
        // spacing before the second column is the sixth cell; the gutter's row
        // number is the second.
        for x in [2, 6] {
            assert_eq!(
                bg(0, x),
                bg(0, 4),
                "the band runs the width of the row, gutter and spacing included"
            );
            assert_ne!(bg(3, x), bg(0, x), "and stops at the range's last row");
        }
    }

    #[test]
    fn a_cell_block_bands_the_rectangle_and_nothing_outside_it() {
        let mut state = showing(sized_page(3, 3));
        // The grid opens in row mode, and only cell mode has a rectangle: the
        // projection gives a column range on no other (visual mode design §3).
        state.contents.mode = SelectionMode::Cell;
        state.contents.rows = Selection::held(0, 1);
        state.contents.columns = Selection::held(0, 1);

        let buffer = draw_state(60, 12, &state);
        let area = block_area(&buffer, "Table contents");
        let bg = |row: u16, column: u16| {
            buffer
                .cell((area.x + 4 + column * 3, area.y + 2 + row))
                .expect("inside the panel")
                .bg
        };

        assert_ne!(
            bg(1, 1),
            bg(1, 2),
            "the block ends at the column range's edge"
        );
        // Columns 1 and 2 rather than 0 and 2: the cursor's own column is 0,
        // and cell mode washes it down every row regardless of the range —
        // reading it here would prove nothing about the range.
        assert_eq!(
            bg(2, 1),
            bg(2, 2),
            "outside the row range, neither cell is banded"
        );
    }

    #[test]
    fn a_column_range_bands_its_columns_and_lights_no_gutter_bar() {
        let mut state = showing(sized_page(3, 3));
        state.contents.mode = SelectionMode::Column;
        state.contents.columns = Selection::held(0, 1);

        let buffer = draw_state(60, 12, &state);
        let area = block_area(&buffer, "Table contents");
        let bar = buffer
            .cell((area.x + 1, area.y + 3))
            .expect("inside the panel");

        assert_eq!(
            bar.symbol(),
            " ",
            "the range is columns; a bar down every row would claim the rows were selected"
        );

        // Row 2 (index 1), off the cursor row: column 0 is inside the range,
        // column 2 is not.
        let bg = |column: u16| {
            buffer
                .cell((area.x + 4 + column * 3, area.y + 3))
                .expect("inside the panel")
                .bg
        };
        assert_ne!(
            bg(0),
            bg(2),
            "a ranged column is banded and an unranged one is not, off the cursor row"
        );
    }

    /// `follow_grid_columns` pulls the viewport over the column cursor against
    /// the geometry of the frame *before* this one, so a resize that narrows
    /// the panel draws one frame whose cursor is off the right of what fits.
    /// ratatui clamps `selected_column` to the last column it drew, which would
    /// band a column the heading gate does not agree with.
    #[test]
    fn column_mode_lights_nothing_when_its_cursor_is_off_the_columns_drawn() {
        let mut state = showing(sized_page(12, 3));
        state.contents.mode = SelectionMode::Column;
        state.contents.column.go_to(11, 12);

        let buffer = draw_state(40, 12, &state);
        let area = block_area(&buffer, "Table contents");
        let selected = Theme::default()
            .style(Token::Selected)
            .bg
            .expect("the cursor column spends a background");

        assert_eq!(
            state.contents.column_scroll.first(),
            0,
            "the viewport has not followed the cursor, which is the state under test"
        );
        for y in area.y + 1..area.y + area.height - 1 {
            for x in area.x + 1..area.x + area.width - 1 {
                assert_ne!(
                    buffer.cell((x, y)).expect("inside the panel").bg,
                    selected,
                    "({x}, {y}) is lit for a column the grid is not drawing"
                );
            }
        }
    }

    /// The heading's wash is the crosshair's, and a row cursor scrolled out of
    /// view leaves the widget painting no crosshair at all — so the heading
    /// cannot stay lit on its own (design §4).
    #[test]
    fn a_heading_is_unwashed_once_the_crosshair_scrolls_out_of_view() {
        let labels: Vec<String> = (0..20).map(|i| format!("row{i:02}")).collect();
        let values: Vec<Vec<CqlValue>> = labels.iter().map(|l| vec![text(l)]).collect();
        let rows: Vec<&[CqlValue]> = values.iter().map(Vec::as_slice).collect();
        let mut state = showing(page_of(&["id"], &rows));
        state.contents.mode = SelectionMode::Cell;

        let crosshair = Theme::default()
            .style(Token::Crosshair)
            .bg
            .expect("the cell mode's crosshair spends a background");
        // The bar and the gutter share one widget column, three wide for a
        // twenty-row page, then a column of spacing: the only heading starts
        // four in from the panel's inner edge.
        let heading = |buffer: &Buffer| {
            let area = block_area(buffer, "Table contents");
            buffer
                .cell((area.x + 1 + 4, 1))
                .expect("inside the panel")
                .bg
        };

        let on_screen = draw_state(60, 12, &state);
        assert_eq!(
            heading(&on_screen),
            crosshair,
            "with the cursor in view the heading is washed with its column"
        );

        state.contents.scroll.follow(12, 1);
        let scrolled_away = draw_state(60, 12, &state);

        assert_ne!(
            heading(&scrolled_away),
            crosshair,
            "and once the selection is off screen nothing is left to wash it"
        );
    }

    /// The wash is composed onto the heading's own token rather than replacing
    /// it, so a key column still says which key it is under the crosshair
    /// wherever the theme keeps its colours (design §4).
    #[test]
    fn a_key_columns_heading_keeps_its_colour_under_the_wash() {
        let theme = Theme::of(ThemeChoice::Ported(Ported::CatppuccinMocha));
        let mut state = showing(page_of(&["id", "email"], &[&[CqlValue::Int(1), text("a")]]));
        state.contents.mode = SelectionMode::Cell;

        let buffer = draw_state_with(60, 12, &state, theme);
        let area = block_area(&buffer, "Table contents");
        let heading = buffer.cell((area.x + 1 + 3, 1)).expect("inside the panel");

        assert_eq!(
            heading.bg,
            theme.palette().crosshair,
            "the cursor's column is washed, heading and all"
        );
        assert_eq!(
            heading.fg,
            theme
                .style(Token::PartitionKey)
                .fg
                .expect("the partition key spends a colour"),
            "and `id` still reads as the partition key under it"
        );
    }

    /// The wash is a background alone wherever the theme keeps its colours, so
    /// a null under the crosshair is still drawn as a null (design §4).
    ///
    /// The null sits off the cursor row on purpose: on it, the row arm of the
    /// crosshair would paint the same background, and dropping the column arm
    /// entirely would go unnoticed.
    #[test]
    fn the_crosshair_leaves_a_flavours_column_colours_alone() {
        let theme = Theme::of(ThemeChoice::Ported(Ported::CatppuccinMocha));
        let mut state = showing(page_of(
            &["id", "value"],
            &[
                &[CqlValue::Int(1), CqlValue::Int(2)],
                &[CqlValue::Int(3), CqlValue::Null],
            ],
        ));
        state.contents.mode = SelectionMode::Cell;
        state.contents.column.go_to(1, 2);

        let buffer = draw_state_with(60, 12, &state, theme);
        let area = block_area(&buffer, "Table contents");
        let null = buffer.cell((area.x + 1 + 6, 3)).expect("inside the panel");

        assert_ne!(
            buffer.cell((area.x + 1, 3)).expect("inside the panel").bg,
            theme.palette().crosshair,
            "the second row is not the cursor row, so only the column arm can wash it"
        );
        assert_eq!(
            null.bg,
            theme.palette().crosshair,
            "the selected column is washed down its whole height"
        );
        assert_eq!(
            null.fg,
            theme.style(Token::Faint).fg.expect("a null is faint"),
            "and the null under it keeps its own colour"
        );
    }

    #[test]
    fn the_grid_scrolls_to_keep_the_selection_on_screen() {
        // The Contents panel holds a header and five rows at this size, so a
        // twenty-row page forces the scroll a plain `Table` would not perform.
        let labels: Vec<String> = (0..20).map(|i| format!("row{i:02}")).collect();
        let values: Vec<Vec<CqlValue>> = labels.iter().map(|l| vec![text(l)]).collect();
        let rows: Vec<&[CqlValue]> = values.iter().map(Vec::as_slice).collect();
        let mut state = showing(page_of(&["id"], &rows));
        select_contents(&mut state, 19, 5);

        let buffer = draw_state(60, 12, &state);
        let drawn: Vec<String> = (2..8).map(|y| row(&buffer, y)).collect();

        assert!(
            drawn.iter().any(|line| line.contains("row19")),
            "the selected row must stay on screen once the page scrolls"
        );
        assert!(
            !drawn.iter().any(|line| line.contains("row00")),
            "a row scrolled out of view must not still be drawn"
        );
    }

    #[test]
    fn a_table_with_no_rows_says_so_where_the_grid_would_be() {
        let state = showing(page_of(&["id"], &[]));

        let buffer = draw_state(60, 12, &state);

        assert_eq!(
            row(&buffer, 1),
            "│▌\u{f084} shop       ││No rows                                   │"
        );
    }

    /// A page of `columns` columns and `rows` rows, every value as wide as its
    /// heading.
    fn sized_page(columns: usize, rows: usize) -> Page {
        Page {
            columns: (0..columns).map(|index| format!("c{index}")).collect(),
            rows: (0..rows)
                .map(|row| lc_domain::model::Row {
                    values: (0..columns).map(|_| text(&format!("v{row}"))).collect(),
                })
                .collect(),
            next: None,
        }
    }

    /// A `width`-wide frame showing a page of `columns` columns and `rows`
    /// rows, with the cursor on its first cell.
    fn frame_showing_grid(columns: usize, width: u16, rows: usize) -> Buffer {
        draw_state(width, 12, &showing(sized_page(columns, rows)))
    }

    /// The same, one column wide, scrolled so that `offset` is the top row.
    fn frame_showing_scrolled_grid(rows: usize, offset: usize) -> Buffer {
        let mut state = showing(sized_page(1, rows));
        state.contents.scroll.follow(offset, 1);

        draw_state(60, 12, &state)
    }

    #[test]
    fn a_grid_wider_than_its_panel_gets_a_horizontal_scrollbar() {
        let buffer = frame_showing_grid(12, 40, 20);

        assert!(has_horizontal_scrollbar(&buffer, "Table contents"));
    }

    #[test]
    fn a_grid_that_fits_gets_no_horizontal_scrollbar() {
        let buffer = frame_showing_grid(2, 200, 20);

        assert!(
            !has_horizontal_scrollbar(&buffer, "Table contents"),
            "its absence is what says nothing is hidden"
        );
    }

    #[test]
    fn the_selected_cell_is_marked_apart_from_the_rest_of_its_row() {
        let mut state = showing(sized_page(4, 3));
        state.contents.mode = SelectionMode::Cell;
        let buffer = draw_state(40, 12, &state);
        let area = block_area(&buffer, "Table contents");
        let underlined = |x: u16, y: u16| {
            buffer
                .cell((x, y))
                .expect("inside the panel")
                .modifier
                .contains(Modifier::UNDERLINED)
        };
        // The top border, then the grid's header row, then the first row —
        // the one the cursor is on, at its first column. That column is two
        // wide and sits past the merged bar+gutter block (bar plus a
        // one-digit gutter) and the one spacing column beside it.
        let first = area.x + 4;
        let selected = area.y + 2;

        assert!(underlined(first, selected), "the cell the cursor is on");
        assert!(underlined(first + 1, selected), "for its whole width");
        assert!(
            !underlined(first + 3, selected),
            "the next column of the same row is not what a command would act on"
        );
        assert!(
            !underlined(first, selected + 1),
            "nor is the same column of the next row"
        );
    }

    /// The crosshair is a locator and the range is a selection, so where the
    /// two cross the range is what the cell has to say (design §6).
    #[test]
    fn a_ranged_cell_keeps_its_band_where_the_crosshair_crosses_it() {
        let mut state = showing(sized_page(4, 3));
        state.contents.mode = SelectionMode::Cell;
        state.contents.rows = Selection::held(0, 1);
        state.contents.columns = Selection::held(0, 1);

        let buffer = draw_state(40, 12, &state);
        let area = block_area(&buffer, "Table contents");
        let bg = |column: u16, row: u16| {
            buffer
                .cell((drawn_column(area, column), area.y + 2 + row))
                .expect("inside the panel")
                .bg
        };
        let theme = Theme::default();

        assert_eq!(
            bg(1, 0),
            theme.palette().ranged,
            "the cursor's own row crosses the range, and the range wins"
        );
        assert_eq!(
            bg(0, 1),
            theme.palette().ranged,
            "so does the cursor's own column"
        );
        assert_eq!(
            bg(2, 0),
            theme.palette().crosshair,
            "the crosshair still runs past the range it crosses"
        );
    }

    #[test]
    fn the_row_and_the_cell_are_marked_in_an_unfocused_panel_too() {
        // `body::grid` ignores the `focused` flag, which is design §3's
        // deliberate decision rather than an oversight: every other selection
        // in the program is drawn unfocused, and the border says which panel
        // is live. True by construction today, so only a fixture drawn with
        // focus elsewhere would notice a future `if focused`.
        let mut state = showing(sized_page(4, 3));
        state.contents.mode = SelectionMode::Cell;
        state.focus = Panel::Tables;

        let buffer = draw_state(40, 12, &state);
        let area = block_area(&buffer, "Table contents");
        let cell = |x: u16| buffer.cell((x, area.y + 2)).expect("inside the panel");
        let first = area.x + 4;
        let highlight = Theme::default()
            .style(Token::Crosshair)
            .bg
            .expect("the cell mode's crosshair spends a background");

        assert!(
            cell(first).modifier.contains(Modifier::UNDERLINED),
            "the selected cell keeps its highlight without focus"
        );
        assert!(
            cell(first + 3).bg == highlight
                && !cell(first + 3).modifier.contains(Modifier::UNDERLINED),
            "and the rest of its row keeps the row tint alone"
        );
    }

    #[test]
    fn the_gutter_is_muted_and_the_values_beside_it_are_not() {
        let buffer = frame_showing_grid(4, 40, 3);
        let area = block_area(&buffer, "Table contents");
        let fg = |x: u16| buffer.cell((x, area.y + 3)).expect("inside the panel").fg;
        let muted = Theme::default()
            .style(Token::Muted)
            .fg
            .expect("muted spends a colour");

        assert_eq!(
            fg(area.x + 2),
            muted,
            "the row number is chrome, not a value"
        );
        assert_ne!(
            fg(area.x + 4),
            muted,
            "and the value beside it reads at full weight"
        );
    }

    /// A twelve-column page whose column cursor visited `path` in order, with
    /// the horizontal viewport following it the way `update` does.
    ///
    /// Twelve two-wide columns in the 22-column body a 40x12 frame gives the
    /// Contents panel, less the merged bar+gutter block and its one spacing
    /// column: six fit, so anything past the sixth has to be reached by
    /// scrolling.
    fn showing_column_path(path: &[usize]) -> State {
        let page = sized_page(12, 3);
        let widths = lc_domain::grid::widths(&page);
        let mut state = showing(page);
        state.contents.mode = SelectionMode::Cell;
        let available = lc_domain::grid::available(22, 1);

        for column in path {
            state.contents.column.go_to(*column, widths.len());
            state
                .contents
                .column_scroll
                .follow(*column, &widths, available);
        }

        state
    }

    /// The x of the `index`th drawn column's first cell: the merged
    /// bar+gutter block and its one spacing column, then two-wide columns
    /// each followed by a spacing one.
    const fn drawn_column(area: Rect, index: u16) -> u16 {
        area.x + 4 + index * 3
    }

    #[test]
    fn the_grid_is_drawn_from_the_column_the_viewport_names() {
        // Out to c8, which forces the scroll, and back to c5 — which the
        // viewport already shows, so it stays where it is. The cursor then
        // sits in the middle of the drawn window rather than against its
        // right edge, where an index that ignored the offset would land on
        // the same cell and be indistinguishable.
        let state = showing_column_path(&[8, 5]);
        assert!(
            state.contents.column_scroll.first() > 0,
            "the fixture must have scrolled sideways or this test proves nothing"
        );

        let buffer = draw_state(40, 12, &state);
        let area = block_area(&buffer, "Table contents");
        let heading = block_row(&buffer, "Table contents", 0);
        let first = block_row(&buffer, "Table contents", 1);
        let underlined = |x: u16| {
            buffer
                .cell((x, area.y + 2))
                .expect("inside the panel")
                .modifier
                .contains(Modifier::UNDERLINED)
        };

        assert!(
            heading.trim_start().starts_with("c3"),
            "the drawn window starts at the offset, got {heading:?}"
        );
        assert!(
            first.starts_with('▌'),
            "the bar is pinned and never scrolls off, got {first:?}"
        );
        // c3 is drawn first, so the cursor's c5 is the third column drawn.
        assert!(
            underlined(drawn_column(area, 2)) && underlined(drawn_column(area, 2) + 1),
            "the cell cursor is on c5, counted from the drawn window rather \
             than from the page: {heading:?}"
        );
        assert!(
            !underlined(drawn_column(area, 3)),
            "and not on the column beside it"
        );
        assert!(
            !underlined(drawn_column(area, 5)),
            "nor on the last column drawn, where an index blind to the offset \
             would land"
        );
    }

    /// The trap `body.rs::grid` documents around `selected_column`: a range
    /// held over the *page*'s column indices must be tested against the page
    /// column a drawn cell names, not the position it happens to draw at.
    #[test]
    fn a_scrolled_grids_column_range_bands_the_page_column_not_the_drawn_position() {
        let mut state = showing_column_path(&[8, 5]);
        assert_eq!(
            state.contents.column_scroll.first(),
            3,
            "the fixture must scroll to c3 or this test proves nothing about \
             page indices"
        );
        // Page column 6 — drawn third in this window (c3, c4, c5, c6, …). A
        // check keyed on the drawn position rather than the page column would
        // either band the wrong cell or, since no drawn position is 6, band
        // nothing at all in a window this size.
        state.contents.columns = Selection::held(6, 6);

        let buffer = draw_state(40, 12, &state);
        let area = block_area(&buffer, "Table contents");
        // Row 1, not the cursor's row 0: the cursor's own row carries the
        // crosshair under the band, and a row that is washed either way proves
        // nothing about the range here.
        let bg = |index: u16| {
            buffer
                .cell((drawn_column(area, index), area.y + 3))
                .expect("inside the panel")
                .bg
        };

        assert_ne!(
            bg(3),
            bg(0),
            "page column 6 draws third in this scrolled window and must be \
             banded there, keyed on its page index rather than its position"
        );
    }

    /// The Contents panel's bottom border at 40x12, between its corners.
    fn contents_bottom(buffer: &Buffer) -> String {
        let area = block_area(buffer, "Table contents");

        (area.x + 1..area.right() - 1)
            .filter_map(|x| buffer.cell((x, area.bottom() - 1)))
            .map(Cell::symbol)
            .collect()
    }

    #[test]
    fn the_horizontal_thumb_sits_at_whichever_end_of_the_track_the_viewport_is_on() {
        let left = contents_bottom(&draw_state(40, 12, &showing_column_path(&[0])));
        let right = contents_bottom(&draw_state(40, 12, &showing_column_path(&[11])));

        assert!(
            left.starts_with(symbols::line::THICK_HORIZONTAL),
            "the first column is on screen, so the thumb belongs at the left: {left:?}"
        );
        assert!(
            right.ends_with(symbols::line::THICK_HORIZONTAL),
            "the last column is on screen, so the thumb belongs at the right: {right:?}"
        );
        assert!(
            !left.ends_with(symbols::line::THICK_HORIZONTAL)
                && !right.starts_with(symbols::line::THICK_HORIZONTAL),
            "a thumb spanning the whole track would say nothing about where the \
             viewport is: {left:?} {right:?}"
        );
    }

    #[test]
    fn rows_are_numbered_from_one() {
        let buffer = frame_showing_grid(2, 200, 20);
        let text = block_row(&buffer, "Table contents", 1);

        assert!(
            text.trim_start_matches(['▌', ' ']).starts_with('1'),
            "got {text:?}"
        );
    }

    #[test]
    fn the_row_number_is_the_pages_own_index_after_a_scroll() {
        // A twelve-row page scrolled to put its fourth row at the top.
        let buffer = frame_showing_scrolled_grid(12, 3);
        let text = block_row(&buffer, "Table contents", 1);

        assert!(
            text.trim_start().starts_with('4'),
            "the fourth row is still row 4, got {text:?}"
        );
    }

    #[test]
    fn a_column_wider_than_the_panel_is_ellipsised_rather_than_squeezed() {
        // `lc_domain::grid::widths` caps a column at 24 and not at the panel,
        // so the natural width outruns the 19 columns of body left once the
        // merged bar+gutter block and its spacing column are taken. `drawn_widths`
        // caps each width against what is left, so ratatui is never
        // overcommitted and the bar keeps its own cells; the value is cut by
        // `styled_cell`, which is where the ellipsis below comes from.
        let state = showing(page_of(
            &["id"],
            &[&[text("8f14e45f-ceea-467a-9c2b-8c4d2a1e9f01")]],
        ));

        let buffer = draw_state(40, 12, &state);
        let drawn = block_row(&buffer, "Table contents", 1);

        assert!(
            drawn.starts_with("▌1 "),
            "the bar and the gutter keep their own width: {drawn:?}"
        );
        assert!(
            drawn.trim_end().ends_with('…'),
            "and the value says it continues: {drawn:?}"
        );
    }

    /// `shop.orders_by_customer` as the fixture declares it.
    fn orders() -> Table {
        Table {
            name: "orders_by_customer".to_owned(),
            primary_key: PrimaryKey {
                partition: vec![column("customer_id", "uuid")],
                clustering: vec![
                    ClusteringColumn {
                        column: column("order_date", "date"),
                        order: ClusteringOrder::Descending,
                    },
                    ClusteringColumn {
                        column: column("order_id", "timeuuid"),
                        order: ClusteringOrder::Ascending,
                    },
                ],
            },
            static_columns: Vec::new(),
            regular_columns: Vec::new(),
            options: TableOptions::new(
                [
                    ("comment", "Order history."),
                    ("gc_grace_seconds", "864000"),
                    ("bloom_filter_fp_chance", "0.01"),
                ]
                .into_iter()
                .map(|(name, value)| (name.to_owned(), value.to_owned()))
                .collect(),
            ),
        }
    }

    /// A state with `shop` unfolded and the cursor on its one table.
    fn describing(table: Table) -> State {
        let schema = Schema {
            keyspaces: BTreeMap::from([(
                "shop".to_owned(),
                Keyspace {
                    name: "shop".to_owned(),
                    replication: Replication::SimpleStrategy { factor: 1 },
                    tables: BTreeMap::from([(table.name.clone(), table)]),
                },
            )]),
        };
        let mut state = State {
            connection: ConnectionState::Connected(Connection {
                id: ConnId::next(),
                posture: Posture::ReadOnly,
                schema: SchemaState::Loaded(schema.clone()),
            }),
            ..State::default()
        };
        state.tables.open(&schema);
        state.tables.open(&schema);
        state
    }

    /// The Ddl panel's top border at 104x41; its body opens on the row below.
    const DDL_BORDER: u16 = 16;
    const DDL_BODY: u16 = DDL_BORDER + 1;
    /// One past the body's last row: the panel's bottom border.
    const DDL_END: u16 = 39;

    /// Inside the Ddl panel's border at 104x41: twenty-two rows of 23 columns.
    /// The body's lines, the selection bar's column included — so a row opens
    /// with the blank it reserves for the bar and a heading, which is no cursor
    /// stop, opens at the margin.
    fn ddl_rows(buffer: &Buffer) -> Vec<String> {
        (DDL_BODY..DDL_END)
            .map(|y| {
                (1..25)
                    .filter_map(|x| buffer.cell((x, y)))
                    .map(Cell::symbol)
                    .collect::<String>()
                    .trim_end()
                    .to_owned()
            })
            .collect()
    }

    /// `describing`, moved to the panel's second tab.
    fn listing(table: Table) -> State {
        let mut state = describing(table);
        state.ddl.show(DdlTab::Columns);
        state
    }

    #[test]
    fn the_columns_tab_draws_every_column_with_its_kind_and_type() {
        let table = Table {
            static_columns: vec![Column {
                name: "region".to_owned(),
                cql_type: "text".to_owned(),
            }],
            ..orders()
        };

        let buffer = draw_state(104, 41, &listing(table));

        assert_eq!(
            ddl_rows(&buffer)[..9],
            [
                // The cursor opens on this row, which is where the bar is.
                "▌\u{f084} Keyspace         shop",
                " \u{f0ce} Table",
                "     orders_by_customer",
                "",
                "4 columns",
                " \u{f030b} customer_id      uuid",
                " \u{f030b} order_date       date",
                " \u{f030b} order_id     timeuuid",
                // No glyph outside the primary key: the row's colour is what
                // says a column is static.
                "   region           text",
            ]
        );
    }

    #[test]
    fn a_title_is_pushed_off_its_corner_by_border_and_not_by_blanks() {
        let buffer = draw(80, 24);
        let border = row(&buffer, 0);
        let lead: String = BORDER_LINE.to_string().repeat(TITLE_INSET);

        assert!(
            border.starts_with(&format!("╭{lead} Tables ")),
            "the gap reads as the frame running on, got {border:?}"
        );
    }

    #[test]
    fn a_border_with_one_column_to_spare_gives_it_to_the_title_and_not_the_marker() {
        // Tables on the sidebar's 16-column floor wearing `/or`: six columns
        // of name, three of marker, four of padding and two of corner leave
        // one. The title takes it, so the border does not read as two
        // alignments at once.
        let mut state = browsing(&["shop"], 0);
        state.apply_filter(Filtered::Tables, Filter::new("or"));

        let buffer = draw_state(40, 12, &state);
        let border = row(&buffer, 0);

        assert!(border.starts_with("╭─ Tables  /or ╮"), "got {border:?}");
    }

    #[test]
    fn a_border_wide_enough_names_both_tabs_and_fades_the_one_not_drawn() {
        // 120 columns puts the sidebar at 30, which holds the whole strip.
        let buffer = draw_state(120, 41, &describing(orders()));
        let border = row(&buffer, top_border(&buffer, "DDL").1);

        assert!(border.contains("DDL - Columns"), "got {border:?}");

        // Past its 'C', which wears the shortcut colour whether or not its tab
        // is the one drawn.
        let (fg, _, modifier) = style_of_first(&buffer, "olumns");
        let faint = Theme::default().style(Token::Faint);

        assert_eq!(fg, faint.fg.expect("faint spends a colour"));
        assert_eq!(modifier, faint.add_modifier, "the tab is not the one drawn");
    }

    #[test]
    fn a_sidebar_on_its_usual_width_still_names_both_tabs() {
        // At 100 columns the sidebar is 25, and the strip needs 13 of the 21
        // a border that wide has to spend on a title.
        let buffer = draw_state(104, 41, &listing(orders()));
        let border = row(&buffer, top_border(&buffer, "DDL").1);

        assert!(border.contains("DDL - Columns"), "got {border:?}");
    }

    #[test]
    fn a_border_too_narrow_for_either_strip_keeps_the_tab_it_is_drawing_whole() {
        // At 40 columns the sidebar is on its 16-column floor, which the
        // strip does not fit. A clipped one would name a tab that does not
        // exist.
        let buffer = draw_state(40, 12, &listing(orders()));
        let border = row(&buffer, top_border(&buffer, "Columns").1);

        assert!(border.contains("Columns"), "got {border:?}");
        assert!(!border.contains("DDL"), "got {border:?}");
    }

    #[test]
    fn a_selected_tables_shape_is_drawn_above_its_options() {
        let buffer = draw_state(104, 41, &describing(orders()));

        assert_eq!(
            ddl_rows(&buffer)[..15],
            [
                // The cursor opens on this row, which is where the bar is.
                "▌\u{f084} Keyspace         shop",
                " \u{f0ce} Table",
                "     orders_by_customer",
                // The blank row between what the panel is about and the shape
                // of what it is about.
                "",
                // A heading opens at the margin: it is no cursor stop, so it
                // keeps the column every row spends on the bar.
                "\u{f030b} Partition Key",
                "   customer_id      uuid",
                "\u{f030b} Clustering Key",
                "   order_date ↓     date",
                "   order_id ↑   timeuuid",
                // And the one splitting what the table is from what it is
                // configured to do.
                "",
                "\u{f013} Options",
                " gc_grace_seconds 864000",
                // `bloom_filter_fp_chance` and its value need 26 of the
                // body's 23, so it keeps the wrapped shape.
                " bloom_filter_fp_chance",
                "   0.01",
                " comment  Order history.",
            ]
        );
        assert!(
            ddl_rows(&buffer)[15..].iter().all(String::is_empty),
            "`comment` sorts last, and the panel has room to spare beneath it"
        );
    }

    /// Every row inside the Ddl body (104x41, see [`ddl_rows`]) whose first
    /// content cell carries the background `Token::Selected` gives.
    fn highlighted_rows(buffer: &Buffer) -> Vec<u16> {
        let highlight = Theme::default()
            .style(Token::Selected)
            .bg
            .expect("the cursor row spends a background");

        (DDL_BODY..DDL_END)
            .filter(|&y| buffer.cell((1, y)).is_some_and(|cell| cell.bg == highlight))
            .collect()
    }

    /// The text a Ddl body row drew, at the same coordinates [`ddl_rows`]
    /// reads.
    fn row_text(buffer: &Buffer, row: u16) -> String {
        (1..25)
            .filter_map(|x| buffer.cell((x, row)))
            .map(Cell::symbol)
            .collect::<String>()
            .trim_end()
            .to_owned()
    }

    #[test]
    fn every_line_of_a_selected_property_is_highlighted() {
        let mut state = describing(orders());
        state.focus = Panel::Ddl;
        // `bloom_filter_fp_chance` and its value need 26 of the body's 23
        // columns, so this row is one cursor stop drawn across two lines. Six
        // rows in: the keyspace, the table, one partition and two clustering
        // columns, then `gc_grace_seconds`.
        state.ddl.cursor.go_to(6, lc_domain::view::ddl_rows(&state));

        let buffer = draw_state(104, 41, &state);

        assert_eq!(
            highlighted_rows(&buffer),
            [29, 30],
            "a wrapped value is one cursor stop, so exactly its own two lines \
             are the selection — not fewer, and not bleeding into comment below"
        );
    }

    #[test]
    fn a_one_line_property_has_exactly_its_own_line_reversed() {
        let mut state = describing(orders());
        state.focus = Panel::Ddl;
        state.ddl.cursor.go_to(5, lc_domain::view::ddl_rows(&state));

        let buffer = draw_state(104, 41, &state);

        assert_eq!(
            highlighted_rows(&buffer),
            [28],
            "gc_grace_seconds fits one line, so exactly it is the selection"
        );
    }

    /// The Tables tree's mark and the Contents grid's, in the third panel that
    /// carries a cursor: a wrapped row wears it on every line it wrapped to,
    /// since the highlight covers every one.
    #[test]
    fn the_ddl_body_reserves_a_column_for_the_selection_bar() {
        let mut state = describing(orders());
        state.focus = Panel::Ddl;
        // `bloom_filter_fp_chance` and its value need more than the body's 23
        // columns, so this row is one cursor stop drawn across two lines.
        state.ddl.cursor.go_to(6, lc_domain::view::ddl_rows(&state));

        let buffer = draw_state(104, 41, &state);
        let bar = |y: u16| buffer.cell((1, y)).expect("inside the panel").symbol();

        assert_eq!(
            highlighted_rows(&buffer),
            [29, 30],
            "the fixture's two lines"
        );
        assert_eq!(bar(29), "▌");
        assert_eq!(bar(30), "▌");
        // The rows of the shape and the one option above the cursor's: each
        // pays the column and marks nothing. 20 and 26 are the blanks between
        // the groups, and 21, 23 and 27 are headings — no cursor stop among
        // them, so none of them pays it.
        for y in [17, 18, 19, 22, 24, 25, 28] {
            assert_eq!(bar(y), " ", "row {y} is a row the cursor is not on");
        }
        assert_eq!(
            bar(27),
            crate::icons::SETTINGS.trim_end(),
            "a heading opens at the margin, in the column a row spends on the bar"
        );
    }

    #[test]
    fn a_section_heading_is_never_drawn_as_the_selection() {
        let mut state = describing(orders());
        state.focus = Panel::Ddl;
        state.ddl.cursor.to_start();

        let buffer = draw_state(104, 41, &state);
        let highlighted = highlighted_rows(&buffer);

        assert!(
            !highlighted.is_empty(),
            "the first row must be drawn as the selection"
        );
        for row in highlighted {
            let text = row_text(&buffer, row);
            assert!(
                !text.ends_with("Partition Key") && !text.ends_with("Options"),
                "row {row} is a heading: {text:?}"
            );
        }
    }

    #[test]
    fn a_ranged_ddl_row_carries_the_bar_and_the_band() {
        let mut state = describing(orders());
        state.focus = Panel::Ddl;
        state.ddl.cursor.to_start();
        // Row 1 is the table identity row, which wraps to two lines
        // (`a_selected_tables_shape_is_drawn_above_its_options`); row 2 is
        // `customer_id`, outside the range.
        state.ddl.selection = Selection::held(0, 1);

        let buffer = draw_state(100, 41, &state);
        let rows = ddl_rows(&buffer);

        assert!(
            rows[1].starts_with('▌'),
            "a row the range covers is marked, as it is in the tree and the grid, got {:?}",
            rows[1]
        );
        let outside = rows
            .iter()
            .find(|line| line.contains("customer_id"))
            .expect("the fixture describes a customer_id column");
        assert!(
            outside.starts_with(' '),
            "and a row outside the range is not, got {outside:?}"
        );
    }

    #[test]
    fn a_selected_block_scrolled_fully_out_of_view_is_not_highlighted() {
        // The Ddl half of `a_selection_scrolled_out_of_view_is_not_highlighted`
        // and its tree counterpart: `details.rs` argues in prose that
        // `Paragraph` needs no such guard because it only ever draws the lines
        // it is scrolled to. This is what checks that argument.
        let mut state = describing(orders());
        state.focus = Panel::Ddl;
        let details = ddl_details(&state);
        state.ddl.cursor.go_to(0, details.row_count());

        // 40x20: a fourteen-column, nine-row Ddl body — narrow enough that
        // `orders()` wraps well past it.
        let total = crate::details::lay_out(&details, 0, Marked::none(), 14, Theme::default())
            .lines
            .len();
        assert!(
            total > 9,
            "the fixture must overflow the panel or this test proves nothing"
        );
        state.ddl.scroll.to_end(total, 9);

        let highlight = Theme::default()
            .style(Token::Selected)
            .bg
            .expect("the cursor row spends a background");
        let buffer = draw_state(40, 20, &state);
        let selected =
            (9..18).any(|y| buffer.cell((1, y)).expect("inside the panel").bg == highlight);

        assert!(
            !selected,
            "customer_id scrolled off the top must not still be drawn as the selection"
        );
    }

    #[test]
    fn a_selected_block_scrolled_partly_out_of_view_keeps_its_visible_lines_highlighted() {
        let mut state = describing(orders());
        state.focus = Panel::Ddl;
        let details = ddl_details(&state);
        let last = details.row_count() - 1;
        state.ddl.cursor.go_to(last, details.row_count());

        // `comment` is the last row and wraps to three lines at this width.
        // Scrolled until its first line is the eighth of the nine visible, two
        // of them are in view and the third is below the fold.
        let laid = crate::details::lay_out(&details, last, Marked::none(), 14, Theme::default());
        let total = laid.lines.len();
        assert_eq!(laid.selection.lines, 3, "comment wraps to three lines");
        for _ in 0..laid.selection.first - 7 {
            state.ddl.scroll.down(total, 9);
        }

        let highlight = Theme::default()
            .style(Token::Selected)
            .bg
            .expect("the cursor row spends a background");
        let buffer = draw_state(40, 20, &state);
        let selected = |y: u16| buffer.cell((1, y)).expect("inside the panel").bg == highlight;

        assert!(!selected(13), "this line belongs to the row above comment");
        assert!(!selected(14), "this line belongs to the row above comment");
        assert!(!selected(15), "this line belongs to the row above comment");
        assert!(
            selected(16),
            "comment's first visible line stays highlighted"
        );
        assert!(
            selected(17),
            "comment's second visible line stays highlighted"
        );
    }

    /// The cursor counts the rows the filter *left*, and the span the frame
    /// reports has to index the same sequence — `details::of` retains before
    /// it returns, so one projection feeds both the body drawn here and the
    /// clamp in the domain. Pulling the retain back out would move the two
    /// apart with nothing else complaining.
    #[test]
    fn the_reported_span_indexes_the_rows_a_filter_left() {
        let mut state = describing(orders());
        state.focus = Panel::Ddl;
        // The table's own name, `order_date` and `order_id` by name, and
        // `comment` by its value; the keyspace and the partition key's column
        // miss, so the partition section goes with it.
        state.apply_filter(Filtered::Ddl, Filter::new("or"));
        let rows = lc_domain::view::ddl_rows(&state);
        // Index 3 is `comment`, the last of the four — spelt out rather than
        // taken from `rows`, so a `rows` that stopped counting the filtered
        // sequence moves the assertions below instead of the cursor.
        state.ddl.cursor.go_to(3, rows);

        let span = geometry_of(104, 41, &state).ddl_selection();
        let buffer = draw_state(104, 41, &state);

        // The body's first line is drawn at `DDL_BODY` (see `ddl_rows`), and the
        // panel is not scrolled, so a span index is a row offset from there.
        let row = DDL_BODY + u16::try_from(span.first).expect("a short body");

        assert_eq!(span.lines, 1, "comment fits one line at this width");
        assert_eq!(
            row_text(&buffer, row),
            "▌comment  Order history.",
            "the last surviving row, not the third row of the unfiltered list"
        );
        assert_eq!(
            highlighted_rows(&buffer),
            [row],
            "and it is the row the frame actually highlighted"
        );
        assert_eq!(
            rows, 4,
            "and the count the domain clamps against is the same sequence"
        );
    }

    #[test]
    fn the_key_columns_are_drawn_in_the_tokens_that_mark_them_as_the_key() {
        // The cursor sits on the last row, off the key columns entirely: the
        // rows under test must read their own token colour, not
        // `Token::Selected`'s flattened highlight text.
        let mut state = describing(orders());
        let details = ddl_details(&state);
        state
            .ddl
            .cursor
            .go_to(details.row_count() - 1, details.row_count());

        let buffer = draw_state(104, 40, &state);
        // Past the bar's column: a property line carries its colour on its
        // spans rather than on the line, and the bar's own span is a blank.
        let colour = |y: u16| buffer.cell((2, y)).expect("inside the panel").fg;
        let theme = Theme::default();
        let partition = theme
            .style(Token::PartitionKey)
            .fg
            .expect("the partition key spends a colour");
        let clustering = theme
            .style(Token::ClusteringKey)
            .fg
            .expect("the clustering key spends a colour");

        assert_eq!(colour(21), partition, "Partition Key");
        assert_eq!(colour(22), partition, "its column");
        assert_eq!(colour(23), clustering, "Clustering Key");
        assert_eq!(colour(24), clustering, "its column");
        // 26 is the blank row the settings group opens with.
        assert_eq!(
            colour(27),
            theme
                .style(Token::Section)
                .fg
                .expect("a section heading spends a colour"),
            "Options names a group rather than a schema concept"
        );
        assert_eq!(
            colour(28),
            theme
                .style(Token::PropertyName)
                .fg
                .expect("a property name is highlighted"),
            "and its rows keep their own colours rather than the heading's"
        );
    }

    #[test]
    fn a_selected_keyspace_is_described_by_its_replication() {
        let mut state = describing(orders());
        let schema = state.schema().cloned().expect("loaded");
        state.tables.close(&schema);

        let buffer = draw_state(104, 40, &state);

        let rows: Vec<String> = ddl_rows(&buffer)[..4]
            .iter()
            .map(|row| row.trim_start().to_owned())
            .collect();

        assert_eq!(
            rows,
            [
                "▌\u{f084} Keyspace         shop",
                "",
                "\u{f013} Replication",
                "class    SimpleStrategy"
            ],
            "no table is selected, so the panel names only the keyspace"
        );
    }

    #[test]
    fn the_ddl_panel_is_empty_until_something_is_selected() {
        let buffer = draw_state(104, 41, &State::default());

        assert!(
            ddl_rows(&buffer).iter().all(String::is_empty),
            "the Tables panel already says why there is nothing; twice is noise"
        );
    }

    /// The `Details` the Ddl panel would draw for `state`.
    fn ddl_details(state: &State) -> lc_domain::view::Details {
        let PanelBody::Details { details, .. } = view_model(state).panels[1].body.clone() else {
            panic!("a selected row has something to describe");
        };
        details
    }

    fn geometry_of(width: u16, height: u16, state: &State) -> Geometry {
        let mut terminal =
            Terminal::new(TestBackend::new(width, height)).expect("TestBackend always initialises");
        let mut geometry = Geometry::default();

        terminal
            .draw(|frame| {
                geometry = render(frame, &view_model(state), Theme::default()).geometry;
            })
            .expect("TestBackend always draws");

        geometry
    }

    #[test]
    fn the_frame_reports_what_each_panel_can_show() {
        // At 40x20: a 16-column sidebar, Tables 8 rows tall and Ddl 11, so
        // six and nine rows of content inside their borders.
        let geometry = geometry_of(40, 20, &describing(orders()));

        assert_eq!(geometry.height(Panel::Tables), 6);
        assert_eq!(geometry.height(Panel::Ddl), 9);
        assert_eq!(
            geometry.ddl_lines(),
            crate::details::lay_out(
                &ddl_details(&describing(orders())),
                0,
                Marked::none(),
                14,
                Theme::default()
            )
            .lines
            .len()
        );
    }

    #[test]
    fn a_grid_reports_the_rows_it_can_show_rather_than_its_height() {
        let state = showing(page_of(&["id"], &[&[CqlValue::Int(1)]]));

        let geometry = geometry_of(60, 12, &state);

        // The Contents panel is 8 rows tall at this size: 6 inside its
        // border, one of which the header takes.
        assert_eq!(
            geometry.height(Panel::Contents),
            5,
            "a page motion must not count the header as a row it can land on"
        );
    }

    #[test]
    fn the_frame_reports_how_wide_the_contents_panel_is() {
        // The Contents block is 44 columns of the 60 at this size, so 42
        // inside its borders.
        let state = showing(page_of(&["id"], &[&[CqlValue::Int(1)]]));

        assert_eq!(geometry_of(60, 12, &state).grid_width(), 42);
        assert_eq!(
            geometry_of(60, 12, &State::default()).grid_width(),
            42,
            "the width is the panel's, and the domain needs it before a page arrives"
        );
    }

    #[test]
    fn a_collapsed_panel_reports_no_room_at_all() {
        let geometry = geometry_of(40, 5, &describing(orders()));

        assert_eq!(geometry.height(Panel::Ddl), 0);
        assert_eq!(geometry.height(Panel::Queries), 0);
    }

    #[test]
    fn the_ddl_panel_scrolls_to_the_lines_below_its_fold() {
        // At 40x20 the sidebar is 16 columns and the Ddl panel is 7 rows, so
        // its body is fourteen columns of five rows — and `orders()` wraps to
        // more than five lines in that.
        const WIDTH: usize = 14;
        const HEIGHT: usize = 5;

        let state = describing(orders());
        let all = crate::details::lay_out(
            &ddl_details(&state),
            0,
            Marked::none(),
            WIDTH,
            Theme::default(),
        )
        .lines;
        assert!(
            all.len() > HEIGHT,
            "the fixture must overflow the panel or this test proves nothing"
        );
        let tail = all.last().expect("the body has lines").to_string();
        assert!(
            !tail.trim().is_empty(),
            "the assertions below need a tail to look for"
        );

        let mut scrolled = describing(orders());
        scrolled.ddl.scroll.to_end(all.len(), HEIGHT);

        let before = draw_state(40, 20, &state);
        let after = draw_state(40, 20, &scrolled);
        // Inside the Ddl panel only: rows 13..17, columns 1..14. A full-width
        // row would also carry whatever the Contents panel is saying.
        let visible = |buffer: &Buffer| -> Vec<String> {
            (13..18)
                .map(|y| {
                    (1..15)
                        .filter_map(|x| buffer.cell((x, y)))
                        .map(Cell::symbol)
                        .collect::<String>()
                })
                .collect()
        };

        assert!(
            !visible(&before)
                .iter()
                .any(|line| line.contains(tail.trim())),
            "the tail starts below the fold"
        );
        assert!(
            visible(&after)
                .iter()
                .any(|line| line.contains(tail.trim())),
            "scrolling to the end must bring it into view"
        );
    }

    /// The Tables panel's right border at 40x12: column 15, between the
    /// corners at rows 0 and 6.
    fn tables_border(buffer: &Buffer) -> String {
        (1..4)
            .filter_map(|y| buffer.cell((15, y)))
            .map(Cell::symbol)
            .collect()
    }

    #[test]
    fn a_panel_whose_content_overflows_grows_a_bar_on_its_border() {
        let names: Vec<String> = (0..12).map(|i| format!("ks{i:02}")).collect();
        let refs: Vec<&str> = names.iter().map(String::as_str).collect();

        let buffer = draw_state(40, 12, &browsing(&refs, 0));

        assert!(
            tables_border(&buffer).contains(THUMB),
            "the thumb says how far through twelve keyspaces three rows are"
        );
    }

    #[test]
    fn the_thumb_sits_at_whichever_end_of_the_track_the_viewport_is_on() {
        let names: Vec<String> = (0..12).map(|i| format!("ks{i:02}")).collect();
        let refs: Vec<&str> = names.iter().map(String::as_str).collect();

        assert!(
            tables_border(&draw_state(40, 12, &browsing(&refs, 0))).starts_with(THUMB),
            "the first keyspace is on screen, so the thumb belongs at the top"
        );
        assert!(
            tables_border(&draw_state(40, 12, &browsing(&refs, 11))).ends_with(THUMB),
            "the last keyspace is on screen, so the thumb belongs at the bottom"
        );
    }

    #[test]
    fn the_track_is_the_border_itself_rather_than_a_second_bar_beside_it() {
        let names: Vec<String> = (0..12).map(|i| format!("ks{i:02}")).collect();
        let refs: Vec<&str> = names.iter().map(String::as_str).collect();

        // Scrolled to the top, so the thumb is at the top and the rest of the
        // column is track. `Scrollbar`'s default set would draw `║` here.
        let border = tables_border(&draw_state(40, 12, &browsing(&refs, 0)));
        let track: String = border.chars().skip_while(|c| *c == '▐').collect();

        assert!(!track.is_empty(), "twelve keyspaces do not fit three rows");
        assert!(
            track.chars().all(|c| c == '│'),
            "the track has to be the border glyph, or the panel grows a second \
             border beside the first: {track:?}"
        );
    }

    #[test]
    fn the_bar_takes_the_border_column_rather_than_a_column_of_content() {
        let names: Vec<String> = (0..12).map(|i| format!("ks{i:02}")).collect();
        let refs: Vec<&str> = names.iter().map(String::as_str).collect();

        let buffer = draw_state(40, 12, &browsing(&refs, 0));

        assert_eq!(
            row(&buffer, 1),
            "│▌\u{f084} ks00       ▐│Press Enter on a table│",
            "the row is as wide as it is without a scrollbar; the scrollbar replaced the border"
        );
    }

    #[test]
    fn what_the_frame_measures_does_not_depend_on_where_it_is_scrolled() {
        // Both termination guards on the render→domain edge rest on this:
        // `lc-app` sends only when the geometry changed, and `update` marks
        // dirty only when the clamp moved the offset. A `Geometry` that varied
        // with the offset it bounds would close the loop
        // (docs/superpowers/specs/2026-08-09-panel-scrolling-design.md §3).
        let state = describing(orders());
        let mut scrolled = describing(orders());
        scrolled.ddl.scroll.to_end(100, 6);

        assert_eq!(geometry_of(40, 20, &state), geometry_of(40, 20, &scrolled));
    }

    #[test]
    fn a_panel_whose_content_fits_keeps_its_border_intact() {
        let buffer = draw_state(40, 12, &browsing(&["shop", "typezoo"], 0));

        assert_eq!(
            tables_border(&buffer),
            "│││",
            "nothing is hidden, so nothing says otherwise"
        );
    }

    #[test]
    fn the_selection_stays_visible_when_the_list_outgrows_the_panel() {
        // Three rows of content fit in a five-row Tables panel at this size;
        // twelve keyspaces force the scroll a plain `List` (no `ListState`)
        // would silently fail to perform.
        let names: Vec<String> = (0..12).map(|i| format!("ks{i:02}")).collect();
        let refs: Vec<&str> = names.iter().map(String::as_str).collect();

        let buffer = draw_state(40, 12, &browsing(&refs, 11));
        let rows: Vec<String> = (1..4).map(|y| row(&buffer, y)).collect();

        assert!(
            rows.iter().any(|line| line.contains("ks11")),
            "the selected row must stay on screen once the list scrolls"
        );
        assert!(
            !rows.iter().any(|line| line.contains("ks00")),
            "a row scrolled out of view must not still be drawn"
        );
    }

    fn draw_mapped(width: u16, height: u16, state: &State) -> (Buffer, FrameMap) {
        let mut terminal =
            Terminal::new(TestBackend::new(width, height)).expect("TestBackend always initialises");
        let mut map = FrameMap::default();

        terminal
            .draw(|frame| {
                map = render(frame, &view_model(state), Theme::default()).map;
            })
            .expect("TestBackend always draws");

        (terminal.backend().buffer().clone(), map)
    }

    #[test]
    fn the_map_resolves_every_drawn_keyspace_to_the_row_that_drew_it() {
        let names = ["shop", "typezoo", "system"];
        let (buffer, map) = draw_mapped(40, 12, &browsing(&names, 0));

        for (index, name) in names.iter().enumerate() {
            let y = (1..7)
                .find(|&y| row(&buffer, y).contains(name))
                .unwrap_or_else(|| panic!("{name} was not drawn"));

            assert_eq!(
                map.hit(2, y),
                Some(Click::Row {
                    panel: Panel::Tables,
                    row: index,
                    column: None
                }),
                "{name} was drawn on screen row {y}"
            );
        }
    }

    #[test]
    fn the_map_resolves_a_scrolled_tree_to_the_rows_that_were_drawn() {
        // Twelve keyspaces in a panel that holds five rows forces the tree to
        // scroll, so the stored offset is not zero and the row under the
        // pointer is not the row of the same index.
        let names: Vec<String> = (0..12).map(|i| format!("ks{i:02}")).collect();
        let refs: Vec<&str> = names.iter().map(String::as_str).collect();

        let (buffer, map) = draw_mapped(40, 12, &browsing(&refs, 11));
        let mut resolved = 0;

        for (index, name) in names.iter().enumerate() {
            let Some(y) = (1..7).find(|&y| row(&buffer, y).contains(name.as_str())) else {
                continue;
            };

            assert_eq!(
                map.hit(2, y),
                Some(Click::Row {
                    panel: Panel::Tables,
                    row: index,
                    column: None
                }),
                "{name} was drawn on screen row {y}"
            );
            resolved += 1;
        }

        assert!(
            resolved >= 2,
            "the test proves nothing unless rows were on screen, got {resolved}"
        );
    }

    #[test]
    fn the_map_resolves_a_scrolled_grid_to_the_rows_that_were_drawn() {
        // Twenty rows in a panel that holds five forces the grid to scroll, so
        // the row under the pointer is not the row of the same index.
        let labels: Vec<String> = (0..20).map(|i| format!("row{i:02}")).collect();
        let values: Vec<Vec<CqlValue>> = labels.iter().map(|l| vec![text(l)]).collect();
        let rows: Vec<&[CqlValue]> = values.iter().map(Vec::as_slice).collect();
        let mut state = showing(page_of(&["id"], &rows));
        select_contents(&mut state, 19, 5);

        let (buffer, map) = draw_mapped(60, 12, &state);
        let mut resolved = 0;

        for (index, label) in labels.iter().enumerate() {
            let Some(y) = (1..8).find(|&y| row(&buffer, y).contains(label.as_str())) else {
                continue;
            };

            assert_eq!(
                map.hit(20, y),
                Some(Click::Row {
                    panel: Panel::Contents,
                    row: index,
                    column: None
                }),
                "{label} was drawn on screen row {y}"
            );
            resolved += 1;
        }

        assert!(
            resolved >= 2,
            "the test proves nothing unless rows were on screen, got {resolved}"
        );
    }

    /// The x of the first cell of screen row `y` holding `text`.
    fn column_of(buffer: &Buffer, y: u16, text: &str) -> u16 {
        let line = row(buffer, y);
        let at = line
            .find(text)
            .unwrap_or_else(|| panic!("{text} was not drawn on row {y}: {line:?}"));

        u16::try_from(line[..at].chars().count()).expect("a frame is narrower than u16::MAX")
    }

    /// A two-column grid. Every value is unique and none is a row number, so
    /// finding one in the frame finds the cell that drew it rather than the
    /// gutter beside it.
    fn grid_state() -> State {
        showing(page_of(
            &["id", "email"],
            &[
                &[text("id-ada"), text("ada@example.com")],
                &[text("id-bob"), text("bob@example.com")],
            ],
        ))
    }

    #[test]
    fn a_grids_heading_names_a_column_rather_than_its_first_row() {
        let (buffer, map) = draw_mapped(60, 12, &grid_state());

        assert!(
            row(&buffer, 1).contains("id") && row(&buffer, 1).contains("email"),
            "row 1 must be the header row this test is about"
        );
        assert!(
            row(&buffer, 2).contains("ada@example.com"),
            "row 2 must carry the first row's value, or `row: 0` below proves nothing"
        );

        assert_eq!(
            map.hit(column_of(&buffer, 1, "email"), 1),
            Some(Click::Column {
                panel: Panel::Contents,
                column: 1
            }),
            "a heading is the column it names"
        );
        assert_eq!(
            map.hit(column_of(&buffer, 2, "ada@example.com"), 2),
            Some(Click::Row {
                panel: Panel::Contents,
                row: 0,
                column: Some(1)
            })
        );
        assert_eq!(
            map.hit(20, 4),
            Some(Click::Panel(Panel::Contents)),
            "below the last row is blank space, not a row"
        );
    }

    #[test]
    fn the_map_resolves_every_cell_to_the_column_that_drew_it() {
        let (buffer, map) = draw_mapped(60, 12, &grid_state());

        for (column, value) in ["id-ada", "ada@example.com"].into_iter().enumerate() {
            assert_eq!(
                map.hit(column_of(&buffer, 2, value), 2),
                Some(Click::Row {
                    panel: Panel::Contents,
                    row: 0,
                    column: Some(column)
                }),
                "{value} was drawn in column {column}"
            );
        }
    }

    #[test]
    fn a_press_on_the_chrome_of_a_panel_names_that_panel() {
        let (_buffer, map) = draw_mapped(40, 12, &browsing(&["shop"], 0));

        assert_eq!(map.hit(0, 0), Some(Click::Panel(Panel::Tables)), "corner");
        assert_eq!(
            map.hit(0, 8),
            Some(Click::Panel(Panel::Ddl)),
            "the Ddl panel's left border"
        );
        assert_eq!(
            map.hit(20, 10),
            Some(Click::Panel(Panel::Queries)),
            "the Queries editor takes no press on its lines"
        );
    }

    #[test]
    fn the_map_resolves_every_drawn_tab_to_the_key_that_reaches_it() {
        let (buffer, map) = draw_mapped(104, 41, &listing(orders()));
        let strip = row(&buffer, DDL_BORDER);

        assert!(
            strip.contains("DDL") && strip.contains("Columns"),
            "row {DDL_BORDER} must be the strip this test is about: {strip:?}"
        );

        for (label, shortcut) in [("DDL", 'd'), ("Columns", 'c')] {
            assert_eq!(
                map.hit(column_of(&buffer, DDL_BORDER, label), DDL_BORDER),
                Some(Click::Tab {
                    panel: Panel::Ddl,
                    shortcut
                }),
                "{label} is reached by {shortcut}, whichever name it is drawn under"
            );
        }
    }

    #[test]
    fn a_single_tab_panel_resolves_its_title_to_its_own_key() {
        let (buffer, map) = draw_mapped(40, 12, &browsing(&["shop"], 0));

        assert_eq!(
            map.hit(column_of(&buffer, 0, "Tables"), 0),
            Some(Click::Tab {
                panel: Panel::Tables,
                shortcut: 'a'
            }),
            "a panel with one tab is its own tab, and `a` is how it is reached"
        );
    }

    #[test]
    fn the_map_resolves_every_line_of_a_wrapped_ddl_row_to_that_row() {
        // The Columns tab of `orders()`: the Table identity row is too wide
        // for 23 columns and wraps onto a second line, and the two rows before
        // the first column are a blank and a heading (see
        // `the_columns_tab_draws_every_column_with_its_kind_and_type`).
        let (buffer, map) = draw_mapped(104, 41, &listing(orders()));

        assert_eq!(
            ddl_rows(&buffer)[..6],
            [
                // The cursor opens on this row, which is where the bar is.
                "▌\u{f084} Keyspace         shop",
                " \u{f0ce} Table",
                "     orders_by_customer",
                "",
                "3 columns",
                " \u{f030b} customer_id      uuid",
            ],
            "the lines this test resolves against"
        );

        let expected = [Some(0), Some(1), Some(1), None, None, Some(2)];

        for (line, row) in expected.into_iter().enumerate() {
            let y = DDL_BORDER + 1 + u16::try_from(line).expect("six lines");
            let click = match row {
                Some(row) => Click::Row {
                    panel: Panel::Ddl,
                    row,
                    column: None,
                },
                None => Click::Panel(Panel::Ddl),
            };

            assert_eq!(map.hit(3, y), Some(click), "line {line}");
        }
    }

    #[test]
    fn the_hint_bar_and_a_frame_below_the_minimum_are_not_clickable() {
        let (_buffer, map) = draw_mapped(40, 12, &browsing(&["shop"], 0));
        assert_eq!(map.hit(5, 11), None, "the bar owns the bottom row");

        let (_buffer, tiny) = draw_mapped(39, 6, &State::default());
        assert_eq!(tiny.hit(0, 0), None, "nothing is drawn, so nothing is hit");
        assert_eq!(tiny.hit(20, 3), None);
    }

    /// A state focused on `focus`, with `text` in the Queries buffer.
    fn with_query(text: &str, focus: Panel) -> State {
        let mut state = State {
            focus,
            ..State::default()
        };
        for character in text.chars() {
            if character == '\n' {
                state.queries.insert_newline();
            } else {
                state.queries.insert(character);
            }
        }
        state
    }

    /// The drawn frame and where the terminal cursor ended up, if it is shown.
    fn drawn_with_cursor(
        width: u16,
        height: u16,
        state: &State,
    ) -> (Buffer, Option<ratatui::layout::Position>) {
        let mut terminal =
            Terminal::new(TestBackend::new(width, height)).expect("TestBackend always initialises");

        terminal
            .draw(|frame| {
                render(frame, &view_model(state), Theme::default());
            })
            .expect("TestBackend always draws");

        let backend = terminal.backend();
        let cursor = backend.cursor_visible().then(|| backend.cursor_position());

        (backend.buffer().clone(), cursor)
    }

    /// The Queries panel at 80x24 is `Rect { x: 20, y: 20, w: 60, h: 3 }`:
    /// borders on rows 20 and 22, its one content row on 21, and its inner
    /// left edge at column 21. Row 23 is split between the loader and the
    /// hint bar (`bar_split`), which does not matter here: the loader sits at
    /// the left and Queries starts at column 20.
    const CONTENT_ROW: u16 = 21;
    const INNER_LEFT: u16 = 21;

    #[test]
    fn the_editor_draws_what_has_been_typed() {
        let (buffer, _) = drawn_with_cursor(80, 24, &with_query("SELECT * FROM t", Panel::Queries));

        assert!(
            row(&buffer, CONTENT_ROW).contains("SELECT * FROM t"),
            "the buffer is on the panel's one content row: {:?}",
            row(&buffer, CONTENT_ROW)
        );
    }

    #[test]
    fn the_cursor_sits_where_the_domain_says_it_is() {
        let state = with_query("SELECT", Panel::Queries);
        let expected = state.queries.cursor();

        let (_, cursor) = drawn_with_cursor(80, 24, &state);

        let cursor = cursor.expect("a focused editor shows a cursor");
        assert_eq!(
            cursor.x,
            INNER_LEFT + u16::try_from(expected.column).expect("a test column fits"),
            "one column past the six typed"
        );
        assert_eq!(cursor.y, CONTENT_ROW);
    }

    #[test]
    fn an_unfocused_editor_sets_no_cursor() {
        let (_, cursor) = drawn_with_cursor(80, 24, &with_query("SELECT", Panel::Tables));

        assert_eq!(
            cursor, None,
            "a cursor in an unfocused panel would say it takes the keys"
        );
    }

    #[test]
    fn a_long_line_scrolls_sideways_and_keeps_the_cursor_on_screen() {
        let long = "SELECT * FROM keyspace_with_a_long_name.table_with_a_long_name WHERE id = 7";
        let state = with_query(long, Panel::Queries);

        let (buffer, cursor) = drawn_with_cursor(80, 24, &state);
        let drawn = row(&buffer, CONTENT_ROW);

        assert!(
            drawn.contains("WHERE id = 7"),
            "the cursor's end of the line is what is shown: {drawn:?}"
        );
        assert!(
            !drawn.contains("SELECT *"),
            "the start has scrolled off to the left: {drawn:?}"
        );
        let cursor = cursor.expect("a focused editor shows a cursor");
        assert!(
            cursor.x < 80,
            "the cursor stays inside the frame at {}",
            cursor.x
        );
    }

    #[test]
    fn a_tall_buffer_scrolls_and_says_so() {
        let state = with_query("one\ntwo\nthree\nfour", Panel::Queries);

        let (buffer, cursor) = drawn_with_cursor(80, 24, &state);
        let drawn = row(&buffer, CONTENT_ROW);

        assert!(
            drawn.contains("four"),
            "the cursor's line is the one drawn: {drawn:?}"
        );
        assert_eq!(
            cursor.expect("a focused editor shows a cursor").y,
            CONTENT_ROW,
            "the cursor's own row is on screen"
        );
        assert!(
            drawn.ends_with(THUMB),
            "four lines in a one-row panel grows the scrollbar: {drawn:?}"
        );
    }

    #[test]
    fn a_double_width_character_does_not_desync_the_cursor_from_the_scroll() {
        // Thirty two-column characters force a horizontal scroll whose
        // arithmetic offset lands inside one of them, not on a boundary
        // between characters.
        let long = format!("{}SELECT", "永".repeat(30));
        let state = with_query(&long, Panel::Queries);

        let (buffer, cursor) = drawn_with_cursor(80, 24, &state);
        let drawn = row(&buffer, CONTENT_ROW);

        assert!(
            drawn.contains("SELECT"),
            "the cursor's end of the line is what is shown: {drawn:?}"
        );
        assert_eq!(
            buffer.cell((INNER_LEFT, CONTENT_ROW)).map(Cell::symbol),
            Some("永"),
            "the drawn row begins on a whole character the offset names, not a half-dropped one: {drawn:?}"
        );

        let cursor = cursor.expect("a focused editor shows a cursor");
        assert_eq!(cursor.y, CONTENT_ROW);
        assert_eq!(
            buffer.cell((cursor.x - 1, CONTENT_ROW)).map(Cell::symbol),
            Some("T"),
            "the cursor sits one cell past the last character typed, not inside or short of it"
        );
    }

    #[test]
    fn an_overlay_hides_the_cursor_even_when_the_editor_is_focused() {
        let state = State {
            overlay: Some(Overlay::Message(Message {
                title: "Query failed".to_owned(),
                message: "boom".to_owned(),
                severity: Severity::Danger,
            })),
            ..with_query("SELECT", Panel::Queries)
        };

        let (_, cursor) = drawn_with_cursor(80, 24, &state);

        assert_eq!(
            cursor, None,
            "an overlay owns the frame; a cursor beneath it would blink over the modal at a position that means nothing there"
        );
    }

    #[test]
    fn an_open_prompts_own_cursor_replaces_the_editors() {
        // Renamed and its expectation updated by Task 9: a prompt used to
        // draw nothing of its own, so the editor's cursor being suppressed
        // meant no cursor showed at all. Now the prompt is a text field with
        // a cursor of its own, on the bar row — still not the editor's, for
        // the same reason as before (design §5.2).
        let state = State {
            overlay: Some(Overlay::Search(Prompt::opened(Filtered::Tables, None))),
            ..with_query("SELECT", Panel::Queries)
        };

        let (_, cursor) = drawn_with_cursor(80, 24, &state);

        // Row 23 is the bar; column 11 is the hint bar's start (9, past the
        // loader and its gap) plus the two cells of the prompt's empty query,
        // `" /"`.
        assert_eq!(
            cursor,
            Some(ratatui::layout::Position::new(11, 23)),
            "a prompt outranks focus exactly as a message or the palette does; \
             the cursor shown is the prompt's own, not the editor's"
        );
    }

    /// The colour, background and modifier of the first cell whose row
    /// contains `needle` — compared field by field rather than as a whole
    /// `Style`, because a `Cell` has no way to say a colour was never set
    /// versus set to the terminal's default, so a `Style` rebuilt from one
    /// cannot be asserted equal to the `Style` a `Theme` hands out.
    fn style_of_first(buffer: &Buffer, needle: &str) -> (Color, Color, Modifier) {
        for y in 0..buffer.area.height {
            let text = row(buffer, y);
            if let Some(byte_at) = text.find(needle) {
                // `str::find` returns a byte offset; the border glyphs before
                // it (`╭`, `─`, …) are multi-byte in UTF-8 but one column
                // each, so the column is the count of `char`s before it, not
                // the byte offset itself.
                let column = text[..byte_at].chars().count();
                let x = u16::try_from(column).expect("a drawn row's width fits u16");
                let cell = buffer.cell((x, y)).expect("a found position is in bounds");
                return (cell.fg, cell.bg, cell.modifier);
            }
        }
        panic!("{needle:?} was not drawn");
    }

    #[test]
    fn a_panel_wears_the_filter_applied_to_it() {
        let mut state = browsing(&["shop"], 0);
        state.apply_filter(Filtered::Tables, Filter::new("ord"));

        let buffer = draw_state(80, 24, &state);

        assert!(
            row(&buffer, top_border(&buffer, "Tables").1).contains("/ord"),
            "the term sits on the panel's own top border"
        );
    }

    #[test]
    fn the_marker_is_drawn_muted() {
        // On an unfocused panel: `Block` patches `border_style` under every
        // title span, focused Tables included (its own shortcut span already
        // reads BOLD from that, ambiently). Filtering Ddl instead, with the
        // default focus on Tables, isolates the marker's own style from that
        // ambient one.
        let mut state = browsing(&["shop"], 0);
        state.apply_filter(Filtered::Ddl, Filter::new("ord"));

        let buffer = draw_state(80, 24, &state);
        let (fg, _bg, modifier) = style_of_first(&buffer, "/ord");
        let expected = Theme::default().style(Token::Muted);

        assert_eq!(
            fg,
            expected.fg.expect("muted spends a colour"),
            "no colour override"
        );
        assert_eq!(expected.bg, None, "muted paints no background of its own");
        assert_eq!(modifier, expected.add_modifier, "no modifier override");
    }

    #[test]
    fn the_marker_is_dropped_rather_than_truncated() {
        let mut state = browsing(&["shop"], 0);
        state.apply_filter(
            Filtered::Tables,
            Filter::new("a_very_long_search_term_indeed"),
        );

        let buffer = draw_state(40, 24, &state);
        let border = row(&buffer, top_border(&buffer, "Tables").1);

        assert!(
            !border.contains('/'),
            "a truncated term is a lie about what is applied, got {border:?}"
        );
        assert!(border.contains("Tables"), "and the title survives");
    }

    #[test]
    fn the_marker_fits_flush_against_the_corner_it_is_tuned_for() {
        // At 40 columns Tables sits on its 16-column floor: 1 (`╭`) + 6
        // (`Tables`) + 4 (`/ord`) + 1 (`╮`) leaves exactly 4 columns, which is
        // what `TITLE_PAD` spends either side of each — so the two sit flush
        // with a gap and no dash between them, nothing is left over for an
        // inset, and one column narrower drops the marker.
        let mut state = browsing(&["shop"], 0);
        state.apply_filter(Filtered::Tables, Filter::new("ord"));

        let buffer = draw_state(40, 24, &state);
        let border = row(&buffer, top_border(&buffer, "Tables").1);

        assert_eq!(
            border, "╭ Tables  /ord ╮╭── Table contents ────╮",
            "the widest term this width can hold, sitting flush with no gap and no overlap"
        );
    }

    #[test]
    fn an_unfocused_panel_still_wears_its_marker() {
        // The marker's whole point: a filter that survives a focus change
        // has to say so from the panel it is applied to, not from the bar,
        // which only ever describes the focused panel (design §5.1).
        let mut state = State {
            focus: Panel::Contents,
            ..browsing(&["shop"], 0)
        };
        state.apply_filter(Filtered::Tables, Filter::new("ord"));

        let buffer = draw_state(80, 24, &state);

        assert!(
            row(&buffer, top_border(&buffer, "Tables").1).contains("/ord"),
            "Tables is unfocused but its own border still carries the marker"
        );
    }

    #[test]
    fn an_open_prompt_owns_the_bar_row() {
        let mut prompt = Prompt::opened(Filtered::Tables, None);
        prompt.push('o');
        prompt.push('r');
        let state = State {
            overlay: Some(Overlay::Search(prompt)),
            ..State::default()
        };

        let buffer = draw_state(80, 24, &state);
        let bar = row(&buffer, 23);

        assert!(bar.contains("/or"), "the query is on the bar: {bar:?}");
        assert!(
            bar.contains("cancel"),
            "and so are the prompt's own keys: {bar:?}"
        );
        assert!(
            !bar.contains("j/k move"),
            "the panel's own hints are gone while the prompt is up: {bar:?}"
        );
    }

    #[test]
    fn the_prompts_cursor_sits_after_the_query() {
        let mut prompt = Prompt::opened(Filtered::Tables, None);
        prompt.push('o');
        prompt.push('r');
        let state = State {
            overlay: Some(Overlay::Search(prompt)),
            ..State::default()
        };

        let (_, cursor) = drawn_with_cursor(80, 24, &state);

        // The bar row is 23 at this size. The hint bar itself starts at
        // column 9 — past the loader's 8 cells and `bar_split`'s 1-cell gap —
        // and the query's leading space plus its two characters put the
        // cursor 4 cells further on.
        assert_eq!(
            cursor,
            Some(ratatui::layout::Position::new(13, 23)),
            "one cell past the last character typed"
        );
    }

    #[test]
    fn the_prompts_cursor_stays_inside_the_frame_when_the_query_fills_the_bar() {
        let mut prompt = Prompt::opened(Filtered::Tables, None);
        for c in "a query long enough to overrun a narrow terminal's hint bar".chars() {
            prompt.push(c);
        }
        let state = State {
            overlay: Some(Overlay::Search(prompt)),
            ..State::default()
        };

        let (_, cursor) = drawn_with_cursor(40, 24, &state);
        let cursor = cursor.expect("the prompt draws a cursor");

        assert!(
            cursor.x < 40,
            "a query wide enough to fill the bar must not push the cursor \
             past the frame's right edge: {cursor:?}"
        );
    }

    #[test]
    fn the_prompts_query_carries_a_theme_token() {
        let mut prompt = Prompt::opened(Filtered::Tables, None);
        prompt.push('o');
        let state = State {
            overlay: Some(Overlay::Search(prompt)),
            ..State::default()
        };

        let buffer = draw_state(80, 24, &state);
        // Column 9: one past the loader and `bar_split`'s gap, landing on
        // the query's leading space.
        let cell = buffer.cell((9, 23)).expect("inside the bar row");
        let muted = Theme::default()
            .style(Token::HintAction)
            .fg
            .expect("the hint bar's action spends a colour");

        assert_eq!(
            cell.fg,
            muted,
            "the query must carry the same token the rest of the bar's text \
             does ({:?} was left bare)",
            cell.symbol()
        );
    }

    /// `text` above already names the `CqlValue` constructor the grid tests
    /// use; this is the same buffer-flattening helper `overlay::tests` keeps
    /// under that name, renamed to fit beside it.
    fn buffer_text(buffer: &Buffer) -> String {
        buffer.content().iter().map(Cell::symbol).collect()
    }

    fn with_palette(width: u16, height: u16) -> Buffer {
        let state = State {
            overlay: Some(Overlay::Commands(CommandsPanel::opened())),
            ..State::default()
        };
        let mut terminal =
            Terminal::new(TestBackend::new(width, height)).expect("TestBackend always initialises");

        terminal
            .draw(|frame| {
                render(frame, &view_model(&state), Theme::default());
            })
            .expect("TestBackend always draws");

        terminal.backend().buffer().clone()
    }

    /// The row a bordered block's top edge sits on, found by its left corner.
    ///
    /// The scan stops at the block's own right corner: panels sit side by
    /// side, so a row read to the frame's edge carries the titles of every
    /// block to the right of this one as well.
    fn top_border(buffer: &Buffer, expect: &str) -> (u16, u16) {
        for y in 0..buffer.area.height {
            for x in 0..buffer.area.width {
                if buffer.cell((x, y)).is_some_and(|cell| cell.symbol() == "╭") {
                    let mut row = String::new();
                    for at in x..buffer.area.width {
                        let Some(cell) = buffer.cell((at, y)) else {
                            break;
                        };
                        row.push_str(cell.symbol());
                        if cell.symbol() == "╮" {
                            break;
                        }
                    }
                    if row.contains(expect) {
                        return (x, y);
                    }
                }
            }
        }
        panic!("no block titled {expect:?} was drawn");
    }

    fn block_width(buffer: &Buffer, expect: &str) -> u16 {
        let (x, y) = top_border(buffer, expect);
        (x + 1..buffer.area.width)
            .find(|at| {
                buffer
                    .cell((*at, y))
                    .is_some_and(|cell| cell.symbol() == "╮")
            })
            .expect("a bordered block has a right corner")
            - x
            + 1
    }

    #[test]
    fn the_palette_lists_the_catalogue() {
        let drawn = buffer_text(&with_palette(100, 40));

        assert!(drawn.contains("Commands"), "the block is titled");
        // Strings only the palette can produce: the hint bar already draws
        // "q quit" along the bottom row, so asserting on "quit" would pass
        // with no palette at all.
        assert!(drawn.contains("Global"));
        assert!(drawn.contains("dismiss the overlay"));
        assert!(drawn.contains("focus Table contents"));
        assert!(drawn.contains("unfold the keyspace"));
    }

    #[test]
    fn the_selected_command_is_highlighted() {
        let buffer = with_palette(100, 40);
        let (x, y) = top_border(&buffer, "Commands");

        // Row 0 of the list is the `Global` heading, so the opened cursor sits
        // on row 1: one down for the border, one for the heading.
        let selected = y + 2;
        let highlight = Theme::default()
            .style(Token::Selected)
            .bg
            .expect("the cursor row spends a background");
        let styled = |at: u16| buffer.cell((x + 2, at)).expect("inside the block").bg == highlight;

        assert!(styled(selected), "the cursor's row is marked");
        assert!(!styled(selected + 1), "and only the cursor's row is");
    }

    /// The whole block titled `title`, borders included.
    fn block_area(buffer: &Buffer, title: &str) -> Rect {
        let (x, y) = top_border(buffer, title);
        let bottom = (y + 1..buffer.area.height)
            .find(|at| {
                buffer
                    .cell((x, *at))
                    .is_some_and(|cell| cell.symbol() == "╰")
            })
            .expect("a bordered block has a bottom edge");

        Rect::new(x, y, block_width(buffer, title), bottom - y + 1)
    }

    /// The `line`th row of the block titled `title`, inside its borders.
    ///
    /// Distinct from [`row_text`], which reads an absolute buffer row at the
    /// Ddl panel's fixed coordinates.
    fn block_row(buffer: &Buffer, title: &str, line: u16) -> String {
        let area = block_area(buffer, title);

        (area.x + 1..area.right() - 1)
            .filter_map(|x| buffer.cell((x, area.y + 1 + line)))
            .map(Cell::symbol)
            .collect()
    }

    /// Whether the block titled `title` has a scrollbar drawn over its right
    /// border. The track scan stops above the bottom border row: that row's
    /// corner is `┘`, which is `!= "│"` whether or not a scrollbar was drawn,
    /// and including it would make the scan true unconditionally.
    fn has_scrollbar(buffer: &Buffer, title: &str) -> bool {
        let area = block_area(buffer, title);
        let x = area.right() - 1;

        (area.y + 1..area.bottom() - 1).any(|y| {
            buffer
                .cell((x, y))
                .is_some_and(|cell| cell.symbol() != symbols::line::VERTICAL)
        })
    }

    /// Whether the block titled `title` has a horizontal scrollbar drawn over
    /// its bottom border. The corners stay `╰` and `╯` whether or not one is
    /// drawn, so the cells between them are what to look at: an untouched
    /// bottom border is all `─`.
    fn has_horizontal_scrollbar(buffer: &Buffer, title: &str) -> bool {
        let area = block_area(buffer, title);
        let y = area.bottom() - 1;

        (area.x + 1..area.right() - 1).any(|x| {
            buffer
                .cell((x, y))
                .is_some_and(|cell| cell.symbol() != symbols::line::HORIZONTAL)
        })
    }

    #[test]
    fn a_palette_that_overflows_says_so() {
        // 70% of 24 rows leaves 14 for a catalogue in the low twenties.
        assert!(
            has_scrollbar(&with_palette(100, 24), "Commands"),
            "a catalogue taller than the box gets a scrollbar"
        );

        // At 100x80 the whole catalogue fits (design §10), so there is no
        // scrollbar — and its absence is what says nothing is hidden.
        assert!(
            !has_scrollbar(&with_palette(100, 80), "Commands"),
            "a frame tall enough to hold the whole catalogue must draw no scrollbar"
        );
    }

    #[test]
    fn the_palette_is_centred() {
        let buffer = with_palette(100, 40);
        let (x, _) = top_border(&buffer, "Commands");
        let width = block_width(&buffer, "Commands");

        assert_eq!(x, (100 - width) / 2, "left margin equals right margin");
    }

    #[test]
    fn a_wide_frame_does_not_get_a_wide_palette() {
        assert_eq!(block_width(&with_palette(200, 40), "Commands"), 60);
        assert_eq!(block_width(&with_palette(100, 40), "Commands"), 50);
    }

    #[test]
    fn a_tall_frame_does_not_get_a_palette_full_of_blank_rows() {
        let buffer = with_palette(100, 80);
        let (x, y) = top_border(&buffer, "Commands");

        let bottom = (y + 1..buffer.area.height)
            .find(|at| {
                buffer
                    .cell((x, *at))
                    .is_some_and(|cell| cell.symbol() == "╰")
            })
            .expect("a bordered block has a bottom edge");

        assert_eq!(
            usize::from(bottom - y) + 1,
            lc_domain::commands::row_count() + 2,
            "the block ends where the catalogue does, not at 70% of 80 rows"
        );
    }

    #[test]
    fn a_frame_too_small_for_a_palette_does_not_get_one() {
        assert!(
            !buffer_text(&with_palette(60, 40)).contains("Commands"),
            "half of 60 columns is under the palette's floor"
        );
    }

    #[test]
    fn the_narrowest_palette_still_fits_its_widest_row() {
        // 50% of 78 columns lands exactly on MIN_W, the floor a wider action
        // would clip against if the margin were counted on one side only.
        let drawn = buffer_text(&with_palette(78, 40));

        assert!(
            drawn.contains("focus Table contents"),
            "the widest action must appear whole, not clipped to 19 characters"
        );
    }

    #[test]
    fn a_frame_that_would_clip_the_widest_row_gets_no_palette() {
        // 50% of 76 columns is 38, one under MIN_W. A gate that let this
        // through would draw a palette whose widest row is clipped by one
        // character rather than refusing it outright.
        let drawn = buffer_text(&with_palette(76, 40));

        assert!(!drawn.contains("Commands"), "the block must not be titled");
        assert!(
            !drawn.contains("Global"),
            "nor must the catalogue be listed"
        );
    }

    #[test]
    fn the_frame_reports_how_many_rows_a_palette_would_show() {
        let state = State::default();
        let mut terminal =
            Terminal::new(TestBackend::new(100, 24)).expect("TestBackend always initialises");
        let mut geometry = Geometry::default();

        terminal
            .draw(|frame| {
                geometry = render(frame, &view_model(&state), Theme::default()).geometry;
            })
            .expect("TestBackend always draws");

        // 70% of 24 rows is 16, less two borders — reported with no palette
        // open, because it is what gates `?` (design §5.1).
        assert_eq!(geometry.command_rows(), 14);
    }

    #[test]
    fn a_frame_too_small_for_a_palette_reports_no_rows() {
        let state = State::default();
        let mut terminal =
            Terminal::new(TestBackend::new(60, 24)).expect("TestBackend always initialises");
        let mut geometry = Geometry::default();

        terminal
            .draw(|frame| {
                geometry = render(frame, &view_model(&state), Theme::default()).geometry;
            })
            .expect("TestBackend always draws");

        assert_eq!(geometry.command_rows(), 0);
    }
}

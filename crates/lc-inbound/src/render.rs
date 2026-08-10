//! Drawing a frame.

use lc_domain::ViewModel;
use lc_domain::state::{Geometry, Panel};
use lc_domain::style::Token;
use lc_domain::view::OverlayView;
use ratatui::Frame;
use ratatui::layout::{Margin, Rect};
use ratatui::style::Style;
use ratatui::symbols;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Scrollbar, ScrollbarOrientation, ScrollbarState};

use crate::body::Extent;
use crate::hit::{FrameMap, PanelHit};
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
/// (doc/specs/2026-08-09-mouse-input-design.md §4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Rendered {
    pub geometry: Geometry,
    pub map: FrameMap,
}

/// Draw one titled block per panel, and report what the frame turned out to
/// be.
///
/// The return value is the domain's only way to learn how tall a panel is and
/// how many lines the Ddl body wrapped to
/// (doc/specs/2026-08-09-panel-scrolling-design.md §3).
pub fn render(frame: &mut Frame, view: &ViewModel, theme: Theme) -> Rendered {
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
    let mut hits = [PanelHit::default(); 4];

    for (index, panel) in view.panels.iter().enumerate() {
        let area = rects.get(panel.panel);

        let (before, shortcut, after) = panel.title.split();
        let title = Line::from(vec![
            Span::raw(before),
            Span::styled(shortcut, theme.style(Token::Shortcut)),
            Span::raw(after),
        ]);

        let mut block = Block::bordered()
            .border_type(BorderType::Rounded)
            .title(title);

        // Right-aligned on the top border, and dropped whole when the border
        // cannot hold both: a truncated term is a lie about what is applied
        // (design §5.1). Drawn on every panel that has one, focused or not —
        // the marker is the only place a filter that survives a focus change
        // still says so.
        if let Some(term) = &panel.filter {
            let marker = format!("/{term}");
            if marker_fits(area.width, panel.title.text(), &marker) {
                block = block.title(
                    Line::from(Span::styled(marker, theme.style(Token::Muted))).right_aligned(),
                );
            }
        }

        let border = if panel.panel == view.focus {
            theme.style(Token::Focused)
        } else {
            Style::new()
        };
        block = block.border_style(border);

        let inner = block.inner(area);
        frame.render_widget(block, area);

        // An overlay owns the frame: nothing beneath it shows a cursor, or it
        // blinks over the modal at a position that means nothing there.
        let focused = panel.panel == view.focus && view.overlay.is_none();

        let body = crate::body::render(frame, inner, &panel.body, focused, theme);

        match &body {
            Some(drawn) => {
                heights[index] = drawn.extent.visible;
                if panel.panel == Panel::Ddl {
                    ddl_lines = drawn.extent.total;
                }
                if drawn.extent.overflows() {
                    scrollbar(frame, area, &drawn.extent, border, theme);
                }
            }
            // `Blank` and `Notice` do not scroll, so what such a panel can
            // show is simply the room it was given.
            None => heights[index] = usize::from(inner.height),
        }

        hits[index] = PanelHit::new(panel.panel, area, inner, body.as_ref());
    }

    let bar = rects.bar();
    let (loader, hint_bar) = bar_split(bar);
    frame.render_widget(crate::loader::line(view.activity), loader);
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
        None => {}
    }

    Rendered {
        geometry: Geometry::new(heights, ddl_lines, command_rows),
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

/// Both titles and the two corners plus a space either side of each.
fn marker_fits(width: u16, title: &str, marker: &str) -> bool {
    let needed = title.chars().count() + marker.chars().count() + 6;
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
/// The track is the border glyph in the border's own style, so it repaints
/// what it covers and only the thumb is left visible. `Scrollbar`'s default
/// set is `DOUBLE_VERTICAL`, whose `║` is what makes an unstyled scrollbar
/// read as a second border beside the first (design §3).
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
    let mut state =
        ScrollbarState::new(extent.total.saturating_sub(extent.visible)).position(extent.position);

    frame.render_stateful_widget(
        // No arrows: they would land on the block's corners.
        Scrollbar::new(ScrollbarOrientation::VerticalRight)
            .begin_symbol(None)
            .end_symbol(None)
            .track_symbol(Some(symbols::line::VERTICAL))
            .track_style(border)
            .thumb_symbol(THUMB)
            .thumb_style(theme.style(Token::Muted)),
        area.inner(Margin {
            horizontal: 0,
            vertical: 1,
        }),
        &mut state,
    );
}

#[cfg(test)]
mod tests {
    use lc_domain::commands::CommandsPanel;
    use lc_domain::contents::TableRef;
    use lc_domain::model::{
        ClusteringColumn, ClusteringOrder, Column, ConnId, CqlValue, DbError, Keyspace, Page,
        PrimaryKey, QueryId, Replication, Schema, Table, TableOptions,
    };
    use lc_domain::pointer::Click;
    use lc_domain::posture::Posture;
    use lc_domain::search::{Filter, Filtered, Prompt};
    use lc_domain::state::{
        Connection, ConnectionState, Geometry, Message, Overlay, Panel, SchemaState,
    };
    use lc_domain::style::Severity;
    use lc_domain::view::PanelBody;
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
                render(frame, &view_model(&state), Theme::TerminalDefault);
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
    fn the_four_panels_are_drawn_where_the_draft_puts_them() {
        // At 40x12 the sidebar sits on its 16-column floor, Tables takes 7
        // rows to Ddl's 4, and Queries sits on its 3-row floor.
        let buffer = draw(40, 12);

        assert_eq!(row(&buffer, 0), "╭Tables────────╮╭Table contents────────╮");
        assert_eq!(row(&buffer, 1), "│Not connected ││Press Enter on a table│");
        assert_eq!(row(&buffer, 6), "╰──────────────╯│                      │");
        assert_eq!(row(&buffer, 7), "╭Table DDL─────╮╰──────────────────────╯");
        assert_eq!(row(&buffer, 8), "│              │╭Queries───────────────╮");
        assert_eq!(row(&buffer, 10), "╰──────────────╯╰──────────────────────╯");
    }

    #[test]
    fn the_hint_bar_sits_on_the_bottom_row() {
        let buffer = draw(80, 24);

        assert_eq!(
            row(&buffer, 23).trim_end(),
            "■⬝⬝⬝⬝⬝⬝⬝  j/k move  l/h fold  ⏎ open  / search │ tab panel  q quit",
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

        assert_eq!(
            hints.trim_end(),
            " j/k scroll  pgdn/pgup page  home/end ends │ ?/esc close  q quit"
        );
    }

    #[test]
    fn the_bar_says_what_the_focused_panel_does() {
        let bars: Vec<String> = Panel::ALL
            .iter()
            .map(|&panel| row(&draw_focused(80, 24, panel), 23).trim_end().to_owned())
            .collect();

        assert!(bars[0].contains("l/h fold"), "Tables folds: {:?}", bars[0]);
        assert!(bars[1].contains("j/k scroll"), "Ddl scrolls: {:?}", bars[1]);
        assert!(
            bars[2].contains("j/k move"),
            "Contents moves: {:?}",
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
            "■⬝⬝⬝⬝⬝⬝⬝  j/k move │ tab panel  q quit",
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
        assert!(!bold(0, 7), "Ddl is not");
        assert!(!bold(16, 8), "Queries is not");
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
        assert!(!bold(0, 7), "Ddl is not");
        assert!(!bold(16, 8), "Queries is not");
    }

    #[test]
    fn each_panel_underlines_the_key_that_focuses_it() {
        let buffer = draw(40, 12);
        let underlined = |x, y| {
            buffer
                .cell((x, y))
                .expect("cell is in bounds")
                .modifier
                .contains(Modifier::UNDERLINED)
        };

        // "╭Tables" and "╭Table contents" on row 0, "╭Table DDL" on row 7,
        // "╭Queries" on row 8 — the border occupies the first column of each.
        assert!(underlined(1, 0), "Tables underlines its T");
        assert!(underlined(23, 0), "Table contents underlines its c");
        assert!(underlined(2, 7), "Table DDL underlines its a");
        assert!(underlined(18, 8), "Queries underlines its u");
    }

    #[test]
    fn nothing_but_the_shortcut_is_underlined() {
        let buffer = draw(40, 12);
        let underlined = |x, y| {
            buffer
                .cell((x, y))
                .expect("cell is in bounds")
                .modifier
                .contains(Modifier::UNDERLINED)
        };

        for x in [0, 2, 3, 6] {
            assert!(!underlined(x, 0), "column {x} of row 0 is not the mark");
        }
        assert!(!underlined(1, 7), "the T of Table DDL is not its mark");
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
                render(frame, &view_model(&state), Theme::TerminalDefault);
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
                geometry = render(frame, &view_model(&state), Theme::TerminalDefault).geometry;
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
        // Five rows of body at the sizes these tests draw. Set with the cursor
        // for the reason `select_contents` sets it: half a state is not a
        // state (list viewport design §6).
        state.tables.scroll.follow(selected, 5);
        state
    }

    fn draw_state(width: u16, height: u16, state: &State) -> Buffer {
        let mut terminal =
            Terminal::new(TestBackend::new(width, height)).expect("TestBackend always initialises");

        terminal
            .draw(|frame| {
                render(frame, &view_model(state), Theme::TerminalDefault);
            })
            .expect("TestBackend always draws");

        terminal.backend().buffer().clone()
    }

    #[test]
    fn keyspaces_are_listed_inside_the_tables_panel() {
        let buffer = draw_state(40, 12, &browsing(&["shop", "typezoo", "system"], 0));

        assert_eq!(row(&buffer, 1), "│  shop        ││Press Enter on a table│");
        assert_eq!(row(&buffer, 2), "│  typezoo     ││                      │");
        assert_eq!(row(&buffer, 3), "│  system      ││                      │");
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

        assert_eq!(row(&buffer, 1), "│▾ shop        ││Press Enter on a table│");
        assert_eq!(row(&buffer, 2), "│    orders    ││                      │");
        assert_eq!(row(&buffer, 3), "│    users     ││                      │");
        assert_eq!(
            row(&buffer, 4),
            "│  typezoo     ││                      │",
            "a keyspace with no tables draws no marker"
        );
    }

    #[test]
    fn the_clusters_own_keyspaces_are_dimmed() {
        let buffer = draw_state(40, 12, &browsing(&["shop", "system"], 0));
        let dim = |y: u16| {
            buffer
                .cell((1, y))
                .expect("inside the panel")
                .modifier
                .contains(Modifier::DIM)
        };

        assert!(!dim(1), "shop is the user's");
        assert!(dim(2), "system is the cluster's");
    }

    #[test]
    fn the_selected_row_is_the_one_the_cursor_is_on() {
        let buffer = draw_state(40, 12, &browsing(&["shop", "typezoo", "system"], 1));
        let reversed = |y: u16| {
            buffer
                .cell((1, y))
                .expect("inside the panel")
                .modifier
                .contains(Modifier::REVERSED)
        };

        assert!(!reversed(1));
        assert!(reversed(2), "typezoo is selected");
        assert!(!reversed(3));
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

        assert_eq!(
            cell.fg,
            Color::Red,
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
            .requested(query, TableRef::new("shop", "orders"));
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
    /// (doc/specs/2026-08-09-list-viewport-design.md §6).
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
        state.contents.scroll.follow(12, 1);

        let buffer = draw_state(60, 12, &state);
        let reversed = |y: u16| {
            buffer
                .cell((17, y))
                .expect("inside the panel")
                .modifier
                .contains(Modifier::REVERSED)
        };

        for y in 2..7 {
            assert!(
                !reversed(y),
                "row 0 is off screen, so nothing drawn is the selection"
            );
        }

        // A control: the same panel, drawn with the selection inside the
        // viewport, does highlight it — so the absence above is the rule at
        // work rather than a fixture that stopped drawing rows.
        let mut on_screen = showing(page_of(&["id"], &rows));
        on_screen.contents.cursor.go_to(12, 20);
        on_screen.contents.scroll.follow(12, 5);
        let buffer = draw_state(60, 12, &on_screen);
        let highlighted = (2..7).any(|y| {
            buffer
                .cell((17, y))
                .expect("inside the panel")
                .modifier
                .contains(Modifier::REVERSED)
        });

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

        let (buffer, _map) = draw_mapped(40, 12, &state);
        let drawn: Vec<String> = (1..6).map(|y| row(&buffer, y)).collect();
        let reversed = |y: u16| {
            buffer
                .cell((1, y))
                .expect("inside the panel")
                .modifier
                .contains(Modifier::REVERSED)
        };

        assert!(
            drawn.iter().any(|line| line.contains("ks11")),
            "the stored offset decides the top row, got {drawn:?}"
        );
        for y in 1..6 {
            assert!(
                !reversed(y),
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
            "╭Tables────────╮╭Table contents — shop.orders (2 rows)─────╮"
        );
        assert_eq!(
            row(&buffer, 1),
            "│▸ shop        ││id email                                  │"
        );
        assert_eq!(
            row(&buffer, 2),
            "│              ││1  ada@example.com                        │"
        );
        assert_eq!(
            row(&buffer, 3),
            "│              ││2  bob@example.com                        │"
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
            "│              ││8f14e45f-ceea-467a-9c2b…                  │",
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
            "│▸ shop              ││a                        b                                        │",
            "the two columns that do not fit are not drawn at all"
        );
        assert_eq!(
            row(&buffer, 2),
            "│                    ││8f14e45f-ceea-467a-9c2b… 8f14e45f-ceea-467a-9c2b…                 │",
            "and the two that do keep their full width and their ellipsis"
        );
    }

    #[test]
    fn the_key_columns_are_drawn_in_the_styles_that_carry_the_physical_shape() {
        let state = showing(page_of(&["id", "email"], &[&[CqlValue::Int(1), text("a")]]));

        let buffer = draw_state(60, 12, &state);
        let header = |x: u16| buffer.cell((x, 1)).expect("inside the panel");

        assert_eq!(header(17).fg, Color::Yellow, "id is the partition key");
        assert!(header(17).modifier.contains(Modifier::BOLD));
        assert_eq!(
            header(20).fg,
            Color::Reset,
            "a regular column spends no colour — it is the baseline"
        );
    }

    #[test]
    fn a_matched_cell_reads_apart_from_an_unmatched_one_on_and_off_the_selected_row() {
        // Row 0 (not selected) and row 1 (the cursor's row) each hold one
        // cell the filter matches and one it does not — the composed case
        // is row 1, where `Token::Match` is drawn under `Token::Selected`'s
        // REVERSED and still has to be tellable apart from the id beside it.
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

        assert_ne!(
            style(20, 2),
            style(17, 2),
            "row 0 is not selected: its matched email must still read apart from its unmatched id"
        );
        assert_ne!(
            style(20, 3),
            style(17, 3),
            "row 1 is the cursor's row: a matched cell must still read apart from an \
             unmatched one under Selected's REVERSED"
        );
    }

    #[test]
    fn a_null_is_dimmed_so_it_reads_apart_from_the_word() {
        let state = showing(page_of(
            &["id", "email"],
            &[&[CqlValue::Int(1), CqlValue::Null]],
        ));

        let buffer = draw_state(60, 12, &state);
        let cell = |x: u16| buffer.cell((x, 2)).expect("inside the panel");

        assert!(
            cell(20).modifier.contains(Modifier::DIM),
            "the null is muted"
        );
        assert!(
            !cell(17).modifier.contains(Modifier::DIM),
            "the value beside it is not"
        );
    }

    #[test]
    fn a_matched_null_composes_match_with_muted_rather_than_losing_either() {
        // Design §4.3 (amended): the renderer composes `Token::Match` with
        // `Token::Muted` rather than one replacing the other, so a matched
        // null stays tellable apart from a matched cell whose text is
        // literally the word "null".
        let mut state = showing(page_of(
            &["id", "value", "note"],
            &[&[CqlValue::Int(1), CqlValue::Null, text("null")]],
        ));
        state.apply_filter(Filtered::Contents, Filter::new("null"));

        let buffer = draw_state(70, 12, &state);
        let cell = |x: u16| buffer.cell((x, 2)).expect("inside the panel");
        let null_cell = cell(21);
        let word_cell = cell(27);

        assert!(
            null_cell.modifier.contains(Modifier::DIM),
            "the matched null must still read as a null"
        );
        assert_eq!(
            null_cell.fg, word_cell.fg,
            "both are matches, so they carry the same match colour"
        );
        assert!(
            !word_cell.modifier.contains(Modifier::DIM),
            "the matched word is not a null and must not be dimmed"
        );
        assert_ne!(
            (null_cell.fg, null_cell.modifier),
            (word_cell.fg, word_cell.modifier),
            "a matched null must still draw differently from a matched non-null cell"
        );
    }

    #[test]
    fn the_selected_row_of_the_grid_is_the_one_the_cursor_is_on() {
        let mut state = showing(page_of(
            &["id"],
            &[&[CqlValue::Int(1)], &[CqlValue::Int(2)]],
        ));
        state.contents.cursor.down(2);

        let buffer = draw_state(60, 12, &state);
        let reversed = |y: u16| {
            buffer
                .cell((17, y))
                .expect("inside the panel")
                .modifier
                .contains(Modifier::REVERSED)
        };

        assert!(!reversed(1), "the header is never the selection");
        assert!(!reversed(2), "nor is the first row");
        assert!(reversed(3), "the second row is");
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
            "│▸ shop        ││No rows                                   │"
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

    /// Inside the Ddl panel's border at 100x41: fourteen rows of 23 columns.
    fn ddl_rows(buffer: &Buffer) -> Vec<String> {
        (25..39)
            .map(|y| {
                (1..24)
                    .filter_map(|x| buffer.cell((x, y)))
                    .map(Cell::symbol)
                    .collect::<String>()
                    .trim_end()
                    .to_owned()
            })
            .collect()
    }

    #[test]
    fn a_selected_tables_shape_is_drawn_above_its_options() {
        let buffer = draw_state(100, 41, &describing(orders()));

        assert_eq!(
            ddl_rows(&buffer),
            [
                "shop.orders_by_customer",
                "",
                "Partition Key",
                "  customer_id      uuid",
                "Clustering Key",
                "  order_date ↓     date",
                "  order_id ↑   timeuuid",
                // The one blank row, splitting what the table is from what it
                // is configured to do.
                "",
                "Options",
                // Short enough for one line each; `bloom_filter_fp_chance` and
                // its value need 26 of the panel's 23, so it keeps the
                // wrapped shape.
                "comment  Order history.",
                "gc_grace_seconds 864000",
                "bloom_filter_fp_chance",
                "  0.01",
                // The row the shorter body leaves unfilled.
                "",
            ]
        );
    }

    #[test]
    fn the_key_columns_are_drawn_in_the_tokens_that_mark_them_as_the_key() {
        let buffer = draw_state(100, 40, &describing(orders()));
        let colour = |y: u16| buffer.cell((1, y)).expect("inside the panel").fg;

        assert_eq!(colour(27), Color::Yellow, "Partition Key");
        assert_eq!(colour(28), Color::Yellow, "its column");
        assert_eq!(colour(29), Color::Cyan, "Clustering Key");
        assert_eq!(colour(30), Color::Cyan, "its column");
        // 32 is the blank row the settings group opens with.
        assert_eq!(
            colour(33),
            Color::Reset,
            "an option is not part of the key structure and must not read as one"
        );
        assert_eq!(
            colour(34),
            Theme::TerminalDefault
                .style(Token::PropertyName)
                .fg
                .expect("a property name is highlighted"),
            "the property name carries the accent its heading does not"
        );
    }

    #[test]
    fn a_selected_keyspace_is_described_by_its_replication() {
        let mut state = describing(orders());
        let schema = state.schema().cloned().expect("loaded");
        state.tables.close(&schema);

        let buffer = draw_state(100, 40, &state);

        assert_eq!(
            ddl_rows(&buffer)[..4],
            ["shop", "", "Replication", "class    SimpleStrategy"],
            "the only section opens the panel, so it takes no blank row of its own"
        );
    }

    #[test]
    fn the_ddl_panel_is_empty_until_something_is_selected() {
        let buffer = draw_state(100, 41, &State::default());

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
                geometry = render(frame, &view_model(state), Theme::TerminalDefault).geometry;
            })
            .expect("TestBackend always draws");

        geometry
    }

    #[test]
    fn the_frame_reports_what_each_panel_can_show() {
        // At 40x20: a 16-column sidebar, Tables 12 rows tall and Ddl 7, so
        // ten and five rows of content inside their borders.
        let geometry = geometry_of(40, 20, &describing(orders()));

        assert_eq!(geometry.height(Panel::Tables), 10);
        assert_eq!(geometry.height(Panel::Ddl), 5);
        assert_eq!(
            geometry.ddl_lines(),
            crate::details::lines(
                &ddl_details(&describing(orders())),
                14,
                Theme::TerminalDefault
            )
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
        let all = crate::details::lines(&ddl_details(&state), WIDTH, Theme::TerminalDefault);
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
        (1..6)
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
            "the thumb says how far through twelve keyspaces five rows are"
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

        assert!(!track.is_empty(), "twelve keyspaces do not fit five rows");
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
            "│  ks00        ▐│Press Enter on a table│",
            "the row is as wide as it is without a bar; the bar replaced the border"
        );
    }

    #[test]
    fn what_the_frame_measures_does_not_depend_on_where_it_is_scrolled() {
        // Both termination guards on the render→domain edge rest on this:
        // `lc-app` sends only when the geometry changed, and `update` marks
        // dirty only when the clamp moved the offset. A `Geometry` that varied
        // with the offset it bounds would close the loop
        // (doc/specs/2026-08-09-panel-scrolling-design.md §3).
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
            "│││││",
            "nothing is hidden, so nothing says otherwise"
        );
    }

    #[test]
    fn the_selection_stays_visible_when_the_list_outgrows_the_panel() {
        // Six rows of content fit in an eight-row Tables panel at this size;
        // twelve keyspaces force the scroll a plain `List` (no `ListState`)
        // would silently fail to perform.
        let names: Vec<String> = (0..12).map(|i| format!("ks{i:02}")).collect();
        let refs: Vec<&str> = names.iter().map(String::as_str).collect();

        let buffer = draw_state(40, 12, &browsing(&refs, 11));
        let rows: Vec<String> = (1..7).map(|y| row(&buffer, y)).collect();

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
                map = render(frame, &view_model(state), Theme::TerminalDefault).map;
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
                    row: index
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
                    row: index
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
                    row: index
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

    #[test]
    fn a_grids_header_is_chrome_rather_than_its_first_row() {
        let state = showing(page_of(
            &["id", "email"],
            &[
                &[CqlValue::Int(1), text("ada@example.com")],
                &[CqlValue::Int(2), text("bob@example.com")],
            ],
        ));

        let (buffer, map) = draw_mapped(60, 12, &state);

        assert!(
            row(&buffer, 1).contains("id") && row(&buffer, 1).contains("email"),
            "row 1 must be the header row this test is about"
        );
        assert!(
            row(&buffer, 2).contains("ada@example.com"),
            "row 2 must carry the first row's value, or `row: 0` below proves nothing"
        );

        assert_eq!(
            map.hit(20, 1),
            Some(Click::Panel(Panel::Contents)),
            "the header names columns; clicking one sorts nothing"
        );
        assert_eq!(
            map.hit(20, 2),
            Some(Click::Row {
                panel: Panel::Contents,
                row: 0
            })
        );
        assert_eq!(
            map.hit(20, 4),
            Some(Click::Panel(Panel::Contents)),
            "below the last row is blank space, not a row"
        );
    }

    #[test]
    fn a_press_on_the_chrome_of_a_panel_names_that_panel() {
        let (_buffer, map) = draw_mapped(40, 12, &browsing(&["shop"], 0));

        assert_eq!(map.hit(0, 0), Some(Click::Panel(Panel::Tables)), "corner");
        assert_eq!(map.hit(3, 0), Some(Click::Panel(Panel::Tables)), "title");
        assert_eq!(map.hit(20, 0), Some(Click::Panel(Panel::Contents)));
        assert_eq!(map.hit(2, 8), Some(Click::Panel(Panel::Ddl)));
        assert_eq!(map.hit(20, 9), Some(Click::Panel(Panel::Queries)));
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
                render(frame, &view_model(state), Theme::TerminalDefault);
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
        let (fg, bg, modifier) = style_of_first(&buffer, "/ord");
        let expected = Theme::TerminalDefault.style(Token::Muted);

        assert_eq!(
            fg,
            expected.fg.unwrap_or(Color::Reset),
            "no colour override"
        );
        assert_eq!(
            bg,
            expected.bg.unwrap_or(Color::Reset),
            "no colour override"
        );
        assert_eq!(
            modifier, expected.add_modifier,
            "the dim modifier, and only that"
        );
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
        // (`Tables`) + 4 (`/ord`) + 1 (`╮`) leaves exactly 4 columns of
        // border between title and marker — `marker_fits`'s constant of 6
        // read as "both corners plus a space either side of each" tuned
        // against this exact border, not picked from the arithmetic alone.
        let mut state = browsing(&["shop"], 0);
        state.apply_filter(Filtered::Tables, Filter::new("ord"));

        let buffer = draw_state(40, 24, &state);
        let border = row(&buffer, top_border(&buffer, "Tables").1);

        assert_eq!(
            border, "╭Tables────/ord╮╭Table contents────────╮",
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

        assert!(
            cell.modifier.contains(Modifier::DIM),
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
                render(frame, &view_model(&state), Theme::TerminalDefault);
            })
            .expect("TestBackend always draws");

        terminal.backend().buffer().clone()
    }

    /// The row a bordered block's top edge sits on, found by its left corner.
    fn top_border(buffer: &Buffer, expect: &str) -> (u16, u16) {
        for y in 0..buffer.area.height {
            for x in 0..buffer.area.width {
                if buffer.cell((x, y)).is_some_and(|cell| cell.symbol() == "╭") {
                    let row: String = (x..buffer.area.width)
                        .filter_map(|at| buffer.cell((at, y)))
                        .map(Cell::symbol)
                        .collect();
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
        let styled = |at: u16| {
            buffer
                .cell((x + 2, at))
                .expect("inside the block")
                .modifier
                .contains(Modifier::REVERSED)
        };

        assert!(styled(selected), "the cursor's row is marked");
        assert!(!styled(selected + 1), "and only the cursor's row is");
    }

    /// Whether the block titled `title` has a scrollbar drawn over its right
    /// border. The track scan stops above the bottom border row: that row's
    /// corner is `┘`, which is `!= "│"` whether or not a scrollbar was drawn,
    /// and including it would make the scan true unconditionally.
    fn has_scrollbar(buffer: &Buffer, title: &str) -> bool {
        let (x, y) = top_border(buffer, title);
        let width = block_width(buffer, title);
        let bottom = (y + 1..buffer.area.height)
            .find(|at| {
                buffer
                    .cell((x, *at))
                    .is_some_and(|cell| cell.symbol() == "╰")
            })
            .expect("a bordered block has a bottom edge");

        (y + 1..bottom).any(|at| {
            buffer
                .cell((x + width - 1, at))
                .is_some_and(|cell| cell.symbol() != "│")
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
        // 50% of 70 columns lands exactly on MIN_W, the floor a wider action
        // would clip against if the margin were counted on one side only.
        let drawn = buffer_text(&with_palette(70, 40));

        assert!(
            drawn.contains("focus Table contents"),
            "the widest action must appear whole, not clipped to 19 characters"
        );
    }

    #[test]
    fn a_frame_that_would_clip_the_widest_row_gets_no_palette() {
        // 50% of 68 columns is 34 — the old, undercounted MIN_W. A gate that
        // let this through would draw a palette whose widest row is clipped
        // by one character rather than refusing it outright.
        let drawn = buffer_text(&with_palette(68, 40));

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
                geometry = render(frame, &view_model(&state), Theme::TerminalDefault).geometry;
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
                geometry = render(frame, &view_model(&state), Theme::TerminalDefault).geometry;
            })
            .expect("TestBackend always draws");

        assert_eq!(geometry.command_rows(), 0);
    }
}

//! Drawing a frame.

use lc_domain::ViewModel;
use lc_domain::state::{Geometry, Panel};
use lc_domain::style::Token;
use lc_domain::view::OverlayView;
use ratatui::Frame;
use ratatui::layout::{Margin, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Scrollbar, ScrollbarOrientation, ScrollbarState};

use crate::body::Extent;
use crate::layout::{Shell, layout};
use crate::theme::Theme;

/// Draw one titled block per panel, and report what the frame turned out to
/// be.
///
/// The return value is the domain's only way to learn how tall a panel is and
/// how many lines the Ddl body wrapped to
/// (doc/specs/2026-08-09-panel-scrolling-design.md §3).
pub fn render(frame: &mut Frame, view: &ViewModel, theme: Theme) -> Geometry {
    let command_rows = crate::palette::visible_rows(frame.area(), lc_domain::commands::row_count());

    let rects = match layout(frame.area()) {
        Shell::Panels(rects) => rects,
        Shell::TooSmall(area) => {
            crate::too_small::render(frame, area);
            return Geometry::default();
        }
    };
    let mut heights = [0; 4];
    let mut ddl_lines = 0;

    for (index, panel) in view.panels.iter().enumerate() {
        let area = rects.get(panel.panel);

        let (before, shortcut, after) = panel.title.split();
        let title = Line::from(vec![
            Span::raw(before),
            Span::styled(shortcut, theme.style(Token::Shortcut)),
            Span::raw(after),
        ]);

        let mut block = Block::bordered().title(title);
        if panel.panel == view.focus {
            block = block.border_style(theme.style(Token::Focused));
        }

        let inner = block.inner(area);
        frame.render_widget(block, area);

        match crate::body::render(frame, inner, &panel.body, theme) {
            Some(extent) => {
                heights[index] = extent.visible;
                if panel.panel == Panel::Ddl {
                    ddl_lines = extent.total;
                }
                if extent.overflows() {
                    scrollbar(frame, area, &extent, theme);
                }
            }
            // `Blank` and `Notice` do not scroll, so what such a panel can
            // show is simply the room it was given.
            None => heights[index] = usize::from(inner.height),
        }
    }

    let bar = rects.bar();
    frame.render_widget(crate::hints::line(&view.hints, bar.width, theme), bar);

    match &view.overlay {
        Some(OverlayView::Message(message)) => crate::overlay::render(frame, message, theme),
        Some(OverlayView::Commands { rows, selected }) => {
            crate::palette::render(frame, rows, *selected, theme);
        }
        None => {}
    }

    Geometry::new(heights, ddl_lines, command_rows)
}

/// Over the block's right border, so it spends no content width — and only
/// when something is hidden, so its absence says nothing is.
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
pub(crate) fn scrollbar(frame: &mut Frame, area: Rect, extent: &Extent, theme: Theme) {
    let mut state =
        ScrollbarState::new(extent.total.saturating_sub(extent.visible)).position(extent.position);

    frame.render_stateful_widget(
        // No arrows: they would land on the block's corners.
        Scrollbar::new(ScrollbarOrientation::VerticalRight)
            .begin_symbol(None)
            .end_symbol(None)
            .style(theme.style(Token::Muted)),
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
    use lc_domain::posture::Posture;
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

        assert_eq!(row(&buffer, 0), "┌Tables────────┐┌Table contents────────┐");
        assert_eq!(row(&buffer, 1), "│Not connected ││Press Enter on a table│");
        assert_eq!(row(&buffer, 6), "└──────────────┘│                      │");
        assert_eq!(row(&buffer, 7), "┌Table DDL─────┐└──────────────────────┘");
        assert_eq!(row(&buffer, 8), "│              │┌Queries───────────────┐");
        assert_eq!(row(&buffer, 10), "└──────────────┘└──────────────────────┘");
    }

    #[test]
    fn the_hint_bar_sits_on_the_bottom_row() {
        let buffer = draw(80, 24);

        assert_eq!(
            row(&buffer, 23).trim_end(),
            " j/k move  l/h fold  ⏎ open │ ? commands  tab panel  q quit"
        );
        assert!(
            !row(&buffer, 22).contains("q quit"),
            "the bar owns one row and no more"
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
            bars[3].contains("nothing here yet"),
            "Queries has nothing: {:?}",
            bars[3]
        );
    }

    #[test]
    fn a_narrow_frame_drops_hints_rather_than_the_bar() {
        let buffer = draw(40, 12);

        assert_eq!(row(&buffer, 11).trim_end(), " j/k move │ tab panel  q quit");
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

        // "┌Tables" and "┌Table contents" on row 0, "┌Table DDL" on row 7,
        // "┌Queries" on row 8 — the border occupies the first column of each.
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
                !text.contains('┌'),
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
                geometry = render(frame, &view_model(&state), Theme::TerminalDefault);
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
        for _ in 0..selected {
            state.tables.cursor.down(names.len());
        }
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
            "┌Tables────────┐┌Table contents — shop.orders (2 rows)─────┐"
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
        // The Contents panel holds a header and six rows at this size, so a
        // twenty-row page forces the scroll a plain `Table` would not perform.
        let labels: Vec<String> = (0..20).map(|i| format!("row{i:02}")).collect();
        let values: Vec<Vec<CqlValue>> = labels.iter().map(|l| vec![text(l)]).collect();
        let rows: Vec<&[CqlValue]> = values.iter().map(Vec::as_slice).collect();
        let mut state = showing(page_of(&["id"], &rows));
        for _ in 0..19 {
            state.contents.cursor.down(20);
        }

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
                "PARTITION",
                "  customer_id      uuid",
                "CLUSTERING",
                "  order_date ↓     date",
                "  order_id ↑   timeuuid",
                "OPTIONS",
                "comment",
                "  Order history.",
                "gc_grace_seconds",
                "  864000",
                "bloom_filter_fp_chance",
                "  0.01",
            ]
        );
    }

    #[test]
    fn the_key_columns_are_drawn_in_the_tokens_that_mark_them_as_the_key() {
        let buffer = draw_state(100, 40, &describing(orders()));
        let colour = |y: u16| buffer.cell((1, y)).expect("inside the panel").fg;

        assert_eq!(colour(27), Color::Yellow, "PARTITION");
        assert_eq!(colour(28), Color::Yellow, "its column");
        assert_eq!(colour(29), Color::Cyan, "CLUSTERING");
        assert_eq!(colour(30), Color::Cyan, "its column");
        assert_eq!(
            colour(32),
            Color::Reset,
            "an option is not part of the key structure and must not read as one"
        );
    }

    #[test]
    fn a_selected_keyspace_is_described_by_its_replication() {
        let mut state = describing(orders());
        let schema = state.schema().cloned().expect("loaded");
        state.tables.close(&schema);

        let buffer = draw_state(100, 40, &state);

        assert_eq!(
            ddl_rows(&buffer)[..5],
            ["shop", "", "REPLICATION", "class", "  SimpleStrategy",]
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
            .draw(|frame| geometry = render(frame, &view_model(state), Theme::TerminalDefault))
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
        scrolled.ddl.to_end(all.len(), HEIGHT);

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
            tables_border(&buffer).contains('█'),
            "the thumb says how far through twelve keyspaces six rows are"
        );
    }

    #[test]
    fn the_thumb_sits_at_whichever_end_of_the_track_the_viewport_is_on() {
        let names: Vec<String> = (0..12).map(|i| format!("ks{i:02}")).collect();
        let refs: Vec<&str> = names.iter().map(String::as_str).collect();

        assert!(
            tables_border(&draw_state(40, 12, &browsing(&refs, 0))).starts_with('█'),
            "the first keyspace is on screen, so the thumb belongs at the top"
        );
        assert!(
            tables_border(&draw_state(40, 12, &browsing(&refs, 11))).ends_with('█'),
            "the last keyspace is on screen, so the thumb belongs at the bottom"
        );
    }

    #[test]
    fn the_bar_takes_the_border_column_rather_than_a_column_of_content() {
        let names: Vec<String> = (0..12).map(|i| format!("ks{i:02}")).collect();
        let refs: Vec<&str> = names.iter().map(String::as_str).collect();

        let buffer = draw_state(40, 12, &browsing(&refs, 0));

        assert_eq!(
            row(&buffer, 1),
            "│  ks00        █│Press Enter on a table│",
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
        scrolled.ddl.to_end(100, 6);

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
                if buffer.cell((x, y)).is_some_and(|cell| cell.symbol() == "┌") {
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
                    .is_some_and(|cell| cell.symbol() == "┐")
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

    #[test]
    fn a_palette_that_overflows_says_so() {
        // 70% of 24 rows leaves 14 for a catalogue in the low twenties.
        let buffer = with_palette(100, 24);
        let (x, y) = top_border(&buffer, "Commands");
        let width = block_width(&buffer, "Commands");

        let track = (y + 1..buffer.area.height).any(|at| {
            buffer
                .cell((x + width - 1, at))
                .is_some_and(|cell| cell.symbol() != "│")
        });

        assert!(track, "a catalogue taller than the box gets a scrollbar");
        assert!(
            !buffer_text(&with_palette(100, 80)).is_empty(),
            "and a frame tall enough to hold it all still draws"
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
                    .is_some_and(|cell| cell.symbol() == "└")
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
                geometry = render(frame, &view_model(&state), Theme::TerminalDefault);
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
                geometry = render(frame, &view_model(&state), Theme::TerminalDefault);
            })
            .expect("TestBackend always draws");

        assert_eq!(geometry.command_rows(), 0);
    }
}

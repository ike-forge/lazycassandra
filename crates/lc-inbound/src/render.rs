//! Drawing a frame.

use lc_domain::ViewModel;
use lc_domain::style::Token;
use ratatui::Frame;
use ratatui::text::{Line, Span};
use ratatui::widgets::Block;

use crate::layout::layout;
use crate::theme::Theme;

/// Draw one titled block per panel, skipping any the terminal is too small to
/// hold.
pub fn render(frame: &mut Frame, view: &ViewModel, theme: Theme) {
    let rects = layout(frame.area());

    for panel in &view.panels {
        let area = rects.get(panel.panel);
        if area.is_empty() {
            continue;
        }

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
        crate::body::render(frame, inner, &panel.body, theme);
    }

    if let Some(overlay) = &view.overlay {
        crate::overlay::render(frame, overlay, theme);
    }
}

#[cfg(test)]
mod tests {
    use lc_domain::contents::TableRef;
    use lc_domain::model::{
        ClusteringColumn, ClusteringOrder, Column, ConnId, CqlValue, DbError, Keyspace, Page,
        PrimaryKey, QueryId, Replication, Schema, Table, TableOptions,
    };
    use lc_domain::posture::Posture;
    use lc_domain::state::{Connection, ConnectionState, Panel, SchemaState};
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
            .draw(|frame| render(frame, &view_model(&state), Theme::TerminalDefault))
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
        // At 40x12 the sidebar sits on its 16-column floor, Tables takes 8
        // rows to Ddl's 4, and Queries sits on its 3-row floor.
        let buffer = draw(40, 12);

        assert_eq!(row(&buffer, 0), "┌Tables────────┐┌Table contents────────┐");
        assert_eq!(row(&buffer, 1), "│Not connected ││Press Enter on a table│");
        assert_eq!(row(&buffer, 7), "└──────────────┘│                      │");
        assert_eq!(row(&buffer, 8), "┌Table DDL─────┐└──────────────────────┘");
        assert_eq!(row(&buffer, 9), "│              │┌Queries───────────────┐");
        assert_eq!(row(&buffer, 11), "└──────────────┘└──────────────────────┘");
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
        assert!(!bold(0, 8), "Ddl is not");
        assert!(!bold(16, 9), "Queries is not");
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
        assert!(!bold(0, 8), "Ddl is not");
        assert!(!bold(16, 9), "Queries is not");
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

        // "┌Tables" and "┌Table contents" on row 0, "┌Table DDL" on row 8,
        // "┌Queries" on row 9 — the border occupies the first column of each.
        assert!(underlined(1, 0), "Tables underlines its T");
        assert!(underlined(23, 0), "Table contents underlines its c");
        assert!(underlined(2, 8), "Table DDL underlines its a");
        assert!(underlined(18, 9), "Queries underlines its u");
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
        assert!(!underlined(1, 8), "the T of Table DDL is not its mark");
    }

    #[test]
    fn a_collapsed_panel_is_not_drawn_at_all() {
        // Five rows leaves no room for a second panel in either column.
        let buffer = draw(40, 5);
        let contains = |needle: &str| (0..5).any(|y| row(&buffer, y).contains(needle));

        assert!(contains("Tables"));
        assert!(contains("Table contents"));
        assert!(!contains("Table DDL"), "Ddl collapsed to zero rows");
        assert!(!contains("Queries"), "Queries collapsed to zero rows");
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
            .draw(|frame| render(frame, &view_model(state), Theme::TerminalDefault))
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

    /// Inside the Ddl panel's border at 100x40: fourteen rows of 23 columns.
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
        let buffer = draw_state(100, 40, &describing(orders()));

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
        let buffer = draw_state(100, 40, &State::default());

        assert!(
            ddl_rows(&buffer).iter().all(String::is_empty),
            "the Tables panel already says why there is nothing; twice is noise"
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
}

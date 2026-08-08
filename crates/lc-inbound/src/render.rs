//! Drawing a frame.

use lc_domain::ViewModel;
use lc_domain::style::Token;
use ratatui::Frame;
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

        let mut block = Block::bordered().title(panel.title.as_str());
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
    use lc_domain::model::{
        ClusteringColumn, ClusteringOrder, Column, ConnId, DbError, Keyspace, PrimaryKey,
        Replication, Schema, Table, TableOptions,
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
        assert_eq!(row(&buffer, 1), "│Not connected ││                      │");
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

        assert_eq!(row(&buffer, 1), "│  shop        ││                      │");
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

        assert_eq!(row(&buffer, 1), "│▾ shop        ││                      │");
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

        assert_eq!(row(&buffer, 1), "│Connecting…   ││                      │");
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

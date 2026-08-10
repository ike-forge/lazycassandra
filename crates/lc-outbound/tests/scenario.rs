//! Whole sessions driven through the in-memory cluster (architecture.md §10).
//!
//! Deterministic: no clock, no socket, no runtime beyond the one `#[tokio::test]`
//! provides to await the port.

use std::collections::{BTreeMap, VecDeque};

use lc_domain::model::{
    Column, ConnId, CqlValue, DbError, Key, KeyPress, Keyspace, Page, PrimaryKey, Replication, Row,
    Schema, Statement, Table, TableOptions,
};
use lc_domain::ports::Cluster;
use lc_domain::posture::Posture;
use lc_domain::state::Panel;
use lc_domain::style::Token;
use lc_domain::update::ROW_LIMIT;
use lc_domain::view::{DetailRow, Details, Marker, OverlayView, PanelBody};
use lc_domain::{Effect, Msg, State, init, update, view_model};
use lc_outbound::fake::FakeCluster;

fn schema_of(names: &[&str]) -> Schema {
    tabled_schema_of(&names.iter().map(|n| (*n, &[][..])).collect::<Vec<_>>())
}

fn table(name: &str) -> Table {
    Table {
        name: name.to_owned(),
        primary_key: PrimaryKey {
            partition: vec![Column {
                name: "id".to_owned(),
                cql_type: "uuid".to_owned(),
            }],
            clustering: Vec::new(),
        },
        static_columns: Vec::new(),
        regular_columns: Vec::new(),
        options: TableOptions::new(BTreeMap::from([
            ("comment".to_owned(), format!("The {name} table.")),
            ("gc_grace_seconds".to_owned(), "864000".to_owned()),
        ])),
    }
}

/// `(keyspace name, its table names)`.
fn tabled_schema_of(keyspaces: &[(&str, &[&str])]) -> Schema {
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

/// Runs the effects a session produces, feeding results back through `update`
/// exactly as `lc-app`'s runtime does.
async fn run(cluster: &dyn Cluster, state: &mut State, effects: Vec<Effect>) {
    let mut pending: VecDeque<Effect> = effects.into();

    while let Some(effect) = pending.pop_front() {
        let msg = match effect {
            Effect::Connect(conn) => match cluster.connect(conn).await {
                Ok(posture) => Msg::Connected { conn, posture },
                Err(error) => Msg::ConnectFailed { conn, error },
            },
            Effect::LoadSchema(conn) => match cluster.describe(conn).await {
                Ok(schema) => Msg::SchemaLoaded {
                    conn,
                    schema: Box::new(schema),
                },
                Err(error) => Msg::SchemaFailed { conn, error },
            },
            Effect::Execute { conn, query, stmt } => match cluster.execute(conn, &stmt).await {
                Ok(page) => Msg::PageReady {
                    query,
                    page: Box::new(page),
                },
                Err(error) => Msg::Failed { query, error },
            },
            _ => continue,
        };

        pending.extend(update(state, msg));
    }
}

#[tokio::test]
async fn connecting_and_describing_fills_the_tables_panel() {
    let cluster = FakeCluster::new(
        schema_of(&["system_schema", "typezoo", "shop"]),
        Posture::ReadOnly,
    );
    let conn = ConnId::next();
    let (mut state, effects) = init(conn);

    run(&cluster, &mut state, effects).await;

    let PanelBody::Tree { rows, selected, .. } = &view_model(&state).panels[0].body else {
        panic!("a described cluster lists its keyspaces");
    };
    assert_eq!(
        rows.iter()
            .map(|row| row.label.as_str())
            .collect::<Vec<_>>(),
        ["shop", "typezoo", "system_schema"]
    );
    assert_eq!(
        rows[0].marker,
        Marker::Leaf,
        "these fixtures have no tables to unfold"
    );
    assert_eq!(*selected, 0);
}

#[tokio::test]
async fn a_cluster_that_cannot_be_reached_raises_an_overlay() {
    let cluster = FakeCluster::failing(DbError::Unreachable("no route to host".to_owned()));
    let conn = ConnId::next();
    let (mut state, effects) = init(conn);

    run(&cluster, &mut state, effects).await;

    let Some(OverlayView::Message(overlay)) = view_model(&state).overlay else {
        panic!("the failure is shown");
    };
    assert!(overlay.message.contains("no route to host"));
    assert!(
        matches!(&view_model(&state).panels[0].body, PanelBody::Notice { text, .. } if text.contains("no route to host")),
        "the panel says it too, so dismissing the overlay does not erase the failure"
    );
}

#[tokio::test]
async fn a_cluster_that_connects_but_cannot_be_described_raises_an_overlay() {
    let cluster = FakeCluster::failing_to_describe(DbError::Invalid("unauthorized".to_owned()));
    let conn = ConnId::next();
    let (mut state, effects) = init(conn);

    run(&cluster, &mut state, effects).await;

    let Some(OverlayView::Message(overlay)) = view_model(&state).overlay else {
        panic!("the failure is shown");
    };
    assert_eq!(overlay.title, "Schema unavailable");
}

#[tokio::test]
async fn pressing_l_unfolds_a_keyspace_and_the_view_model_shows_its_tables() {
    let cluster = FakeCluster::new(
        tabled_schema_of(&[("shop", &["orders", "users"])]),
        Posture::ReadOnly,
    );
    let conn = ConnId::next();
    let (mut state, effects) = init(conn);
    run(&cluster, &mut state, effects).await;

    let effects = update(&mut state, Msg::Input(KeyPress::new(Key::Char('l'))));
    assert!(
        effects.is_empty(),
        "unfolding is a pure state change and must emit no effect (table-list design §2)"
    );

    let PanelBody::Tree { rows, selected, .. } = &view_model(&state).panels[0].body else {
        panic!("a described cluster with tables lists a tree");
    };
    assert_eq!(
        rows.iter()
            .map(|row| (row.depth, row.marker, row.label.as_str()))
            .collect::<Vec<_>>(),
        [
            (0, Marker::Unfolded, "shop"),
            (1, Marker::Leaf, "orders"),
            (1, Marker::Leaf, "users"),
        ]
    );
    assert_eq!(*selected, 0);
}

#[tokio::test]
async fn pressing_h_from_a_table_lands_on_its_parent_keyspace() {
    let cluster = FakeCluster::new(
        tabled_schema_of(&[("shop", &["orders", "users"])]),
        Posture::ReadOnly,
    );
    let conn = ConnId::next();
    let (mut state, effects) = init(conn);
    run(&cluster, &mut state, effects).await;

    update(&mut state, Msg::Input(KeyPress::new(Key::Char('l'))));
    update(&mut state, Msg::Input(KeyPress::new(Key::Char('l'))));

    let PanelBody::Tree { selected, .. } = &view_model(&state).panels[0].body else {
        panic!("a described cluster with tables lists a tree");
    };
    assert_eq!(*selected, 1, "the second l steps into the first table");

    let effects = update(&mut state, Msg::Input(KeyPress::new(Key::Char('h'))));
    assert!(
        effects.is_empty(),
        "stepping out to the parent is a pure state change and must emit no effect (table-list design §2)"
    );

    let PanelBody::Tree { rows, selected, .. } = &view_model(&state).panels[0].body else {
        panic!("a described cluster with tables lists a tree");
    };
    assert_eq!(*selected, 0);
    assert_eq!(
        rows[0].marker,
        Marker::Unfolded,
        "stepping out must not fold the keyspace"
    );
}

/// The keyspace's one table, its partition key `id` (see [`table`]) and a
/// regular column.
fn customers() -> Page {
    Page {
        columns: vec!["id".to_owned(), "email".to_owned()],
        rows: vec![
            Row {
                values: vec![
                    CqlValue::Uuid("8f14e45f".to_owned()),
                    CqlValue::Text("ada@example.com".to_owned()),
                ],
            },
            Row {
                values: vec![CqlValue::Uuid("c9f0f895".to_owned()), CqlValue::Null],
            },
        ],
        next: None,
    }
}

fn press(state: &mut State, key: Key) -> Vec<Effect> {
    update(state, Msg::Input(KeyPress::new(key)))
}

/// Connect, describe, unfold `shop`, step onto `orders`.
async fn on_a_table(cluster: &FakeCluster) -> State {
    let conn = ConnId::next();
    let (mut state, effects) = init(conn);
    run(cluster, &mut state, effects).await;

    let effects = press(&mut state, Key::Char('l'));
    run(cluster, &mut state, effects).await;
    let effects = press(&mut state, Key::Char('l'));
    run(cluster, &mut state, effects).await;

    state
}

#[tokio::test]
async fn pressing_enter_on_a_table_fills_the_contents_panel() {
    let cluster = FakeCluster::new(
        tabled_schema_of(&[("shop", &["orders"])]),
        Posture::ReadOnly,
    )
    .returning(customers());
    let mut state = on_a_table(&cluster).await;

    let effects = press(&mut state, Key::Enter);
    run(&cluster, &mut state, effects).await;

    assert_eq!(
        cluster.executed(),
        vec![Statement::select_all("shop", "orders", ROW_LIMIT)],
        "the panel reads exactly the statement it shows"
    );

    let view = view_model(&state);
    assert_eq!(
        view.panels[2].title.text(),
        "Table contents — shop.orders (2 rows)"
    );

    let PanelBody::Grid {
        columns,
        rows,
        selected,
        ..
    } = &view.panels[2].body
    else {
        panic!("a table that was read draws a grid");
    };
    assert_eq!(
        columns
            .iter()
            .map(|column| (column.name.as_str(), column.token))
            .collect::<Vec<_>>(),
        [
            ("id", Some(Token::PartitionKey)),
            ("email", Some(Token::RegularColumn)),
        ]
    );
    assert_eq!(
        rows.iter()
            .map(|row| row
                .iter()
                .map(|cell| cell.text.as_str())
                .collect::<Vec<_>>())
            .collect::<Vec<_>>(),
        [["8f14e45f", "ada@example.com"], ["c9f0f895", "null"]]
    );
    assert_eq!(rows[1][1].token, Some(Token::Muted), "the null is muted");
    assert_eq!(*selected, 0);
}

#[tokio::test]
async fn a_table_that_cannot_be_read_says_so_in_the_panel_alone() {
    let cluster = FakeCluster::new(
        tabled_schema_of(&[("shop", &["orders"])]),
        Posture::ReadOnly,
    )
    .refusing_to_execute(DbError::Invalid("unauthorized".to_owned()));
    let mut state = on_a_table(&cluster).await;

    let effects = press(&mut state, Key::Enter);
    run(&cluster, &mut state, effects).await;

    let view = view_model(&state);
    assert!(
        matches!(&view.panels[2].body, PanelBody::Notice { text, .. } if text.contains("unauthorized")),
    );
    assert!(
        view.overlay.is_none(),
        "one panel failing must not interrupt the whole application"
    );
}

#[tokio::test]
async fn reading_a_table_lands_focus_in_contents_and_j_walks_the_grid_rather_than_the_tree() {
    let cluster = FakeCluster::new(
        tabled_schema_of(&[("shop", &["orders"])]),
        Posture::ReadOnly,
    )
    .returning(customers());
    let mut state = on_a_table(&cluster).await;
    let effects = press(&mut state, Key::Enter);
    run(&cluster, &mut state, effects).await;

    let PanelBody::Tree { selected, .. } = &view_model(&state).panels[0].body else {
        panic!("the tree is still showing");
    };
    let tree_row = *selected;

    // Enter already took focus to Contents (design §1); `j` walks the grid.
    press(&mut state, Key::Char('j'));

    let view = view_model(&state);
    let PanelBody::Grid { selected, .. } = &view.panels[2].body else {
        panic!("a table that was read draws a grid");
    };
    assert_eq!(*selected, 1, "j moved the grid");

    let PanelBody::Tree { selected, .. } = &view.panels[0].body else {
        panic!("the tree is still showing");
    };
    assert_eq!(*selected, tree_row, "and left the tree where it was");
}

#[tokio::test]
async fn reading_a_second_table_cancels_the_first_and_drops_its_late_page() {
    let cluster = FakeCluster::new(
        tabled_schema_of(&[("shop", &["orders", "users"])]),
        Posture::ReadOnly,
    )
    .returning(customers());
    let mut state = on_a_table(&cluster).await;

    // Deliberately not run yet: the first read has to still be outstanding
    // when the second is issued.
    let first = press(&mut state, Key::Enter);
    state.focus = Panel::Tables;
    press(&mut state, Key::Char('j'));
    let second = press(&mut state, Key::Enter);

    assert!(
        second
            .iter()
            .any(|effect| matches!(effect, Effect::Cancel(_))),
        "the superseded read is cancelled"
    );

    // The cancelled read answers anyway, as a slow coordinator would.
    run(&cluster, &mut state, first).await;
    run(&cluster, &mut state, second).await;

    assert_eq!(
        cluster.executed(),
        vec![
            Statement::select_all("shop", "orders", ROW_LIMIT),
            Statement::select_all("shop", "users", ROW_LIMIT),
        ]
    );
    assert_eq!(
        view_model(&state).panels[2].title.text(),
        "Table contents — shop.users (2 rows)",
        "a late page for a read the user left must not resurrect it"
    );
}

fn ddl_details(state: &State) -> Details {
    let PanelBody::Details { details, .. } = &view_model(state).panels[1].body else {
        panic!("a selected row has something to describe");
    };
    details.clone()
}

#[tokio::test]
async fn stepping_onto_a_table_describes_it_in_the_ddl_panel() {
    let cluster = FakeCluster::new(
        tabled_schema_of(&[("shop", &["orders", "users"])]),
        Posture::ReadOnly,
    );
    let conn = ConnId::next();
    let (mut state, effects) = init(conn);
    run(&cluster, &mut state, effects).await;

    let keyspace = ddl_details(&state);
    assert_eq!(keyspace.heading, "shop");
    assert_eq!(
        keyspace
            .sections
            .iter()
            .map(|section| section.heading.as_str())
            .collect::<Vec<_>>(),
        ["Replication"]
    );

    update(&mut state, Msg::Input(KeyPress::new(Key::Char('l'))));
    let effects = update(&mut state, Msg::Input(KeyPress::new(Key::Char('l'))));
    assert!(
        effects.is_empty(),
        "the whole schema arrived with describe, so describing a table is not I/O"
    );

    let table = ddl_details(&state);
    assert_eq!(table.heading, "shop.orders");
    assert_eq!(
        table
            .sections
            .iter()
            .map(|section| section.heading.as_str())
            .collect::<Vec<_>>(),
        ["Partition Key", "Options"],
        "these fixtures have no clustering columns, so that section is absent"
    );
    assert_eq!(
        table.sections[1].rows,
        [
            DetailRow::Property {
                name: "gc_grace_seconds".to_owned(),
                value: "864000".to_owned(),
            },
            DetailRow::Property {
                name: "comment".to_owned(),
                value: "The orders table.".to_owned(),
            },
        ],
        "layout order, not the alphabetical order the map stores"
    );
}

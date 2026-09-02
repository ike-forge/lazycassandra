//! Whole sessions driven through the in-memory cluster (architecture.md §10).
//!
//! Deterministic: no clock, no socket, no runtime beyond the one `#[tokio::test]`
//! provides to await the port.

use lc_domain::clock::Stamp;
use lc_domain::log::Outcome;
use lc_domain::model::{Column, CqlValue, DbError, Key, KeyPress, Page, Row, Statement};
use lc_domain::posture::Posture;
use lc_domain::state::Panel;
use lc_domain::style::Token;
use lc_domain::update::ROW_LIMIT;
use lc_domain::view::{DetailRow, Details, OverlayView, PanelBody, RowKind, Section};
use lc_domain::{Effect, Msg, State, init, update, view_model};
use lc_outbound::fake::FakeCluster;

mod support;
use support::{press, run, schema_of, tabled_schema_of};

#[tokio::test]
async fn connecting_and_describing_fills_the_tables_panel() {
    let cluster = FakeCluster::new(
        schema_of(&["system_schema", "typezoo", "shop"]),
        Posture::ReadOnly,
    );
    let (mut state, effects) = init(support::connected_config(), Vec::new(), Stamp::default());

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
        rows[0].kind,
        RowKind::Keyspace,
        "these fixtures have no tables to hang off a keyspace"
    );
    assert_eq!(*selected, 0);
}

#[tokio::test]
async fn a_cluster_that_cannot_be_reached_raises_a_fatal_modal() {
    let cluster = FakeCluster::failing(DbError::Unreachable("no route to host".to_owned()));
    let (mut state, effects) = init(support::connected_config(), Vec::new(), Stamp::default());

    run(&cluster, &mut state, effects).await;

    let Some(OverlayView::Fatal(overlay)) = view_model(&state).overlay else {
        panic!("the failure ends the process rather than starting a half-connected app");
    };
    assert!(overlay.message.contains("no route to host"));
    assert!(
        matches!(&view_model(&state).panels[0].body, PanelBody::Notice { text, token } if !text.contains("no route to host") && *token == Some(Token::Muted)),
        "the modal owns the failure; the panel behind it is back to being about keyspaces"
    );
}

#[tokio::test]
async fn a_cluster_that_connects_but_cannot_be_described_raises_a_fatal_modal() {
    let cluster = FakeCluster::failing_to_describe(DbError::Invalid("unauthorized".to_owned()));
    let (mut state, effects) = init(support::connected_config(), Vec::new(), Stamp::default());

    run(&cluster, &mut state, effects).await;

    let Some(OverlayView::Fatal(overlay)) = view_model(&state).overlay else {
        panic!("the failure ends the process rather than starting a half-connected app");
    };
    assert_eq!(overlay.title, "Schema unavailable");
}

#[tokio::test]
async fn pressing_l_unfolds_a_keyspace_and_the_view_model_shows_its_tables() {
    let cluster = FakeCluster::new(
        tabled_schema_of(&[("shop", &["orders", "users"])]),
        Posture::ReadOnly,
    );
    let (mut state, effects) = init(support::connected_config(), Vec::new(), Stamp::default());
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
            .map(|row| (row.kind, row.label.as_str()))
            .collect::<Vec<_>>(),
        [
            (RowKind::Keyspace, "shop"),
            (RowKind::Table { last: false }, "orders"),
            (RowKind::Table { last: true }, "users"),
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
    let (mut state, effects) = init(support::connected_config(), Vec::new(), Stamp::default());
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
    assert!(
        rows.iter().any(|row| row.kind.is_table()),
        "stepping out must not fold the keyspace"
    );
}

/// The keyspace's one table, its partition key `id` (see `support::table`)
/// and a regular column.
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

/// Connect, describe, unfold `shop`, step onto `orders`.
async fn on_a_table(cluster: &FakeCluster) -> State {
    let (mut state, effects) = init(support::connected_config(), Vec::new(), Stamp::default());
    run(cluster, &mut state, effects).await;

    let effects = press(&mut state, Key::Char('l'));
    run(cluster, &mut state, effects).await;
    let effects = press(&mut state, Key::Char('l'));
    run(cluster, &mut state, effects).await;

    state
}

#[tokio::test]
async fn pressing_enter_on_a_table_fills_the_contents_panel() {
    let mut schema = tabled_schema_of(&[("shop", &["orders"])]);
    schema
        .keyspaces
        .get_mut("shop")
        .and_then(|keyspace| keyspace.tables.get_mut("orders"))
        .expect("the fixture made it")
        .regular_columns = vec![Column {
        name: "email".to_owned(),
        cql_type: "text".to_owned(),
    }];
    let cluster = FakeCluster::new(schema, Posture::ReadOnly).returning(customers());
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
        "[3]-Table contents — shop.orders (2 rows)"
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
    assert_eq!(rows[1][1].token, Some(Token::Faint), "the null is faint");
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
        matches!(&view.panels[2].body, PanelBody::Notice { text, .. }
            if !text.contains("unauthorized") && text.contains("[4]")),
        "the panel points at the trail rather than quoting the error"
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
    state.focus_on(Panel::Tables);
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
        "[3]-Table contents — shop.users (2 rows)",
        "a late page for a read the user left must not resurrect it"
    );
}

/// The `(label, name)` pairs of the identity rows the panel opens with.
fn named(details: &Details) -> Vec<(&'static str, &str)> {
    details
        .rows()
        .filter_map(|row| match row {
            DetailRow::Identity { kind, name } => Some((kind.label(), name.as_str())),
            _ => None,
        })
        .collect()
}

/// The headings the sections carry, which the identity group has none of.
fn headings(details: &Details) -> Vec<&str> {
    details
        .sections
        .iter()
        .filter_map(|section| {
            section
                .heading
                .as_ref()
                .map(|heading| heading.text.as_str())
        })
        .collect()
}

fn section<'a>(details: &'a Details, heading: &str) -> &'a Section {
    details
        .sections
        .iter()
        .find(|section| {
            section
                .heading
                .as_ref()
                .is_some_and(|own| own.text == heading)
        })
        .unwrap_or_else(|| panic!("{heading} is missing"))
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
    let (mut state, effects) = init(support::connected_config(), Vec::new(), Stamp::default());
    run(&cluster, &mut state, effects).await;

    let keyspace = ddl_details(&state);
    assert_eq!(named(&keyspace), [("Keyspace", "shop")]);
    assert_eq!(headings(&keyspace), ["Replication"]);

    update(&mut state, Msg::Input(KeyPress::new(Key::Char('l'))));
    let effects = update(&mut state, Msg::Input(KeyPress::new(Key::Char('l'))));
    assert!(
        effects.is_empty(),
        "the whole schema arrived with describe, so describing a table is not I/O"
    );

    let table = ddl_details(&state);
    assert_eq!(
        named(&table),
        [("Keyspace", "shop"), ("Table", "orders")],
        "the panel names what it describes in rows of its own"
    );
    assert_eq!(
        headings(&table),
        ["Partition Key", "Options"],
        "these fixtures have no clustering columns, so that section is absent"
    );
    assert_eq!(
        section(&table, "Options").rows,
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

// The tier that catches what the unit tests cannot see: that the trail reads
// in the order the session actually ran things, and that every effect it
// issued was eventually closed out (query log design §10).
#[tokio::test]
async fn a_session_leaves_a_trail_of_everything_it_ran() {
    let cluster = FakeCluster::new(
        tabled_schema_of(&[("shop", &["orders"])]),
        Posture::ReadOnly,
    );
    let mut state = on_a_table(&cluster).await;

    let effects = press(&mut state, Key::Enter);
    run(&cluster, &mut state, effects).await;

    let texts: Vec<String> = state
        .log
        .entries()
        .map(|entry| entry.subject().text())
        .collect();

    assert!(texts[0].starts_with("Connecting"), "{texts:?}");
    assert!(texts[1].starts_with("Reading schema"), "{texts:?}");
    assert!(
        texts[2].contains(&format!("LIMIT {ROW_LIMIT}")),
        "a table open logs the limit appended for it: {texts:?}"
    );
    assert!(
        state
            .log
            .entries()
            .all(|entry| !matches!(entry.outcome(), Outcome::Pending { .. })),
        "every entry was closed out: {texts:?}"
    );
}

/// The whole drop path, driven through the ports: the tree names a table, the
/// confirmation names the statement, and the success re-reads the schema the
/// drop just changed (docs/superpowers/specs/2026-08-31-drop-design.md §4).
#[tokio::test]
async fn dropping_a_table_removes_it_from_the_tree() {
    let cluster = FakeCluster::new(
        tabled_schema_of(&[("shop", &["orders", "users"])]),
        Posture::Writable,
    )
    .re_describing(tabled_schema_of(&[("shop", &["users"])]));
    let mut state = on_a_table(&cluster).await;
    // What a rendered frame reports; `lc-inbound`'s `drop::fits` and the
    // block it measures are held together by its own tests.
    state.geometry = state.geometry.with_drop_fits(true);

    let effects = press(&mut state, Key::Char('d'));
    run(&cluster, &mut state, effects).await;
    let effects = press(&mut state, Key::Char('y'));
    run(&cluster, &mut state, effects).await;

    assert_eq!(
        cluster
            .executed()
            .iter()
            .map(|stmt| stmt.cql.as_str())
            .collect::<Vec<_>>(),
        ["DROP TABLE \"shop\".\"orders\""],
        "the statement the confirmation named is the statement that ran"
    );

    let PanelBody::Tree { rows, .. } = &view_model(&state).panels[0].body else {
        panic!("the tree is still a tree");
    };
    assert_eq!(
        rows.iter()
            .map(|row| row.label.as_str())
            .collect::<Vec<_>>(),
        ["shop", "users"],
        "the re-read schema is what the tree draws"
    );
}

/// A read-only connection never gets as far as the question (design §1), and
/// nothing reaches the cluster.
#[tokio::test]
async fn a_read_only_connection_drops_nothing() {
    let cluster = FakeCluster::new(
        tabled_schema_of(&[("shop", &["orders"])]),
        Posture::ReadOnly,
    );
    let mut state = on_a_table(&cluster).await;
    state.geometry = state.geometry.with_drop_fits(true);

    let effects = press(&mut state, Key::Char('d'));
    run(&cluster, &mut state, effects).await;

    assert!(
        matches!(
            view_model(&state).overlay,
            Some(OverlayView::Message(message)) if message.title == "Read-only connection"
        ),
        "the posture refuses the drop, and says so rather than swallowing the key"
    );
    assert!(cluster.executed().is_empty());
}

/// A keyspace takes its name typed back before anything runs (design §2).
#[tokio::test]
async fn dropping_a_keyspace_takes_its_name_typed_back() {
    let cluster = FakeCluster::new(
        tabled_schema_of(&[("shop", &["orders"]), ("billing", &[])]),
        Posture::Writable,
    )
    .re_describing(tabled_schema_of(&[("shop", &["orders"])]));
    let (mut state, effects) = init(support::connected_config(), Vec::new(), Stamp::default());
    run(&cluster, &mut state, effects).await;
    state.geometry = state.geometry.with_drop_fits(true);

    let effects = press(&mut state, Key::Char('d'));
    run(&cluster, &mut state, effects).await;

    let effects = press(&mut state, Key::Enter);
    run(&cluster, &mut state, effects).await;
    assert!(
        cluster.executed().is_empty(),
        "an empty field drops nothing"
    );

    for character in "billing".chars() {
        let effects = press(&mut state, Key::Char(character));
        run(&cluster, &mut state, effects).await;
    }
    let effects = press(&mut state, Key::Enter);
    run(&cluster, &mut state, effects).await;

    assert_eq!(
        cluster
            .executed()
            .iter()
            .map(|stmt| stmt.cql.as_str())
            .collect::<Vec<_>>(),
        ["DROP KEYSPACE \"billing\""]
    );

    let PanelBody::Tree { rows, .. } = &view_model(&state).panels[0].body else {
        panic!("the tree is still a tree");
    };
    assert_eq!(
        rows.iter()
            .map(|row| row.label.as_str())
            .collect::<Vec<_>>(),
        ["shop"],
        "the tree draws the keyspaces the re-read schema still has"
    );
}

/// The whole delete path, driven through the ports: the grid names rows, the
/// confirmation counts them and names the statement, and the success re-reads
/// the page rather than the schema
/// (docs/superpowers/specs/2026-08-31-drop-design.md §3.1, §4).
#[tokio::test]
async fn deleting_the_selected_rows_re_reads_the_grid() {
    let cluster = FakeCluster::new(
        tabled_schema_of(&[("shop", &["orders"])]),
        Posture::Writable,
    )
    .returning(keyed_rows(3));
    let mut state = on_a_table(&cluster).await;
    state.geometry = state.geometry.with_drop_fits(true);

    let effects = press(&mut state, Key::Enter);
    run(&cluster, &mut state, effects).await;

    // Two rows held, from the cursor down.
    let effects = press(&mut state, Key::Char('v'));
    run(&cluster, &mut state, effects).await;
    let effects = press(&mut state, Key::Char('j'));
    run(&cluster, &mut state, effects).await;

    let effects = press(&mut state, Key::Char('d'));
    run(&cluster, &mut state, effects).await;

    let Some(OverlayView::Drop(view)) = view_model(&state).overlay else {
        panic!("the confirmation is up");
    };
    assert_eq!(view.rows, Some(2), "the block counts what it would delete");
    assert!(
        lc_domain::style::text(&view.title).contains("Delete 2 rows from shop.orders"),
        "{:?}",
        view.title
    );

    let effects = press(&mut state, Key::Char('y'));
    run(&cluster, &mut state, effects).await;

    let ran: Vec<String> = cluster
        .executed()
        .iter()
        .map(|stmt| stmt.cql.clone())
        .collect();
    assert_eq!(
        ran.len(),
        3,
        "the read, the batch, and the re-read: {ran:?}"
    );
    assert!(ran[1].starts_with("BEGIN BATCH"), "{}", ran[1]);
    assert_eq!(ran[1].matches("DELETE FROM").count(), 2, "{}", ran[1]);
    assert_eq!(
        ran[2], "SELECT * FROM \"shop\".\"orders\" LIMIT 200",
        "the grid is re-read so it stops drawing rows that are gone"
    );
}

/// The gate that makes the rest of it safe: a page that does not carry every
/// primary key column addresses no row, so `d` opens nothing (design §3.1).
#[tokio::test]
async fn a_page_without_the_whole_primary_key_deletes_nothing() {
    let cluster = FakeCluster::new(
        tabled_schema_of(&[("shop", &["orders"])]),
        Posture::Writable,
    )
    .returning(Page {
        columns: vec!["email".to_owned()],
        rows: vec![Row {
            values: vec![CqlValue::Text("ada@example.com".to_owned())],
        }],
        next: None,
    });
    let mut state = on_a_table(&cluster).await;
    state.geometry = state.geometry.with_drop_fits(true);

    let effects = press(&mut state, Key::Enter);
    run(&cluster, &mut state, effects).await;
    let before = cluster.executed().len();

    let effects = press(&mut state, Key::Char('d'));
    run(&cluster, &mut state, effects).await;

    assert!(view_model(&state).overlay.is_none());
    assert_eq!(cluster.executed().len(), before);
}

/// The fixture table's key is `(id uuid, created_at timestamp)`.
fn keyed_rows(count: usize) -> Page {
    Page {
        columns: vec!["id".to_owned(), "created_at".to_owned(), "email".to_owned()],
        rows: (0..count)
            .map(|index| Row {
                values: vec![
                    CqlValue::Uuid(format!("8f14e45f-000{index}")),
                    CqlValue::Text("2023-11-14T22:13:20.000Z".to_owned()),
                    CqlValue::Text("ada@example.com".to_owned()),
                ],
            })
            .collect(),
        next: None,
    }
}

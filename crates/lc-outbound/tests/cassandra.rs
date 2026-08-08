//! The integration tier (architecture.md §10): a real cluster, real CQL.
//!
//! Run with `mise run test:it` after `mise run db:up`.

#![cfg(feature = "integration")]

use std::collections::BTreeMap;

use lc_domain::model::{
    ClusteringOrder, ConnId, CqlValue, DbError, Keyspace, KeyspaceKind, Replication, Statement,
};
use lc_domain::ports::Cluster;
use lc_domain::posture::Posture;
use lc_domain::update::ROW_LIMIT;
use lc_outbound::cassandra::CassandraCluster;

fn contact_point() -> String {
    let port = std::env::var("LC_CASSANDRA_PORT").unwrap_or_else(|_| "9042".to_owned());
    format!("127.0.0.1:{port}")
}

#[tokio::test]
async fn the_fixture_cluster_reports_the_keyspaces_the_seed_created() {
    let conn = ConnId::next();
    let cluster = CassandraCluster::new(conn, contact_point(), Posture::ReadOnly);

    cluster.connect(conn).await.expect("the fixture is up");
    let schema = cluster.describe(conn).await.expect("describe succeeds");

    let shop = schema.keyspaces.get("shop").expect("shop exists");
    assert_eq!(
        shop.replication,
        Replication::NetworkTopologyStrategy {
            factors: [("datacenter1".to_owned(), 1)].into_iter().collect(),
        }
    );

    let typezoo = schema.keyspaces.get("typezoo").expect("typezoo exists");
    assert_eq!(
        typezoo.replication,
        Replication::SimpleStrategy { factor: 1 }
    );

    assert!(schema.keyspaces.contains_key("edge_cases"));
}

#[tokio::test]
async fn the_clusters_own_keyspaces_sort_after_the_fixtures() {
    let conn = ConnId::next();
    let cluster = CassandraCluster::new(conn, contact_point(), Posture::ReadOnly);

    cluster.connect(conn).await.expect("the fixture is up");
    let schema = cluster.describe(conn).await.expect("describe succeeds");

    let kinds: Vec<KeyspaceKind> = schema
        .keyspaces_in_display_order()
        .map(Keyspace::kind)
        .collect();
    let first_system = kinds
        .iter()
        .position(|kind| *kind == KeyspaceKind::System)
        .expect("a real cluster has system keyspaces");

    assert!(
        kinds[..first_system]
            .iter()
            .all(|kind| *kind == KeyspaceKind::User),
        "system keyspaces must not be interleaved with the user's"
    );
}

#[allow(
    clippy::expect_used,
    reason = "allow-expect-in-tests only covers #[test]-attributed items, not helpers they call"
)]
async fn fixture_schema() -> lc_domain::model::Schema {
    let conn = ConnId::next();
    let cluster = CassandraCluster::new(conn, contact_point(), Posture::ReadOnly);

    cluster.connect(conn).await.expect("the fixture is up");
    cluster.describe(conn).await.expect("describe succeeds")
}

#[tokio::test]
async fn a_keyspaces_tables_arrive_with_the_schema_and_exclude_its_views() {
    let schema = fixture_schema().await;

    let shop = schema.keyspaces.get("shop").expect("shop exists");

    assert_eq!(
        shop.tables.keys().map(String::as_str).collect::<Vec<_>>(),
        [
            "audit_log_by_day",
            "customers_by_id",
            "inventory_by_warehouse",
            "order_items_by_order",
            "orders_by_customer",
            "product_view_counts",
            "products_by_category",
        ],
        "customers_by_email is a materialized view, not a table"
    );
}

#[tokio::test]
async fn a_tables_clustering_order_survives_the_round_trip() {
    let schema = fixture_schema().await;

    let orders = schema
        .keyspaces
        .get("shop")
        .expect("shop exists")
        .tables
        .get("orders_by_customer")
        .expect("orders_by_customer exists");

    assert_eq!(
        orders
            .primary_key
            .partition
            .iter()
            .map(|c| c.name.as_str())
            .collect::<Vec<_>>(),
        ["customer_id"]
    );
    assert_eq!(
        orders
            .primary_key
            .clustering
            .iter()
            .map(|c| (c.column.name.as_str(), c.order))
            .collect::<Vec<_>>(),
        [
            ("order_date", ClusteringOrder::Descending),
            ("order_id", ClusteringOrder::Ascending),
        ],
        "the driver's metadata drops this, so the extra query is what earns it"
    );
}

#[tokio::test]
async fn static_columns_are_reported_apart_from_regular_ones() {
    let schema = fixture_schema().await;

    let table = schema
        .keyspaces
        .get("typezoo")
        .expect("typezoo exists")
        .tables
        .get("static_columns")
        .expect("static_columns exists");

    assert_eq!(
        table
            .static_columns
            .iter()
            .map(|c| c.name.as_str())
            .collect::<Vec<_>>(),
        ["calibrated_at", "device_model", "firmware"]
    );
    assert_eq!(
        table
            .regular_columns
            .iter()
            .map(|c| c.name.as_str())
            .collect::<Vec<_>>(),
        ["humidity_pct", "temperature_c"],
        "regular columns come back alphabetical, which is all Cassandra knows"
    );
}

#[tokio::test]
async fn a_column_reports_the_cql_type_the_cluster_stores() {
    let schema = fixture_schema().await;

    let table = schema
        .keyspaces
        .get("typezoo")
        .expect("typezoo exists")
        .tables
        .get("collection_types")
        .expect("collection_types exists");

    let frozen = table
        .regular_columns
        .iter()
        .find(|c| c.name == "frozen_list")
        .expect("frozen_list exists");

    assert_eq!(frozen.cql_type, "frozen<list<text>>");
}

#[expect(
    clippy::expect_used,
    reason = "allow-expect-in-tests only covers #[test]-attributed items, not helpers they call"
)]
async fn read(keyspace: &str, table: &str) -> lc_domain::model::Page {
    let conn = ConnId::next();
    let cluster = CassandraCluster::new(conn, contact_point(), Posture::ReadOnly);

    cluster.connect(conn).await.expect("the fixture is up");
    cluster
        .execute(conn, &Statement::select_all(keyspace, table, ROW_LIMIT))
        .await
        .expect("the read succeeds")
}

/// One row's cells as the grid would draw them, keyed by column name.
#[expect(
    clippy::expect_used,
    reason = "allow-expect-in-tests only covers #[test]-attributed items, not helpers they call"
)]
fn row_by_id(page: &lc_domain::model::Page, id: i64) -> BTreeMap<String, CqlValue> {
    let position = page
        .columns
        .iter()
        .position(|name| name == "id")
        .expect("every fixture table keys on id");

    let row = page
        .rows
        .iter()
        .find(|row| row.values[position] == CqlValue::Int(id))
        .expect("the seed wrote this row");

    page.columns
        .iter()
        .cloned()
        .zip(row.values.iter().cloned())
        .collect()
}

fn rendered(row: &BTreeMap<String, CqlValue>, column: &str) -> String {
    lc_domain::cell::text(&row[column])
}

#[tokio::test]
async fn a_read_returns_the_tables_own_column_order() {
    let page = read("shop", "customers_by_id").await;

    assert_eq!(
        page.columns.first().map(String::as_str),
        Some("customer_id"),
        "SELECT * leads with the partition key, as the table declares it"
    );
    assert!(page.next.is_none(), "a LIMIT leaves no cursor to follow");
}

#[tokio::test]
async fn every_scalar_type_renders_as_text_a_human_can_read() {
    let page = read("typezoo", "all_scalar_types").await;
    let row = row_by_id(&page, 1);

    assert_eq!(rendered(&row, "col_ascii"), "plain-ascii");
    assert_eq!(rendered(&row, "col_text"), "ordinary text");
    assert_eq!(rendered(&row, "col_int"), "42");
    assert_eq!(rendered(&row, "col_bigint"), "4294967296");
    assert_eq!(rendered(&row, "col_smallint"), "512");
    assert_eq!(rendered(&row, "col_tinyint"), "7");
    assert_eq!(rendered(&row, "col_boolean"), "true");
    assert_eq!(rendered(&row, "col_blob"), "0x00010203ff");
    assert_eq!(rendered(&row, "col_inet"), "192.168.1.1");
    assert_eq!(rendered(&row, "col_date"), "2026-03-14");
    assert_eq!(rendered(&row, "col_time"), "13:30:54.234000000");
    assert_eq!(rendered(&row, "col_timestamp"), "2026-03-14T13:30:54.234Z");
    assert_eq!(
        rendered(&row, "col_uuid"),
        "d0000000-0000-4000-8000-000000000001"
    );
    assert_eq!(
        rendered(&row, "col_varint"),
        "123456789012345678901234567890",
        "a varint outruns i64 and must not be truncated to fit one"
    );
    assert_eq!(
        rendered(&row, "col_decimal"),
        "3.14159265358979323846",
        "a decimal keeps every digit it was written with"
    );
    // 89h4m48s, as the seed wrote it.
    assert_eq!(rendered(&row, "col_duration"), "89h4m48s");
}

#[tokio::test]
async fn the_extremes_of_each_type_render_rather_than_overflow() {
    let page = read("typezoo", "all_scalar_types").await;
    let row = row_by_id(&page, 2);

    assert_eq!(rendered(&row, "col_bigint"), "-9223372036854775808");
    assert_eq!(rendered(&row, "col_blob"), "0x");
    assert_eq!(rendered(&row, "col_date"), "1970-01-01");
    assert_eq!(rendered(&row, "col_time"), "00:00:00.000000000");
    assert_eq!(rendered(&row, "col_timestamp"), "1970-01-01T00:00:00.000Z");
    assert_eq!(
        rendered(&row, "col_varint"),
        "-99999999999999999999999999999999999999"
    );
    assert_eq!(
        rendered(&row, "col_inet"),
        "2001:db8:85a3::8a2e:370:7334",
        "an IPv6 address renders in its own notation"
    );

    let far_future = row_by_id(&page, 3);
    assert_eq!(rendered(&far_future, "col_date"), "9999-12-31");
    assert_eq!(
        rendered(&far_future, "col_timestamp"),
        "9999-12-31T23:59:59.999Z"
    );
}

#[tokio::test]
async fn collections_keep_their_shape_across_the_boundary() {
    let page = read("typezoo", "collection_types").await;
    let row = row_by_id(&page, 1);

    assert_eq!(rendered(&row, "list_of_text"), "[alpha, beta, gamma]");
    assert_eq!(rendered(&row, "list_of_int"), "[1, 2, 3, 5, 8, 13, 21]");
    assert_eq!(rendered(&row, "map_text_int"), "{reads: 1204, writes: 87}");
    assert_eq!(rendered(&row, "frozen_list"), "[frozen, list]");
    assert_eq!(rendered(&row, "frozen_set"), "{1, 2, 3}");
    assert_eq!(rendered(&row, "frozen_map"), "{k: v}");
    assert_eq!(
        rendered(&row, "map_uuid_text"),
        "{d0000000-0000-4000-8000-000000000001: primary}",
        "a map our model cannot key is rendered whole rather than flattened"
    );
    assert_eq!(
        rendered(&row, "list_of_frozen_map"),
        "[{a: 1}, {b: 2, c: 3}]"
    );
}

#[tokio::test]
async fn user_defined_types_and_tuples_render_rather_than_being_refused() {
    let udt = read("typezoo", "udt_types").await;
    let row = row_by_id(&udt, 1);

    let home = rendered(&row, "home");
    assert!(home.starts_with('{'), "a UDT renders as its fields: {home}");
    assert!(home.contains("street: "), "and names each of them: {home}");

    let tuples = read("typezoo", "tuple_types").await;
    let row = row_by_id(&tuples, 1);

    let pair = rendered(&row, "pair");
    assert!(
        pair.starts_with('('),
        "a tuple keeps its own brackets: {pair}"
    );
}

#[tokio::test]
async fn a_null_is_distinct_from_an_empty_value() {
    let page = read("typezoo", "nulls_and_empties").await;

    let never_written = row_by_id(&page, 2);
    assert_eq!(never_written["maybe_text"], CqlValue::Null);
    assert_eq!(never_written["maybe_int"], CqlValue::Null);

    let explicit_null = row_by_id(&page, 3);
    assert_eq!(explicit_null["maybe_text"], CqlValue::Null);

    let empty = row_by_id(&page, 4);
    assert_eq!(
        empty["maybe_text"],
        CqlValue::Text(String::new()),
        "an empty string is a value the user wrote, not an absence"
    );
    assert_eq!(empty["maybe_blob"], CqlValue::Blob(Vec::new()));
    assert_eq!(
        empty["maybe_list"],
        CqlValue::Null,
        "Cassandra stores an empty collection as no collection at all"
    );
}

#[tokio::test]
async fn a_read_is_bounded_by_the_limit_it_carries() {
    let conn = ConnId::next();
    let cluster = CassandraCluster::new(conn, contact_point(), Posture::ReadOnly);
    cluster.connect(conn).await.expect("the fixture is up");

    let page = cluster
        .execute(
            conn,
            &Statement::select_all("typezoo", "all_scalar_types", 1),
        )
        .await
        .expect("the read succeeds");

    assert_eq!(page.rows.len(), 1);
}

#[tokio::test]
async fn a_statement_the_cluster_rejects_comes_back_as_an_error_not_a_panic() {
    let conn = ConnId::next();
    let cluster = CassandraCluster::new(conn, contact_point(), Posture::ReadOnly);
    cluster.connect(conn).await.expect("the fixture is up");

    let result = cluster
        .execute(conn, &Statement::select_all("typezoo", "no_such_table", 1))
        .await;

    assert!(matches!(result, Err(DbError::Invalid(_))));
}

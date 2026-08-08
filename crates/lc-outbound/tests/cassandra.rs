//! The integration tier (architecture.md §10): a real cluster, real CQL.
//!
//! Run with `mise run test:it` after `mise run db:up`.

#![cfg(feature = "integration")]

use lc_domain::model::{ClusteringOrder, ConnId, Keyspace, KeyspaceKind, Replication};
use lc_domain::ports::Cluster;
use lc_domain::posture::Posture;
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

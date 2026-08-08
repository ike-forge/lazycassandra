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

#[tokio::test]
async fn a_table_reports_the_options_the_seed_set_and_the_ones_cassandra_defaulted() {
    let schema = fixture_schema().await;

    let orders = schema
        .keyspaces
        .get("shop")
        .expect("shop exists")
        .tables
        .get("orders_by_customer")
        .expect("orders_by_customer exists");

    assert_eq!(
        orders.options.get("comment"),
        Some("Order history for one customer, newest day first."),
        "10-shop-schema.cql sets this one"
    );
    assert_eq!(orders.options.get("gc_grace_seconds"), Some("864000"));

    let compaction = orders
        .options
        .get("compaction")
        .expect("every table has a compaction strategy");
    assert!(
        compaction.contains("SizeTieredCompactionStrategy"),
        "a map option keeps its entries rather than collapsing to a class name: {compaction}"
    );
    assert!(
        compaction.starts_with("{'class': "),
        "map entries are sorted, so class leads: {compaction}"
    );
}

#[tokio::test]
async fn every_table_in_the_fixture_describes_itself() {
    let schema = fixture_schema().await;

    for keyspace in schema.keyspaces.values() {
        for table in keyspace.tables.values() {
            assert!(
                !table.options.is_empty(),
                "{}.{} reported no options at all",
                keyspace.name,
                table.name
            );
        }
    }
}

#[tokio::test]
async fn the_option_set_covers_what_the_importance_ranking_expects_of_cassandra_5() {
    let schema = fixture_schema().await;

    let table = schema
        .keyspaces
        .get("shop")
        .expect("shop exists")
        .tables
        .get("customers_by_id")
        .expect("customers_by_id exists");

    // Not the whole ranking: it deliberately also names properties older and
    // newer majors carry, which this cluster does not have.
    for name in [
        "additional_write_policy",
        "allow_auto_snapshot",
        "bloom_filter_fp_chance",
        "caching",
        "cdc",
        "comment",
        "compaction",
        "compression",
        "crc_check_chance",
        "default_time_to_live",
        "extensions",
        "flags",
        "gc_grace_seconds",
        "id",
        "incremental_backups",
        "max_index_interval",
        "memtable_flush_period_in_ms",
        "min_index_interval",
        "read_repair",
        "speculative_retry",
    ] {
        assert!(
            table.options.get(name).is_some(),
            "cassandra:5.0.8 reports {name} and the wildcard select must carry it through"
        );
    }

    assert_eq!(
        table.options.get("keyspace_name"),
        None,
        "the identifying columns are the panel's heading, not properties"
    );
    assert_eq!(table.options.get("table_name"), None);
}

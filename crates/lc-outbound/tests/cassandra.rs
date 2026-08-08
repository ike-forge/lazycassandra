//! The integration tier (architecture.md §10): a real cluster, real CQL.
//!
//! Run with `mise run test:it` after `mise run db:up`.

#![cfg(feature = "integration")]

use lc_domain::model::{ConnId, Keyspace, KeyspaceKind, Replication};
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

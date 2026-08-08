//! Whole sessions driven through the in-memory cluster (architecture.md §10).
//!
//! Deterministic: no clock, no socket, no runtime beyond the one `#[tokio::test]`
//! provides to await the port.

use std::collections::{BTreeMap, VecDeque};

use lc_domain::model::{ConnId, DbError, Keyspace, Replication, Schema};
use lc_domain::ports::Cluster;
use lc_domain::posture::Posture;
use lc_domain::view::PanelBody;
use lc_domain::{Effect, Msg, State, init, update, view_model};
use lc_outbound::fake::FakeCluster;

fn schema_of(names: &[&str]) -> Schema {
    Schema {
        keyspaces: names
            .iter()
            .map(|name| {
                (
                    (*name).to_owned(),
                    Keyspace {
                        name: (*name).to_owned(),
                        replication: Replication::SimpleStrategy { factor: 1 },
                        tables: BTreeMap::new(),
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

    let PanelBody::Keyspaces { rows, selected } = &view_model(&state).panels[0].body else {
        panic!("a described cluster lists its keyspaces");
    };
    assert_eq!(
        rows.iter().map(|row| row.name.as_str()).collect::<Vec<_>>(),
        ["shop", "typezoo", "system_schema"]
    );
    assert_eq!(*selected, 0);
}

#[tokio::test]
async fn a_cluster_that_cannot_be_reached_raises_an_overlay() {
    let cluster = FakeCluster::failing(DbError::Unreachable("no route to host".to_owned()));
    let conn = ConnId::next();
    let (mut state, effects) = init(conn);

    run(&cluster, &mut state, effects).await;

    let overlay = view_model(&state).overlay.expect("the failure is shown");
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

    let overlay = view_model(&state).overlay.expect("the failure is shown");
    assert_eq!(overlay.title, "Schema unavailable");
}

//! The `Cluster` port over `scylla`.
//!
//! Driver types stop here: everything crossing back into the domain is our own
//! vocabulary (architecture.md §5).

use std::collections::BTreeMap;
use std::sync::Arc;

use async_trait::async_trait;
use lc_domain::model::{
    ConnId, DbError, Keyspace, Page, PagingState, Replication, Schema, Statement,
};
use lc_domain::ports::Cluster;
use lc_domain::posture::Posture;
use scylla::client::session::Session;
use scylla::client::session_builder::SessionBuilder;
use scylla::cluster::ClusterState;
use scylla::cluster::metadata::{Keyspace as DriverKeyspace, Strategy};
use tokio::sync::RwLock;

/// One cluster, reached over CQL.
///
/// `Cluster::connect` carries a [`ConnId`] and no address, so the adapter has
/// to already know where that id points; `&self` is why the session sits behind
/// a lock. One target rather than a map is the honest match for a `State` that
/// also holds exactly one connection.
#[derive(Debug)]
pub struct CassandraCluster {
    conn: ConnId,
    contact_point: String,
    posture: Posture,
    session: RwLock<Option<Arc<Session>>>,
}

impl CassandraCluster {
    #[must_use]
    pub fn new(conn: ConnId, contact_point: String, posture: Posture) -> Self {
        Self {
            conn,
            contact_point,
            posture,
            session: RwLock::new(None),
        }
    }

    fn check(&self, conn: ConnId) -> Result<(), DbError> {
        if conn == self.conn {
            Ok(())
        } else {
            Err(DbError::Other(
                "this adapter serves one connection".to_owned(),
            ))
        }
    }

    async fn session(&self, conn: ConnId) -> Result<Arc<Session>, DbError> {
        self.check(conn)?;
        self.session
            .read()
            .await
            .clone()
            .ok_or_else(|| DbError::Unreachable("not connected".to_owned()))
    }
}

#[async_trait]
impl Cluster for CassandraCluster {
    async fn connect(&self, conn: ConnId) -> Result<Posture, DbError> {
        self.check(conn)?;

        // Boxed to satisfy clippy::large_futures: SessionBuilder::build's
        // future is large enough to be worth moving off this async fn's stack.
        let session = Box::pin(
            SessionBuilder::new()
                .known_node(&self.contact_point)
                .build(),
        )
        .await
        .map_err(|error| DbError::Unreachable(error.to_string()))?;

        *self.session.write().await = Some(Arc::new(session));

        Ok(self.posture)
    }

    async fn describe(&self, conn: ConnId) -> Result<Schema, DbError> {
        let session = self.session(conn).await?;
        session
            .refresh_metadata()
            .await
            .map_err(|error| DbError::Other(error.to_string()))?;

        Ok(schema(&session.get_cluster_state()))
    }

    async fn execute(&self, _conn: ConnId, _stmt: &Statement) -> Result<Page, DbError> {
        Err(DbError::Other(
            "executing statements is not implemented yet".to_owned(),
        ))
    }

    async fn fetch_page(&self, _conn: ConnId, _cursor: &PagingState) -> Result<Page, DbError> {
        Err(DbError::Other("paging is not implemented yet".to_owned()))
    }
}

fn schema(state: &ClusterState) -> Schema {
    Schema {
        keyspaces: state
            .keyspaces_iter()
            .map(|(name, keyspace)| (name.to_owned(), to_keyspace(name, keyspace)))
            .collect(),
    }
}

fn to_keyspace(name: &str, keyspace: &DriverKeyspace) -> Keyspace {
    Keyspace {
        name: name.to_owned(),
        replication: replication(&keyspace.strategy),
        // Tables arrive with the unfold slice: mapping them means turning the
        // driver's partition_key/clustering_key name lists and its ColumnKind
        // map into our PrimaryKey, which drawing a tree root does not need.
        tables: BTreeMap::new(),
    }
}

fn replication(strategy: &Strategy) -> Replication {
    match strategy {
        Strategy::SimpleStrategy { replication_factor } => Replication::SimpleStrategy {
            factor: factor(*replication_factor),
        },
        Strategy::NetworkTopologyStrategy {
            datacenter_repfactors,
        } => Replication::NetworkTopologyStrategy {
            factors: datacenter_repfactors
                .iter()
                .map(|(datacenter, rf)| (datacenter.clone(), factor(*rf)))
                .collect(),
        },
        Strategy::LocalStrategy => Replication::Other {
            class: "LocalStrategy".to_owned(),
        },
        Strategy::Other { name, .. } => Replication::Other {
            class: name.clone(),
        },
        // `Strategy` is #[non_exhaustive]; a variant we have never seen
        // still deserves a name. Unlike the `Other` arm above, there is no
        // field to read one from, so this reports the full Debug rendering
        // — braces and fields included — rather than a bare name.
        other => Replication::Other {
            class: format!("{other:?}"),
        },
    }
}

/// Replication factors arrive as `usize`. Nothing near `u32::MAX` is a real
/// factor, so saturating is the honest conversion.
fn factor(value: usize) -> u32 {
    u32::try_from(value).unwrap_or(u32::MAX)
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use scylla::cluster::metadata::Strategy;

    use super::*;

    #[test]
    fn a_simple_strategy_reports_its_factor() {
        let strategy = Strategy::SimpleStrategy {
            replication_factor: 3,
        };

        assert_eq!(
            replication(&strategy),
            Replication::SimpleStrategy { factor: 3 }
        );
    }

    #[test]
    fn a_network_topology_strategy_preserves_every_datacenters_factor() {
        let strategy = Strategy::NetworkTopologyStrategy {
            datacenter_repfactors: HashMap::from([
                ("datacenter1".to_owned(), 3),
                ("datacenter2".to_owned(), 2),
            ]),
        };

        assert_eq!(
            replication(&strategy),
            Replication::NetworkTopologyStrategy {
                factors: BTreeMap::from([
                    ("datacenter1".to_owned(), 3),
                    ("datacenter2".to_owned(), 2),
                ]),
            }
        );
    }

    #[test]
    fn a_local_strategy_is_reported_by_its_cassandra_name() {
        assert_eq!(
            replication(&Strategy::LocalStrategy),
            Replication::Other {
                class: "LocalStrategy".to_owned(),
            }
        );
    }

    #[test]
    fn an_unrecognised_named_strategy_keeps_its_name_and_drops_its_data() {
        let strategy = Strategy::Other {
            name: "SomeOtherStrategy".to_owned(),
            data: HashMap::from([("some_option".to_owned(), "some_value".to_owned())]),
        };

        assert_eq!(
            replication(&strategy),
            Replication::Other {
                class: "SomeOtherStrategy".to_owned(),
            }
        );
    }

    #[test]
    fn factor_saturates_at_u32_max_instead_of_overflowing() {
        assert_eq!(factor(3), 3);
        assert_eq!(factor(usize::MAX), u32::MAX);
    }

    // Not gated behind `integration`: `check` refuses before the adapter ever
    // opens a socket, so this needs no cluster and belongs in `mise run ci`.
    #[tokio::test]
    async fn connecting_with_an_id_the_adapter_does_not_serve_is_refused() {
        let conn = ConnId::next();
        let cluster = CassandraCluster::new(conn, "127.0.0.1:0".to_owned(), Posture::ReadOnly);

        assert!(cluster.connect(ConnId::next()).await.is_err());
    }
}

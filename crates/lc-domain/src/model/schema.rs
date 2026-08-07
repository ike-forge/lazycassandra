//! Schema, with the physical layout made structural.
//!
//! vision.md §2 turns on the partition key being impossible to overlook, so
//! [`PrimaryKey`] is a type that distinguishes partition from clustering
//! columns rather than a `Vec<Column>` that has to be re-interpreted at render
//! time (architecture.md §4).

use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Schema {
    /// Ordered so iteration and rendering are deterministic
    /// (conventions.md §4).
    pub keyspaces: BTreeMap<String, Keyspace>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Keyspace {
    pub name: String,
    pub replication: Replication,
    pub tables: BTreeMap<String, Table>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Replication {
    SimpleStrategy {
        factor: u32,
    },
    NetworkTopologyStrategy {
        /// Replication factor per datacenter.
        factors: BTreeMap<String, u32>,
    },
    Other {
        class: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Table {
    pub name: String,
    pub primary_key: PrimaryKey,
    /// Everything not part of the primary key.
    pub regular_columns: Vec<Column>,
}

/// The physical shape of a table.
///
/// Partition columns decide *where* a row lives; clustering columns decide
/// *how it is ordered within* that partition. Conflating the two is the
/// mistake the whole tool exists to prevent, so they are separate fields.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrimaryKey {
    /// Never empty: a Cassandra table always has at least one partition
    /// column.
    pub partition: Vec<Column>,
    pub clustering: Vec<ClusteringColumn>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClusteringColumn {
    pub column: Column,
    pub order: ClusteringOrder,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ClusteringOrder {
    #[default]
    Ascending,
    Descending,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Column {
    pub name: String,
    /// The CQL type as the cluster reports it, e.g. `frozen<list<text>>`.
    pub cql_type: String,
}

impl Table {
    /// Every column, primary key first, in the order the table declares them.
    pub fn columns(&self) -> impl Iterator<Item = &Column> {
        self.primary_key
            .partition
            .iter()
            .chain(self.primary_key.clustering.iter().map(|c| &c.column))
            .chain(self.regular_columns.iter())
    }
}

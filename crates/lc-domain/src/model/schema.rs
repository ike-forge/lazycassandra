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

/// Whether a keyspace is the cluster's own or somebody's schema.
///
/// Derived from the name rather than stored on [`Keyspace`], so an adapter
/// cannot report a kind that disagrees with it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum KeyspaceKind {
    User,
    System,
}

impl KeyspaceKind {
    /// Classifies a keyspace by name. The separator matters: `system_auth` is
    /// Cassandra's, `systematic` is not.
    #[must_use]
    pub fn of(name: &str) -> Self {
        if name == "system" || name.starts_with("system_") {
            Self::System
        } else {
            Self::User
        }
    }
}

impl Keyspace {
    /// Whether the cluster owns this keyspace or the user does.
    #[must_use]
    pub fn kind(&self) -> KeyspaceKind {
        KeyspaceKind::of(&self.name)
    }
}

impl Schema {
    /// User keyspaces first, then the cluster's own, each alphabetically.
    ///
    /// The alphabetical part is free: `keyspaces` is a `BTreeMap`, so two
    /// filtered passes are cheaper and clearer than partitioning into vectors.
    pub fn keyspaces_in_display_order(&self) -> impl Iterator<Item = &Keyspace> {
        let user = self
            .keyspaces
            .values()
            .filter(|keyspace| keyspace.kind() == KeyspaceKind::User);
        let system = self
            .keyspaces
            .values()
            .filter(|keyspace| keyspace.kind() == KeyspaceKind::System);

        user.chain(system)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn keyspace(name: &str) -> Keyspace {
        Keyspace {
            name: name.to_owned(),
            replication: Replication::SimpleStrategy { factor: 1 },
            tables: BTreeMap::new(),
        }
    }

    fn schema(names: &[&str]) -> Schema {
        Schema {
            keyspaces: names
                .iter()
                .map(|name| ((*name).to_owned(), keyspace(name)))
                .collect(),
        }
    }

    #[test]
    fn the_clusters_own_keyspaces_are_system_and_the_system_prefixed_ones() {
        assert_eq!(KeyspaceKind::of("system"), KeyspaceKind::System);
        assert_eq!(KeyspaceKind::of("system_schema"), KeyspaceKind::System);
        assert_eq!(
            KeyspaceKind::of("system_virtual_schema"),
            KeyspaceKind::System
        );
    }

    #[test]
    fn a_user_keyspace_merely_starting_with_system_is_not_the_clusters() {
        // The separator is load-bearing: `systematic` is somebody's schema.
        assert_eq!(KeyspaceKind::of("systematic"), KeyspaceKind::User);
        assert_eq!(KeyspaceKind::of("shop"), KeyspaceKind::User);
    }

    #[test]
    fn a_keyspaces_kind_follows_its_name() {
        assert_eq!(keyspace("shop").kind(), KeyspaceKind::User);
        assert_eq!(keyspace("system_auth").kind(), KeyspaceKind::System);
    }

    #[test]
    fn display_order_puts_user_keyspaces_before_the_clusters_own() {
        let schema = schema(&["system_schema", "typezoo", "system", "shop"]);

        let order: Vec<&str> = schema
            .keyspaces_in_display_order()
            .map(|keyspace| keyspace.name.as_str())
            .collect();

        assert_eq!(order, ["shop", "typezoo", "system", "system_schema"]);
    }

    #[test]
    fn display_order_of_an_empty_schema_is_empty() {
        assert_eq!(Schema::default().keyspaces_in_display_order().count(), 0);
    }
}

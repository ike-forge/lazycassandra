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
    /// Columns stored once per partition rather than once per row.
    ///
    /// Separate from `regular_columns` because static-ness is a fact about
    /// physical layout, which is the thing this model exists to keep visible.
    pub static_columns: Vec<Column>,
    /// Everything that is neither part of the primary key nor static.
    pub regular_columns: Vec<Column>,
    pub options: TableOptions,
}

/// Every property `system_schema.tables` reports for one table, as text.
///
/// Text rather than a struct of typed fields, and not by omission: the column
/// set of `system_schema.tables` differs across Cassandra majors — 3.11 has
/// `read_repair_chance`, 4.0 replaces it with `read_repair`, 5.0 adds
/// `memtable` — so a fixed field list would encode a claim that is false, and
/// would silently drop whatever the running cluster knows and we do not. The
/// ordering below is the part that is real knowledge
/// (doc/specs/2026-08-09-table-metadata-design.md §3).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TableOptions {
    by_name: BTreeMap<String, String>,
}

/// Table options worth seeing first, most important first.
///
/// Leads with what silently costs data or latency — what the table is for, the
/// TTL that deletes rows out from under you, the tombstone window, the
/// compaction strategy — and trails with what gets looked up once a year.
const BY_IMPORTANCE: &[&str] = &[
    "comment",
    "default_time_to_live",
    "gc_grace_seconds",
    "compaction",
    "compression",
    "caching",
    "bloom_filter_fp_chance",
    "speculative_retry",
    "additional_write_policy",
    "read_repair",
    "cdc",
    "crc_check_chance",
    "min_index_interval",
    "max_index_interval",
    "memtable",
    "memtable_flush_period_in_ms",
    "allow_auto_snapshot",
    "incremental_backups",
    "flags",
    "id",
    "extensions",
];

impl TableOptions {
    #[must_use]
    pub const fn new(by_name: BTreeMap<String, String>) -> Self {
        Self { by_name }
    }

    /// Ranked properties first in [`BY_IMPORTANCE`] order, then everything
    /// else alphabetically.
    ///
    /// The tail is what keeps a cluster whose properties predate or postdate
    /// that list fully visible rather than half-shown.
    pub fn in_display_order(&self) -> impl Iterator<Item = (&str, &str)> {
        let ranked = BY_IMPORTANCE
            .iter()
            .filter_map(|name| self.get(name).map(|value| (*name, value)));

        let rest = self
            .by_name
            .iter()
            .filter(|(name, _)| !BY_IMPORTANCE.contains(&name.as_str()))
            .map(|(name, value)| (name.as_str(), value.as_str()));

        ranked.chain(rest)
    }

    #[must_use]
    pub fn get(&self, name: &str) -> Option<&str> {
        self.by_name.get(name).map(String::as_str)
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.by_name.is_empty()
    }
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
    /// Every column: partition key, then clustering key, then static, then
    /// regular.
    pub fn columns(&self) -> impl Iterator<Item = &Column> {
        self.primary_key
            .partition
            .iter()
            .chain(self.primary_key.clustering.iter().map(|c| &c.column))
            .chain(self.static_columns.iter())
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

    fn column(name: &str, cql_type: &str) -> Column {
        Column {
            name: name.to_owned(),
            cql_type: cql_type.to_owned(),
        }
    }

    fn readings() -> Table {
        Table {
            name: "readings".to_owned(),
            primary_key: PrimaryKey {
                partition: vec![column("device_id", "text")],
                clustering: vec![ClusteringColumn {
                    column: column("reading_at", "timestamp"),
                    order: ClusteringOrder::Descending,
                }],
            },
            static_columns: vec![column("firmware", "text")],
            regular_columns: vec![column("temperature_c", "float")],
            options: TableOptions::default(),
        }
    }

    fn options(pairs: &[(&str, &str)]) -> TableOptions {
        TableOptions::new(
            pairs
                .iter()
                .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
                .collect(),
        )
    }

    fn ordered(options: &TableOptions) -> Vec<&str> {
        options.in_display_order().map(|(name, _)| name).collect()
    }

    #[test]
    fn options_are_listed_by_importance_rather_than_by_name() {
        let options = options(&[
            ("bloom_filter_fp_chance", "0.01"),
            ("comment", "orders"),
            ("gc_grace_seconds", "864000"),
        ]);

        assert_eq!(
            ordered(&options),
            ["comment", "gc_grace_seconds", "bloom_filter_fp_chance"]
        );
    }

    #[test]
    fn a_property_this_cassandra_version_does_not_have_is_simply_absent() {
        let options = options(&[("comment", "orders"), ("id", "abc")]);

        assert_eq!(ordered(&options), ["comment", "id"]);
    }

    #[test]
    fn a_property_the_ranking_has_never_heard_of_is_listed_after_the_ranked_ones() {
        // 3.11's read repair settings, which 4.0 removed: unranked here, and
        // still shown, because a cluster's own vocabulary outranks ours.
        let options = options(&[
            ("read_repair_chance", "0.0"),
            ("dclocal_read_repair_chance", "0.1"),
            ("comment", "orders"),
            ("id", "abc"),
        ]);

        assert_eq!(
            ordered(&options),
            [
                "comment",
                "id",
                "dclocal_read_repair_chance",
                "read_repair_chance"
            ],
            "unranked properties sort alphabetically after every ranked one"
        );
    }

    #[test]
    fn a_table_whose_options_row_was_missing_lists_nothing() {
        let options = TableOptions::default();

        assert!(options.is_empty());
        assert_eq!(ordered(&options), [""; 0]);
    }

    #[test]
    fn every_ranked_name_is_listed_once() {
        let mut seen = std::collections::BTreeSet::new();
        for name in BY_IMPORTANCE {
            assert!(
                seen.insert(*name),
                "{name} is ranked twice, so one of the two positions is dead"
            );
        }
    }

    #[test]
    fn every_column_is_listed_with_the_key_first_and_static_before_regular() {
        let table = readings();
        let names: Vec<&str> = table.columns().map(|c| c.name.as_str()).collect();

        assert_eq!(
            names,
            ["device_id", "reading_at", "firmware", "temperature_c"]
        );
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

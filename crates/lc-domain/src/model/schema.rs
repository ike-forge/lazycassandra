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
    /// The table's secondary indexes, by name.
    ///
    /// Empty on the great majority of tables, which is a fact the Indexes tab
    /// reports rather than hides (indexes design §2).
    pub indexes: Vec<Index>,
    pub options: TableOptions,
}

/// One secondary index over a table.
///
/// `scylla::cluster::metadata` carries none, so this is read off
/// `system_schema.indexes` the way clustering order and the CQL type text are
/// (indexes design §3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Index {
    pub name: String,
    pub target: IndexTarget,
    pub kind: IndexKind,
}

/// Who implements an index.
///
/// Two arms rather than Cassandra's three kinds: `KEYS` and `COMPOSITES` say
/// nothing [`IndexTarget`] does not already carry — a `KEYS` index is the one
/// whose target is `keys(column)` — while a custom one names a class no
/// target implies.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IndexKind {
    /// Cassandra's own, whichever of `KEYS` and `COMPOSITES` it recorded.
    Builtin,
    /// `CUSTOM`, and the class behind it: SASI, SAI, or a third party's.
    Custom { class: String },
}

/// What an index is built over.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexTarget {
    /// The column, unquoted — the name [`Table::column`] can be asked for.
    pub column: String,
    /// `None` where the whole column is indexed, which is every index over a
    /// column that is not a collection.
    pub part: Option<CollectionPart>,
}

/// Which part of a collection column an index covers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CollectionPart {
    Keys,
    Values,
    Entries,
    Full,
}

impl CollectionPart {
    /// The CQL function that names it, which is also how the target was
    /// written down.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Keys => "keys",
            Self::Values => "values",
            Self::Entries => "entries",
            Self::Full => "full",
        }
    }

    fn of(label: &str) -> Option<Self> {
        [Self::Keys, Self::Values, Self::Entries, Self::Full]
            .into_iter()
            .find(|part| part.label() == label)
    }
}

impl IndexTarget {
    /// Reads `system_schema.indexes`' `target` option: a column name, or a
    /// collection modality wrapping one — `keys(tags)`, `entries(prices)`.
    ///
    /// A target this build cannot read is kept whole as the column name. It
    /// then draws as the cluster wrote it and matches no column, which costs
    /// the type at the margin and nothing else — where dropping it would lose
    /// an index the table really has.
    #[must_use]
    pub fn parse(target: &str) -> Self {
        let split = target
            .split_once('(')
            .and_then(|(label, rest)| Some((CollectionPart::of(label)?, rest.strip_suffix(')')?)));

        match split {
            Some((part, column)) => Self {
                column: unquoted(column),
                part: Some(part),
            },
            None => Self {
                column: unquoted(target),
                part: None,
            },
        }
    }

    /// The target as the panel draws it: the modality, over the column's own
    /// name rather than the quoted form the cluster reported it under.
    #[must_use]
    pub fn text(&self) -> String {
        match self.part {
            Some(part) => format!("{}({})", part.label(), self.column),
            None => self.column.clone(),
        }
    }
}

/// A quoted identifier's own name: the quotes off and CQL's doubling undone.
///
/// Cassandra writes the target with the quotes a case-sensitive column needs,
/// and a column is stored under the name inside them.
fn unquoted(identifier: &str) -> String {
    identifier
        .strip_prefix('"')
        .and_then(|rest| rest.strip_suffix('"'))
        .map_or_else(|| identifier.to_owned(), |name| name.replace("\"\"", "\""))
}

/// Every property `system_schema.tables` reports for one table, as text.
///
/// Text rather than a struct of typed fields, and not by omission: the column
/// set of `system_schema.tables` differs across Cassandra majors — 3.11 has
/// `read_repair_chance`, 4.0 replaces it with `read_repair`, 5.0 adds
/// `memtable` — so a fixed field list would encode a claim that is false, and
/// would silently drop whatever the running cluster knows and we do not. The
/// ordering below is the part that is real knowledge
/// (docs/superpowers/specs/2026-08-09-table-metadata-design.md §3).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TableOptions {
    by_name: BTreeMap<String, String>,
}

/// Table options in the order the panel lists them, most worth seeing first.
///
/// A layout rule, not an importance rule. These are the properties whose
/// values fit on one line, so they read as a scannable block; the three in
/// [`BULKY`] wrap and would split it
/// (docs/superpowers/specs/2026-08-10-ddl-selection-design.md §2).
const RANKED: &[&str] = &[
    "default_time_to_live",
    "gc_grace_seconds",
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

/// The options long enough to wrap the panel, drawn last.
///
/// Below the unranked tail rather than above it, so a wide option from a
/// Cassandra major this build predates cannot land between the compact table
/// and the blocks under it. `comment` loses the top of the panel to buy that,
/// which is a real cost and a deliberate one (design §2).
const BULKY: &[&str] = &["comment", "compaction", "compression"];

fn listed(name: &str) -> bool {
    RANKED.contains(&name) || BULKY.contains(&name)
}

impl TableOptions {
    #[must_use]
    pub const fn new(by_name: BTreeMap<String, String>) -> Self {
        Self { by_name }
    }

    /// Ranked properties first, then whatever this build has not heard of,
    /// alphabetically, then the bulky ones.
    ///
    /// The middle chunk is what keeps a cluster whose properties predate or
    /// postdate [`RANKED`] fully visible rather than half-shown.
    pub fn in_display_order(&self) -> impl Iterator<Item = (&str, &str)> {
        let named = |names: &'static [&'static str]| {
            names
                .iter()
                .filter_map(|name| self.get(name).map(|value| (*name, value)))
        };

        let rest = self
            .by_name
            .iter()
            .filter(|(name, _)| !listed(name))
            .map(|(name, value)| (name.as_str(), value.as_str()));

        named(RANKED).chain(rest).chain(named(BULKY))
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

    /// The column `name` names, whatever part of the table it plays.
    #[must_use]
    pub fn column(&self, name: &str) -> Option<&Column> {
        self.columns().find(|column| column.name == name)
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
            indexes: Vec::new(),
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
    fn options_are_listed_by_layout_rather_than_by_name() {
        let options = options(&[
            ("bloom_filter_fp_chance", "0.01"),
            ("comment", "orders"),
            ("gc_grace_seconds", "864000"),
        ]);

        assert_eq!(
            ordered(&options),
            ["gc_grace_seconds", "bloom_filter_fp_chance", "comment"]
        );
    }

    #[test]
    fn a_property_this_cassandra_version_does_not_have_is_simply_absent() {
        let options = options(&[("comment", "orders"), ("id", "abc")]);

        assert_eq!(ordered(&options), ["id", "comment"]);
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
                "id",
                "dclocal_read_repair_chance",
                "read_repair_chance",
                "comment",
            ],
            "unranked properties sort alphabetically after every ranked one, and the bulky ones sort last of all"
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
        for name in RANKED.iter().chain(BULKY.iter()) {
            assert!(
                seen.insert(*name),
                "{name} is ranked twice, so one of the two positions is dead"
            );
        }
    }

    #[test]
    fn the_bulky_options_are_listed_below_even_the_ones_this_build_has_never_heard_of() {
        let options = TableOptions::new(BTreeMap::from([
            ("comment".to_owned(), "Order history.".to_owned()),
            (
                "compaction".to_owned(),
                "{'class': 'SizeTieredCompactionStrategy'}".to_owned(),
            ),
            (
                "compression".to_owned(),
                "{'chunk_length_in_kb': '16'}".to_owned(),
            ),
            ("gc_grace_seconds".to_owned(), "864000".to_owned()),
            ("zzz_option_from_the_future".to_owned(), "on".to_owned()),
        ]));

        assert_eq!(
            options
                .in_display_order()
                .map(|(name, _)| name)
                .collect::<Vec<_>>(),
            [
                "gc_grace_seconds",
                "zzz_option_from_the_future",
                "comment",
                "compaction",
                "compression",
            ],
            "an unranked option must not land between the compact table and the blocks below it"
        );
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
    fn a_plain_target_is_the_whole_column() {
        let target = IndexTarget::parse("status");

        assert_eq!(target.column, "status");
        assert_eq!(target.part, None);
        assert_eq!(target.text(), "status");
    }

    #[test]
    fn a_collection_target_keeps_the_part_it_covers() {
        for (text, part) in [
            ("keys(tags)", CollectionPart::Keys),
            ("values(tags)", CollectionPart::Values),
            ("entries(prices)", CollectionPart::Entries),
            ("full(path)", CollectionPart::Full),
        ] {
            let target = IndexTarget::parse(text);

            assert_eq!(target.part, Some(part));
            assert_eq!(
                target.text(),
                text,
                "the modality is drawn as CQL writes it"
            );
        }
    }

    #[test]
    fn a_case_sensitive_target_names_the_column_the_table_stores() {
        // Cassandra writes the target quoted, and `Table::column` is asked
        // for the name inside the quotes.
        assert_eq!(IndexTarget::parse("\"Status\"").column, "Status");
        assert_eq!(IndexTarget::parse("keys(\"Tags\")").column, "Tags");
        assert_eq!(
            IndexTarget::parse("\"say \"\"hi\"\"\"").column,
            "say \"hi\"",
            "CQL doubles a quote inside an identifier"
        );
    }

    #[test]
    fn a_target_this_build_cannot_read_is_kept_whole() {
        // Dropping it would lose an index the table really has; kept, it
        // draws as the cluster wrote it and matches no column.
        let target = IndexTarget::parse("something(a, b)");

        assert_eq!(target.part, None);
        assert_eq!(target.text(), "something(a, b)");
    }

    #[test]
    fn a_column_is_found_whatever_part_of_the_table_it_plays() {
        let table = readings();

        for name in ["device_id", "reading_at", "firmware", "temperature_c"] {
            assert_eq!(
                table.column(name).map(|column| column.name.as_str()),
                Some(name)
            );
        }
        assert!(table.column("gone").is_none());
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

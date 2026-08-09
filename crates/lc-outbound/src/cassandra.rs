//! The `Cluster` port over `scylla`.
//!
//! Driver types stop here: everything crossing back into the domain is our own
//! vocabulary (architecture.md §5).

use std::collections::BTreeMap;
use std::sync::Arc;

use async_trait::async_trait;
use lc_domain::model::{
    ClusteringColumn, ClusteringOrder, Column, ConnId, DbError, Keyspace, Page, PagingState,
    PrimaryKey, Replication, Row, Schema, Statement, Table, TableOptions,
};
use lc_domain::ports::Cluster;
use lc_domain::posture::Posture;
use scylla::client::session::Session;
use scylla::client::session_builder::SessionBuilder;
use scylla::cluster::ClusterState;
use scylla::cluster::metadata::{Keyspace as DriverKeyspace, Strategy};
use scylla::value::{CqlValue as DriverValue, Row as DriverRow};
use tokio::sync::RwLock;

use lc_domain::cell;

use crate::value::to_value;

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

        let columns = column_rows(&session).await?;
        let options = option_rows(&session).await?;

        schema(&session.get_cluster_state(), &columns, &options)
    }

    /// Unpaged because the statement carries its own `LIMIT`
    /// (doc/specs/2026-08-09-table-contents-design.md §3). `Page::next` is
    /// `None` for the same reason: there is no cursor to hand back.
    async fn execute(&self, conn: ConnId, stmt: &Statement) -> Result<Page, DbError> {
        let session = self.session(conn).await?;

        let result = session
            .query_unpaged(stmt.cql.as_str(), &[])
            .await
            .map_err(|error| DbError::Invalid(error.to_string()))?
            .into_rows_result()
            .map_err(|error| DbError::Invalid(error.to_string()))?;

        let columns = result
            .column_specs()
            .iter()
            .map(|spec| spec.name().to_owned())
            .collect();

        let rows = result
            .rows::<DriverRow>()
            .map_err(|error| DbError::Invalid(error.to_string()))?
            .map(|row| {
                row.map(|row| Row {
                    values: row.columns.iter().map(|v| to_value(v.as_ref())).collect(),
                })
                .map_err(|error| DbError::Invalid(error.to_string()))
            })
            .collect::<Result<Vec<_>, DbError>>()?;

        Ok(Page {
            columns,
            rows,
            next: None,
        })
    }

    async fn fetch_page(&self, _conn: ConnId, _cursor: &PagingState) -> Result<Page, DbError> {
        Err(DbError::Other("paging is not implemented yet".to_owned()))
    }
}

/// The three things `scylla::cluster::metadata` does not carry: clustering
/// order, the CQL type as text, and whether a column is static
/// (doc/specs/2026-08-08-table-list-design.md §3).
const COLUMNS_QUERY: &str = "SELECT keyspace_name, table_name, column_name, kind, type, \
                             clustering_order FROM system_schema.columns";

/// One row of `system_schema.columns`.
struct ColumnRow {
    column_name: String,
    kind: String,
    cql_type: String,
    clustering_order: String,
}

/// Every column in the cluster, grouped by the table it belongs to.
///
/// Materialized view columns are keyed by the view's own name, so they match
/// no entry in `keyspace.tables` and fall out of the join.
async fn column_rows(
    session: &Session,
) -> Result<BTreeMap<(String, String), Vec<ColumnRow>>, DbError> {
    let result = session
        .query_unpaged(COLUMNS_QUERY, &[])
        .await
        .map_err(|error| DbError::Other(error.to_string()))?
        .into_rows_result()
        .map_err(|error| DbError::Other(error.to_string()))?;

    let rows = result
        .rows::<(String, String, String, String, String, String)>()
        .map_err(|error| DbError::Other(error.to_string()))?;

    let mut grouped: BTreeMap<(String, String), Vec<ColumnRow>> = BTreeMap::new();
    for row in rows {
        let (keyspace, table, column_name, kind, cql_type, clustering_order) =
            row.map_err(|error| DbError::Other(error.to_string()))?;

        grouped
            .entry((keyspace, table))
            .or_default()
            .push(ColumnRow {
                column_name,
                kind,
                cql_type,
                clustering_order,
            });
    }

    Ok(grouped)
}

/// Every table's own settings.
///
/// `SELECT *` rather than named columns: the column set of
/// `system_schema.tables` moves between Cassandra majors — 3.11 has
/// `read_repair_chance`, 4.0 replaces it with `read_repair`, 5.0 adds
/// `memtable` — so naming them would make this a hard error on any cluster
/// missing one, while a wildcard reports whatever that cluster knows
/// (doc/specs/2026-08-09-table-metadata-design.md §2).
const OPTIONS_QUERY: &str = "SELECT * FROM system_schema.tables";

/// The two columns that identify a row rather than describe it. They are the
/// panel's heading, so listing them again would be noise.
const KEYSPACE_NAME: &str = "keyspace_name";
const TABLE_NAME: &str = "table_name";

async fn option_rows(
    session: &Session,
) -> Result<BTreeMap<(String, String), TableOptions>, DbError> {
    let result = session
        .query_unpaged(OPTIONS_QUERY, &[])
        .await
        .map_err(|error| DbError::Other(error.to_string()))?
        .into_rows_result()
        .map_err(|error| DbError::Other(error.to_string()))?;

    let names: Vec<String> = result
        .column_specs()
        .iter()
        .map(|spec| spec.name().to_owned())
        .collect();

    let rows = result
        .rows::<DriverRow>()
        .map_err(|error| DbError::Other(error.to_string()))?;

    let mut grouped = BTreeMap::new();
    for row in rows {
        let row = row.map_err(|error| DbError::Other(error.to_string()))?;
        if let Some((key, options)) = to_options(&names, row) {
            grouped.insert(key, options);
        }
    }

    Ok(grouped)
}

/// `None` when the row carries no keyspace or table name, which no real
/// `system_schema.tables` row does — the wildcard select is what makes it
/// expressible at all.
fn to_options(names: &[String], row: DriverRow) -> Option<((String, String), TableOptions)> {
    let mut keyspace = None;
    let mut table = None;
    let mut by_name = BTreeMap::new();

    for (name, value) in names.iter().zip(row.columns) {
        match name.as_str() {
            KEYSPACE_NAME => keyspace = value.as_ref().and_then(DriverValue::as_text).cloned(),
            TABLE_NAME => table = value.as_ref().and_then(DriverValue::as_text).cloned(),
            _ => {
                by_name.insert(name.clone(), option_text(value.as_ref()));
            }
        }
    }

    Some(((keyspace?, table?), TableOptions::new(by_name)))
}

/// One option as the panel shows it.
///
/// Values go through the same `to_value` + [`cell::text`] pair the Contents
/// grid uses, rather than a second formatter: `cell::text` is documented as
/// the reason every renderer agrees on what a blob or a collection looks
/// like, and a private opinion here would be exactly the drift it exists to
/// prevent.
///
/// The one thing on top is the empty string. `comment` defaults to `''` on
/// every table, and a property drawn with a blank line under it reads as
/// broken rather than as unset.
fn option_text(value: Option<&DriverValue>) -> String {
    let text = cell::text(&to_value(value));

    if text.is_empty() {
        "''".to_owned()
    } else {
        text
    }
}

/// Builds a table from the driver's key ordering and `system_schema`'s column
/// detail.
///
/// `None` when the two reads disagreed because DDL landed between them: a key
/// column the driver named has no row in `system_schema`, or the driver's own
/// `partition_key` came back empty — `collect::<Option<Vec<_>>>()` over an
/// empty iterator is `Some(vec![])`, not `None`, so that case needs its own
/// check. Either way `PrimaryKey::partition` documents itself as never empty,
/// and this is the one place that guarantee is enforced.
fn to_table(
    name: &str,
    partition_key: &[String],
    clustering_key: &[String],
    rows: &[ColumnRow],
    options: TableOptions,
) -> Option<Table> {
    let by_name: BTreeMap<&str, &ColumnRow> = rows
        .iter()
        .map(|row| (row.column_name.as_str(), row))
        .collect();

    let partition: Vec<Column> = partition_key
        .iter()
        .map(|name| by_name.get(name.as_str()).map(|row| column(row)))
        .collect::<Option<_>>()?;

    if partition.is_empty() {
        return None;
    }

    let clustering: Vec<ClusteringColumn> = clustering_key
        .iter()
        .map(|name| {
            by_name.get(name.as_str()).map(|row| ClusteringColumn {
                column: column(row),
                order: clustering_order(&row.clustering_order),
            })
        })
        .collect::<Option<_>>()?;

    Some(Table {
        name: name.to_owned(),
        primary_key: PrimaryKey {
            partition,
            clustering,
        },
        static_columns: of_kind(rows, "static"),
        regular_columns: of_kind(rows, "regular"),
        options,
    })
}

fn column(row: &ColumnRow) -> Column {
    Column {
        name: row.column_name.clone(),
        cql_type: row.cql_type.clone(),
    }
}

/// Static and regular columns in name order: Cassandra records `position = -1`
/// for both and keeps no declaration order, so this is what the cluster
/// actually knows — `DESCRIBE` prints them the same way.
fn of_kind(rows: &[ColumnRow], kind: &str) -> Vec<Column> {
    let mut columns: Vec<Column> = rows
        .iter()
        .filter(|row| row.kind == kind)
        .map(column)
        .collect();
    columns.sort_by(|left, right| left.name.cmp(&right.name));
    columns
}

/// `clustering_order` is `asc`, `desc`, or `none` for a column outside the
/// clustering key — and `none` only ever reaches here for a key column when
/// the cluster is mid-upgrade, where ascending is Cassandra's own default.
fn clustering_order(order: &str) -> ClusteringOrder {
    if order.eq_ignore_ascii_case("desc") {
        ClusteringOrder::Descending
    } else {
        ClusteringOrder::Ascending
    }
}

fn schema(
    state: &ClusterState,
    columns: &BTreeMap<(String, String), Vec<ColumnRow>>,
    options: &BTreeMap<(String, String), TableOptions>,
) -> Result<Schema, DbError> {
    let keyspaces = state
        .keyspaces_iter()
        .map(|(name, keyspace)| {
            Ok((
                name.to_owned(),
                to_keyspace(name, keyspace, columns, options)?,
            ))
        })
        .collect::<Result<BTreeMap<_, _>, DbError>>()?;

    Ok(Schema { keyspaces })
}

fn to_keyspace(
    name: &str,
    keyspace: &DriverKeyspace,
    columns: &BTreeMap<(String, String), Vec<ColumnRow>>,
    options: &BTreeMap<(String, String), TableOptions>,
) -> Result<Keyspace, DbError> {
    let mut tables = BTreeMap::new();

    for (table_name, table) in &keyspace.tables {
        let key = (name.to_owned(), table_name.clone());
        let rows = columns.get(&key).map_or(&[][..], Vec::as_slice);

        // An absent options row is not a failure the way an absent column row
        // is: `PrimaryKey::partition` documents itself as never empty, while
        // options carry no invariant at all (design §2).
        let settings = options.get(&key).cloned().unwrap_or_default();

        let mapped = to_table(
            table_name,
            &table.partition_key,
            &table.clustering_key,
            rows,
            settings,
        )
        .ok_or_else(|| {
            DbError::Other(format!("schema changed while reading {name}.{table_name}"))
        })?;

        tables.insert(table_name.clone(), mapped);
    }

    Ok(Keyspace {
        name: name.to_owned(),
        replication: replication(&keyspace.strategy),
        tables,
    })
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

    fn column_row(name: &str, kind: &str, cql_type: &str, order: &str) -> ColumnRow {
        ColumnRow {
            column_name: name.to_owned(),
            kind: kind.to_owned(),
            cql_type: cql_type.to_owned(),
            clustering_order: order.to_owned(),
        }
    }

    fn readings_rows() -> Vec<ColumnRow> {
        vec![
            column_row("device_id", "partition_key", "text", "none"),
            column_row("reading_at", "clustering", "timestamp", "desc"),
            column_row("seq", "clustering", "int", "asc"),
            column_row("temperature_c", "regular", "float", "none"),
            column_row("humidity_pct", "regular", "float", "none"),
            column_row("firmware", "static", "text", "none"),
            column_row("device_model", "static", "text", "none"),
        ]
    }

    fn readings() -> Table {
        to_table(
            "readings",
            &["device_id".to_owned()],
            &["reading_at".to_owned(), "seq".to_owned()],
            &readings_rows(),
            TableOptions::default(),
        )
        .expect("every key column has a row")
    }

    #[test]
    fn a_clustering_column_keeps_the_order_the_table_declares() {
        let table = readings();

        assert_eq!(
            table
                .primary_key
                .clustering
                .iter()
                .map(|c| (c.column.name.as_str(), c.order))
                .collect::<Vec<_>>(),
            [
                ("reading_at", ClusteringOrder::Descending),
                ("seq", ClusteringOrder::Ascending),
            ]
        );
    }

    #[test]
    fn key_columns_keep_the_drivers_order_rather_than_the_rows() {
        let table = to_table(
            "composite",
            &["b".to_owned(), "a".to_owned()],
            &[],
            &[
                column_row("a", "partition_key", "int", "none"),
                column_row("b", "partition_key", "int", "none"),
            ],
            TableOptions::default(),
        )
        .expect("every key column has a row");

        assert_eq!(
            table
                .primary_key
                .partition
                .iter()
                .map(|c| c.name.as_str())
                .collect::<Vec<_>>(),
            ["b", "a"],
            "partition order decides routing, so it is never re-sorted"
        );
    }

    #[test]
    fn static_columns_are_kept_apart_from_regular_ones() {
        let table = readings();

        assert_eq!(
            table
                .static_columns
                .iter()
                .map(|c| c.name.as_str())
                .collect::<Vec<_>>(),
            ["device_model", "firmware"]
        );
        assert_eq!(
            table
                .regular_columns
                .iter()
                .map(|c| c.name.as_str())
                .collect::<Vec<_>>(),
            ["humidity_pct", "temperature_c"],
            "Cassandra keeps no declaration order for these, so name order is what it knows"
        );
    }

    #[test]
    fn a_column_carries_the_cql_type_the_cluster_stores() {
        let table = to_table(
            "collections",
            &["id".to_owned()],
            &[],
            &[
                column_row("id", "partition_key", "int", "none"),
                column_row("frozen_list", "regular", "frozen<list<text>>", "none"),
            ],
            TableOptions::default(),
        )
        .expect("every key column has a row");

        assert_eq!(table.regular_columns[0].cql_type, "frozen<list<text>>");
    }

    #[test]
    fn a_table_whose_key_column_has_no_row_is_refused() {
        let missing = to_table(
            "racing",
            &["device_id".to_owned()],
            &[],
            &[column_row("temperature_c", "regular", "float", "none")],
            TableOptions::default(),
        );

        assert!(
            missing.is_none(),
            "a table displayed without its partition key is the one thing that must never happen"
        );
    }

    #[test]
    fn a_table_whose_driver_reported_partition_key_is_empty_is_refused() {
        // scylla-1.7.0 can report `partition_key: vec![]` for a table its own
        // metadata read raced against a DDL change, with no error
        // (`validate_key_columns(vec![])` returns `Ok(vec![])`,
        // scylla-1.7.0/src/cluster/metadata/fetching.rs:1084-1090). The
        // columns query can still see that table, so `rows` here is
        // non-empty even though the partition key is not.
        let empty_partition = to_table(
            "ghost",
            &[],
            &[],
            &[column_row("device_id", "partition_key", "text", "none")],
            TableOptions::default(),
        );

        assert!(
            empty_partition.is_none(),
            "an empty partition key must be refused even though every named key column resolved"
        );
    }

    #[test]
    fn clustering_order_defaults_to_ascending_for_anything_but_desc() {
        assert_eq!(clustering_order("desc"), ClusteringOrder::Descending);
        assert_eq!(clustering_order("DESC"), ClusteringOrder::Descending);
        assert_eq!(clustering_order("asc"), ClusteringOrder::Ascending);
        assert_eq!(clustering_order("none"), ClusteringOrder::Ascending);
    }

    fn driver_text(text: &str) -> DriverValue {
        DriverValue::Text(text.to_owned())
    }

    #[test]
    fn an_option_is_rendered_the_way_the_contents_grid_renders_a_cell() {
        // The point of the assertion is the shared renderer, not these
        // values: `cell::text` is where the form is decided and tested.
        assert_eq!(
            option_text(Some(&driver_text("Order history."))),
            "Order history."
        );
        assert_eq!(option_text(Some(&DriverValue::Int(864_000))), "864000");
        assert_eq!(option_text(Some(&DriverValue::Boolean(false))), "false");
        assert_eq!(
            option_text(Some(&DriverValue::Blob(vec![0x00, 0x0f, 0xff]))),
            "0x000fff"
        );
    }

    #[test]
    fn a_map_option_keeps_every_entry_and_a_stable_order() {
        let compaction = DriverValue::Map(vec![
            (driver_text("min_threshold"), driver_text("4")),
            (
                driver_text("class"),
                driver_text("org.apache.cassandra.db.compaction.SizeTieredCompactionStrategy"),
            ),
            (driver_text("max_threshold"), driver_text("32")),
        ]);

        assert_eq!(
            option_text(Some(&compaction)),
            "{class: org.apache.cassandra.db.compaction.SizeTieredCompactionStrategy, \
             max_threshold: 32, min_threshold: 4}",
            "a coordinator's serialisation order must not reach the screen"
        );
    }

    #[test]
    fn an_empty_string_is_shown_as_the_empty_literal_rather_than_as_nothing() {
        assert_eq!(
            option_text(Some(&driver_text(""))),
            "''",
            "a property with a blank line under it reads as broken, not as unset"
        );
    }

    #[test]
    fn a_column_this_table_left_unset_is_null_rather_than_absent() {
        assert_eq!(option_text(None), "null");
    }

    fn names(names: &[&str]) -> Vec<String> {
        names.iter().map(|name| (*name).to_owned()).collect()
    }

    #[test]
    fn a_row_is_keyed_by_its_keyspace_and_table_and_describes_itself_with_the_rest() {
        let row = DriverRow {
            columns: vec![
                Some(driver_text("shop")),
                Some(driver_text("orders_by_customer")),
                Some(DriverValue::Int(864_000)),
            ],
        };

        let (key, options) = to_options(
            &names(&["keyspace_name", "table_name", "gc_grace_seconds"]),
            row,
        )
        .expect("a row with both names is keyed");

        assert_eq!(key, ("shop".to_owned(), "orders_by_customer".to_owned()));
        assert_eq!(
            options.in_display_order().collect::<Vec<_>>(),
            [("gc_grace_seconds", "864000")],
            "the two identifying columns are the panel's heading, not properties"
        );
    }

    #[test]
    fn a_cluster_older_than_the_ranking_reports_fewer_properties_rather_than_failing() {
        // 3.11's shape: no `read_repair`, and two columns 4.0 removed.
        let row = DriverRow {
            columns: vec![
                Some(driver_text("shop")),
                Some(driver_text("orders_by_customer")),
                Some(DriverValue::Double(0.0)),
                Some(DriverValue::Double(0.1)),
            ],
        };

        let (_, options) = to_options(
            &names(&[
                "keyspace_name",
                "table_name",
                "read_repair_chance",
                "dclocal_read_repair_chance",
            ]),
            row,
        )
        .expect("a row with both names is keyed");

        assert_eq!(
            options.in_display_order().collect::<Vec<_>>(),
            [
                ("dclocal_read_repair_chance", "0.1"),
                ("read_repair_chance", "0")
            ],
            "a name the ranking has never heard of is still the cluster's own vocabulary"
        );
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

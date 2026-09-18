//! The `Cluster` port over `scylla`.
//!
//! Driver types stop here: everything crossing back into the domain is our own
//! vocabulary (architecture.md §5).

use std::collections::BTreeMap;
use std::net::SocketAddr;
use std::sync::Arc;

use async_trait::async_trait;
use lc_domain::config::{ConnectionConfig, Discovery, PageSize};
use lc_domain::model::{
    ClusteringColumn, ClusteringOrder, Column, ConnId, DbError, Index, IndexKind, IndexTarget,
    Keyspace, Page, PagingState, PrimaryKey, Replication, Row, Schema, Statement, Table,
    TableOptions,
};
use lc_domain::ports::Cluster;
use lc_domain::posture::Posture;
use scylla::client::session::Session;
use scylla::client::session_builder::SessionBuilder;
use scylla::cluster::ClusterState;
use scylla::cluster::metadata::{Keyspace as DriverKeyspace, Strategy};
use scylla::errors::TranslationError;
use scylla::policies::address_translator::{AddressTranslator, UntranslatedPeer};
use scylla::value::{CqlValue as DriverValue, Row as DriverRow};
use tokio::sync::RwLock;

use lc_domain::cell;

use crate::secret::Secrets;
use crate::value::to_value;

/// One cluster, reached over CQL.
///
/// Holds whichever connection was opened last rather than one fixed at
/// construction: the address is chosen from the picker at runtime, so an
/// adapter built around a single compiled-in contact point could not serve it
/// (connection picker design §6). `&self` is why the session sits behind a
/// lock. One target rather than a map is still the honest match for a `State`
/// that holds exactly one connection — opening a second replaces the first,
/// and the [`ConnId`] guard is what makes a late result for the one left
/// behind droppable.
#[derive(Debug, Default)]
pub struct CassandraCluster {
    open: RwLock<Option<Open>>,
}

/// Folds every address the cluster reports onto the one endpoint the user can
/// reach.
///
/// A host filter cannot do this job. Every peer read from `system.local` and
/// `system.peers` is a `NodeAddr::Translatable` — the node holding the control
/// connection included (`cluster/metadata/fetching.rs`) — so a filter narrow
/// enough to keep only the tunnelled node rejects that node too, and the load
/// balancing policy is left with an empty plan. Translation is the only
/// mechanism the driver offers: there is no knob that turns topology discovery
/// off.
///
/// The cluster still reports every node, so the driver holds one `Node` per
/// peer and pools each to the same socket. Redundant, and the honest price of
/// reaching a cluster through a single tunnel — the alternative is naming
/// every pod's in-cluster address in the config file, where it would rot on
/// the next reschedule.
#[derive(Debug)]
struct ToTunnel(SocketAddr);

#[async_trait]
impl AddressTranslator for ToTunnel {
    async fn translate_address(
        &self,
        _peer: &UntranslatedPeer,
    ) -> Result<SocketAddr, TranslationError> {
        Ok(self.0)
    }
}

/// The one host a tunnelled connection states, resolved.
///
/// Resolved here rather than held as a `SocketAddr` in the config: a tunnel is
/// as likely to be named `localhost` as written as an address, and the
/// translator owes the driver a `SocketAddr`.
async fn tunnel(target: &ConnectionConfig) -> Result<SocketAddr, DbError> {
    let [host] = target.hosts.as_slice() else {
        return Err(DbError::Unreachable(
            "a tunnelled connection reaches exactly one host".to_owned(),
        ));
    };

    tokio::net::lookup_host(host)
        .await
        .map_err(|error| DbError::Unreachable(format!("{host}: {error}")))?
        .next()
        .ok_or_else(|| DbError::Unreachable(format!("{host} resolved to no address")))
}

/// The session in hand, and the id it answers to.
#[derive(Debug)]
struct Open {
    conn: ConnId,
    session: Arc<Session>,
}

impl CassandraCluster {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    async fn session(&self, conn: ConnId) -> Result<Arc<Session>, DbError> {
        match self.open.read().await.as_ref() {
            Some(open) if open.conn == conn => Ok(Arc::clone(&open.session)),
            // A statement for a connection this adapter has moved past. The
            // domain drops such results by id anyway; refusing here means the
            // driver is never asked in the first place.
            Some(_) => Err(DbError::Other(
                "this adapter serves one connection".to_owned(),
            )),
            None => Err(DbError::Unreachable("not connected".to_owned())),
        }
    }
}

#[async_trait]
impl Cluster for CassandraCluster {
    async fn connect(&self, conn: ConnId, target: &ConnectionConfig) -> Result<Posture, DbError> {
        let mut builder = SessionBuilder::new().known_nodes(&target.hosts);

        if let Some(credentials) = &target.credentials {
            // Resolved here rather than in the effect runtime so the plaintext
            // never crosses into `lc-domain` (secret.rs).
            let (username, password) = Secrets
                .credentials(credentials)
                .await
                .map_err(|error| DbError::Unreachable(error.to_string()))?;

            builder = builder.user(username, password);
        }

        if target.discovery == Discovery::ContactPoints {
            builder = builder.address_translator(Arc::new(ToTunnel(tunnel(target).await?)));
        }

        // Boxed to satisfy clippy::large_futures: SessionBuilder::build's
        // future is large enough to be worth moving off this async fn's stack.
        let session = Box::pin(builder.build())
            .await
            .map_err(|error| DbError::Unreachable(error.to_string()))?;

        *self.open.write().await = Some(Open {
            conn,
            session: Arc::new(session),
        });

        Ok(target.posture)
    }

    async fn describe(&self, conn: ConnId) -> Result<Schema, DbError> {
        let session = self.session(conn).await?;
        session
            .refresh_metadata()
            .await
            .map_err(|error| DbError::Other(error.to_string()))?;

        let columns = column_rows(&session).await?;
        let options = option_rows(&session).await?;
        let indexes = index_rows(&session).await?;

        schema(&session.get_cluster_state(), &columns, &options, &indexes)
    }

    /// Unpaged: only mutations and DDL come through here, and they return no
    /// rows to page (grid paging design §1).
    async fn execute(&self, conn: ConnId, stmt: &Statement) -> Result<Page, DbError> {
        let session = self.session(conn).await?;

        let result = session
            .query_unpaged(stmt.cql.as_str(), &[])
            .await
            .map_err(|error| DbError::Invalid(error.to_string()))?;

        to_page(result)
    }

    async fn execute_paged(
        &self,
        conn: ConnId,
        stmt: &Statement,
        page_size: PageSize,
        cursor: Option<&PagingState>,
    ) -> Result<Page, DbError> {
        let session = self.session(conn).await?;
        // `PageSize` tops out far below `i32::MAX`, and scylla panics on a
        // size that is not positive.
        let statement = scylla::statement::unprepared::Statement::new(stmt.cql.as_str())
            .with_page_size(i32::try_from(page_size.get()).unwrap_or(i32::MAX));
        let paging = cursor.map_or(
            scylla::response::PagingState::start(),
            |PagingState(bytes)| scylla::response::PagingState::new_from_raw_bytes(bytes.clone()),
        );

        let (result, response) = session
            .query_single_page(statement, &[], paging)
            .await
            .map_err(|error| DbError::Invalid(error.to_string()))?;

        let next = match response.into_paging_control_flow() {
            std::ops::ControlFlow::Continue(state) => state
                .as_bytes_slice()
                .map(|bytes| PagingState(bytes.to_vec())),
            std::ops::ControlFlow::Break(()) => None,
        };

        let mut page = to_page(result)?;
        page.next = next;

        Ok(page)
    }
}

/// Converts a driver result into our own vocabulary. Shared by `execute` and
/// `execute_paged`, which differ only in how they got the result and what
/// they do with `Page::next` afterwards.
fn to_page(result: scylla::response::query_result::QueryResult) -> Result<Page, DbError> {
    use scylla::response::query_result::IntoRowsResultError;

    let result = match result.into_rows_result() {
        Ok(result) => result,
        // A mutation and a DDL statement answer with a `Void` result, which
        // is not an error: `Cluster::execute` is the unpaged path and every
        // statement that comes through it returns no rows (§5). The driver
        // spells "no rows to read" as a failed downcast, so the empty page is
        // built here rather than reported as a rejection the cluster never
        // made.
        Err(IntoRowsResultError::ResultNotRows(_)) => {
            return Ok(Page {
                columns: Vec::new(),
                rows: Vec::new(),
                next: None,
            });
        }
        Err(error) => return Err(DbError::Invalid(error.to_string())),
    };

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

/// The three things `scylla::cluster::metadata` does not carry: clustering
/// order, the CQL type as text, and whether a column is static
/// (docs/superpowers/specs/2026-08-08-table-list-design.md §3).
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
/// (docs/superpowers/specs/2026-08-09-table-metadata-design.md §2).
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

/// `scylla::cluster::metadata` carries no indexes at all, so the whole of what
/// the Indexes tab draws is read here.
///
/// Named columns rather than a wildcard, the opposite of [`OPTIONS_QUERY`]:
/// these five have been `system_schema.indexes`' entire column set since
/// Cassandra 3.0, and a target or a kind we failed to ask for is a tab that
/// silently says "none".
const INDEXES_QUERY: &str = "SELECT keyspace_name, table_name, index_name, kind, options \
                             FROM system_schema.indexes";

/// The `options` entry naming what an index covers, and the one naming who
/// implements it.
const TARGET: &str = "target";
const CLASS_NAME: &str = "class_name";

/// Every index in the cluster, grouped by the table it is built on.
///
/// A view's indexes — Cassandra allows none today — would key by the view's
/// own name and fall out of the join, the way its columns do.
async fn index_rows(session: &Session) -> Result<BTreeMap<(String, String), Vec<Index>>, DbError> {
    let result = session
        .query_unpaged(INDEXES_QUERY, &[])
        .await
        .map_err(|error| DbError::Other(error.to_string()))?
        .into_rows_result()
        .map_err(|error| DbError::Other(error.to_string()))?;

    // `Option` around the map because a null column deserializes into no
    // collection rather than an empty one, and an index whose options are
    // null is an index with no target — still worth listing by name.
    let rows = result
        .rows::<(
            String,
            String,
            String,
            String,
            Option<BTreeMap<String, String>>,
        )>()
        .map_err(|error| DbError::Other(error.to_string()))?;

    let mut grouped: BTreeMap<(String, String), Vec<Index>> = BTreeMap::new();
    for row in rows {
        let (keyspace, table, name, kind, options) =
            row.map_err(|error| DbError::Other(error.to_string()))?;
        let options = options.unwrap_or_default();

        grouped
            .entry((keyspace, table))
            .or_default()
            .push(to_index(name, &kind, &options));
    }

    // `index_name` is the clustering column, so the rows already arrive in
    // this order; sorting says so rather than relying on it
    // (conventions.md §4).
    for indexes in grouped.values_mut() {
        indexes.sort_by(|left, right| left.name.cmp(&right.name));
    }

    Ok(grouped)
}

/// A `CUSTOM` index without a `class_name` is a row Cassandra does not write,
/// and the honest reading of one is that there is no class to name.
fn to_index(name: String, kind: &str, options: &BTreeMap<String, String>) -> Index {
    let class = options.get(CLASS_NAME).cloned();
    let kind = match class.filter(|_| kind.eq_ignore_ascii_case("custom")) {
        Some(class) => IndexKind::Custom { class },
        None => IndexKind::Builtin,
    };

    Index {
        name,
        target: IndexTarget::parse(options.get(TARGET).map_or("", String::as_str)),
        kind,
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
    indexes: Vec<Index>,
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
        indexes,
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
    indexes: &BTreeMap<(String, String), Vec<Index>>,
) -> Result<Schema, DbError> {
    let keyspaces = state
        .keyspaces_iter()
        .map(|(name, keyspace)| {
            Ok((
                name.to_owned(),
                to_keyspace(name, keyspace, columns, options, indexes)?,
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
    indexes: &BTreeMap<(String, String), Vec<Index>>,
) -> Result<Keyspace, DbError> {
    let mut tables = BTreeMap::new();

    for (table_name, table) in &keyspace.tables {
        let key = (name.to_owned(), table_name.clone());
        let rows = columns.get(&key).map_or(&[][..], Vec::as_slice);

        // An absent options row is not a failure the way an absent column row
        // is: `PrimaryKey::partition` documents itself as never empty, while
        // options carry no invariant at all (design §2).
        let settings = options.get(&key).cloned().unwrap_or_default();

        // Absent for the same reason and with the same weight as an absent
        // options row: a table with no index is the common case, not a read
        // that came back short.
        let table_indexes = indexes.get(&key).cloned().unwrap_or_default();

        let mapped = to_table(
            table_name,
            &table.partition_key,
            &table.clustering_key,
            rows,
            table_indexes,
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
            Vec::new(),
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
            Vec::new(),
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
            Vec::new(),
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
            Vec::new(),
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
            Vec::new(),
            TableOptions::default(),
        );

        assert!(
            empty_partition.is_none(),
            "an empty partition key must be refused even though every named key column resolved"
        );
    }

    fn index_options(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
            .collect()
    }

    #[test]
    fn a_cassandra_index_carries_its_target_and_nothing_about_a_class() {
        for kind in ["COMPOSITES", "KEYS"] {
            let index = to_index(
                "orders_by_status".to_owned(),
                kind,
                &index_options(&[("target", "status")]),
            );

            assert_eq!(index.kind, IndexKind::Builtin);
            assert_eq!(index.target, IndexTarget::parse("status"));
        }
    }

    #[test]
    fn a_custom_index_carries_the_class_implementing_it() {
        let index = to_index(
            "orders_sasi".to_owned(),
            "CUSTOM",
            &index_options(&[
                ("target", "status"),
                ("class_name", "org.apache.cassandra.index.sasi.SASIIndex"),
            ]),
        );

        assert_eq!(
            index.kind,
            IndexKind::Custom {
                class: "org.apache.cassandra.index.sasi.SASIIndex".to_owned(),
            }
        );
    }

    #[test]
    fn an_index_with_no_options_is_still_listed_by_name() {
        // Neither a null `options` map nor a `CUSTOM` row without a class is
        // something Cassandra writes; both are expressible, and dropping the
        // index would hide one the table really has.
        let index = to_index("orders_ghost".to_owned(), "CUSTOM", &BTreeMap::new());

        assert_eq!(index.name, "orders_ghost");
        assert_eq!(index.kind, IndexKind::Builtin);
        assert_eq!(index.target.text(), "");
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

    // Not gated behind `integration`: the guard refuses before the adapter
    // ever reaches a socket, so this needs no cluster and belongs in
    // `mise run ci`.
    //
    // The guard moved with the address: `connect` now binds whichever id it
    // is handed, because the picker chooses the target at runtime, so what is
    // refused is a *statement* for a connection this adapter is not holding
    // (connection picker design §6).
    #[tokio::test]
    async fn a_statement_before_any_connection_is_refused() {
        let cluster = CassandraCluster::new();

        assert!(matches!(
            cluster.describe(ConnId::next()).await,
            Err(DbError::Unreachable(_))
        ));
    }
}

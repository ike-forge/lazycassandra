//! Reads a table's health from every node's own system tables.

use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use lc_domain::contents::TableRef;
use lc_domain::health::{Gossip, Health, Metric, NodeOutcome, NodeReport, Readings, Thresholds};
use lc_domain::model::DbError;
use scylla::client::session::Session;
use scylla::cluster::Node;
use scylla::policies::load_balancing::{NodeIdentifier, SingleTargetLoadBalancingPolicy};
use scylla::statement::unprepared::Statement;
use scylla::value::{CqlValue as DriverValue, Row as DriverRow};
use tokio::task::JoinSet;

const MIB: f64 = 1024.0 * 1024.0;

const LOCAL: &str = "SELECT host_id, broadcast_address, data_center, release_version \
                     FROM system.local";

/// Keyed by `(keyspace_name, table_name)`, one value column in MiB.
const SIZES: [(&str, Metric); 3] = [
    ("disk_usage", Metric::DiskUsed),
    ("max_partition_size", Metric::LargestPartition),
    ("max_sstable_size", Metric::LargestSstable),
];

/// 5.0's guardrail, 4.1's sized setting, 4.0's megabytes.
const PARTITION_SETTINGS: [&str; 3] = [
    "partition_size_warn_threshold",
    "compaction_large_partition_warning_threshold",
    "compaction_large_partition_warning_threshold_mb",
];

const PEERS: &str = "SELECT peer, data_center, host_id, release_version FROM system.peers_v2";

/// A node gossips peers' state under `status_with_port` only; `status` is its
/// own.
const GOSSIP: &str = "SELECT host_id, status_with_port, status, load FROM system_views.gossip_info";

struct Answer {
    host: String,
    node: Arc<Node>,
    report: NodeReport,
}

pub(crate) async fn read(session: &Arc<Session>, table: &TableRef) -> Result<Health, DbError> {
    let nodes = session.get_cluster_state().get_nodes_info().to_vec();
    if nodes.is_empty() {
        return Err(DbError::Unreachable("the driver knows no node".to_owned()));
    }

    let mut reads = JoinSet::new();
    for node in nodes {
        let (session, table) = (Arc::clone(session), table.clone());
        reads.spawn(async move { node_report(&session, node, &table).await });
    }

    let mut answered: Vec<Answer> = Vec::new();
    while let Some(joined) = reads.join_next().await {
        let answer =
            joined.map_err(|error| DbError::Other(format!("health read panicked: {error}")))?;
        // Behind contact-points discovery several `Node`s answer as one host.
        if answered.iter().any(|seen| seen.host == answer.host) {
            continue;
        }
        answered.push(answer);
    }

    let reached = answered
        .iter()
        .find(|answer| matches!(answer.report.outcome, NodeOutcome::Read { .. }))
        .map(|answer| Arc::clone(&answer.node));
    let mut known: Vec<(String, NodeReport)> = answered
        .into_iter()
        .map(|answer| (answer.host, answer.report))
        .collect();

    if let Some(reached) = reached {
        for (host, peer) in peers(session, &reached).await {
            match known.iter_mut().find(|(seen, _)| *seen == host) {
                // A failed node never said what it runs; its peers know.
                Some((_, report)) => {
                    report.version = report.version.take().or(peer.version);
                    report.datacenter = report.datacenter.take().or(peer.datacenter);
                }
                None => known.push((host, peer)),
            }
        }
        let gossip = gossip(session, &reached).await;
        for (host, report) in &mut known {
            report.gossip = gossip
                .iter()
                .find(|(id, _)| id == host)
                .map(|(_, gossip)| gossip.clone());
        }
    }

    let mut nodes: Vec<NodeReport> = known.into_iter().map(|(_, report)| report).collect();
    nodes.sort_by(|left, right| left.address.cmp(&right.address));

    Ok(Health { nodes })
}

/// The caller drops the ones the driver reached too.
async fn peers(session: &Session, reached: &Arc<Node>) -> Vec<(String, NodeReport)> {
    let Ok(rows) = rows(session, reached, PEERS, ()).await else {
        return Vec::new();
    };

    rows.into_iter()
        .filter_map(|row| {
            let mut columns = row.columns.into_iter();
            let address = match columns.next().flatten()? {
                DriverValue::Inet(ip) => ip.to_string(),
                _ => return None,
            };
            let datacenter = columns.next().flatten().and_then(text);
            let host = match columns.next().flatten()? {
                DriverValue::Uuid(id) => id.to_string(),
                _ => return None,
            };
            let version = columns.next().flatten().and_then(text);

            Some((
                host,
                NodeReport {
                    address,
                    datacenter,
                    version,
                    gossip: None,
                    outcome: NodeOutcome::Unreached,
                },
            ))
        })
        .collect()
}

/// Empty before 5.0.
async fn gossip(session: &Session, reached: &Arc<Node>) -> Vec<(String, Gossip)> {
    let Ok(rows) = rows(session, reached, GOSSIP, ()).await else {
        return Vec::new();
    };

    rows.into_iter()
        .filter_map(|row| {
            let [host, with_port, status, load] = <[_; 4]>::try_from(row.columns)
                .ok()?
                .map(|value| value.and_then(text));
            let status = with_port.or(status)?;

            Some((host?, gossip_state(&status, load.as_deref())))
        })
        .collect()
}

/// `STATUS` carries tokens after the state; `LOAD` is bytes as a float.
fn gossip_state(status: &str, load: Option<&str>) -> Gossip {
    Gossip {
        status: status.split(',').next().unwrap_or(status).to_owned(),
        load: load.and_then(|load| load.trim().parse().ok()),
    }
}

/// A failed `system.local` read is the node failing; a source missing after
/// that is a metric its version lacks. Answered nodes are keyed by their
/// reported `host_id`, which behind a tunnel is the tunnelled node's.
async fn node_report(session: &Session, node: Arc<Node>, table: &TableRef) -> Answer {
    let failed = |reason: String| Answer {
        host: node.host_id.to_string(),
        report: NodeReport {
            address: node.address.ip().to_string(),
            datacenter: node.datacenter.clone(),
            version: None,
            gossip: None,
            outcome: NodeOutcome::Failed(reason),
        },
        node: Arc::clone(&node),
    };

    let local = match rows(session, &node, LOCAL, ()).await {
        Ok(rows) => rows.into_iter().next(),
        Err(error) => return failed(error),
    };
    let Some(local) = local else {
        return failed("system.local is empty".to_owned());
    };

    let mut columns = local.columns.into_iter();
    let host = match columns.next().flatten() {
        Some(DriverValue::Uuid(id)) => id.to_string(),
        _ => node.host_id.to_string(),
    };
    let address = columns.next().flatten().and_then(|value| match value {
        DriverValue::Inet(ip) => Some(ip.to_string()),
        _ => None,
    });
    let datacenter = columns.next().flatten().and_then(text);
    let version = columns.next().flatten().and_then(text);

    let report = NodeReport {
        address: address.unwrap_or_else(|| node.address.ip().to_string()),
        datacenter: datacenter.or_else(|| node.datacenter.clone()),
        gossip: None,
        outcome: if version.as_deref().is_some_and(predates_virtual_tables) {
            NodeOutcome::Unsupported
        } else {
            NodeOutcome::Read {
                readings: readings(session, &node, table).await,
                thresholds: thresholds(session, &node).await,
            }
        },
        version,
    };

    Answer { host, node, report }
}

fn predates_virtual_tables(version: &str) -> bool {
    version
        .split('.')
        .next()
        .and_then(|major| major.parse::<u32>().ok())
        .is_some_and(|major| major < 4)
}

type Fold = fn(&[DriverRow], &mut Readings);

const LISTS: [(&str, Fold); 4] = [
    (
        "SELECT partitions_count, mean_partition_size FROM system.table_estimates \
         WHERE keyspace_name = ? AND table_name = ? AND range_type = 'local'",
        estimates,
    ),
    (
        "SELECT progress, total, unit FROM system_views.sstable_tasks \
         WHERE keyspace_name = ? AND table_name = ?",
        tasks,
    ),
    (
        "SELECT compacted_at FROM system.compaction_history \
         WHERE keyspace_name = ? AND columnfamily_name = ? ALLOW FILTERING",
        last_compaction,
    ),
    (
        "SELECT size_on_disk FROM system_views.snapshots \
         WHERE keyspace_name = ? AND table_name = ? ALLOW FILTERING",
        snapshots,
    ),
];

const SUMMARIES: [(&str, &str, &[Metric]); 5] = [
    (
        "tombstones_per_read",
        "max, p99th",
        &[Metric::TombstonesMax, Metric::TombstonesP99],
    ),
    ("rows_per_read", "p99th", &[Metric::RowsPerReadP99]),
    (
        "local_read_latency",
        "per_second, p50th_ms, p99th_ms",
        &[
            Metric::ReadRate,
            Metric::ReadLatencyP50,
            Metric::ReadLatencyP99,
        ],
    ),
    (
        "local_scan_latency",
        "per_second, p99th_ms",
        &[Metric::ScanRate, Metric::ScanLatencyP99],
    ),
    (
        "local_write_latency",
        "per_second, p50th_ms, p99th_ms",
        &[
            Metric::WriteRate,
            Metric::WriteLatencyP50,
            Metric::WriteLatencyP99,
        ],
    ),
];

async fn readings(session: &Session, node: &Arc<Node>, table: &TableRef) -> Readings {
    let key = (table.keyspace.as_str(), table.table.as_str());
    let mut readings = Readings::default();

    for (view, metric) in SIZES {
        let cql = format!(
            "SELECT mebibytes FROM system_views.{view} WHERE keyspace_name = ? AND table_name = ?"
        );
        if let Some(mebibytes) = first(session, node, &cql, key).await {
            readings.set(metric, mebibytes * MIB);
        }
    }

    for (view, selected, metrics) in SUMMARIES {
        let cql = format!(
            "SELECT count, {selected} FROM system_views.{view} \
             WHERE keyspace_name = ? AND table_name = ?"
        );
        if let Ok(rows) = rows(session, node, &cql, key).await
            && let Some(row) = rows.first()
        {
            summary(metrics, &numbers(row), &mut readings);
        }
    }

    for (cql, fold) in LISTS {
        if let Ok(rows) = rows(session, node, cql, key).await {
            fold(&rows, &mut readings);
        }
    }

    readings
}

/// A histogram that has seen nothing reads zero, which is no latency, not a
/// fast one; only a rate is true at zero.
fn summary(metrics: &[Metric], values: &[Option<f64>], readings: &mut Readings) {
    let seen = values
        .first()
        .copied()
        .flatten()
        .is_some_and(|count| count > 0.0);

    for (metric, value) in metrics.iter().zip(&values[1.min(values.len())..]) {
        if let Some(value) = value
            && (seen || metric.unit() == lc_domain::health::Unit::PerSecond)
        {
            readings.set(*metric, *value);
        }
    }
}

/// Weighted by count, so the mean is over partitions.
fn estimates(rows: &[DriverRow], readings: &mut Readings) {
    let (partitions, bytes) =
        rows.iter()
            .map(numbers)
            .fold((0.0, 0.0), |(partitions, bytes), row| {
                let count = row.first().copied().flatten().unwrap_or(0.0);
                let mean = row.get(1).copied().flatten().unwrap_or(0.0);
                (partitions + count, bytes + count * mean)
            });
    readings.set(Metric::Partitions, partitions);
    if partitions > 0.0 {
        readings.set(Metric::MeanPartition, bytes / partitions);
    }
}

/// Tasks counted in keys or ranges add no byte backlog.
fn tasks(rows: &[DriverRow], readings: &mut Readings) {
    let backlog = rows
        .iter()
        .filter_map(|row| {
            let unit = row.columns.get(2).cloned().flatten().and_then(text);
            let mut values = numbers(row).into_iter();
            match (values.next(), values.next(), unit.as_deref()) {
                (Some(Some(progress)), Some(Some(total)), Some("bytes")) => {
                    Some((total - progress).max(0.0))
                }
                _ => None,
            }
        })
        .fold(0.0, total);
    readings.set(Metric::RunningCompactions, count(rows.len()));
    readings.set(Metric::CompactionBacklog, backlog);
}

/// `Sum` over empty `f64` is `-0.0`, which draws negative.
fn total(sum: f64, value: f64) -> f64 {
    sum + value
}

fn last_compaction(rows: &[DriverRow], readings: &mut Readings) {
    let latest = rows
        .iter()
        .filter_map(|row| match row.columns.first() {
            Some(Some(DriverValue::Timestamp(at))) => Some(at.0),
            _ => None,
        })
        .max();
    if let Some(age) = latest.and_then(seconds_since) {
        readings.set(Metric::LastCompaction, age);
    }
}

fn snapshots(rows: &[DriverRow], readings: &mut Readings) {
    let size = rows
        .iter()
        .filter_map(|row| numbers(row).into_iter().next().flatten())
        .fold(0.0, total);
    readings.set(Metric::Snapshots, count(rows.len()));
    readings.set(Metric::SnapshotSize, size);
}

async fn thresholds(session: &Session, node: &Arc<Node>) -> Thresholds {
    let names = ["tombstone_warn_threshold", "tombstone_failure_threshold"]
        .into_iter()
        .chain(PARTITION_SETTINGS)
        .map(|name| format!("'{name}'"))
        .collect::<Vec<_>>()
        .join(", ");
    let cql = format!("SELECT name, value FROM system_views.settings WHERE name IN ({names})");

    let Ok(rows) = rows(session, node, &cql, ()).await else {
        return Thresholds::default();
    };
    let settings: Vec<(String, String)> = rows
        .into_iter()
        .filter_map(|row| {
            let mut columns = row.columns.into_iter();
            Some((
                columns.next().flatten().and_then(text)?,
                columns.next().flatten().and_then(text)?,
            ))
        })
        .collect();
    let setting = |name: &str| {
        settings
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.as_str())
    };

    Thresholds {
        tombstone_warn: setting("tombstone_warn_threshold").and_then(positive),
        tombstone_failure: setting("tombstone_failure_threshold").and_then(positive),
        large_partition: PARTITION_SETTINGS.into_iter().find_map(|name| {
            let value = setting(name)?;
            if name.ends_with("_mb") {
                positive(value).map(|mebibytes| mebibytes * MIB)
            } else {
                storage(value)
            }
        }),
    }
}

/// Cassandra spells "disabled" as `-1` or `null`.
fn positive(value: &str) -> Option<f64> {
    value
        .trim()
        .parse::<f64>()
        .ok()
        .filter(|value| *value > 0.0)
}

fn storage(value: &str) -> Option<f64> {
    let value = value.trim();
    let split = value
        .find(|character: char| !character.is_ascii_digit() && character != '.')
        .unwrap_or(value.len());
    let (number, unit) = value.split_at(split);
    let scale = match unit.trim() {
        "" | "B" => 1.0,
        "KiB" => 1024.0,
        "MiB" => MIB,
        "GiB" => MIB * 1024.0,
        _ => return None,
    };
    positive(number).map(|number| number * scale)
}

fn seconds_since(millis: i64) -> Option<f64> {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()?
        .as_millis();
    let then = u128::try_from(millis).ok()?;
    Some(count(
        usize::try_from(now.saturating_sub(then) / 1000).ok()?,
    ))
}

#[expect(
    clippy::cast_precision_loss,
    reason = "a row count or age in seconds is far below 2^52"
)]
const fn count(value: usize) -> f64 {
    value as f64
}

fn text(value: DriverValue) -> Option<String> {
    match value {
        DriverValue::Text(text) | DriverValue::Ascii(text) => Some(text),
        _ => None,
    }
}

/// By value, so a column whose type moved between versions still reads.
#[expect(
    clippy::cast_precision_loss,
    reason = "a byte count or a counter is far below 2^52"
)]
fn numbers(row: &DriverRow) -> Vec<Option<f64>> {
    row.columns
        .iter()
        .map(|value| match value.as_ref()? {
            DriverValue::BigInt(value) | DriverValue::Counter(scylla::value::Counter(value)) => {
                Some(*value as f64)
            }
            DriverValue::Int(value) => Some(f64::from(*value)),
            DriverValue::Double(value) => Some(*value),
            DriverValue::Float(value) => Some(f64::from(*value)),
            _ => None,
        })
        .collect()
}

async fn first(
    session: &Session,
    node: &Arc<Node>,
    cql: &str,
    values: (&str, &str),
) -> Option<f64> {
    let rows = rows(session, node, cql, values).await.ok()?;
    numbers(rows.first()?).into_iter().next().flatten()
}

/// Every source here is node-local.
async fn rows(
    session: &Session,
    node: &Arc<Node>,
    cql: &str,
    values: impl scylla::serialize::row::SerializeRow,
) -> Result<Vec<DriverRow>, String> {
    let mut statement = Statement::new(cql);
    statement.set_load_balancing_policy(Some(SingleTargetLoadBalancingPolicy::new(
        NodeIdentifier::Node(Arc::clone(node)),
        None,
    )));

    session
        .query_unpaged(statement, values)
        .await
        .map_err(|error| error.to_string())?
        .into_rows_result()
        .map_err(|error| error.to_string())?
        .rows::<DriverRow>()
        .map_err(|error| error.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_sized_setting_reads_in_bytes_whatever_its_unit() {
        assert_eq!(storage("100MiB"), Some(100.0 * MIB));
        assert_eq!(storage("2GiB"), Some(2048.0 * MIB));
        assert_eq!(storage("512"), Some(512.0));
        assert_eq!(storage("null"), None);
        assert_eq!(storage("10 parsecs"), None);
    }

    #[test]
    fn minus_one_and_null_both_mean_disabled() {
        assert_eq!(positive("1000"), Some(1000.0));
        assert_eq!(positive("-1"), None);
        assert_eq!(positive("null"), None);
    }

    fn row(columns: Vec<DriverValue>) -> DriverRow {
        DriverRow {
            columns: columns.into_iter().map(Some).collect(),
        }
    }

    #[test]
    fn the_mean_partition_is_weighted_by_each_ranges_count() {
        let mut readings = Readings::default();
        estimates(
            &[
                row(vec![DriverValue::BigInt(3), DriverValue::BigInt(100)]),
                row(vec![DriverValue::BigInt(1), DriverValue::BigInt(500)]),
            ],
            &mut readings,
        );

        assert_eq!(readings.get(Metric::Partitions), Some(4.0));
        assert_eq!(readings.get(Metric::MeanPartition), Some(200.0));
    }

    #[test]
    fn only_a_task_counted_in_bytes_adds_to_the_backlog() {
        let task = |progress, total, unit: &str| {
            row(vec![
                DriverValue::BigInt(progress),
                DriverValue::BigInt(total),
                DriverValue::Text(unit.to_owned()),
            ])
        };
        let mut readings = Readings::default();
        tasks(
            &[task(25, 100, "bytes"), task(0, 9_999, "keys")],
            &mut readings,
        );

        assert_eq!(readings.get(Metric::RunningCompactions), Some(2.0));
        assert_eq!(readings.get(Metric::CompactionBacklog), Some(75.0));
    }

    #[test]
    fn a_histogram_that_has_seen_nothing_reports_only_its_rate() {
        let metrics = [Metric::ReadRate, Metric::ReadLatencyP99];
        let mut idle = Readings::default();
        summary(&metrics, &[Some(0.0), Some(0.0), Some(0.0)], &mut idle);
        let mut busy = Readings::default();
        summary(&metrics, &[Some(7.0), Some(1.2), Some(0.4)], &mut busy);

        assert_eq!(idle.get(Metric::ReadRate), Some(0.0));
        assert_eq!(idle.get(Metric::ReadLatencyP99), None);
        assert_eq!(busy.get(Metric::ReadLatencyP99), Some(0.4));
    }

    #[test]
    fn no_snapshot_is_zero_rather_than_unknown() {
        let mut readings = Readings::default();
        snapshots(&[], &mut readings);

        assert_eq!(readings.get(Metric::Snapshots), Some(0.0));
        assert!(
            readings
                .get(Metric::SnapshotSize)
                .is_some_and(f64::is_sign_positive),
            "an empty sum is 0, not -0"
        );
    }

    #[test]
    fn a_gossip_status_drops_the_tokens_after_its_state() {
        let gossip = gossip_state("NORMAL,-1508567132727724545", Some("3168085.0"));

        assert_eq!(gossip.status, "NORMAL");
        assert_eq!(gossip.load, Some(3_168_085.0));
        assert_eq!(gossip_state("shutdown,true", None).status, "shutdown");
    }

    #[test]
    fn only_a_node_older_than_4_lacks_virtual_tables() {
        assert!(predates_virtual_tables("3.11.17"));
        assert!(!predates_virtual_tables("4.0.0"));
        assert!(!predates_virtual_tables("5.0.8"));
        assert!(!predates_virtual_tables("unparseable"));
    }
}

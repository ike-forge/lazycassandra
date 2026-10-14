//! A table's health as each node reports it, and the tree the block draws it
//! as (docs/superpowers/specs/2026-10-13-table-health-design.md).

use std::collections::{BTreeMap, BTreeSet};
use std::time::Duration;

use crate::bytes::Bytes;
use crate::contents::TableRef;
use crate::model::{DbError, QueryId};
use crate::state::Cursor;
use crate::style::Severity;

/// Cassandra 4.0's `compaction_large_partition_warning_threshold_mb` default,
/// for a node that configures no large-partition threshold at all.
const LARGE_PARTITION: f64 = 100.0 * 1024.0 * 1024.0;

/// What every node said about one table.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Health {
    pub nodes: Vec<NodeReport>,
}

/// One node's answer, under the name it gives itself.
#[derive(Debug, Clone, PartialEq)]
pub struct NodeReport {
    pub address: String,
    pub datacenter: Option<String>,
    pub version: Option<String>,
    /// What gossip on a reached node says about this one. `None` where no
    /// reached node has `system_views.gossip_info` (before 5.0).
    pub gossip: Option<Gossip>,
    pub outcome: NodeOutcome,
}

/// One node's gossip state, as the node that answered hears it.
#[derive(Debug, Clone, PartialEq)]
pub struct Gossip {
    /// The first word of the gossip `STATUS`: `NORMAL`, `LEAVING`,
    /// `shutdown`…
    pub status: String,
    /// Bytes the node holds across every table.
    pub load: Option<f64>,
}

impl Gossip {
    /// Gossip spells a drained or stopped node `shutdown`; a node that died
    /// without saying so still reads `NORMAL` until it is removed, and only
    /// the failure detector, which CQL does not expose, knows otherwise.
    fn severity(&self) -> Option<Severity> {
        if self.status.eq_ignore_ascii_case("NORMAL") {
            None
        } else if self.status.eq_ignore_ascii_case("shutdown") {
            Some(Severity::Danger)
        } else {
            Some(Severity::Warning)
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum NodeOutcome {
    Read {
        readings: Readings,
        thresholds: Thresholds,
    },
    /// Older than 4.0: no `system_views` to read.
    Unsupported,
    Failed(String),
    /// Named in a reached node's `system.peers_v2` and never asked: behind
    /// `discovery = "contact-points"` every request lands on the tunnelled
    /// node, and a virtual table answers only for the node it is read on.
    Unreached,
}

/// The metrics a node reported, each in its metric's [`Unit`]. A metric
/// missing here is one the node's version does not expose.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Readings(BTreeMap<Metric, f64>);

impl Readings {
    #[must_use]
    pub fn get(&self, metric: Metric) -> Option<f64> {
        self.0.get(&metric).copied()
    }

    pub fn set(&mut self, metric: Metric, value: f64) {
        self.0.insert(metric, value);
    }
}

/// A node's own limits, from `system_views.settings`. `None` where the node
/// leaves one unset or disabled.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Thresholds {
    pub tombstone_warn: Option<f64>,
    pub tombstone_failure: Option<f64>,
    /// In bytes.
    pub large_partition: Option<f64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Category {
    Storage,
    Reads,
    Writes,
    Compaction,
    Maintenance,
}

impl Category {
    pub const ALL: [Self; 5] = [
        Self::Storage,
        Self::Reads,
        Self::Writes,
        Self::Compaction,
        Self::Maintenance,
    ];

    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Storage => "Storage",
            Self::Reads => "Reads",
            Self::Writes => "Writes",
            Self::Compaction => "Compaction",
            Self::Maintenance => "Maintenance",
        }
    }
}

/// Declaration order is the order the block lists them in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Metric {
    DiskUsed,
    LargestPartition,
    MeanPartition,
    Partitions,
    LargestSstable,
    TombstonesMax,
    TombstonesP99,
    RowsPerReadP99,
    ReadLatencyP50,
    ReadLatencyP99,
    ReadRate,
    ScanLatencyP99,
    ScanRate,
    WriteLatencyP50,
    WriteLatencyP99,
    WriteRate,
    RunningCompactions,
    CompactionBacklog,
    LastCompaction,
    Snapshots,
    SnapshotSize,
}

/// What a metric's value counts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unit {
    Bytes,
    Count,
    Millis,
    PerSecond,
    /// Seconds before the read.
    Age,
}

/// How a metric's node values become the one its own row carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Aggregate {
    Total,
    Worst,
    /// The smallest age.
    Latest,
}

impl Aggregate {
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Total => "total",
            Self::Worst => "max",
            Self::Latest => "latest",
        }
    }

    fn of(self, values: impl Iterator<Item = f64>) -> Option<f64> {
        values.reduce(|left, right| match self {
            Self::Total => left + right,
            Self::Worst => left.max(right),
            Self::Latest => left.min(right),
        })
    }
}

impl Metric {
    pub const ALL: [Self; 21] = [
        Self::DiskUsed,
        Self::LargestPartition,
        Self::MeanPartition,
        Self::Partitions,
        Self::LargestSstable,
        Self::TombstonesMax,
        Self::TombstonesP99,
        Self::RowsPerReadP99,
        Self::ReadLatencyP50,
        Self::ReadLatencyP99,
        Self::ReadRate,
        Self::ScanLatencyP99,
        Self::ScanRate,
        Self::WriteLatencyP50,
        Self::WriteLatencyP99,
        Self::WriteRate,
        Self::RunningCompactions,
        Self::CompactionBacklog,
        Self::LastCompaction,
        Self::Snapshots,
        Self::SnapshotSize,
    ];

    #[must_use]
    pub const fn category(self) -> Category {
        match self {
            Self::DiskUsed
            | Self::LargestPartition
            | Self::MeanPartition
            | Self::Partitions
            | Self::LargestSstable => Category::Storage,
            Self::TombstonesMax
            | Self::TombstonesP99
            | Self::RowsPerReadP99
            | Self::ReadLatencyP50
            | Self::ReadLatencyP99
            | Self::ReadRate
            | Self::ScanLatencyP99
            | Self::ScanRate => Category::Reads,
            Self::WriteLatencyP50 | Self::WriteLatencyP99 | Self::WriteRate => Category::Writes,
            Self::RunningCompactions | Self::CompactionBacklog | Self::LastCompaction => {
                Category::Compaction
            }
            Self::Snapshots | Self::SnapshotSize => Category::Maintenance,
        }
    }

    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::DiskUsed => "Disk used",
            Self::LargestPartition => "Largest partition",
            Self::MeanPartition => "Mean partition",
            Self::Partitions => "Partitions (est.)",
            Self::LargestSstable => "Largest SSTable",
            Self::TombstonesMax => "Tombstones/read, max",
            Self::TombstonesP99 => "Tombstones/read, p99",
            Self::RowsPerReadP99 => "Rows/read, p99",
            Self::ReadLatencyP50 | Self::WriteLatencyP50 => "Latency, p50",
            Self::ReadLatencyP99 | Self::WriteLatencyP99 => "Latency, p99",
            Self::ReadRate => "Reads",
            Self::ScanLatencyP99 => "Range scan latency, p99",
            Self::ScanRate => "Range scans",
            Self::WriteRate => "Writes",
            Self::RunningCompactions => "Running",
            Self::CompactionBacklog => "Remaining",
            Self::LastCompaction => "Last finished",
            Self::Snapshots => "Snapshots",
            Self::SnapshotSize => "Snapshot size",
        }
    }

    #[must_use]
    pub const fn unit(self) -> Unit {
        match self {
            Self::DiskUsed
            | Self::LargestPartition
            | Self::MeanPartition
            | Self::LargestSstable
            | Self::CompactionBacklog
            | Self::SnapshotSize => Unit::Bytes,
            Self::Partitions
            | Self::TombstonesMax
            | Self::TombstonesP99
            | Self::RowsPerReadP99
            | Self::RunningCompactions
            | Self::Snapshots => Unit::Count,
            Self::ReadLatencyP50
            | Self::ReadLatencyP99
            | Self::ScanLatencyP99
            | Self::WriteLatencyP50
            | Self::WriteLatencyP99 => Unit::Millis,
            Self::ReadRate | Self::ScanRate | Self::WriteRate => Unit::PerSecond,
            Self::LastCompaction => Unit::Age,
        }
    }

    /// Partitions take the worst rather than the total: each node counts the
    /// replicas it holds, so a sum counts every partition once per replica.
    #[must_use]
    pub const fn aggregate(self) -> Aggregate {
        match self {
            Self::DiskUsed
            | Self::ReadRate
            | Self::ScanRate
            | Self::WriteRate
            | Self::RunningCompactions
            | Self::CompactionBacklog
            | Self::Snapshots
            | Self::SnapshotSize => Aggregate::Total,
            Self::LastCompaction => Aggregate::Latest,
            Self::LargestPartition
            | Self::MeanPartition
            | Self::Partitions
            | Self::LargestSstable
            | Self::TombstonesMax
            | Self::TombstonesP99
            | Self::RowsPerReadP99
            | Self::ReadLatencyP50
            | Self::ReadLatencyP99
            | Self::ScanLatencyP99
            | Self::WriteLatencyP50
            | Self::WriteLatencyP99 => Aggregate::Worst,
        }
    }
}

/// `value` in `unit`, as the block draws it.
#[must_use]
pub fn format(unit: Unit, value: f64) -> String {
    match unit {
        Unit::Bytes => Bytes::new(whole(value)).to_string(),
        Unit::Count => count(value),
        Unit::Millis if value < 10.0 => format!("{value:.2} ms"),
        Unit::Millis if value < 1000.0 => format!("{value:.0} ms"),
        Unit::Millis => format!("{:.1} s", value / 1000.0),
        Unit::PerSecond if value == 0.0 => "0/s".to_owned(),
        Unit::PerSecond if value < 1.0 => format!("{value:.2}/s"),
        Unit::PerSecond => format!("{}/s", count(value)),
        Unit::Age => age(Duration::from_secs(whole(value))),
    }
}

#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "a reading is non-negative and far below u64::MAX; `as` saturates either way"
)]
const fn whole(value: f64) -> u64 {
    value.round() as u64
}

fn count(value: f64) -> String {
    if value < 10.0 && value.fract() != 0.0 {
        return format!("{value:.1}");
    }
    if value < 10_000.0 {
        return whole(value).to_string();
    }
    if value < 10_000_000.0 {
        return format!("{:.1}k", value / 1000.0);
    }
    format!("{:.1}M", value / 1_000_000.0)
}

fn age(age: Duration) -> String {
    let seconds = age.as_secs();
    match seconds {
        0..60 => format!("{seconds} s ago"),
        60..3600 => format!("{} min ago", seconds / 60),
        3600..86_400 => format!("{} h ago", seconds / 3600),
        _ => format!("{} d ago", seconds / 86_400),
    }
}

/// One thing worth an operator's attention, on one node.
#[derive(Debug, Clone, PartialEq)]
pub struct Warning {
    /// The metric it is about, so its rows can carry the mark too; `None` for
    /// a node that answered nothing.
    pub metric: Option<Metric>,
    pub node: String,
    pub severity: Severity,
    pub text: String,
}

impl Health {
    /// Every warning, in node order and then metric order.
    #[must_use]
    pub fn warnings(&self) -> Vec<Warning> {
        let (answered, known) = self.coverage();
        let partial = self
            .nodes
            .iter()
            .any(|node| matches!(node.outcome, NodeOutcome::Unreached))
            .then(|| Warning {
                metric: None,
                node: String::new(),
                severity: Severity::Info,
                text: format!(
                    "this connection reaches {answered} of {known} nodes; \
                     totals and maxima cover those alone"
                ),
            });

        partial
            .into_iter()
            .chain(self.nodes.iter().flat_map(NodeReport::warnings))
            .collect()
    }

    /// How many nodes answered, of every node known to the cluster.
    #[must_use]
    pub fn coverage(&self) -> (usize, usize) {
        let answered = self
            .nodes
            .iter()
            .filter(|node| matches!(node.outcome, NodeOutcome::Read { .. }))
            .count();
        (answered, self.nodes.len())
    }

    /// The aggregate of `metric` over every node that reported it.
    #[must_use]
    pub fn aggregate(&self, metric: Metric) -> Option<f64> {
        metric
            .aggregate()
            .of(self.nodes.iter().filter_map(|node| node.reading(metric)))
    }

    /// Whether any node failed to answer, which is what opens `Nodes`.
    #[must_use]
    pub fn any_unanswered(&self) -> bool {
        self.nodes
            .iter()
            .any(|node| !matches!(node.outcome, NodeOutcome::Read { .. }))
    }
}

impl NodeReport {
    #[must_use]
    pub fn reading(&self, metric: Metric) -> Option<f64> {
        match &self.outcome {
            NodeOutcome::Read { readings, .. } => readings.get(metric),
            NodeOutcome::Unsupported | NodeOutcome::Failed(_) | NodeOutcome::Unreached => None,
        }
    }

    fn warnings(&self) -> Vec<Warning> {
        let gossip = self.gossip.as_ref().and_then(|gossip| {
            gossip.severity().map(|severity| Warning {
                metric: None,
                node: self.address.clone(),
                severity,
                text: format!("gossip reports {}", gossip.status),
            })
        });

        gossip.into_iter().chain(self.outcome_warnings()).collect()
    }

    fn outcome_warnings(&self) -> Vec<Warning> {
        let warning = |metric, severity, text| Warning {
            metric,
            node: self.address.clone(),
            severity,
            text,
        };

        let (readings, thresholds) = match &self.outcome {
            NodeOutcome::Read {
                readings,
                thresholds,
            } => (readings, thresholds),
            NodeOutcome::Unsupported => {
                return vec![warning(
                    None,
                    Severity::Warning,
                    "older than Cassandra 4.0, reports nothing".to_owned(),
                )];
            }
            NodeOutcome::Failed(_) => {
                return vec![warning(
                    None,
                    Severity::Warning,
                    "did not answer".to_owned(),
                )];
            }
            NodeOutcome::Unreached => return Vec::new(),
        };

        let mut warnings = Vec::new();

        if let Some(max) = readings.get(Metric::TombstonesMax) {
            let breach = [
                (thresholds.tombstone_failure, Severity::Danger, "failure"),
                (thresholds.tombstone_warn, Severity::Warning, "warn"),
            ]
            .into_iter()
            .find_map(|(limit, severity, name)| {
                limit
                    .filter(|limit| max >= *limit)
                    .map(|limit| (limit, severity, name))
            });
            if let Some((limit, severity, name)) = breach {
                warnings.push(warning(
                    Some(Metric::TombstonesMax),
                    severity,
                    format!(
                        "{} tombstones in one read, {name} threshold {}",
                        count(max),
                        count(limit)
                    ),
                ));
            }
        }

        if let Some(largest) = readings.get(Metric::LargestPartition) {
            let limit = thresholds.large_partition.unwrap_or(LARGE_PARTITION);
            if largest >= limit {
                warnings.push(warning(
                    Some(Metric::LargestPartition),
                    Severity::Warning,
                    format!(
                        "partition of {} over {}",
                        format(Unit::Bytes, largest),
                        format(Unit::Bytes, limit)
                    ),
                ));
            }
        }

        if let (Some(snapshots), Some(live)) = (
            readings.get(Metric::SnapshotSize),
            readings.get(Metric::DiskUsed),
        ) && snapshots > live
        {
            warnings.push(warning(
                Some(Metric::SnapshotSize),
                Severity::Warning,
                format!(
                    "snapshots hold {}, more than the {} live",
                    format(Unit::Bytes, snapshots),
                    format(Unit::Bytes, live)
                ),
            ));
        }

        warnings
    }
}

/// A heading at the margin.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Section {
    Warnings,
    Category(Category),
    Nodes,
}

impl Section {
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Warnings => "Warnings",
            Self::Category(category) => category.label(),
            Self::Nodes => "Nodes",
        }
    }
}

/// A row that folds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Fold {
    Section(Section),
    Metric(Metric),
}

impl Fold {
    const fn place(self) -> Place {
        match self {
            Self::Section(section) => Place::Heading(section),
            Self::Metric(metric) => Place::Metric(metric),
        }
    }
}

/// What a row of the tree is, and so where `h` takes the cursor from it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Place {
    Heading(Section),
    Metric(Metric),
    /// A row with nothing under it, and the row it hangs from.
    Leaf(Fold),
}

/// Where a row sits, which is what decides the limbs drawn before it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Indent {
    Root,
    Child { last: bool },
    Grandchild { last: bool, parent_last: bool },
}

impl Indent {
    const fn depth(self) -> u8 {
        match self {
            Self::Root => 0,
            Self::Child { .. } => 1,
            Self::Grandchild { .. } => 2,
        }
    }
}

/// One drawn row. The view lifts it as it is: the domain has already said
/// what each cell holds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    pub place: Place,
    pub indent: Indent,
    /// `Some(open)` on a row that folds.
    pub fold: Option<bool>,
    pub label: String,
    pub value: String,
    /// The aggregate a metric's row carries, or what a node is running.
    pub note: String,
    pub severity: Option<Severity>,
}

/// Whether the read has come back.
#[derive(Debug, Clone, PartialEq)]
pub enum Report {
    Loading,
    Failed(DbError),
    Loaded(Health),
}

/// The open block: one table, one read, and the reader's place in the tree.
#[derive(Debug, Clone, PartialEq)]
pub struct HealthPanel {
    table: TableRef,
    request: QueryId,
    report: Report,
    open: BTreeSet<Fold>,
    cursor: Cursor,
}

impl HealthPanel {
    /// Waiting on `request`, with the tree's default folds already chosen so
    /// the first frame after the answer lands needs no decision.
    #[must_use]
    pub fn new(table: TableRef, request: QueryId) -> Self {
        let open = std::iter::once(Section::Warnings)
            .chain(Category::ALL.map(Section::Category))
            .map(Fold::Section)
            .collect();

        Self {
            table,
            request,
            report: Report::Loading,
            open,
            cursor: Cursor::default(),
        }
    }

    #[must_use]
    pub const fn table(&self) -> &TableRef {
        &self.table
    }

    #[must_use]
    pub const fn request(&self) -> QueryId {
        self.request
    }

    #[must_use]
    pub const fn report(&self) -> &Report {
        &self.report
    }

    #[must_use]
    pub const fn is_loading(&self) -> bool {
        matches!(self.report, Report::Loading)
    }

    #[must_use]
    pub const fn selected(&self) -> usize {
        self.cursor.index()
    }

    pub fn landed(&mut self, result: Result<Health, DbError>) {
        self.report = match result {
            Ok(health) => {
                if health.any_unanswered() {
                    self.open.insert(Fold::Section(Section::Nodes));
                }
                Report::Loaded(health)
            }
            Err(error) => Report::Failed(error),
        };
    }

    /// The tree as it stands, empty until the read lands.
    #[must_use]
    pub fn rows(&self) -> Vec<Row> {
        match &self.report {
            Report::Loaded(health) => tree(health, &self.open),
            Report::Loading | Report::Failed(_) => Vec::new(),
        }
    }

    pub fn down(&mut self) {
        self.cursor.down(self.rows().len());
    }

    pub const fn up(&mut self) {
        self.cursor.up();
    }

    pub fn page_down(&mut self, height: usize) {
        self.cursor.page_down(self.rows().len(), height);
    }

    pub const fn page_up(&mut self, height: usize) {
        self.cursor.page_up(height);
    }

    pub const fn to_start(&mut self) {
        self.cursor.to_start();
    }

    pub fn to_end(&mut self) {
        self.cursor.to_end(self.rows().len());
    }

    /// `⏎`: folds an open row and unfolds a folded one.
    pub fn toggle(&mut self) {
        if let Some(fold) = self.fold_here()
            && !self.open.remove(&fold)
        {
            self.open.insert(fold);
        }
    }

    /// `l`: unfolds a folded row, and steps onto the first row under an open
    /// one.
    pub fn step_in(&mut self) {
        let Some(fold) = self.fold_here() else {
            return;
        };
        if self.open.insert(fold) {
            return;
        }
        let rows = self.rows();
        let at = self.cursor.index();
        let has_child = match (rows.get(at), rows.get(at + 1)) {
            (Some(row), Some(next)) => next.indent.depth() > row.indent.depth(),
            _ => false,
        };
        if has_child {
            self.cursor.down(rows.len());
        }
    }

    /// `h`: folds an open row, and steps out to the row above it otherwise.
    pub fn step_out(&mut self) {
        let rows = self.rows();
        let Some(row) = rows.get(self.cursor.index()) else {
            return;
        };

        let parent = match row.place {
            Place::Leaf(parent) => Some(parent),
            Place::Metric(metric) if !self.open.contains(&Fold::Metric(metric)) => {
                Some(Fold::Section(Section::Category(metric.category())))
            }
            Place::Metric(metric) => {
                self.open.remove(&Fold::Metric(metric));
                return;
            }
            Place::Heading(section) => {
                self.open.remove(&Fold::Section(section));
                return;
            }
        };

        let target = parent.map(Fold::place);
        if let Some(at) = rows.iter().position(|row| Some(row.place) == target) {
            self.cursor.go_to(at, rows.len());
        }
    }

    fn fold_here(&self) -> Option<Fold> {
        let rows = self.rows();
        let row = rows.get(self.cursor.index())?;
        row.fold?;
        match row.place {
            Place::Heading(section) => Some(Fold::Section(section)),
            Place::Metric(metric) => Some(Fold::Metric(metric)),
            Place::Leaf(_) => None,
        }
    }
}

fn tree(health: &Health, open: &BTreeSet<Fold>) -> Vec<Row> {
    let warnings = health.warnings();
    let mut rows = Vec::new();

    let worst = warnings.iter().map(|warning| warning.severity).max();
    let tally = if warnings.is_empty() {
        "none".to_owned()
    } else {
        warnings.len().to_string()
    };
    if heading(&mut rows, open, Section::Warnings, worst, tally) {
        let last = warnings.len().saturating_sub(1);
        rows.extend(warnings.iter().enumerate().map(|(at, warning)| Row {
            place: Place::Leaf(Fold::Section(Section::Warnings)),
            indent: Indent::Child { last: at == last },
            fold: None,
            label: warning.text.clone(),
            value: warning.node.clone(),
            note: String::new(),
            severity: Some(warning.severity),
        }));
    }

    for category in Category::ALL {
        category_rows(&mut rows, health, &warnings, open, category);
    }

    let last = health.nodes.len().saturating_sub(1);
    let nodes: Vec<Row> = health
        .nodes
        .iter()
        .enumerate()
        .map(|(at, node)| node_row(node, at == last))
        .collect();
    let worst = nodes.iter().filter_map(|row| row.severity).max();
    if heading(
        &mut rows,
        open,
        Section::Nodes,
        worst,
        health.nodes.len().to_string(),
    ) {
        rows.extend(nodes);
    }

    rows
}

/// Pushes a section's heading, and answers whether it is open.
fn heading(
    rows: &mut Vec<Row>,
    open: &BTreeSet<Fold>,
    section: Section,
    severity: Option<Severity>,
    note: String,
) -> bool {
    let is_open = open.contains(&Fold::Section(section));
    rows.push(Row {
        place: Place::Heading(section),
        indent: Indent::Root,
        fold: Some(is_open),
        label: section.label().to_owned(),
        value: String::new(),
        note,
        severity,
    });
    is_open
}

fn flagged(warnings: &[Warning], metric: Metric, node: Option<&str>) -> Option<Severity> {
    warnings
        .iter()
        .filter(|warning| warning.metric == Some(metric))
        .filter(|warning| node.is_none_or(|node| warning.node == node))
        .map(|warning| warning.severity)
        .max()
}

fn category_rows(
    rows: &mut Vec<Row>,
    health: &Health,
    warnings: &[Warning],
    open: &BTreeSet<Fold>,
    category: Category,
) {
    let metrics: Vec<Metric> = Metric::ALL
        .into_iter()
        .filter(|metric| metric.category() == category)
        .collect();
    let worst = metrics
        .iter()
        .filter_map(|metric| flagged(warnings, *metric, None))
        .max();

    if !heading(
        rows,
        open,
        Section::Category(category),
        worst,
        String::new(),
    ) {
        return;
    }

    let last_metric = metrics.len().saturating_sub(1);
    for (at, metric) in metrics.into_iter().enumerate() {
        let parent_last = at == last_metric;
        let is_open = open.contains(&Fold::Metric(metric));
        let aggregate = health.aggregate(metric);
        rows.push(Row {
            place: Place::Metric(metric),
            indent: Indent::Child { last: parent_last },
            fold: Some(is_open),
            label: metric.label().to_owned(),
            value: reading(metric, aggregate),
            note: if aggregate.is_some() && health.nodes.len() > 1 {
                metric.aggregate().label().to_owned()
            } else {
                String::new()
            },
            severity: flagged(warnings, metric, None),
        });

        if !is_open {
            continue;
        }
        let last_node = health.nodes.len().saturating_sub(1);
        rows.extend(health.nodes.iter().enumerate().map(|(at, node)| Row {
            place: Place::Leaf(Fold::Metric(metric)),
            indent: Indent::Grandchild {
                last: at == last_node,
                parent_last,
            },
            fold: None,
            label: node.address.clone(),
            value: reading(metric, node.reading(metric)),
            note: String::new(),
            severity: flagged(warnings, metric, Some(&node.address)),
        }));
    }
}

fn reading(metric: Metric, value: Option<f64>) -> String {
    value.map_or_else(|| "n/a".to_owned(), |value| format(metric.unit(), value))
}

fn node_row(node: &NodeReport, last: bool) -> Row {
    let gossip = node.gossip.as_ref();
    let (value, severity) = match &node.outcome {
        NodeOutcome::Read { .. } => (
            gossip.map_or_else(String::new, |gossip| gossip.status.to_lowercase()),
            None,
        ),
        NodeOutcome::Unsupported => ("older than 4.0".to_owned(), Some(Severity::Warning)),
        NodeOutcome::Failed(reason) => (reason.clone(), Some(Severity::Warning)),
        NodeOutcome::Unreached => ("not reachable through this connection".to_owned(), None),
    };
    let severity = severity.max(gossip.and_then(Gossip::severity));
    let running = node
        .version
        .iter()
        .cloned()
        .chain(
            gossip
                .and_then(|gossip| gossip.load)
                .map(|load| format!("{} on disk", format(Unit::Bytes, load))),
        )
        .collect::<Vec<_>>()
        .join(" · ");

    Row {
        place: Place::Leaf(Fold::Section(Section::Nodes)),
        indent: Indent::Child { last },
        fold: None,
        label: node.datacenter.as_ref().map_or_else(
            || node.address.clone(),
            |datacenter| format!("{} ({datacenter})", node.address),
        ),
        value,
        note: running,
        severity,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(address: &str, pairs: &[(Metric, f64)], thresholds: Thresholds) -> NodeReport {
        let mut readings = Readings::default();
        for (metric, value) in pairs {
            readings.set(*metric, *value);
        }
        NodeReport {
            address: address.to_owned(),
            datacenter: None,
            version: Some("5.0.8".to_owned()),
            gossip: None,
            outcome: NodeOutcome::Read {
                readings,
                thresholds,
            },
        }
    }

    fn failed(address: &str) -> NodeReport {
        NodeReport {
            address: address.to_owned(),
            datacenter: None,
            version: None,
            gossip: None,
            outcome: NodeOutcome::Failed("connection refused".to_owned()),
        }
    }

    fn unreached(address: &str) -> NodeReport {
        NodeReport {
            address: address.to_owned(),
            datacenter: None,
            version: Some("5.0.8".to_owned()),
            gossip: Some(Gossip {
                status: "NORMAL".to_owned(),
                load: Some(2048.0),
            }),
            outcome: NodeOutcome::Unreached,
        }
    }

    fn gossiping(mut report: NodeReport, status: &str) -> NodeReport {
        report.gossip = Some(Gossip {
            status: status.to_owned(),
            load: None,
        });
        report
    }

    fn warn_at(warn: f64, failure: f64) -> Thresholds {
        Thresholds {
            tombstone_warn: Some(warn),
            tombstone_failure: Some(failure),
            large_partition: None,
        }
    }

    fn panel(health: Health) -> HealthPanel {
        let mut panel = HealthPanel::new(TableRef::new("shop", "orders"), QueryId::next());
        panel.landed(Ok(health));
        panel
    }

    fn labels(panel: &HealthPanel) -> Vec<String> {
        panel.rows().into_iter().map(|row| row.label).collect()
    }

    fn row<'a>(rows: &'a [Row], label: &str) -> &'a Row {
        rows.iter()
            .find(|row| row.label == label)
            .unwrap_or_else(|| panic!("no row {label}"))
    }

    #[test]
    fn sizes_and_rates_add_up_across_nodes_and_percentiles_take_the_worst() {
        let health = Health {
            nodes: vec![
                node(
                    "10.0.0.1",
                    &[(Metric::DiskUsed, 100.0), (Metric::ReadLatencyP99, 4.0)],
                    Thresholds::default(),
                ),
                node(
                    "10.0.0.2",
                    &[(Metric::DiskUsed, 50.0), (Metric::ReadLatencyP99, 9.0)],
                    Thresholds::default(),
                ),
            ],
        };

        assert_eq!(health.aggregate(Metric::DiskUsed), Some(150.0));
        assert_eq!(health.aggregate(Metric::ReadLatencyP99), Some(9.0));
        assert_eq!(health.aggregate(Metric::Snapshots), None);
    }

    #[test]
    fn the_most_recent_compaction_is_the_youngest_age() {
        let health = Health {
            nodes: vec![
                node(
                    "a",
                    &[(Metric::LastCompaction, 7200.0)],
                    Thresholds::default(),
                ),
                node(
                    "b",
                    &[(Metric::LastCompaction, 60.0)],
                    Thresholds::default(),
                ),
            ],
        };

        assert_eq!(health.aggregate(Metric::LastCompaction), Some(60.0));
    }

    #[test]
    fn a_read_over_the_nodes_tombstone_thresholds_warns_and_past_failure_is_danger() {
        let health = Health {
            nodes: vec![
                node(
                    "a",
                    &[(Metric::TombstonesMax, 999.0)],
                    warn_at(1000.0, 100_000.0),
                ),
                node(
                    "b",
                    &[(Metric::TombstonesMax, 4200.0)],
                    warn_at(1000.0, 100_000.0),
                ),
                node(
                    "c",
                    &[(Metric::TombstonesMax, 100_000.0)],
                    warn_at(1000.0, 100_000.0),
                ),
            ],
        };

        let warnings = health.warnings();

        assert_eq!(
            warnings
                .iter()
                .map(|warning| (warning.node.as_str(), warning.severity))
                .collect::<Vec<_>>(),
            [("b", Severity::Warning), ("c", Severity::Danger)]
        );
        assert_eq!(
            warnings[0].text,
            "4200 tombstones in one read, warn threshold 1000"
        );
    }

    #[test]
    fn a_large_partition_warns_at_the_nodes_threshold_or_at_100_mib_without_one() {
        let mib = 1024.0 * 1024.0;
        let health = Health {
            nodes: vec![
                node(
                    "a",
                    &[(Metric::LargestPartition, 99.0 * mib)],
                    Thresholds::default(),
                ),
                node(
                    "b",
                    &[(Metric::LargestPartition, 100.0 * mib)],
                    Thresholds::default(),
                ),
                node(
                    "c",
                    &[(Metric::LargestPartition, 20.0 * mib)],
                    Thresholds {
                        large_partition: Some(10.0 * mib),
                        ..Thresholds::default()
                    },
                ),
            ],
        };

        let flagged: Vec<String> = health
            .warnings()
            .into_iter()
            .map(|warning| warning.node)
            .collect();

        assert_eq!(flagged, ["b", "c"]);
    }

    #[test]
    fn snapshots_larger_than_the_live_data_warn() {
        let health = Health {
            nodes: vec![node(
                "a",
                &[(Metric::DiskUsed, 10.0), (Metric::SnapshotSize, 11.0)],
                Thresholds::default(),
            )],
        };

        assert_eq!(
            health.warnings()[0].metric,
            Some(Metric::SnapshotSize),
            "a forgotten snapshot is the usual full disk"
        );
    }

    #[test]
    fn a_node_that_did_not_answer_is_a_warning_and_opens_the_nodes_section() {
        let panel = panel(Health {
            nodes: vec![node("a", &[], Thresholds::default()), failed("b")],
        });
        let rows = panel.rows();

        assert_eq!(row(&rows, "did not answer").value, "b");
        assert_eq!(row(&rows, "Nodes").fold, Some(true));
        assert_eq!(row(&rows, "b").value, "connection refused");
    }

    #[test]
    fn a_node_only_named_by_its_peers_is_listed_and_the_partial_view_is_said_once() {
        let panel = panel(Health {
            nodes: vec![
                node("a", &[(Metric::DiskUsed, 1.0)], Thresholds::default()),
                unreached("b"),
                unreached("c"),
            ],
        });
        let rows = panel.rows();

        let said: Vec<&Row> = rows
            .iter()
            .filter(|row| {
                row.label
                    .starts_with("this connection reaches 1 of 3 nodes")
            })
            .collect();
        assert_eq!(said.len(), 1);
        assert_eq!(said[0].severity, Some(Severity::Info));
        assert_eq!(row(&rows, "Nodes").fold, Some(true));
        assert_eq!(
            row(&rows, "b").value,
            "not reachable through this connection"
        );
        assert_eq!(row(&rows, "b").note, "5.0.8 · 2.0 KB on disk");
        assert_eq!(
            row(&rows, "Nodes").severity,
            None,
            "an unreached node is not by itself a fault"
        );
        assert_eq!(
            row(&rows, "Disk used").value,
            "1 B",
            "an unreached node contributes nothing to a total"
        );
    }

    #[test]
    fn gossip_that_is_not_normal_warns_and_shutdown_is_danger() {
        let health = Health {
            nodes: vec![
                gossiping(node("a", &[], Thresholds::default()), "NORMAL"),
                gossiping(unreached("b"), "LEAVING"),
                gossiping(unreached("c"), "shutdown"),
            ],
        };

        let flagged: Vec<(String, Severity, String)> = health
            .warnings()
            .into_iter()
            .filter(|warning| !warning.node.is_empty())
            .map(|warning| (warning.node, warning.severity, warning.text))
            .collect();

        assert_eq!(
            flagged,
            [
                (
                    "b".to_owned(),
                    Severity::Warning,
                    "gossip reports LEAVING".to_owned()
                ),
                (
                    "c".to_owned(),
                    Severity::Danger,
                    "gossip reports shutdown".to_owned()
                ),
            ]
        );
    }

    #[test]
    fn the_nodes_heading_carries_its_worst_row() {
        let panel = panel(Health {
            nodes: vec![
                node("a", &[], Thresholds::default()),
                gossiping(unreached("b"), "shutdown"),
            ],
        });

        assert_eq!(row(&panel.rows(), "Nodes").severity, Some(Severity::Danger));
    }

    #[test]
    fn every_node_answering_says_nothing_about_coverage() {
        let health = Health {
            nodes: vec![
                node("a", &[], Thresholds::default()),
                node("b", &[], Thresholds::default()),
            ],
        };

        assert_eq!(health.coverage(), (2, 2));
        assert!(health.warnings().is_empty());
    }

    #[test]
    fn warnings_and_categories_open_metrics_folded() {
        let panel = panel(Health {
            nodes: vec![node("a", &[(Metric::DiskUsed, 1.0)], Thresholds::default())],
        });
        let rows = panel.rows();

        assert_eq!(row(&rows, "Warnings").fold, Some(true));
        assert_eq!(row(&rows, "Warnings").note, "none");
        assert_eq!(row(&rows, "Storage").fold, Some(true));
        assert_eq!(row(&rows, "Disk used").fold, Some(false));
        assert_eq!(row(&rows, "Nodes").fold, Some(false));
        assert!(
            !labels(&panel).contains(&"a".to_owned()),
            "nodes stay folded"
        );
    }

    #[test]
    fn a_metric_no_node_reported_reads_not_available() {
        let panel = panel(Health {
            nodes: vec![node("a", &[], Thresholds::default())],
        });

        assert_eq!(row(&panel.rows(), "Largest SSTable").value, "n/a");
    }

    #[test]
    fn the_aggregate_is_named_only_where_there_is_more_than_one_node() {
        let one = panel(Health {
            nodes: vec![node("a", &[(Metric::DiskUsed, 1.0)], Thresholds::default())],
        });
        let two = panel(Health {
            nodes: vec![
                node("a", &[(Metric::DiskUsed, 1.0)], Thresholds::default()),
                node("b", &[(Metric::DiskUsed, 1.0)], Thresholds::default()),
            ],
        });

        assert_eq!(row(&one.rows(), "Disk used").note, "");
        assert_eq!(row(&two.rows(), "Disk used").note, "total");
    }

    fn seat(panel: &mut HealthPanel, label: &str) {
        let at = labels(panel)
            .iter()
            .position(|row| row == label)
            .unwrap_or_else(|| panic!("no row {label}"));
        panel.cursor.go_to(at, panel.rows().len());
    }

    fn selected_label(panel: &HealthPanel) -> String {
        labels(panel)[panel.selected()].clone()
    }

    #[test]
    fn l_unfolds_a_metric_into_its_nodes_then_steps_onto_the_first() {
        let mut panel = panel(Health {
            nodes: vec![
                node(
                    "10.0.0.1",
                    &[(Metric::DiskUsed, 1.0)],
                    Thresholds::default(),
                ),
                node(
                    "10.0.0.2",
                    &[(Metric::DiskUsed, 2.0)],
                    Thresholds::default(),
                ),
            ],
        });
        seat(&mut panel, "Disk used");

        panel.step_in();
        assert_eq!(selected_label(&panel), "Disk used");
        assert!(labels(&panel).contains(&"10.0.0.2".to_owned()));

        panel.step_in();
        assert_eq!(selected_label(&panel), "10.0.0.1");
    }

    #[test]
    fn h_steps_out_of_a_node_then_folds_its_metric_then_steps_to_the_heading() {
        let mut panel = panel(Health {
            nodes: vec![node("10.0.0.1", &[], Thresholds::default())],
        });
        seat(&mut panel, "Mean partition");
        panel.step_in();
        panel.step_in();
        assert_eq!(selected_label(&panel), "10.0.0.1");

        panel.step_out();
        assert_eq!(selected_label(&panel), "Mean partition");

        panel.step_out();
        assert!(!labels(&panel).contains(&"10.0.0.1".to_owned()));

        panel.step_out();
        assert_eq!(selected_label(&panel), "Storage");

        panel.step_out();
        assert!(!labels(&panel).contains(&"Disk used".to_owned()));
    }

    #[test]
    fn enter_toggles_a_heading_and_does_nothing_on_a_leaf() {
        let mut panel = panel(Health {
            nodes: vec![node("a", &[], Thresholds::default()), failed("b")],
        });
        seat(&mut panel, "Reads");
        panel.toggle();
        assert!(!labels(&panel).contains(&"Tombstones/read, max".to_owned()));
        panel.toggle();
        assert!(labels(&panel).contains(&"Tombstones/read, max".to_owned()));

        seat(&mut panel, "did not answer");
        let before = panel.rows();
        panel.toggle();
        assert_eq!(panel.rows(), before);
    }

    #[test]
    fn a_metric_row_and_its_node_carry_the_warning_mark() {
        let mut panel = panel(Health {
            nodes: vec![
                node("a", &[(Metric::TombstonesMax, 1.0)], warn_at(10.0, 100.0)),
                node("b", &[(Metric::TombstonesMax, 50.0)], warn_at(10.0, 100.0)),
            ],
        });
        seat(&mut panel, "Tombstones/read, max");
        panel.step_in();
        let rows = panel.rows();

        assert_eq!(row(&rows, "Reads").severity, Some(Severity::Warning));
        assert_eq!(
            row(&rows, "Tombstones/read, max").severity,
            Some(Severity::Warning)
        );
        assert_eq!(row(&rows, "a").severity, None);
        assert_eq!(row(&rows, "b").severity, Some(Severity::Warning));
    }

    #[test]
    fn a_grandchild_knows_whether_its_metric_was_the_last_of_its_category() {
        let mut panel = panel(Health {
            nodes: vec![node("a", &[], Thresholds::default())],
        });
        seat(&mut panel, "Snapshot size");
        panel.step_in();

        let rows = panel.rows();
        let leaf = rows
            .iter()
            .find(|row| row.place == Place::Leaf(Fold::Metric(Metric::SnapshotSize)))
            .expect("the node under the last metric");

        assert_eq!(
            leaf.indent,
            Indent::Grandchild {
                last: true,
                parent_last: true
            }
        );
    }

    #[test]
    fn readings_are_drawn_in_their_metrics_unit() {
        assert_eq!(format(Unit::Bytes, 3_565_158.0), "3.4 MB");
        assert_eq!(format(Unit::Count, 4200.0), "4200");
        assert_eq!(format(Unit::Count, 42_000.0), "42.0k");
        assert_eq!(format(Unit::Count, 2.5), "2.5");
        assert_eq!(format(Unit::Millis, 0.4213), "0.42 ms");
        assert_eq!(format(Unit::Millis, 120.0), "120 ms");
        assert_eq!(format(Unit::Millis, 2500.0), "2.5 s");
        assert_eq!(format(Unit::PerSecond, 1.185), "1.2/s");
        assert_eq!(format(Unit::PerSecond, 0.04), "0.04/s");
        assert_eq!(format(Unit::PerSecond, 0.0), "0/s");
        assert_eq!(format(Unit::Age, 42.0), "42 s ago");
        assert_eq!(format(Unit::Age, 7200.0), "2 h ago");
        assert_eq!(format(Unit::Age, 200_000.0), "2 d ago");
    }

    #[test]
    fn every_metric_belongs_to_a_category_that_lists_it() {
        for category in Category::ALL {
            assert!(
                Metric::ALL
                    .iter()
                    .any(|metric| metric.category() == category),
                "{category:?} would draw an empty heading"
            );
        }
    }
}

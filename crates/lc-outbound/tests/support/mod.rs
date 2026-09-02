//! Shared fixtures and the driving loop for the scenario tiers
//! (architecture.md §10).
//!
//! `run` pumps `update`'s effects through the ports and feeds the resulting
//! messages back, exactly as `lc-app`'s runtime does. Every scenario file
//! under `tests/` reuses it rather than rolling its own loop.
//!
//! Cargo compiles this module once per test binary that declares `mod
//! support;`, and no one binary uses every item — the dead-code lint would
//! otherwise flag whichever half a given file leaves untouched.
#![allow(
    dead_code,
    reason = "shared across test binaries with different subsets in use"
)]

use std::collections::{BTreeMap, VecDeque};
use std::path::PathBuf;
use std::time::Duration;

use lc_domain::config::{ConnectionConfig, ConnectionName};
use lc_domain::export::{Encoder, Format};
use lc_domain::model::{
    Column, ConnId, JobId, Key, KeyPress, Keyspace, Page, PrimaryKey, Replication, Schema,
    Statement, Table, TableOptions,
};
use lc_domain::ports::{Cluster, ExportError, Sink, Writer};
use lc_domain::posture::Posture;
use lc_domain::{Effect, Msg, Source, State, update};

pub(crate) fn schema_of(names: &[&str]) -> Schema {
    tabled_schema_of(&names.iter().map(|n| (*n, &[][..])).collect::<Vec<_>>())
}

pub(crate) fn table(name: &str) -> Table {
    Table {
        name: name.to_owned(),
        primary_key: PrimaryKey {
            partition: vec![Column {
                name: "id".to_owned(),
                cql_type: "uuid".to_owned(),
            }],
            clustering: Vec::new(),
        },
        static_columns: Vec::new(),
        regular_columns: Vec::new(),
        options: TableOptions::new(BTreeMap::from([
            ("comment".to_owned(), format!("The {name} table.")),
            ("gc_grace_seconds".to_owned(), "864000".to_owned()),
        ])),
    }
}

/// `(keyspace name, its table names)`.
pub(crate) fn tabled_schema_of(keyspaces: &[(&str, &[&str])]) -> Schema {
    Schema {
        keyspaces: keyspaces
            .iter()
            .map(|(name, tables)| {
                (
                    (*name).to_owned(),
                    Keyspace {
                        name: (*name).to_owned(),
                        replication: Replication::SimpleStrategy { factor: 1 },
                        tables: tables
                            .iter()
                            .map(|t| ((*t).to_owned(), table(t)))
                            .collect::<BTreeMap<_, _>>(),
                    },
                )
            })
            .collect(),
    }
}

pub(crate) fn press(state: &mut State, key: Key) -> Vec<Effect> {
    update(state, Msg::Input(KeyPress::new(key)))
}

/// A config naming one connection and making it the default, which is what
/// `init` needs before it will connect to anything at all: nothing is reached
/// unless a file named it (connection picker design §2).
#[allow(
    clippy::expect_used,
    reason = "allow-expect-in-tests only covers #[test]-attributed items, not helpers they call"
)]
pub(crate) fn connected_config() -> lc_domain::Config {
    let name = ConnectionName::try_new("fixture").expect("a valid name");

    lc_domain::Config {
        connections: vec![ConnectionConfig {
            name: name.clone(),
            hosts: vec!["127.0.0.1:9042".to_owned()],
            posture: Posture::ReadOnly,
            discovery: lc_domain::config::Discovery::default(),
            datacenter: None,
            credentials: None,
        }],
        default_connection: Some(name),
        ..lc_domain::Config::default()
    }
}

/// Drives a session with no file destination in play. Any `Effect::Export`
/// reaching this is a scenario bug: use [`run_with_writer`] instead.
pub(crate) async fn run(cluster: &dyn Cluster, state: &mut State, effects: Vec<Effect>) {
    run_with_writer(cluster, None, state, effects).await;
}

/// Drives a session, following an export through `writer` when the session
/// produces one.
pub(crate) async fn run_with_writer(
    cluster: &dyn Cluster,
    writer: Option<&dyn Writer>,
    state: &mut State,
    effects: Vec<Effect>,
) {
    let mut pending: VecDeque<Effect> = effects.into();

    while let Some(effect) = pending.pop_front() {
        let messages: Vec<Msg> = match effect {
            Effect::Connect { conn, target } => vec![match cluster.connect(conn, &target).await {
                Ok(posture) => Msg::Connected {
                    conn,
                    posture,
                    took: Duration::ZERO,
                },
                Err(error) => Msg::ConnectFailed {
                    conn,
                    error,
                    took: Duration::ZERO,
                },
            }],
            Effect::LoadSchema(conn) => vec![match cluster.describe(conn).await {
                Ok(schema) => Msg::SchemaLoaded {
                    conn,
                    schema: Box::new(schema),
                    took: Duration::ZERO,
                },
                Err(error) => Msg::SchemaFailed {
                    conn,
                    error,
                    took: Duration::ZERO,
                },
            }],
            Effect::Execute { conn, query, stmt } => {
                vec![match cluster.execute(conn, &stmt).await {
                    Ok(page) => Msg::PageReady {
                        query,
                        page: Box::new(page),
                        took: Duration::ZERO,
                    },
                    Err(error) => Msg::Failed {
                        query,
                        error,
                        took: Duration::ZERO,
                    },
                }]
            }
            Effect::Export {
                conn,
                job,
                source,
                destination,
                format,
                overwrite,
            } => {
                let Some(writer) = writer else {
                    panic!("a scenario producing Effect::Export needs a writer to drive it");
                };
                let job = Job {
                    conn,
                    id: job,
                    source,
                    destination,
                    format,
                    overwrite,
                };
                run_export(cluster, writer, &job).await
            }
            _ => continue,
        };

        for msg in messages {
            pending.extend(update(state, msg));
        }
    }
}

/// What one export needs, bundled the way `lc-app`'s own `Job` is (mirrors
/// `crates/lc-app/src/export.rs::Job`).
struct Job {
    conn: ConnId,
    id: JobId,
    source: Source,
    destination: PathBuf,
    format: Format,
    overwrite: bool,
}

/// Reproduces the sequencing of `lc-app`'s `export::run` against the ports,
/// as messages rather than a channel send. `lc-app` is a binary crate and
/// cannot be a dependency of `lc-outbound`, so this cannot call that code —
/// only mirror it (task 11 brief). Keep the two in step by hand: any change
/// to `crates/lc-app/src/export.rs`'s sequencing belongs here too.
async fn run_export(cluster: &dyn Cluster, writer: &dyn Writer, job: &Job) -> Vec<Msg> {
    let mut messages = Vec::new();
    let result = export(cluster, writer, job, &mut messages).await;
    messages.push(Msg::ExportFinished {
        job: job.id,
        result,
        took: Duration::ZERO,
    });
    messages
}

async fn export(
    cluster: &dyn Cluster,
    writer: &dyn Writer,
    job: &Job,
    messages: &mut Vec<Msg>,
) -> Result<usize, ExportError> {
    let mut sink = writer.open(&job.destination, job.overwrite).await?;
    let mut encoder = Encoder::new(job.format);

    let result = match write(cluster, sink.as_mut(), &mut encoder, job, messages).await {
        Ok(written) => sink.finish().await.map(|()| written),
        Err(error) => Err(error),
    };

    if result.is_err() {
        sink.abort().await;
    }

    result
}

async fn write(
    cluster: &dyn Cluster,
    sink: &mut dyn Sink,
    encoder: &mut Encoder,
    job: &Job,
    messages: &mut Vec<Msg>,
) -> Result<usize, ExportError> {
    let written = match &job.source {
        Source::Fetched { columns, rows } => {
            sink.write(&encoder.start(columns)).await?;
            sink.write(&encoder.chunk(columns, rows)).await?;
            let written = rows.len();
            messages.push(Msg::ExportProgress {
                job: job.id,
                rows: written,
            });
            written
        }
        Source::Whole(statement) => pages(cluster, sink, encoder, job, statement, messages).await?,
    };

    sink.write(&encoder.finish()).await?;

    Ok(written)
}

async fn pages(
    cluster: &dyn Cluster,
    sink: &mut dyn Sink,
    encoder: &mut Encoder,
    job: &Job,
    statement: &Statement,
    messages: &mut Vec<Msg>,
) -> Result<usize, ExportError> {
    let mut cursor = None;
    let mut written = 0;
    let mut started = false;

    loop {
        let page: Page = cluster
            .execute_paged(job.conn, statement, cursor.as_ref())
            .await
            .map_err(ExportError::Cluster)?;

        if !started {
            sink.write(&encoder.start(&page.columns)).await?;
            started = true;
        }

        sink.write(&encoder.chunk(&page.columns, &page.rows))
            .await?;
        written += page.rows.len();
        messages.push(Msg::ExportProgress {
            job: job.id,
            rows: written,
        });

        match page.next {
            Some(next) => cursor = Some(next),
            None => return Ok(written),
        }
    }
}

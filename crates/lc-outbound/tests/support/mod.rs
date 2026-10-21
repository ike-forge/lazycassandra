//! Shared fixtures and the loop that pumps effects through the ports, as
//! `lc-app`'s runtime does. Each test binary uses part of this, hence the
//! dead-code allow.
#![allow(
    dead_code,
    reason = "shared across test binaries with different subsets in use"
)]

use std::collections::{BTreeMap, VecDeque};
use std::num::NonZeroUsize;
use std::time::Duration;

use lc_domain::config::{ConnectionConfig, ConnectionName, PageSize};
use lc_domain::export::{Destination, Encoder, Exported, Format};
use lc_domain::model::{
    Column, ConnId, JobId, Key, KeyPress, Keyspace, Page, PrimaryKey, Replication, Schema,
    Statement, Table, TableOptions,
};
use lc_domain::ports::{Cluster, ExportError, Sink, Writer};
use lc_domain::posture::Posture;
use lc_domain::{Effect, Msg, Source, State, update};
use lc_outbound::buffer::Buffer;

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
        indexes: Vec::new(),
        options: TableOptions::new(BTreeMap::from([
            ("comment".to_owned(), format!("The {name} table.")),
            ("gc_grace_seconds".to_owned(), "864000".to_owned()),
        ])),
    }
}

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

/// `init` connects only to a default a config names.
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
            tls: lc_domain::config::Tls::Off,
        }],
        default_connection: Some(name),
        ..lc_domain::Config::default()
    }
}

/// An `Effect::Export` here is a scenario bug: use [`run_with_writer`].
pub(crate) async fn run(cluster: &dyn Cluster, state: &mut State, effects: Vec<Effect>) {
    run_with_writer(cluster, None, state, effects).await;
}

/// Returns what reached the clipboard.
pub(crate) async fn run_with_writer(
    cluster: &dyn Cluster,
    writer: Option<&dyn Writer>,
    state: &mut State,
    effects: Vec<Effect>,
) -> Vec<String> {
    let mut pending: VecDeque<Effect> = effects.into();
    let mut yanked = Vec::new();

    while let Some(effect) = pending.pop_front() {
        if let Effect::Yank(line) = &effect {
            yanked.push(line.clone());
        }
        let messages: Vec<Msg> = match effect {
            Effect::Connect {
                conn,
                target,
                driver,
            } => vec![match cluster.connect(conn, &target, &driver).await {
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
            Effect::ExecutePaged {
                conn,
                query,
                stmt,
                page_size,
                cursor,
            } => vec![match cluster
                .execute_paged(conn, &stmt, page_size, cursor.as_ref())
                .await
            {
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
            }],
            Effect::Export {
                conn,
                job,
                source,
                destination,
                format,
                page_size,
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
                    page_size,
                };
                run_export(cluster, writer, &job).await
            }
            _ => continue,
        };

        for msg in messages {
            pending.extend(update(state, msg));
        }
    }

    yanked
}

struct Job {
    conn: ConnId,
    id: JobId,
    source: Source,
    destination: Destination,
    format: Format,
    page_size: PageSize,
}

/// Mirrors `lc-app`'s `export::run`, which a library test cannot call. Keep the
/// two in step by hand.
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
) -> Result<Exported, ExportError> {
    let mut encoder = Encoder::new(job.format);

    let (path, overwrite) = match &job.destination {
        Destination::File { path, overwrite } => (path, *overwrite),
        Destination::Clipboard(limit) => {
            let mut buffer = Buffer::new(*limit);
            let rows = write(cluster, &mut buffer, &mut encoder, job, messages).await?;
            let text = buffer.into_text();

            return Ok(Exported::Copied { rows, text });
        }
    };
    let mut sink = writer.open(path, overwrite).await?;

    let result = match write(cluster, sink.as_mut(), &mut encoder, job, messages).await {
        Ok(written) => sink.finish().await.map(|()| written),
        Err(error) => Err(error),
    };

    if result.is_err() {
        sink.abort().await;
    }

    result.map(Exported::Written)
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
        Source::Whole { statement, limit } => {
            pages(cluster, sink, encoder, job, statement, *limit, messages).await?
        }
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
    limit: Option<NonZeroUsize>,
    messages: &mut Vec<Msg>,
) -> Result<usize, ExportError> {
    let mut cursor = None;
    let mut written = 0;
    let mut started = false;

    loop {
        let page: Page = cluster
            .execute_paged(job.conn, statement, job.page_size, cursor.as_ref())
            .await
            .map_err(ExportError::Cluster)?;

        if !started {
            sink.write(&encoder.start(&page.columns)).await?;
            started = true;
        }

        let room = limit.map_or(usize::MAX, |limit| limit.get() - written);
        let taken = page.rows.len().min(room);

        sink.write(&encoder.chunk(&page.columns, &page.rows[..taken]))
            .await?;
        written += taken;
        messages.push(Msg::ExportProgress {
            job: job.id,
            rows: written,
        });

        match page.next {
            Some(next)
                if taken == page.rows.len() && Some(written) != limit.map(NonZeroUsize::get) =>
            {
                cursor = Some(next);
            }
            _ => return Ok(written),
        }
    }
}

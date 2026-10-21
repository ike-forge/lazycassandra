//! The one effect that reads the cluster and writes a file or the clipboard's
//! buffer, so it lives where both ports are. `lc-outbound/tests/support/mod.rs` mirrors this sequencing
//! by hand; keep them in step.

use std::num::NonZeroUsize;
use std::sync::Arc;

use lc_domain::config::PageSize;
use lc_domain::export::{Destination, Encoder, Exported, Format};
use lc_domain::model::{ConnId, JobId, Page, Statement};
use lc_domain::ports::{Cluster, ExportError, Sink, Writer};
use lc_domain::{Msg, Source};
use lc_outbound::buffer::Buffer;
use tokio::sync::mpsc::UnboundedSender;

use crate::input::Incoming;

pub(crate) struct Job {
    pub(crate) conn: ConnId,
    pub(crate) id: JobId,
    pub(crate) source: Source,
    pub(crate) destination: Destination,
    pub(crate) format: Format,
    pub(crate) page_size: PageSize,
}

pub(crate) fn spawn(
    tx: &UnboundedSender<Incoming>,
    cluster: &Arc<dyn Cluster>,
    writer: &Arc<dyn Writer>,
    job: Job,
) {
    let (tx, cluster, writer) = (tx.clone(), Arc::clone(cluster), Arc::clone(writer));

    tokio::spawn(async move {
        let id = job.id;
        let started = std::time::Instant::now();
        let result = run(&tx, cluster.as_ref(), writer.as_ref(), job).await;
        let took = started.elapsed();
        let _sent = tx.send(
            Msg::ExportFinished {
                job: id,
                result,
                took,
            }
            .into(),
        );
    });
}

async fn run(
    tx: &UnboundedSender<Incoming>,
    cluster: &dyn Cluster,
    writer: &dyn Writer,
    job: Job,
) -> Result<Exported, ExportError> {
    let mut encoder = Encoder::new(job.format);

    let (path, overwrite) = match &job.destination {
        Destination::File { path, overwrite } => (path, *overwrite),
        Destination::Clipboard(limit) => {
            let mut buffer = Buffer::new(*limit);
            let rows = write(tx, cluster, &mut buffer, &mut encoder, &job).await?;
            let text = buffer.into_text();

            return Ok(Exported::Copied { rows, text });
        }
    };
    let mut sink = writer.open(path, overwrite).await?;

    // A partial file is not a result in any format, and `finish` is the flush
    // most likely to leave one.
    let result = match write(tx, cluster, sink.as_mut(), &mut encoder, &job).await {
        Ok(written) => sink.finish().await.map(|()| written),
        Err(error) => Err(error),
    };

    if result.is_err() {
        sink.abort().await;
    }

    result.map(Exported::Written)
}

async fn write(
    tx: &UnboundedSender<Incoming>,
    cluster: &dyn Cluster,
    sink: &mut dyn Sink,
    encoder: &mut Encoder,
    job: &Job,
) -> Result<usize, ExportError> {
    let written = match &job.source {
        Source::Fetched { columns, rows } => {
            sink.write(&encoder.start(columns)).await?;
            sink.write(&encoder.chunk(columns, rows)).await?;
            let written = rows.len();
            report(tx, job.id, written);
            written
        }
        Source::Whole { statement, limit } => {
            pages(tx, cluster, sink, encoder, job, statement, *limit).await?
        }
    };

    sink.write(&encoder.finish()).await?;

    Ok(written)
}

/// The header comes from the first page, which carries the columns.
async fn pages(
    tx: &UnboundedSender<Incoming>,
    cluster: &dyn Cluster,
    sink: &mut dyn Sink,
    encoder: &mut Encoder,
    job: &Job,
    statement: &Statement,
    limit: Option<NonZeroUsize>,
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
        report(tx, job.id, written);

        match page.next {
            Some(next) if Some(written) != limit.map(NonZeroUsize::get) => cursor = Some(next),
            _ => return Ok(written),
        }
    }
}

fn report(tx: &UnboundedSender<Incoming>, job: JobId, rows: usize) {
    // A closed channel means the main loop has ended.
    let _sent = tx.send(Msg::ExportProgress { job, rows }.into());
}

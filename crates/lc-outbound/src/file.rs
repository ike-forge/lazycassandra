//! The [`Writer`] port over `tokio::fs`.
//!
//! [`Writer`]: lc_domain::ports::Writer

use std::path::{Path, PathBuf};

use async_trait::async_trait;
use lc_domain::ports::{ExportError, Sink, Writer};
use tokio::fs::File;
use tokio::io::{AsyncWriteExt, BufWriter};

#[derive(Debug, Clone, Copy, Default)]
pub struct FileWriter;

#[async_trait]
impl Writer for FileWriter {
    async fn open(&self, path: &Path, overwrite: bool) -> Result<Box<dyn Sink>, ExportError> {
        let mut options = tokio::fs::OpenOptions::new();
        options.write(true);

        if overwrite {
            options.create(true).truncate(true);
        } else {
            // One syscall checks and creates, so nothing can appear between.
            options.create_new(true);
        }

        let file = options.open(path).await.map_err(|error| {
            if error.kind() == std::io::ErrorKind::AlreadyExists {
                ExportError::Exists
            } else {
                ExportError::Io(error.to_string())
            }
        })?;

        Ok(Box::new(FileSink {
            file: BufWriter::new(file),
            path: path.to_owned(),
        }))
    }
}

struct FileSink {
    file: BufWriter<File>,
    path: PathBuf,
}

#[async_trait]
impl Sink for FileSink {
    async fn write(&mut self, chunk: &str) -> Result<(), ExportError> {
        self.file
            .write_all(chunk.as_bytes())
            .await
            .map_err(|error| ExportError::Io(error.to_string()))
    }

    async fn finish(&mut self) -> Result<(), ExportError> {
        self.file
            .flush()
            .await
            .map_err(|error| ExportError::Io(error.to_string()))
    }

    async fn abort(&mut self) {
        let _flushed = self.file.flush().await;
        let _removed = tokio::fs::remove_file(&self.path).await;
    }
}

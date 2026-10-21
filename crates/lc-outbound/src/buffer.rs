//! The [`Sink`] a clipboard copy fills.
//!
//! [`Sink`]: lc_domain::ports::Sink

use async_trait::async_trait;
use lc_domain::config::ClipboardMb;
use lc_domain::ports::{ExportError, Sink};

/// Refuses the chunk that would cross the limit, so an oversized result stops
/// fetching there rather than after the last page.
#[derive(Debug)]
pub struct Buffer {
    limit: ClipboardMb,
    text: String,
}

impl Buffer {
    #[must_use]
    pub const fn new(limit: ClipboardMb) -> Self {
        Self {
            limit,
            text: String::new(),
        }
    }

    #[must_use]
    pub fn into_text(self) -> String {
        self.text
    }
}

#[async_trait]
impl Sink for Buffer {
    async fn write(&mut self, chunk: &str) -> Result<(), ExportError> {
        if self.text.len() + chunk.len() > self.limit.bytes() {
            return Err(ExportError::TooLarge(self.limit));
        }
        self.text.push_str(chunk);

        Ok(())
    }

    async fn finish(&mut self) -> Result<(), ExportError> {
        Ok(())
    }

    async fn abort(&mut self) {
        self.text = String::new();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn one_mb() -> ClipboardMb {
        ClipboardMb::try_new(1).expect("in range")
    }

    #[tokio::test]
    async fn a_result_exactly_at_the_limit_is_copied() {
        let mut buffer = Buffer::new(one_mb());
        let half = "a".repeat(512 * 1024);

        buffer.write(&half).await.expect("under");
        buffer.write(&half).await.expect("at, not over");

        assert_eq!(buffer.into_text().len(), 1024 * 1024);
    }

    #[tokio::test]
    async fn the_byte_past_the_limit_fails_the_copy() {
        let mut buffer = Buffer::new(one_mb());

        buffer.write(&"a".repeat(1024 * 1024)).await.expect("at");

        assert_eq!(
            buffer.write("b").await,
            Err(ExportError::TooLarge(one_mb()))
        );
    }

    #[tokio::test]
    async fn the_limit_counts_bytes_not_characters() {
        let mut buffer = Buffer::new(one_mb());

        assert_eq!(
            buffer.write(&"é".repeat(512 * 1024 + 1)).await,
            Err(ExportError::TooLarge(one_mb())),
            "two bytes each"
        );
    }
}

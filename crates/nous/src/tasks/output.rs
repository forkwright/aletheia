//! Disk-backed task output with streaming read-back.
//!
//! WHY: Large shell outputs should not live in memory. Writing to a temp file
//! as output arrives lets callers page through results via `AsyncRead` without
//! loading the full buffer.

use std::io;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::task::{Context, Poll};

use snafu::{ResultExt as _, Snafu};
use tokio::fs;
use tokio::io::{AsyncRead, AsyncWriteExt as _, ReadBuf};

/// Errors from task output I/O.
#[derive(Debug, Snafu)]
#[snafu(visibility(pub(crate)))]
#[expect(
    missing_docs,
    reason = "snafu error variant fields (source, location, path) are self-documenting via display format"
)]
#[non_exhaustive]
pub enum OutputError {
    /// Failed to create the output temp file.
    #[snafu(display("failed to create output file: {source}"))]
    Create {
        source: io::Error,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Failed to write to the output file.
    #[snafu(display("failed to write output: {source}"))]
    Write {
        source: io::Error,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Failed to flush the output file.
    #[snafu(display("failed to flush output: {source}"))]
    Flush {
        source: io::Error,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Failed to open the output file for reading.
    #[snafu(display("failed to open output for reading: {source}"))]
    Open {
        source: io::Error,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Failed to remove the output file.
    #[snafu(display("failed to remove output file at {}: {source}", path.display()))]
    Remove {
        path: PathBuf,
        source: io::Error,
        #[snafu(implicit)]
        location: snafu::Location,
    },
}

/// Writes task output to a temp file as it arrives.
pub struct OutputWriter {
    file: fs::File,
    path: PathBuf,
}

impl OutputWriter {
    /// Create a new output writer backed by a temp file in `dir`.
    ///
    /// # Errors
    ///
    /// Returns an error if the directory cannot be created or if the output
    /// file cannot be created.
    ///
    /// # Cancel safety
    ///
    /// Not cancel-safe. If cancelled after directory creation but before
    /// file creation, the directory remains but the file is not created.
    /// The caller must handle this partial state.
    pub async fn new(dir: &Path) -> Result<Self, OutputError> {
        // WHY: Create parent dir if missing so callers don't need to pre-create.
        fs::create_dir_all(dir).await.context(CreateSnafu)?;

        let path = dir.join(format!("task-output-{}.log", koina::uuid::uuid_v4()));
        let file = fs::File::create(&path).await.context(CreateSnafu)?;

        Ok(Self { file, path })
    }

    /// Append a chunk of output.
    ///
    /// # Errors
    ///
    /// Returns an error if writing to or flushing the output file fails.
    ///
    /// # Cancel safety
    ///
    /// Not cancel-safe. If cancelled after `write_all` but before `flush`,
    /// data may be buffered but not persisted to disk. The next call will
    /// overwrite this buffered data, potentially causing data loss.
    pub async fn write_chunk(&mut self, data: &[u8]) -> Result<(), OutputError> {
        self.file.write_all(data).await.context(WriteSnafu)?;
        self.file.flush().await.context(FlushSnafu)?;
        Ok(())
    }

    /// The path to the backing file.
    pub fn path(&self) -> &Path {
        &self.path
    }
}

/// Streaming reader over a task's disk-backed output.
///
/// Implements `AsyncRead` so callers can page through output without loading
/// the entire file into memory.
pub struct OutputReader {
    file: fs::File,
}

impl OutputReader {
    /// Open the output file at `path` for streaming reads.
    ///
    /// # Errors
    ///
    /// Returns an error if the output file cannot be opened for reading.
    ///
    /// # Cancel safety
    ///
    /// Cancel-safe. File opening is atomic; no partial state on cancellation.
    pub async fn open(path: &Path) -> Result<Self, OutputError> {
        let file = fs::File::open(path).await.context(OpenSnafu)?;
        Ok(Self { file })
    }
}

impl AsyncRead for OutputReader {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        Pin::new(&mut self.file).poll_read(cx, buf)
    }
}

/// Remove a task's output file from disk.
///
/// WHY: Called during GC eviction to reclaim disk space from stale tasks.
pub(crate) async fn remove_output_file(path: &Path) -> Result<(), OutputError> {
    fs::remove_file(path).await.context(RemoveSnafu {
        path: path.to_path_buf(),
    })?;
    Ok(())
}

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test assertions")]
mod tests {
    use tokio::io::AsyncReadExt as _;

    use super::*;

    #[tokio::test]
    async fn write_and_read_output() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut writer = OutputWriter::new(dir.path()).await.expect("create writer");

        writer.write_chunk(b"hello ").await.expect("write 1");
        writer.write_chunk(b"world").await.expect("write 2");

        let path = writer.path().to_path_buf();
        let mut reader = OutputReader::open(&path).await.expect("open reader");
        let mut contents = String::new();
        reader
            .read_to_string(&mut contents)
            .await
            .expect("read output");

        assert_eq!(contents, "hello world");
    }

    #[tokio::test]
    async fn remove_output_cleans_up() {
        let dir = tempfile::tempdir().expect("tempdir");
        let writer = OutputWriter::new(dir.path()).await.expect("create writer");
        let path = writer.path().to_path_buf();

        assert!(path.exists());
        remove_output_file(&path).await.expect("remove");
        assert!(!path.exists());
    }

    #[tokio::test]
    async fn open_missing_file_returns_error() {
        let path = PathBuf::from("/tmp/aletheia-nonexistent-output.log");
        let result = OutputReader::open(&path).await;
        assert!(result.is_err());
    }
}

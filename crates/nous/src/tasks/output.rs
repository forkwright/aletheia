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
pub enum OutputError {
    /// Failed to create the output temp file.
    #[snafu(display("failed to CREATE output file: {source}"))]
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
    pub async fn new(dir: &Path) -> Result<Self, OutputError> {
        // WHY: Create parent dir if missing so callers don't need to pre-create.
        fs::create_dir_all(dir).await.context(CreateSnafu)?;

        let path = dir.join(format!("task-output-{}.log", uuid::Uuid::new_v4()));
        let file = fs::File::create(&path).await.context(CreateSnafu)?;

        Ok(Self { file, path })
    }

    /// Append a chunk of output.
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
mod tests {
    use tokio::io::AsyncReadExt as _;

    use super::*;

    #[tokio::test]
    async fn write_and_read_output() {
        let dir = tempfile::tempdir().unwrap_or_default();
        let mut writer = OutputWriter::new(dir.path()).await.unwrap_or_default();

        writer.write_chunk(b"hello ").await.unwrap_or_default();
        writer.write_chunk(b"world").await.unwrap_or_default();

        let path = writer.path().to_path_buf();
        let mut reader = OutputReader::open(&path).await.unwrap_or_default();
        let mut contents = String::new();
        reader
            .read_to_string(&mut contents)
            .await
            .unwrap_or_default();

        assert_eq!(contents, "hello world");
    }

    #[tokio::test]
    async fn remove_output_cleans_up() {
        let dir = tempfile::tempdir().unwrap_or_default();
        let writer = OutputWriter::new(dir.path()).await.unwrap_or_default();
        let path = writer.path().to_path_buf();

        assert!(path.exists());
        remove_output_file(&path).await.unwrap_or_default();
        assert!(!path.exists());
    }

    #[tokio::test]
    async fn open_missing_file_returns_error() {
        let path = PathBuf::from("/tmp/aletheia-nonexistent-output.log");
        let result = OutputReader::open(&path).await;
        assert!(result.is_err());
    }
}

//! Training data capture: append-only JSONL writer for conversation turns.
//!
//! Captures successful conversation turns as structured records for future
//! model fine-tuning. Each record contains the user message, assistant
//! response, model identifier, token usage, and timing metadata.
//!
//! Records are written one-per-line in JSON Lines format, matching the
//! structure used by `workflow/training/` in the kanon control plane.
//!
//! # Quality gate
//!
//! Only turns where the assistant produced a non-empty response with no
//! errors are captured. Empty responses and error recoveries are excluded
//! to keep the training corpus clean.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

// Re-export types from eidos for convenience
pub use eidos::training::{TrainingConfig, TrainingRecord};
use jiff::Timestamp;
use snafu::{ResultExt, Snafu};
use tracing::{debug, warn};

/// Errors from training data capture operations.
#[derive(Debug, Snafu)]
#[snafu(visibility(pub(crate)))]
#[non_exhaustive]
#[expect(
    missing_docs,
    reason = "snafu error variant fields (source, path) are self-documenting via display format"
)]
pub enum TrainingCaptureError {
    /// Failed to create the training data directory.
    #[snafu(display("failed to create training directory {}: {source}", path.display()))]
    CreateDir {
        path: PathBuf,
        source: std::io::Error,
    },

    /// Failed to open the JSONL output file for appending.
    #[snafu(display("failed to open training file {}: {source}", path.display()))]
    OpenFile {
        path: PathBuf,
        source: std::io::Error,
    },

    /// Failed to serialize a training record to JSON.
    #[snafu(display("failed to serialize training record: {source}"))]
    Serialize { source: serde_json::Error },

    /// Failed to write a training record to the JSONL file.
    #[snafu(display("failed to write training record to {}: {source}", path.display()))]
    WriteRecord {
        path: PathBuf,
        source: std::io::Error,
    },
}

/// Result alias for training capture operations.
pub type Result<T> = std::result::Result<T, TrainingCaptureError>;

/// Borrowed inputs to [`TrainingCapture::maybe_capture`].
///
/// Bundles the seven per-turn fields into a single record so the call sites
/// remain self-documenting and so the function signature stays under the
/// workspace's `too_many_arguments` threshold.
#[derive(Debug, Clone, Copy)]
pub struct CaptureInput<'a> {
    /// Session identifier the turn belongs to.
    pub session_id: &'a str,
    /// Nous identifier (agent name) handling the turn.
    pub nous_id: &'a str,
    /// Raw user message that started the turn.
    pub user_message: &'a str,
    /// Final assistant response produced by the model.
    pub assistant_response: &'a str,
    /// Model identifier used for this turn (e.g. `claude-sonnet-4-20250514`).
    pub model: &'a str,
    /// Total tokens consumed by the turn (prompt + completion).
    pub tokens: u64,
    /// Stop reason reported by the provider.
    pub stop_reason: &'a str,
}

/// Append-only training data writer.
///
/// Writes [`TrainingRecord`]s as JSON Lines to a file on disk. The writer
/// ensures the output directory exists on construction and opens the file
/// in append mode for each write to avoid holding file handles across turns.
pub struct TrainingCapture {
    /// Full path to the JSONL output file.
    file_path: PathBuf,
}

impl TrainingCapture {
    /// Create a new training capture writer.
    ///
    /// `instance_root` is the base directory of the aletheia instance
    /// (typically the working directory). The output file is placed at
    /// `{instance_root}/{config.path}/conversations.jsonl`.
    ///
    /// Creates the output directory if it does not exist.
    ///
    /// # Errors
    ///
    /// Returns [`TrainingCaptureError::CreateDir`] if the directory cannot
    /// be created.
    pub fn new(instance_root: &Path, config: &TrainingConfig) -> Result<Self> {
        let dir = instance_root.join(&config.path);
        fs::create_dir_all(&dir).context(CreateDirSnafu { path: &dir })?;
        let file_path = dir.join("conversations.jsonl");
        debug!(path = %file_path.display(), "training capture initialized");
        Ok(Self { file_path })
    }

    /// Write a training record to the JSONL file.
    ///
    /// Opens the file in append mode, serializes the record as a single
    /// JSON line, and flushes. Each call is independent: no file handle
    /// is held between writes.
    ///
    /// # Errors
    ///
    /// Returns an error if the file cannot be opened, the record cannot
    /// be serialized, or the write fails.
    pub fn write_record(&self, record: &TrainingRecord) -> Result<()> {
        let mut line = serde_json::to_string(record).context(SerializeSnafu)?;
        line.push('\n');

        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.file_path)
            .context(OpenFileSnafu {
                path: &self.file_path,
            })?;

        file.write_all(line.as_bytes())
            .context(WriteRecordSnafu {
                path: &self.file_path,
            })?;

        debug!(
            session_id = %record.session_id,
            nous_id = %record.nous_id,
            tokens = record.tokens,
            "training record captured"
        );
        Ok(())
    }

    /// Capture a conversation turn if it passes the quality gate.
    ///
    /// Quality gate criteria:
    /// - Assistant response must be non-empty
    /// - Stop reason must not indicate an error
    ///
    /// Returns `true` if the record was written, `false` if it was
    /// filtered out by the quality gate. I/O errors are logged as
    /// warnings and do not propagate: training capture must never
    /// block the pipeline.
    pub fn maybe_capture(&self, input: CaptureInput<'_>) -> bool {
        // Quality gate: reject empty or error responses
        if input.assistant_response.is_empty() {
            debug!(session_id = input.session_id, "training capture skipped: empty response");
            return false;
        }

        // WHY: error stop reasons indicate the model failed to produce a
        // usable response. Including these in training data would teach
        // the model to reproduce failure modes.
        if input.stop_reason == "error" || input.stop_reason == "max_tokens_exceeded" {
            debug!(
                session_id = input.session_id,
                stop_reason = input.stop_reason,
                "training capture skipped: error stop reason"
            );
            return false;
        }

        let record = TrainingRecord {
            session_id: input.session_id.to_owned(),
            nous_id: input.nous_id.to_owned(),
            user_message: input.user_message.to_owned(),
            assistant_response: input.assistant_response.to_owned(),
            model: input.model.to_owned(),
            tokens: input.tokens,
            timestamp: Timestamp::now(),
        };

        match self.write_record(&record) {
            Ok(()) => true,
            Err(e) => {
                // WHY: training capture is advisory. A write failure must
                // never block or fail the conversation pipeline.
                warn!(error = %e, session_id = input.session_id, "training capture write failed");
                false
            }
        }
    }

    /// Path to the JSONL output file.
    #[must_use]
    pub fn file_path(&self) -> &Path {
        &self.file_path
    }
}

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test assertions")]
#[expect(clippy::indexing_slicing, reason = "test assertions on a known-length collection")]
mod tests {
    use super::*;

    #[test]
    fn training_config_defaults() {
        let config = TrainingConfig::default();
        assert!(!config.enabled);
        assert_eq!(config.path, "data/training");
    }

    #[test]
    fn training_capture_writes_jsonl() {
        let dir = tempfile::tempdir().expect("tempdir");
        let config = TrainingConfig {
            enabled: true,
            path: "training".to_owned(),
        };
        let capture = TrainingCapture::new(dir.path(), &config).expect("new");

        let record = TrainingRecord {
            session_id: "ses-1".to_owned(),
            nous_id: "syn".to_owned(),
            user_message: "Hello".to_owned(),
            assistant_response: "Hi there!".to_owned(),
            model: "claude-opus-4-20250514".to_owned(),
            tokens: 150,
            timestamp: Timestamp::UNIX_EPOCH,
        };
        capture.write_record(&record).expect("write");

        let content = std::fs::read_to_string(capture.file_path()).expect("read");
        let lines: Vec<&str> = content.lines().collect();
        assert_eq!(lines.len(), 1);

        let parsed: TrainingRecord = serde_json::from_str(lines[0]).expect("parse");
        assert_eq!(parsed.session_id, "ses-1");
        assert_eq!(parsed.nous_id, "syn");
        assert_eq!(parsed.user_message, "Hello");
        assert_eq!(parsed.assistant_response, "Hi there!");
        assert_eq!(parsed.tokens, 150);
    }

    #[test]
    fn training_capture_appends() {
        let dir = tempfile::tempdir().expect("tempdir");
        let config = TrainingConfig {
            enabled: true,
            path: "training".to_owned(),
        };
        let capture = TrainingCapture::new(dir.path(), &config).expect("new");

        for i in 0..3 {
            let record = TrainingRecord {
                session_id: format!("ses-{i}"),
                nous_id: "syn".to_owned(),
                user_message: format!("msg-{i}"),
                assistant_response: format!("resp-{i}"),
                model: "test-model".to_owned(),
                tokens: 100,
                timestamp: Timestamp::UNIX_EPOCH,
            };
            capture.write_record(&record).expect("write");
        }

        let content = std::fs::read_to_string(capture.file_path()).expect("read");
        let lines: Vec<&str> = content.lines().collect();
        assert_eq!(lines.len(), 3);
    }

    #[test]
    fn quality_gate_rejects_empty_response() {
        let dir = tempfile::tempdir().expect("tempdir");
        let config = TrainingConfig {
            enabled: true,
            path: "training".to_owned(),
        };
        let capture = TrainingCapture::new(dir.path(), &config).expect("new");

        let captured = capture.maybe_capture(CaptureInput {
            session_id: "ses-1",
            nous_id: "syn",
            user_message: "Hi",
            assistant_response: "",
            model: "end_turn",
            tokens: 10,
            stop_reason: "end_turn",
        });
        assert!(!captured);

        // File should not exist or be empty
        let exists = capture.file_path().exists();
        assert!(!exists);
    }

    #[test]
    fn quality_gate_rejects_error_stop_reason() {
        let dir = tempfile::tempdir().expect("tempdir");
        let config = TrainingConfig {
            enabled: true,
            path: "training".to_owned(),
        };
        let capture = TrainingCapture::new(dir.path(), &config).expect("new");

        let captured = capture.maybe_capture(CaptureInput {
            session_id: "ses-1",
            nous_id: "syn",
            user_message: "Hi",
            assistant_response: "Response",
            model: "test",
            tokens: 50,
            stop_reason: "error",
        });
        assert!(!captured);
    }

    #[test]
    fn quality_gate_accepts_good_response() {
        let dir = tempfile::tempdir().expect("tempdir");
        let config = TrainingConfig {
            enabled: true,
            path: "training".to_owned(),
        };
        let capture = TrainingCapture::new(dir.path(), &config).expect("new");

        let captured = capture.maybe_capture(CaptureInput {
            session_id: "ses-1",
            nous_id: "syn",
            user_message: "Hello",
            assistant_response: "Hi there!",
            model: "test-model",
            tokens: 150,
            stop_reason: "end_turn",
        });
        assert!(captured);

        let content = std::fs::read_to_string(capture.file_path()).expect("read");
        assert_eq!(content.lines().count(), 1);
    }

    #[test]
    fn training_record_serde_roundtrip() {
        let record = TrainingRecord {
            session_id: "ses-1".to_owned(),
            nous_id: "syn".to_owned(),
            user_message: "test input".to_owned(),
            assistant_response: "test output".to_owned(),
            model: "claude-opus-4-20250514".to_owned(),
            tokens: 200,
            timestamp: Timestamp::UNIX_EPOCH,
        };

        let json = serde_json::to_string(&record).expect("serialize");
        let back: TrainingRecord = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back.session_id, record.session_id);
        assert_eq!(back.tokens, record.tokens);
    }
}

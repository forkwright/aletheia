//! Training data capture: sharded JSONL writer with manifest for conversation turns.
//!
//! Captures successful conversation turns as structured records for future
//! model fine-tuning. Each record contains the user message, assistant
//! response, model identifier, token usage, and timing metadata.
//!
//! Records are written one-per-line in JSON Lines format, matching the
//! structure used by `workflow/training/` in the kanon control plane.
//!
//! # Shard rotation
//!
//! When the active shard file exceeds `TrainingConfig::max_shard_bytes`,
//! the writer closes it and opens a new shard. Shard files are named
//! `training-YYYYMMDD-NNNN.jsonl` (date + zero-padded sequence number).
//! A `manifest.json` tracks all shards, record counts, and schema versions.
//!
//! # Backward compatibility
//!
//! If no `manifest.json` exists but a legacy `conversations.jsonl` file is
//! present, the writer treats it as the sole shard and creates a manifest
//! from it on first write.
//!
//! # Quality gate
//!
//! Only turns where the assistant produced substantive text content with a
//! clean stop reason are captured. The gate rejects:
//! - Empty or whitespace-only responses
//! - Tool-use-only turns (tool calls present but no text content)
//! - Error, degraded, or max-tokens stop reasons
//!
//! This keeps the training corpus clean of failure modes and non-content
//! turns that would teach the model to reproduce degenerate outputs.

use std::fs::{self, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

// Re-export types from eidos for convenience
pub use eidos::training::{TrainingConfig, TrainingRecord, TRAINING_RECORD_SCHEMA_VERSION};
use jiff::Timestamp;
use serde::{Deserialize, Serialize};
use snafu::{ResultExt, Snafu};
use tracing::{debug, info, warn};

// ── Errors ───────────────────────────────────────────────────────────────

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

    /// Failed to read or parse the manifest file.
    #[snafu(display("failed to read manifest {}: {source}", path.display()))]
    ReadManifest {
        path: PathBuf,
        source: std::io::Error,
    },

    /// Failed to deserialize the manifest file.
    #[snafu(display("failed to parse manifest {}: {source}", path.display()))]
    ParseManifest {
        path: PathBuf,
        source: serde_json::Error,
    },

    /// Failed to write the manifest file.
    #[snafu(display("failed to write manifest {}: {source}", path.display()))]
    WriteManifest {
        path: PathBuf,
        source: std::io::Error,
    },
}

/// Result alias for training capture operations.
pub type Result<T> = std::result::Result<T, TrainingCaptureError>;

// ── Manifest ─────────────────────────────────────────────────────────────

/// Current manifest schema version.
const MANIFEST_VERSION: u32 = 1;

/// Manifest tracking all training data shards.
///
/// Written to `manifest.json` in the training data directory. Updated
/// atomically (write-to-temp then rename) after each shard rotation and
/// periodically after writes to keep record counts current.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrainingManifest {
    /// Schema version for forward compatibility.
    pub version: u32,
    /// Ordered list of shard files (oldest first).
    pub shards: Vec<ShardEntry>,
    /// Total record count across all shards.
    pub total_records: u64,
    /// Minimum schema version seen across all records.
    pub schema_version_min: u32,
    /// Maximum schema version seen across all records.
    pub schema_version_max: u32,
    /// When the manifest was first created.
    pub created_at: Timestamp,
    /// When the manifest was last modified.
    pub modified_at: Timestamp,
}

/// Metadata for a single training data shard file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShardEntry {
    /// Filename of the shard (relative to the training directory).
    pub filename: String,
    /// Number of records in this shard.
    pub record_count: u64,
    /// File size in bytes (at last manifest update).
    pub size_bytes: u64,
    /// When this shard was created.
    pub created_at: Timestamp,
    /// When this shard was last written to.
    pub modified_at: Timestamp,
}

impl TrainingManifest {
    /// Create a new empty manifest.
    fn new() -> Self {
        let now = Timestamp::now();
        Self {
            version: MANIFEST_VERSION,
            shards: Vec::new(),
            total_records: 0,
            schema_version_min: TRAINING_RECORD_SCHEMA_VERSION,
            schema_version_max: TRAINING_RECORD_SCHEMA_VERSION,
            created_at: now,
            modified_at: now,
        }
    }

    /// Recompute `total_records` from shard entries.
    ///
    /// Used during manifest recovery when record counts may have drifted
    /// from the actual shard contents.
    pub fn recompute_totals(&mut self) {
        self.total_records = self.shards.iter().map(|s| s.record_count).sum();
    }
}

// ── Quality gate types ───────────────────────────────────────────────────

/// Stop reason classification for the training capture quality gate.
///
/// WHY: the provider-level `StopReason` lives in hermeneus which is a higher
/// layer than mneme. Rather than adding an upward dependency, this enum
/// captures just what the quality gate needs. Callers parse the string stop
/// reason into this enum at the call site.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaptureStopReason {
    /// Normal end of turn — safe to capture.
    EndTurn,
    /// Model requested tool use — may or may not have text content.
    ToolUse,
    /// Hit max tokens limit — response is likely truncated.
    MaxTokens,
    /// Hit a stop sequence — safe to capture.
    StopSequence,
    /// Degraded mode — LLM was unavailable, response is synthetic.
    Degraded,
    /// Any unrecognized stop reason.
    Unknown,
}

impl CaptureStopReason {
    /// Parse a wire-format stop reason string into the enum.
    ///
    /// Unrecognized values map to [`CaptureStopReason::Unknown`] rather than
    /// failing, since new provider stop reasons shouldn't crash capture.
    ///
    /// WHY `parse` not `from_str`: this is infallible (unknown maps to a
    /// variant, not an error), so it doesn't match the `FromStr` trait's
    /// fallible signature.
    #[must_use]
    pub fn parse(s: &str) -> Self {
        match s {
            "end_turn" => Self::EndTurn,
            "tool_use" => Self::ToolUse,
            "max_tokens" => Self::MaxTokens,
            "stop_sequence" => Self::StopSequence,
            "degraded" => Self::Degraded,
            _ => Self::Unknown,
        }
    }

    /// Whether this stop reason indicates the response should be excluded
    /// from training data.
    fn is_rejected(self) -> bool {
        matches!(
            self,
            Self::MaxTokens | Self::Degraded | Self::Unknown
        )
    }
}

/// Borrowed inputs to [`TrainingCapture::maybe_capture`].
///
/// Bundles the per-turn fields into a single record so the call sites
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
    pub stop_reason: CaptureStopReason,
    /// Whether the turn included any tool calls.
    ///
    /// WHY: tool-use-only turns (tool calls present but no text content)
    /// are not useful training data — they teach the model to produce
    /// empty text responses.
    pub has_tool_calls: bool,
}

// ── Shard naming ─────────────────────────────────────────────────────────

/// Legacy single-file name used before sharding was introduced.
const LEGACY_FILENAME: &str = "conversations.jsonl";

/// Manifest filename.
const MANIFEST_FILENAME: &str = "manifest.json";

/// Generate a shard filename for the given date and sequence number.
///
/// Format: `training-YYYYMMDD-NNNN.jsonl`
fn shard_filename(date: &str, seq: u16) -> String {
    format!("training-{date}-{seq:04}.jsonl")
}

/// Extract the date prefix from a shard filename, or `None` if the name
/// doesn't match the expected pattern.
fn parse_shard_date(filename: &str) -> Option<&str> {
    let rest = filename.strip_prefix("training-")?;
    let rest = rest.strip_suffix(".jsonl")?;
    // rest should be "YYYYMMDD-NNNN" (13 ASCII bytes, separator at index 8)
    if rest.len() == 13 && rest.as_bytes().get(8) == Some(&b'-') {
        // SAFETY: indices 0..8 are within an ASCII-only region (digits) so
        // this cannot split a multi-byte character.
        Some(rest.get(..8)?)
    } else {
        None
    }
}

/// Extract the sequence number from a shard filename.
fn parse_shard_seq(filename: &str) -> Option<u16> {
    let rest = filename.strip_prefix("training-")?;
    let rest = rest.strip_suffix(".jsonl")?;
    if rest.len() == 13 {
        // SAFETY: indices 9.. are within an ASCII-only region (digits).
        rest.get(9..)?.parse().ok()
    } else {
        None
    }
}

// ── Writer ───────────────────────────────────────────────────────────────

/// Training data writer with size-based shard rotation and manifest tracking.
///
/// On construction, loads or creates a manifest. Each [`write_record`] call
/// appends to the active shard. When the shard exceeds the configured
/// `max_shard_bytes`, the writer rotates to a new shard and updates the
/// manifest atomically.
///
/// [`write_record`]: TrainingCapture::write_record
pub struct TrainingCapture {
    /// Training data directory.
    dir: PathBuf,
    /// Maximum shard size before rotation.
    max_shard_bytes: u64,
    /// In-memory manifest state.
    manifest: TrainingManifest,
}

impl TrainingCapture {
    /// Create a new training capture writer.
    ///
    /// `instance_root` is the base directory of the aletheia instance
    /// (typically the working directory). The training directory is
    /// `{instance_root}/{config.path}`.
    ///
    /// On first use, if no manifest exists:
    /// - If a legacy `conversations.jsonl` is present, it becomes the first
    ///   shard and a manifest is created from it.
    /// - Otherwise a fresh manifest and first shard are created.
    ///
    /// Creates the output directory if it does not exist.
    ///
    /// # Errors
    ///
    /// Returns [`TrainingCaptureError::CreateDir`] if the directory cannot
    /// be created. Returns manifest-related errors if an existing manifest
    /// is unreadable.
    pub fn new(instance_root: &Path, config: &TrainingConfig) -> Result<Self> {
        let dir = instance_root.join(&config.path);
        fs::create_dir_all(&dir).context(CreateDirSnafu { path: &dir })?;

        let manifest_path = dir.join(MANIFEST_FILENAME);
        let manifest = if manifest_path.exists() {
            load_manifest(&manifest_path)?
        } else {
            bootstrap_manifest(&dir)?
        };

        debug!(
            path = %dir.display(),
            shards = manifest.shards.len(),
            total_records = manifest.total_records,
            "training capture initialized"
        );

        Ok(Self {
            dir,
            max_shard_bytes: config.max_shard_bytes,
            manifest,
        })
    }

    /// Write a training record to the active shard, rotating if needed.
    ///
    /// Opens the active shard in append mode, serializes the record as a
    /// single JSON line, and flushes. If the shard exceeds the size
    /// threshold after the write, a new shard is started and the manifest
    /// is persisted.
    ///
    /// # Errors
    ///
    /// Returns an error if the file cannot be opened, the record cannot
    /// be serialized, the write fails, or the manifest cannot be updated.
    pub fn write_record(&mut self, record: &TrainingRecord) -> Result<()> {
        // Ensure we have an active shard.
        if self.manifest.shards.is_empty() {
            self.create_new_shard()?;
        }

        // WHY: `create_new_shard` pushes a shard, so `last_mut` cannot be
        // `None` here. If it somehow were, treat it as an I/O error rather
        // than panicking in library code.
        let Some(active) = self.manifest.shards.last_mut() else {
            return Err(TrainingCaptureError::WriteRecord {
                path: self.dir.clone(),
                source: std::io::Error::other("no active shard after creation"),
            });
        };

        let shard_path = self.dir.join(&active.filename);
        let mut line = serde_json::to_string(record).context(SerializeSnafu)?;
        line.push('\n');
        let line_bytes = u64::try_from(line.len()).unwrap_or(u64::MAX);

        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&shard_path)
            .context(OpenFileSnafu { path: &shard_path })?;

        file.write_all(line.as_bytes())
            .context(WriteRecordSnafu { path: &shard_path })?;

        // Update shard metadata.
        active.record_count += 1;
        active.size_bytes += line_bytes;
        active.modified_at = Timestamp::now();

        // Update manifest-level schema version range.
        if record.schema_version < self.manifest.schema_version_min {
            self.manifest.schema_version_min = record.schema_version;
        }
        if record.schema_version > self.manifest.schema_version_max {
            self.manifest.schema_version_max = record.schema_version;
        }
        self.manifest.total_records += 1;
        self.manifest.modified_at = active.modified_at;

        debug!(
            session_id = %record.session_id,
            nous_id = %record.nous_id,
            tokens = record.tokens,
            shard = %active.filename,
            "training record captured"
        );

        // Rotate if the shard exceeds the size threshold.
        if active.size_bytes >= self.max_shard_bytes {
            self.rotate()?;
        }

        // Persist manifest after every write so record counts stay current.
        // WHY: the cost of a small JSON write is negligible compared to the
        // LLM turn that produced this record, and it means the manifest is
        // always recoverable even if the process crashes.
        self.persist_manifest()?;

        Ok(())
    }

    /// Capture a conversation turn if it passes the quality gate.
    ///
    /// Quality gate criteria:
    /// - Assistant response must contain non-whitespace text
    /// - Stop reason must not indicate an error or degraded mode
    /// - Turn must not be tool-use-only (tool calls with no text content)
    ///
    /// Returns `true` if the record was written, `false` if it was
    /// filtered out by the quality gate. I/O errors are logged as
    /// warnings and do not propagate: training capture must never
    /// block the pipeline.
    pub fn maybe_capture(&mut self, input: CaptureInput<'_>) -> bool {
        // WHY: empty and whitespace-only responses teach the model to produce
        // vacuous output. `.trim().is_empty()` catches both `""` and `"  \n"`.
        if input.assistant_response.trim().is_empty() {
            debug!(session_id = input.session_id, "training capture skipped: empty/whitespace response");
            return false;
        }

        // WHY: rejected stop reasons indicate the model failed to produce a
        // usable response (max_tokens = truncated, degraded = synthetic,
        // unknown = unrecognized provider state). Including these would teach
        // the model to reproduce failure modes.
        if input.stop_reason.is_rejected() {
            debug!(
                session_id = input.session_id,
                stop_reason = ?input.stop_reason,
                "training capture skipped: rejected stop reason"
            );
            return false;
        }

        // WHY: tool-use-only turns (tool calls present but the "response" is
        // just tool invocation scaffolding) don't represent useful assistant
        // behavior for text generation training. The text content in these
        // turns is typically empty or trivial preamble.
        if input.has_tool_calls && input.stop_reason == CaptureStopReason::ToolUse {
            debug!(
                session_id = input.session_id,
                "training capture skipped: tool-use-only turn"
            );
            return false;
        }

        let record = TrainingRecord {
            schema_version: TRAINING_RECORD_SCHEMA_VERSION,
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

    /// Path to the training data directory.
    #[must_use]
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Path to the active shard file, or `None` if no shards exist yet.
    #[must_use]
    pub fn file_path(&self) -> Option<PathBuf> {
        self.manifest
            .shards
            .last()
            .map(|s| self.dir.join(&s.filename))
    }

    /// Reference to the in-memory manifest.
    #[must_use]
    pub fn manifest(&self) -> &TrainingManifest {
        &self.manifest
    }

    // ── Internal helpers ─────────────────────────────────────────────────

    /// Create a new shard file and add it to the manifest.
    fn create_new_shard(&mut self) -> Result<()> {
        let today = jiff::Zoned::now().strftime("%Y%m%d").to_string();
        let seq = self.next_seq_for_date(&today);
        let filename = shard_filename(&today, seq);
        let shard_path = self.dir.join(&filename);

        // Touch the file to create it.
        OpenOptions::new()
            .create(true)
            .append(true)
            .open(&shard_path)
            .context(OpenFileSnafu { path: &shard_path })?;

        let now = Timestamp::now();
        self.manifest.shards.push(ShardEntry {
            filename: filename.clone(),
            record_count: 0,
            size_bytes: 0,
            created_at: now,
            modified_at: now,
        });

        info!(shard = %filename, "created new training shard");
        Ok(())
    }

    /// Rotate: close the current shard and start a new one.
    fn rotate(&mut self) -> Result<()> {
        if let Some(active) = self.manifest.shards.last() {
            info!(
                shard = %active.filename,
                records = active.record_count,
                size_bytes = active.size_bytes,
                "rotating training shard"
            );
        }
        self.create_new_shard()?;
        Ok(())
    }

    /// Determine the next sequence number for a given date.
    fn next_seq_for_date(&self, date: &str) -> u16 {
        let max_existing = self
            .manifest
            .shards
            .iter()
            .filter_map(|s| {
                let d = parse_shard_date(&s.filename)?;
                if d == date {
                    parse_shard_seq(&s.filename)
                } else {
                    None
                }
            })
            .max()
            .unwrap_or(0);

        // If no existing shards for this date, start at 0. Otherwise increment.
        if self
            .manifest
            .shards
            .iter()
            .any(|s| parse_shard_date(&s.filename) == Some(date))
        {
            max_existing.saturating_add(1)
        } else {
            0
        }
    }

    /// Persist the manifest to disk atomically (write tmp then rename).
    fn persist_manifest(&self) -> Result<()> {
        let manifest_path = self.dir.join(MANIFEST_FILENAME);
        let tmp_path = self.dir.join(".manifest.json.tmp");

        let json =
            serde_json::to_string_pretty(&self.manifest).context(SerializeSnafu)?;

        fs::write(&tmp_path, json.as_bytes()).context(WriteManifestSnafu {
            path: &manifest_path,
        })?;

        fs::rename(&tmp_path, &manifest_path).context(WriteManifestSnafu {
            path: &manifest_path,
        })?;

        Ok(())
    }
}

// ── Free functions ───────────────────────────────────────────────────────

/// Load an existing manifest from disk.
fn load_manifest(path: &Path) -> Result<TrainingManifest> {
    let content = fs::read_to_string(path).context(ReadManifestSnafu { path })?;
    let manifest: TrainingManifest =
        serde_json::from_str(&content).context(ParseManifestSnafu { path })?;
    Ok(manifest)
}

/// Bootstrap a manifest from existing files in the training directory.
///
/// If a legacy `conversations.jsonl` exists, it becomes the first shard.
/// Otherwise, an empty manifest is created.
fn bootstrap_manifest(dir: &Path) -> Result<TrainingManifest> {
    let legacy_path = dir.join(LEGACY_FILENAME);
    let mut manifest = TrainingManifest::new();

    if legacy_path.exists() {
        // Count records and determine schema version range from the legacy file.
        let (record_count, min_ver, max_ver) = scan_jsonl_stats(&legacy_path)?;
        let size_bytes = fs::metadata(&legacy_path).map_or(0, |m| m.len());

        let now = Timestamp::now();
        manifest.shards.push(ShardEntry {
            filename: LEGACY_FILENAME.to_owned(),
            record_count,
            size_bytes,
            created_at: now,
            modified_at: now,
        });
        manifest.total_records = record_count;
        if record_count > 0 {
            manifest.schema_version_min = min_ver;
            manifest.schema_version_max = max_ver;
        }

        info!(
            records = record_count,
            size_bytes,
            "bootstrapped manifest from legacy conversations.jsonl"
        );
    }

    Ok(manifest)
}

/// Scan a JSONL file to count records and find schema version range.
///
/// Reads each line and attempts to extract `schema_version`. Lines that
/// fail to parse are silently skipped (the corpus may contain partial
/// writes from crashes).
fn scan_jsonl_stats(path: &Path) -> Result<(u64, u32, u32)> {
    /// Minimal struct to peek at `schema_version` without full deserialization.
    #[derive(Deserialize)]
    struct VersionPeek {
        #[serde(default)]
        schema_version: u32,
    }

    let file = fs::File::open(path).context(OpenFileSnafu { path })?;
    let reader = BufReader::new(file);

    let mut count: u64 = 0;
    let mut min_ver = u32::MAX;
    let mut max_ver = 0;

    for line in reader.lines() {
        let Ok(line) = line else { continue };
        if line.trim().is_empty() {
            continue;
        }

        count += 1;

        if let Ok(peek) = serde_json::from_str::<VersionPeek>(&line) {
            if peek.schema_version < min_ver {
                min_ver = peek.schema_version;
            }
            if peek.schema_version > max_ver {
                max_ver = peek.schema_version;
            }
        }
    }

    // If no records found, set sane defaults.
    if count == 0 {
        min_ver = TRAINING_RECORD_SCHEMA_VERSION;
        max_ver = TRAINING_RECORD_SCHEMA_VERSION;
    }

    Ok((count, min_ver, max_ver))
}

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test assertions")]
#[expect(clippy::indexing_slicing, reason = "test assertions on a known-length collection")]
mod tests {
    use super::*;

    /// Helper: build a default `CaptureInput` for a normal successful turn.
    /// Tests override individual fields to exercise specific gate conditions.
    fn good_input() -> CaptureInput<'static> {
        CaptureInput {
            session_id: "ses-1",
            nous_id: "syn",
            user_message: "Hello",
            assistant_response: "Hi there!",
            model: "test-model",
            tokens: 150,
            stop_reason: CaptureStopReason::EndTurn,
            has_tool_calls: false,
        }
    }

    fn test_config() -> TrainingConfig {
        TrainingConfig {
            enabled: true,
            path: "training".to_owned(),
            max_shard_bytes: 50 * 1024 * 1024,
        }
    }

    #[test]
    fn training_config_defaults() {
        let config = TrainingConfig::default();
        assert!(!config.enabled);
        assert_eq!(config.path, "data/training");
        assert_eq!(config.max_shard_bytes, 50 * 1024 * 1024);
    }

    #[test]
    fn training_capture_writes_to_shard() {
        let dir = tempfile::tempdir().expect("tempdir");
        let config = test_config();
        let mut capture = TrainingCapture::new(dir.path(), &config).expect("new");

        let record = TrainingRecord {
            schema_version: TRAINING_RECORD_SCHEMA_VERSION,
            session_id: "ses-1".to_owned(),
            nous_id: "syn".to_owned(),
            user_message: "Hello".to_owned(),
            assistant_response: "Hi there!".to_owned(),
            model: "claude-opus-4-20250514".to_owned(),
            tokens: 150,
            timestamp: Timestamp::UNIX_EPOCH,
        };
        capture.write_record(&record).expect("write");

        // Should have created a shard file and manifest.
        let manifest = capture.manifest();
        assert_eq!(manifest.shards.len(), 1);
        assert_eq!(manifest.total_records, 1);
        assert!(manifest.shards[0].filename.starts_with("training-"));
        assert!(manifest.shards[0].filename.ends_with(".jsonl"));

        // Read back the shard content.
        let shard_path = capture.file_path().expect("has active shard");
        let content = std::fs::read_to_string(&shard_path).expect("read");
        let lines: Vec<&str> = content.lines().collect();
        assert_eq!(lines.len(), 1);

        let parsed: TrainingRecord = serde_json::from_str(lines[0]).expect("parse");
        assert_eq!(parsed.schema_version, TRAINING_RECORD_SCHEMA_VERSION);
        assert_eq!(parsed.session_id, "ses-1");

        // Manifest file should exist on disk.
        let manifest_path = dir.path().join("training").join(MANIFEST_FILENAME);
        assert!(manifest_path.exists());
    }

    #[test]
    fn training_capture_appends_to_shard() {
        let dir = tempfile::tempdir().expect("tempdir");
        let config = test_config();
        let mut capture = TrainingCapture::new(dir.path(), &config).expect("new");

        for i in 0..3 {
            let record = TrainingRecord {
                schema_version: TRAINING_RECORD_SCHEMA_VERSION,
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

        let shard_path = capture.file_path().expect("has active shard");
        let content = std::fs::read_to_string(&shard_path).expect("read");
        let lines: Vec<&str> = content.lines().collect();
        assert_eq!(lines.len(), 3);

        assert_eq!(capture.manifest().total_records, 3);
        assert_eq!(capture.manifest().shards.len(), 1);
        assert_eq!(capture.manifest().shards[0].record_count, 3);
    }

    #[test]
    fn rotation_on_size_threshold() {
        let dir = tempfile::tempdir().expect("tempdir");
        // Set a tiny threshold so rotation triggers quickly.
        let config = TrainingConfig {
            enabled: true,
            path: "training".to_owned(),
            max_shard_bytes: 100, // 100 bytes
        };
        let mut capture = TrainingCapture::new(dir.path(), &config).expect("new");

        // Write enough records to trigger at least one rotation.
        for i in 0..10 {
            let record = TrainingRecord {
                schema_version: TRAINING_RECORD_SCHEMA_VERSION,
                session_id: format!("ses-{i}"),
                nous_id: "syn".to_owned(),
                user_message: format!("message number {i}"),
                assistant_response: format!("response number {i}"),
                model: "test-model".to_owned(),
                tokens: 100,
                timestamp: Timestamp::UNIX_EPOCH,
            };
            capture.write_record(&record).expect("write");
        }

        let manifest = capture.manifest();
        assert!(
            manifest.shards.len() > 1,
            "expected multiple shards after rotation, got {}",
            manifest.shards.len()
        );
        assert_eq!(manifest.total_records, 10);

        // Verify all shard files exist and record counts add up.
        let mut total_from_shards: u64 = 0;
        for shard in &manifest.shards {
            let path = dir.path().join("training").join(&shard.filename);
            assert!(path.exists(), "shard file {} should exist", shard.filename);
            let content = std::fs::read_to_string(&path).expect("read shard");
            let line_count = content.lines().count() as u64;
            assert_eq!(
                shard.record_count, line_count,
                "shard {} record_count mismatch",
                shard.filename
            );
            total_from_shards += line_count;
        }
        assert_eq!(total_from_shards, 10);
    }

    #[test]
    fn backward_compat_legacy_file() {
        let dir = tempfile::tempdir().expect("tempdir");
        let training_dir = dir.path().join("training");
        fs::create_dir_all(&training_dir).expect("create training dir");

        // Write a legacy conversations.jsonl file.
        let legacy_path = training_dir.join(LEGACY_FILENAME);
        let record = TrainingRecord {
            schema_version: TRAINING_RECORD_SCHEMA_VERSION,
            session_id: "legacy-1".to_owned(),
            nous_id: "syn".to_owned(),
            user_message: "old message".to_owned(),
            assistant_response: "old response".to_owned(),
            model: "test-model".to_owned(),
            tokens: 50,
            timestamp: Timestamp::UNIX_EPOCH,
        };
        let line = serde_json::to_string(&record).expect("serialize");
        fs::write(&legacy_path, format!("{line}\n")).expect("write legacy");

        // Now create a TrainingCapture — should bootstrap from the legacy file.
        let config = test_config();
        let mut capture = TrainingCapture::new(dir.path(), &config).expect("new");

        let manifest = capture.manifest();
        assert_eq!(manifest.shards.len(), 1);
        assert_eq!(manifest.shards[0].filename, LEGACY_FILENAME);
        assert_eq!(manifest.shards[0].record_count, 1);
        assert_eq!(manifest.total_records, 1);

        // Write a new record — should go into the legacy shard (it's still
        // under the size threshold).
        let captured = capture.maybe_capture(good_input());
        assert!(captured);
        assert_eq!(capture.manifest().total_records, 2);
        // Should still be writing to the legacy shard.
        assert_eq!(capture.manifest().shards.len(), 1);
        assert_eq!(capture.manifest().shards[0].filename, LEGACY_FILENAME);
    }

    #[test]
    fn manifest_persisted_on_disk() {
        let dir = tempfile::tempdir().expect("tempdir");
        let config = test_config();
        let mut capture = TrainingCapture::new(dir.path(), &config).expect("new");

        capture.maybe_capture(good_input());

        // Load manifest from disk independently.
        let manifest_path = dir.path().join("training").join(MANIFEST_FILENAME);
        let content = fs::read_to_string(&manifest_path).expect("read manifest");
        let disk_manifest: TrainingManifest =
            serde_json::from_str(&content).expect("parse manifest");

        assert_eq!(disk_manifest.version, MANIFEST_VERSION);
        assert_eq!(disk_manifest.total_records, 1);
        assert_eq!(disk_manifest.shards.len(), 1);
    }

    #[test]
    fn reload_manifest_across_instances() {
        let dir = tempfile::tempdir().expect("tempdir");
        let config = test_config();

        // First instance writes 3 records.
        {
            let mut capture = TrainingCapture::new(dir.path(), &config).expect("new");
            for i in 0..3 {
                let record = TrainingRecord {
                    schema_version: TRAINING_RECORD_SCHEMA_VERSION,
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
        }

        // Second instance loads the manifest and continues.
        {
            let mut capture = TrainingCapture::new(dir.path(), &config).expect("new");
            assert_eq!(capture.manifest().total_records, 3);

            let record = TrainingRecord {
                schema_version: TRAINING_RECORD_SCHEMA_VERSION,
                session_id: "ses-3".to_owned(),
                nous_id: "syn".to_owned(),
                user_message: "msg-3".to_owned(),
                assistant_response: "resp-3".to_owned(),
                model: "test-model".to_owned(),
                tokens: 100,
                timestamp: Timestamp::UNIX_EPOCH,
            };
            capture.write_record(&record).expect("write");
            assert_eq!(capture.manifest().total_records, 4);
        }
    }

    #[test]
    fn shard_filename_format() {
        assert_eq!(shard_filename("20260414", 0), "training-20260414-0000.jsonl");
        assert_eq!(shard_filename("20260414", 1), "training-20260414-0001.jsonl");
        assert_eq!(shard_filename("20260414", 42), "training-20260414-0042.jsonl");
    }

    #[test]
    fn parse_shard_naming() {
        assert_eq!(parse_shard_date("training-20260414-0000.jsonl"), Some("20260414"));
        assert_eq!(parse_shard_seq("training-20260414-0003.jsonl"), Some(3));
        assert_eq!(parse_shard_date("conversations.jsonl"), None);
        assert_eq!(parse_shard_seq("conversations.jsonl"), None);
    }

    // ── Quality gate: empty / whitespace ──────────────────────────────────

    #[test]
    fn quality_gate_rejects_empty_response() {
        let dir = tempfile::tempdir().expect("tempdir");
        let config = test_config();
        let mut capture = TrainingCapture::new(dir.path(), &config).expect("new");

        let captured = capture.maybe_capture(CaptureInput {
            assistant_response: "",
            ..good_input()
        });
        assert!(!captured, "empty response should be rejected");
    }

    #[test]
    fn quality_gate_rejects_whitespace_only_response() {
        let dir = tempfile::tempdir().expect("tempdir");
        let config = test_config();
        let mut capture = TrainingCapture::new(dir.path(), &config).expect("new");

        for ws in ["  ", "\n", "\t\n  ", "   \n\n   "] {
            let captured = capture.maybe_capture(CaptureInput {
                assistant_response: ws,
                ..good_input()
            });
            assert!(!captured, "whitespace-only response {ws:?} should be rejected");
        }
    }

    // ── Quality gate: stop reasons ────────────────────────────────────────

    #[test]
    fn quality_gate_rejects_max_tokens_stop_reason() {
        let dir = tempfile::tempdir().expect("tempdir");
        let config = test_config();
        let mut capture = TrainingCapture::new(dir.path(), &config).expect("new");

        let captured = capture.maybe_capture(CaptureInput {
            stop_reason: CaptureStopReason::MaxTokens,
            ..good_input()
        });
        assert!(!captured, "max_tokens stop reason should be rejected");
    }

    #[test]
    fn quality_gate_rejects_degraded_stop_reason() {
        let dir = tempfile::tempdir().expect("tempdir");
        let config = test_config();
        let mut capture = TrainingCapture::new(dir.path(), &config).expect("new");

        let captured = capture.maybe_capture(CaptureInput {
            stop_reason: CaptureStopReason::Degraded,
            ..good_input()
        });
        assert!(!captured, "degraded stop reason should be rejected");
    }

    #[test]
    fn quality_gate_rejects_unknown_stop_reason() {
        let dir = tempfile::tempdir().expect("tempdir");
        let config = test_config();
        let mut capture = TrainingCapture::new(dir.path(), &config).expect("new");

        let captured = capture.maybe_capture(CaptureInput {
            stop_reason: CaptureStopReason::Unknown,
            ..good_input()
        });
        assert!(!captured, "unknown stop reason should be rejected");
    }

    // ── Quality gate: tool-use-only ───────────────────────────────────────

    #[test]
    fn quality_gate_rejects_tool_use_only_turn() {
        let dir = tempfile::tempdir().expect("tempdir");
        let config = test_config();
        let mut capture = TrainingCapture::new(dir.path(), &config).expect("new");

        let captured = capture.maybe_capture(CaptureInput {
            assistant_response: "Let me check that.",
            stop_reason: CaptureStopReason::ToolUse,
            has_tool_calls: true,
            ..good_input()
        });
        assert!(!captured, "tool-use-only turn (tool_use stop + has_tool_calls) should be rejected");
    }

    #[test]
    fn quality_gate_accepts_tool_use_with_end_turn() {
        let dir = tempfile::tempdir().expect("tempdir");
        let config = test_config();
        let mut capture = TrainingCapture::new(dir.path(), &config).expect("new");

        // Turn that used tools but ended with text (end_turn)
        let captured = capture.maybe_capture(CaptureInput {
            assistant_response: "Based on the file contents, here is the answer.",
            stop_reason: CaptureStopReason::EndTurn,
            has_tool_calls: true,
            ..good_input()
        });
        assert!(captured, "tool-using turn that ended with text should be accepted");
    }

    // ── Quality gate: happy path ──────────────────────────────────────────

    #[test]
    fn quality_gate_accepts_good_response() {
        let dir = tempfile::tempdir().expect("tempdir");
        let config = test_config();
        let mut capture = TrainingCapture::new(dir.path(), &config).expect("new");

        let captured = capture.maybe_capture(good_input());
        assert!(captured);

        let shard_path = capture.file_path().expect("has shard");
        let content = std::fs::read_to_string(shard_path).expect("read");
        assert_eq!(content.lines().count(), 1);
    }

    #[test]
    fn quality_gate_accepts_stop_sequence() {
        let dir = tempfile::tempdir().expect("tempdir");
        let config = test_config();
        let mut capture = TrainingCapture::new(dir.path(), &config).expect("new");

        let captured = capture.maybe_capture(CaptureInput {
            stop_reason: CaptureStopReason::StopSequence,
            ..good_input()
        });
        assert!(captured, "stop_sequence with content should be accepted");
    }

    // ── CaptureStopReason parsing ─────────────────────────────────────────

    #[test]
    fn capture_stop_reason_from_str() {
        assert_eq!(CaptureStopReason::parse("end_turn"), CaptureStopReason::EndTurn);
        assert_eq!(CaptureStopReason::parse("tool_use"), CaptureStopReason::ToolUse);
        assert_eq!(CaptureStopReason::parse("max_tokens"), CaptureStopReason::MaxTokens);
        assert_eq!(CaptureStopReason::parse("stop_sequence"), CaptureStopReason::StopSequence);
        assert_eq!(CaptureStopReason::parse("degraded"), CaptureStopReason::Degraded);
        assert_eq!(CaptureStopReason::parse("error"), CaptureStopReason::Unknown);
        assert_eq!(CaptureStopReason::parse("anything_else"), CaptureStopReason::Unknown);
    }

    // ── Serde roundtrip ───────────────────────────────────────────────────

    #[test]
    fn training_record_serde_roundtrip() {
        let record = TrainingRecord {
            schema_version: TRAINING_RECORD_SCHEMA_VERSION,
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
        assert_eq!(back.schema_version, TRAINING_RECORD_SCHEMA_VERSION);
        assert_eq!(back.session_id, record.session_id);
        assert_eq!(back.tokens, record.tokens);
    }

    // ── Manifest serde roundtrip ─────────────────────────────────────────

    #[test]
    fn manifest_serde_roundtrip() {
        let now = Timestamp::now();
        let manifest = TrainingManifest {
            version: MANIFEST_VERSION,
            shards: vec![ShardEntry {
                filename: "training-20260414-0000.jsonl".to_owned(),
                record_count: 42,
                size_bytes: 8192,
                created_at: now,
                modified_at: now,
            }],
            total_records: 42,
            schema_version_min: 1,
            schema_version_max: 1,
            created_at: now,
            modified_at: now,
        };

        let json = serde_json::to_string_pretty(&manifest).expect("serialize");
        let back: TrainingManifest = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back.version, MANIFEST_VERSION);
        assert_eq!(back.total_records, 42);
        assert_eq!(back.shards.len(), 1);
        assert_eq!(back.shards[0].filename, "training-20260414-0000.jsonl");
    }
}

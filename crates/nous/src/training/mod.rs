//! Training data capture: sharded JSONL writer for conversation turns.
//!
//! Captures successful conversation turns as structured records for future
//! model fine-tuning. Each record contains the user message, assistant
//! response, model identifier, token usage, timing metadata, and optional
//! episteme labels (turn classification, correction signal, fact types,
//! quality score).
//!
//! Records are written one-per-line in JSON Lines format, matching the
//! structure used by `workflow/training/` in the kanon control plane.
//!
//! # Shard rotation
//!
//! When a shard file exceeds [`TrainingConfig::max_shard_bytes`], it is
//! closed and a new shard is started with an incremented sequence number.
//! Shard naming: `training-YYYYMMDD-NNNN.jsonl`. A manifest file tracks
//! all shards, record counts, and schema version range.
//!
//! # Backward compatibility
//!
//! If a legacy `conversations.jsonl` file exists in the training directory,
//! it is treated as the first shard and incorporated into the manifest on
//! first access.
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
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use aletheia_classify::Classifier;
// Re-export types from eidos for convenience
pub use eidos::training::{
    RecallSignals, RecalledFact, TRAINING_RECORD_SCHEMA_VERSION, ToolOutcome, TrainingConfig,
    TrainingRecord,
};
use jiff::Timestamp;
use serde::{Deserialize, Serialize};
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

    /// Failed to read file metadata.
    #[snafu(display("failed to read metadata for {}: {source}", path.display()))]
    ReadMetadata {
        path: PathBuf,
        source: std::io::Error,
    },

    /// Failed to persist the training manifest.
    #[snafu(display("failed to persist training manifest to {}: {source}", path.display()))]
    PersistManifest {
        path: PathBuf,
        source: std::io::Error,
    },

    /// Failed to serialize the training manifest.
    #[snafu(display("failed to serialize training manifest: {source}"))]
    SerializeManifest { source: serde_json::Error },

    /// Failed to rename temporary manifest file.
    #[snafu(display("failed to rename {} to {}: {source}", from.display(), to.display()))]
    RenameManifest {
        from: PathBuf,
        to: PathBuf,
        source: std::io::Error,
    },
}

/// Result alias for training capture operations.
pub type Result<T> = std::result::Result<T, TrainingCaptureError>;

/// Stop reason classification for the training capture quality gate.
///
/// WHY: the provider-level `StopReason` lives in hermeneus which is a higher
/// layer than mneme. Rather than adding an upward dependency, this enum
/// captures just what the quality gate needs. Callers parse the string stop
/// reason into this enum at the call site.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
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
        matches!(self, Self::MaxTokens | Self::Degraded | Self::Unknown)
    }
}

/// Borrowed inputs to [`TrainingCapture::maybe_capture`].
///
/// Bundles the per-turn fields into a single record so the call sites
/// remain self-documenting and so the function signature stays under the
/// workspace's `too_many_arguments` threshold.
#[derive(Debug, Clone)]
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

    // ── Episteme labels ──────────────────────────────────────────────
    /// Classification of the conversation turn (e.g. "discussion", "correction").
    pub turn_type: Option<String>,
    /// Whether this turn corrects a previous response.
    pub is_correction: Option<bool>,
    /// Types of facts extracted from this turn.
    pub fact_types: Option<Vec<String>>,

    // ── Behavioural signals (v3) ──────────────────────────────────────
    /// Outcomes of tool calls made during the turn, in invocation order.
    ///
    /// `None` preserves "no tool calls were made" vs `Some(vec![])`
    /// which means "tool call outcome capture was configured but
    /// produced no entries" (should be unreachable in practice).
    pub tool_outcomes: Option<Vec<ToolOutcome>>,

    /// Recall stage signals (facts recalled, which were referenced).
    ///
    /// `None` means the recall stage was skipped or produced no result.
    pub recall_signals: Option<RecallSignals>,
}

impl CaptureInput<'_> {
    /// Compute the derived quality score for this turn.
    ///
    /// The score is a weighted combination of real signals, scaled to
    /// `[0.0, 1.0]`:
    ///
    /// | Signal | Weight | Source |
    /// |---|---|---|
    /// | Tool call success rate | 0.40 | `tool_outcomes` — all success = 1.0, all failure = 0.0 |
    /// | Recall utilization rate | 0.20 | fraction of injected recalled facts that were referenced in the output |
    /// | Response substance (length-scaled, saturating at 400 chars) | 0.20 | `assistant_response` |
    /// | Non-error stop reason | 0.10 | `stop_reason` — `EndTurn` / `StopSequence` = 1.0 |
    /// | Correction penalty | 0.10 | `is_correction = Some(true)` → 0.0 else 1.0 |
    ///
    /// WHY this mix: these are the only DPO/ORPO-relevant signals
    /// available without a judge model. Tool success is the strongest
    /// signal because failed trajectories teach the wrong behaviour.
    /// Recall utilization rewards turns that actually used injected
    /// memory. Response substance avoids over-weighting short
    /// acknowledgements. The correction penalty biases the corpus
    /// away from turns the user had to rewrite.
    ///
    /// WHY return `Option<f32>`: when a turn lacks *any* signals
    /// (no tool calls, no recall, trivial response) the score would
    /// collapse to its length-and-stop-reason components and mislead
    /// downstream preference learning. In that case returning `None`
    /// lets the trainer skip the record rather than treat it as a
    /// high-confidence label.
    #[must_use]
    pub fn compute_quality_score(&self) -> Option<f32> {
        // WHY constants: clearly named weights make the formula auditable
        // and easy to re-tune once RL training produces ground truth.
        const W_TOOLS: f32 = 0.40;
        const W_RECALL: f32 = 0.20;
        const W_SUBSTANCE: f32 = 0.20;
        const W_STOP: f32 = 0.10;
        const W_CORRECTION: f32 = 0.10;
        const SUBSTANCE_SATURATE_CHARS: f32 = 400.0;

        let mut score = 0.0_f32;
        let mut have_any_signal = false;

        // Tool success rate.
        if let Some(outcomes) = self.tool_outcomes.as_ref()
            && !outcomes.is_empty()
        {
            have_any_signal = true;
            let successes = outcomes.iter().filter(|o| o.success).count();
            // WHY f32 cast: 0..=count fits, division is bounded.
            #[expect(
                clippy::cast_precision_loss,
                clippy::as_conversions,
                reason = "usize→f32: counts fit in f32 precision for realistic turn sizes"
            )]
            let rate = successes as f32 / outcomes.len() as f32; // kanon:ignore RUST/as-cast
            score += W_TOOLS * rate;
        }

        // Recall utilization rate: referenced / injected.
        if let Some(recall) = self.recall_signals.as_ref()
            && recall.results_injected > 0
        {
            have_any_signal = true;
            let referenced =
                u32::try_from(recall.facts.iter().filter(|f| f.was_referenced).count())
                    .unwrap_or(u32::MAX);
            // WHY min: guard against a stale recall_signals where
            // facts.len() > results_injected.
            let denom = recall.results_injected.max(1);
            #[expect(
                clippy::cast_precision_loss,
                clippy::as_conversions,
                reason = "u32→f32: recall counts are small"
            )]
            let rate = (referenced.min(denom) as f32) / (denom as f32); // kanon:ignore RUST/as-cast
            score += W_RECALL * rate;
        }

        // Response substance, saturating.
        {
            #[expect(
                clippy::cast_precision_loss,
                clippy::as_conversions,
                reason = "usize→f32: char counts fit in f32 for any realistic response"
            )]
            let len = self.assistant_response.chars().count() as f32; // kanon:ignore RUST/as-cast
            let substance = (len / SUBSTANCE_SATURATE_CHARS).min(1.0);
            score += W_SUBSTANCE * substance;
            // substance alone is not a "signal" — a short response can
            // still be valid. We intentionally do NOT set
            // have_any_signal here.
        }

        // Stop reason.
        let stop_ok = matches!(
            self.stop_reason,
            CaptureStopReason::EndTurn | CaptureStopReason::StopSequence
        );
        score += W_STOP * if stop_ok { 1.0 } else { 0.0 };

        // Correction penalty.
        if let Some(is_corr) = self.is_correction {
            have_any_signal = true;
            score += W_CORRECTION * if is_corr { 0.0 } else { 1.0 };
        }

        if !have_any_signal {
            return None;
        }
        Some(score.clamp(0.0, 1.0))
    }
}

/// Manifest tracking shard files, record counts, and schema version range.
///
/// Persisted atomically as `training-manifest.json` in the training directory.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrainingManifest {
    /// Ordered list of shard file names (relative to the training directory).
    pub shards: Vec<ShardEntry>,
    /// Total records across all shards.
    pub total_records: u64,
    /// Minimum schema version seen across all records.
    pub schema_version_min: u32,
    /// Maximum schema version seen across all records.
    pub schema_version_max: u32,
}

/// A single shard entry in the manifest.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShardEntry {
    /// File name (relative to the training directory).
    pub file_name: String,
    /// Number of records in this shard.
    pub record_count: u64,
    /// Size in bytes (last known).
    pub size_bytes: u64,
}

impl TrainingManifest {
    /// Create a new empty manifest.
    fn new() -> Self {
        Self {
            shards: Vec::new(),
            total_records: 0,
            schema_version_min: TRAINING_RECORD_SCHEMA_VERSION,
            schema_version_max: TRAINING_RECORD_SCHEMA_VERSION,
        }
    }

    /// Persist the manifest atomically: write to temp, then rename.
    fn persist(&self, manifest_path: &Path) -> Result<()> {
        let json = serde_json::to_string_pretty(self).context(SerializeManifestSnafu)?;

        let tmp_path = manifest_path.with_extension("json.tmp");

        // WHY: std::fs::write is disallowed by project lint config.
        // Use OpenOptions for explicit create-truncate-write.
        let mut file = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(&tmp_path)
            .context(PersistManifestSnafu { path: &tmp_path })?;
        file.write_all(json.as_bytes())
            .context(PersistManifestSnafu { path: &tmp_path })?;

        fs::rename(&tmp_path, manifest_path).context(RenameManifestSnafu {
            from: &tmp_path,
            to: manifest_path,
        })?;

        Ok(())
    }
}

/// Sharded, append-only training data writer.
///
/// Writes [`TrainingRecord`]s as JSON Lines to shard files on disk. When the
/// current shard exceeds [`TrainingConfig::max_shard_bytes`], the writer
/// rotates to a new shard. A [`TrainingManifest`] is persisted after each
/// write for crash recovery.
///
/// If an author classifier is configured, turns are additionally filtered
/// at an authorship gate: if the user message is classified as non-user-authored
/// with confidence >= the configured threshold, the turn is rejected and logged
/// rather than written to training storage.
pub struct TrainingCapture {
    /// Training data directory.
    dir: PathBuf,
    /// Full path to the current shard file.
    current_shard: PathBuf,
    /// Path to the manifest file.
    manifest_path: PathBuf,
    /// In-memory manifest state.
    manifest: TrainingManifest,
    /// Maximum shard size before rotation.
    max_shard_bytes: u64,
    /// Whether to apply PII redaction before writing each record.
    pii_filter_enabled: bool,
    /// Optional author classifier for filtering non-user-authored text.
    ///
    /// If `Some`, applies an authorship gate before writing.
    /// If `None`, no authorship filtering is applied.
    classifier: Option<Arc<Classifier>>,
}

impl TrainingCapture {
    /// Create a new training capture writer.
    ///
    /// `instance_root` is the base directory of the aletheia instance
    /// (typically the working directory). Shards are placed at
    /// `{instance_root}/{config.path}/training-YYYYMMDD-NNNN.jsonl`.
    ///
    /// If a legacy `conversations.jsonl` file exists, it is adopted as the
    /// first shard and recorded in the manifest.
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

        let manifest_path = dir.join("training-manifest.json");
        let legacy_path = dir.join("conversations.jsonl");

        // Load or initialize manifest
        let mut manifest = if manifest_path.exists() {
            let content = fs::read_to_string(&manifest_path).unwrap_or_default();
            serde_json::from_str(&content).unwrap_or_else(|_| TrainingManifest::new())
        } else {
            TrainingManifest::new()
        };

        // Backward compat: adopt legacy file as first shard
        if legacy_path.exists()
            && !manifest
                .shards
                .iter()
                .any(|s| s.file_name == "conversations.jsonl")
        {
            let meta =
                fs::metadata(&legacy_path).context(ReadMetadataSnafu { path: &legacy_path })?;
            let line_count = fs::read_to_string(&legacy_path)
                .unwrap_or_default()
                .lines()
                .filter(|l| !l.trim().is_empty())
                .count();
            #[expect(clippy::as_conversions, reason = "usize→u64: line count fits in u64")]
            let record_count = line_count as u64; // kanon:ignore RUST/as-cast

            manifest.shards.push(ShardEntry {
                file_name: "conversations.jsonl".to_owned(),
                record_count,
                size_bytes: meta.len(),
            });
            manifest.total_records += record_count;
            // Legacy records may be schema v0 or v1
            if record_count > 0 {
                manifest.schema_version_min = 0;
            }
        }

        // Determine current shard: either the last shard if still under limit,
        // or allocate a new one.
        let current_shard = Self::resolve_current_shard(&dir, &manifest, config.max_shard_bytes)?;

        // Ensure the new shard is registered in the manifest if it's new
        let shard_name = current_shard
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();
        if !manifest.shards.iter().any(|s| s.file_name == shard_name) {
            manifest.shards.push(ShardEntry {
                file_name: shard_name,
                record_count: 0,
                size_bytes: 0,
            });
        }

        // Persist initial manifest state
        manifest.persist(&manifest_path)?;

        debug!(path = %dir.display(), shards = manifest.shards.len(), "training capture initialized");

        Ok(Self {
            dir,
            current_shard,
            manifest_path,
            manifest,
            max_shard_bytes: config.max_shard_bytes,
            pii_filter_enabled: config.pii_filter_enabled,
            classifier: None,
        })
    }

    /// Resolve which shard file to write to. Returns the last shard if it's
    /// under the size limit, or creates a new shard name.
    fn resolve_current_shard(
        dir: &Path,
        manifest: &TrainingManifest,
        max_shard_bytes: u64,
    ) -> Result<PathBuf> {
        if let Some(last) = manifest.shards.last() {
            let last_path = dir.join(&last.file_name);
            if last_path.exists() {
                let meta =
                    fs::metadata(&last_path).context(ReadMetadataSnafu { path: &last_path })?;
                if meta.len() < max_shard_bytes {
                    return Ok(last_path);
                }
            } else {
                // Shard referenced in manifest but missing on disk — create it
                return Ok(last_path);
            }
        }

        // Allocate a new shard
        Ok(dir.join(Self::new_shard_name(dir)))
    }

    /// Generate a new shard file name: `training-YYYYMMDD-NNNN.jsonl`.
    fn new_shard_name(dir: &Path) -> String {
        let today = jiff::civil::date(
            Timestamp::now().to_zoned(jiff::tz::TimeZone::UTC).year(),
            Timestamp::now().to_zoned(jiff::tz::TimeZone::UTC).month(),
            Timestamp::now().to_zoned(jiff::tz::TimeZone::UTC).day(),
        );
        let date_str = format!("{:04}{:02}{:02}", today.year(), today.month(), today.day());

        // Find the highest sequence number for today's date
        let prefix = format!("training-{date_str}-");
        let mut max_seq: u32 = 0;

        if let Ok(entries) = fs::read_dir(dir) {
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().to_string();
                if let Some(rest) = name.strip_prefix(&prefix)
                    && let Some(seq_str) = rest.strip_suffix(".jsonl")
                    && let Ok(seq) = seq_str.parse::<u32>()
                {
                    max_seq = max_seq.max(seq);
                }
            }
        }

        format!("training-{date_str}-{:04}.jsonl", max_seq + 1)
    }

    /// Rotate to a new shard file.
    fn rotate(&mut self) {
        let new_name = Self::new_shard_name(&self.dir);
        let new_path = self.dir.join(&new_name);
        self.current_shard = new_path;
        self.manifest.shards.push(ShardEntry {
            file_name: new_name,
            record_count: 0,
            size_bytes: 0,
        });
        debug!(
            shard = self.manifest.shards.len(),
            "training capture rotated to new shard"
        );
    }

    /// Write a training record to the current shard JSONL file.
    ///
    /// Opens the file in append mode, serializes the record as a single
    /// JSON line, and flushes. Rotates to a new shard if the current file
    /// exceeds `max_shard_bytes`. Each call is independent: no file handle
    /// is held between writes.
    ///
    /// # Errors
    ///
    /// Returns an error if the file cannot be opened, the record cannot
    /// be serialized, or the write fails.
    pub fn write_record(&mut self, record: &TrainingRecord) -> Result<()> {
        // Check if rotation is needed before writing
        if self.current_shard.exists() {
            let meta = fs::metadata(&self.current_shard).context(ReadMetadataSnafu {
                path: &self.current_shard,
            })?;
            if meta.len() >= self.max_shard_bytes {
                self.rotate();
            }
        }

        let mut line = serde_json::to_string(record).context(SerializeSnafu)?;
        line.push('\n');

        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.current_shard)
            .context(OpenFileSnafu {
                path: &self.current_shard,
            })?;

        file.write_all(line.as_bytes()).context(WriteRecordSnafu {
            path: &self.current_shard,
        })?;

        // Update manifest
        if let Some(last) = self.manifest.shards.last_mut() {
            last.record_count += 1;
            // Update size estimate from the line we just wrote
            #[expect(clippy::as_conversions, reason = "usize→u64: line length fits in u64")]
            {
                last.size_bytes += line.len() as u64; // kanon:ignore RUST/as-cast
            }
        }
        self.manifest.total_records += 1;
        self.manifest.schema_version_max =
            self.manifest.schema_version_max.max(record.schema_version);
        self.manifest.schema_version_min =
            self.manifest.schema_version_min.min(record.schema_version);

        // Persist manifest atomically
        self.manifest.persist(&self.manifest_path)?;

        debug!(
            session_id = %record.session_id,
            nous_id = %record.nous_id,
            tokens = record.tokens,
            shard = %self.current_shard.file_name().unwrap_or_default().to_string_lossy(),
            "training record captured"
        );
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
            debug!(
                session_id = input.session_id,
                "training capture skipped: empty/whitespace response"
            );
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

        // NEW: authorship gate filters non-user-authored text (AI-generated,
        // system scaffolding, templates) to prevent training data contamination.
        // Confidence threshold is configurable; default is 0.85 (conservative).
        // Threshold is hardcoded to 0.85 for v1; could be made configurable later.
        const AUTHORSHIP_FILTER_CONFIDENCE_THRESHOLD: f32 = 0.85;

        if let Some(classifier) = &self.classifier {
            match classifier.classify(input.user_message) {
                Ok(probs) => {
                    let class = probs.argmax();
                    let confidence = probs.confidence();

                    if class != aletheia_classify::AuthorClass::User
                        && confidence >= AUTHORSHIP_FILTER_CONFIDENCE_THRESHOLD
                    {
                        debug!(
                            session_id = input.session_id,
                            class = class.as_str(),
                            confidence = confidence,
                            "training capture skipped: authorship gate rejected non-user text"
                        );
                        return false;
                    }
                }
                Err(e) => {
                    // WHY: classifier failures must not block the pipeline.
                    // Log the error and continue with capture (graceful degradation).
                    warn!(
                        error = %e,
                        session_id = input.session_id,
                        "authorship classification failed; continuing without filter"
                    );
                }
            }
        }

        // WHY compute quality before PII filtering: quality_score is
        // derived from signals (tool outcomes, recall, stop reason,
        // correction flag) not from text content, so redaction order
        // is irrelevant. Computing it here keeps the borrow of
        // `input` lifetimes clean before we move fields into the
        // record below.
        let quality_score = input.compute_quality_score();

        // WHY apply PII redaction at write time: the filter is a
        // training-time safeguard, not a commit-time scanner. Both
        // `user_message` and `assistant_response` are scrubbed because
        // either can contain pasted secrets — e.g. a user sharing a
        // key for debugging, or the assistant echoing a key back.
        let (user_message, assistant_response, pii_redacted) = if self.pii_filter_enabled {
            let (u, u_changed) = pii::redact(input.user_message);
            let (a, a_changed) = pii::redact(input.assistant_response);
            // `pii_redacted = true` whenever the policy was applied
            // AND at least one match was found. Callers reading the
            // corpus can filter on this flag to find records that had
            // sensitive content.
            (u, a, u_changed || a_changed)
        } else {
            (
                input.user_message.to_owned(),
                input.assistant_response.to_owned(),
                false,
            )
        };

        let record = TrainingRecord {
            schema_version: TRAINING_RECORD_SCHEMA_VERSION,
            session_id: input.session_id.to_owned(),
            nous_id: input.nous_id.to_owned(),
            user_message,
            assistant_response,
            model: input.model.to_owned(),
            tokens: input.tokens,
            timestamp: Timestamp::now(),
            turn_type: input.turn_type,
            is_correction: input.is_correction,
            fact_types: input.fact_types,
            quality_score,
            tool_outcomes: input.tool_outcomes,
            recall_signals: input.recall_signals,
            pii_redacted,
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

    /// Path to the current shard JSONL output file.
    #[must_use]
    pub fn file_path(&self) -> &Path {
        &self.current_shard
    }

    /// Path to the training data directory.
    #[must_use]
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Reference to the in-memory manifest.
    #[must_use]
    pub fn manifest(&self) -> &TrainingManifest {
        &self.manifest
    }

    /// Set the author classifier for this capture session.
    ///
    /// If `Some`, the classifier is used to filter non-user-authored text
    /// from training capture via the authorship gate in `maybe_capture`.
    /// If `None`, no authorship filtering is applied.
    pub fn set_classifier(&mut self, classifier: Option<Arc<Classifier>>) {
        self.classifier = classifier;
    }
}

pub mod dpo;
pub mod pii;

pub use dpo::{DpoExtractor, DpoPair, DpoWriter};
pub use pii::redact as redact_pii;

#[cfg(test)]
#[path = "../training_tests.rs"]
mod training_tests;

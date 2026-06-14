//! DPO preference pair extraction from correction turns.
//!
//! When a user corrects a previous agent response, the sequence
//! Turn N → Turn N+1 (correction) → Turn N+2 (corrected response)
//! forms a free preference pair:
//!
//! | Field | Source |
//! |-------|--------|
//! | `prompt` | Turn N user message |
//! | `rejected` | Turn N assistant response |
//! | `chosen` | Turn N+2 assistant response |
//!
//! Pairs are written to `dpo-pairs-YYYYMMDD.jsonl` in the training
//! directory. A semantic-similarity gate validates that the prompt
//! and the chosen-turn user message address the same question.
//!
//! # Observability
//!
//! ## Events
//! | Event | Level | Fields | Condition |
//! |-------|-------|--------|-----------|
//! | `dpo.pair_captured` | info | `session_id`, `rejected_turn`, `chosen_turn` | Pair passed validation and was written |
//! | `dpo.pair_rejected` | debug | `session_id`, `reason` | Pair failed semantic validation |
//! | `dpo.pending_correction` | debug | `session_id`, `turn` | Correction detected, waiting for chosen response |
//!
//! ## Metrics
//! | Metric | Type | Labels | Condition |
//! |--------|------|--------|-----------|
//! | `aletheia_dpo_pairs_captured_total` | counter | `nous_id` | Per validated pair written |

use std::collections::HashMap;
use std::collections::HashSet;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};
// kanon:ignore RUST/std-mutex-in-async — DpoExtractor is sync-only O(1) state; std::sync::Mutex is correct for LazyLock global
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use snafu::{ResultExt, Snafu};
use tracing::{debug, info};

/// Errors from DPO pair extraction and persistence.
#[derive(Debug, Snafu)]
#[snafu(visibility(pub(crate)))]
#[non_exhaustive]
#[expect(
    missing_docs,
    reason = "snafu error variant fields are self-documenting via display format"
)]
// kanon:ignore RUST/non-exhaustive-enum — already #[non_exhaustive]; false positive from attribute ordering
pub enum DpoError {
    /// Failed to create the DPO output directory.
    #[snafu(display("failed to create DPO directory {}: {source}", path.display()))]
    CreateDir {
        path: PathBuf,
        source: std::io::Error,
    },

    /// Failed to open the DPO JSONL file for appending.
    #[snafu(display("failed to open DPO file {}: {source}", path.display()))]
    OpenFile {
        path: PathBuf,
        source: std::io::Error,
    },

    /// Failed to serialize a DPO pair to JSON.
    #[snafu(display("failed to serialize DPO pair: {source}"))]
    Serialize { source: serde_json::Error },

    /// Failed to write a DPO pair to the JSONL file.
    #[snafu(display("failed to write DPO pair to {}: {source}", path.display()))]
    WritePair {
        path: PathBuf,
        source: std::io::Error,
    },

    /// Failed to read file metadata.
    #[snafu(display("failed to read metadata for {}: {source}", path.display()))]
    ReadMetadata {
        path: PathBuf,
        source: std::io::Error,
    },
}

/// Result alias for DPO operations.
pub type Result<T> = std::result::Result<T, DpoError>;

/// A single DPO preference pair extracted from a correction sequence.
///
/// Serialized as one JSON line in the output JSONL file.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DpoPair {
    /// The user prompt that both the rejected and chosen responses answer.
    pub prompt: String,
    /// The corrected assistant response (preferred).
    pub chosen: String,
    /// The original assistant response that was corrected (dispreferred).
    pub rejected: String,
    /// Session identifier linking the pair to its conversation.
    // kanon:ignore RUST/primitive-for-domain-id — existing String-based ID; migrating to newtype requires cross-crate API changes
    pub session_id: String,
    /// Turn number of the rejected response.
    pub rejected_turn: u64,
    /// Turn number of the chosen response.
    pub chosen_turn: u64,
}

/// Snapshot of a single turn's data needed for DPO extraction.
#[derive(Debug, Clone)]
struct TurnSnapshot {
    turn_number: u64,
    user_message: String,
    assistant_response: String,
}

/// Pending state for a correction sequence.
///
/// When Turn N+1 is detected as a correction, we store Turn N's
/// prompt and rejected response, then wait for Turn N+2 to supply
/// the chosen response.
#[derive(Debug, Clone)]
struct PendingCorrection {
    /// User message from Turn N (the prompt).
    prompt: String,
    /// Assistant response from Turn N (the rejected response).
    rejected: String,
    /// Turn number of the rejected response.
    rejected_turn: u64,
}

/// Minimum Jaccard similarity for the semantic validation gate.
///
/// WHY: 0.35 catches rephrased questions and keyword overlap while
/// filtering out topic switches and pure acknowledgements.
const SEMANTIC_SIMILARITY_THRESHOLD: f64 = 0.5;

/// Maximum length in characters for a continuation message that
/// bypasses the semantic gate.
///
/// WHY: short messages like "ok", "thanks", "go on" are valid
/// continuations of the prior turn and should not block pair capture.
const CONTINUATION_MAX_CHARS: usize = 20;

/// Extractor that detects correction→response sequences and produces
/// [`DpoPair`]s.
///
/// Maintains a small per-session buffer of the most recent turn and
/// at most one pending correction. State is bounded: old pending
/// state is silently overwritten if a new correction arrives before
/// the chosen response.
pub struct DpoExtractor {
    /// Most recent non-correction turn per session.
    last_turn: HashMap<String, TurnSnapshot>,
    /// Pending correction waiting for the chosen response.
    pending: HashMap<String, PendingCorrection>,
}

/// Redact sensitive values from turn text before it is stored or emitted.
///
/// Always runs koina's lightweight secret redactor so raw API keys,
/// OAuth/JWT-like tokens, and password-shaped assignments never reach
/// the DPO JSONL. The operator-toggleable full nous training PII suite
/// runs only when `pii_filter_enabled` is `true`.
fn redact_turn_text(
    user_message: &str,
    assistant_response: &str,
    pii_filter_enabled: bool,
) -> (String, String) {
    let user_message = koina::redact::redact_sensitive(user_message);
    let assistant_response = koina::redact::redact_sensitive(assistant_response);

    if pii_filter_enabled {
        let (user_message, _) = crate::training::pii::redact(&user_message);
        let (assistant_response, _) = crate::training::pii::redact(&assistant_response);
        (user_message, assistant_response)
    } else {
        (user_message, assistant_response)
    }
}

impl DpoExtractor {
    /// Create a new extractor with empty state.
    #[must_use]
    pub fn new() -> Self {
        Self {
            last_turn: HashMap::new(),
            pending: HashMap::new(),
        }
    }

    /// Process a completed turn and emit a [`DpoPair`] if a full
    /// correction sequence has been observed.
    ///
    /// # Sequence detection
    ///
    /// 1. **Turn N** (normal): stored in `last_turn`.
    /// 2. **Turn N+1** (`is_correction = true`): the previous turn
    ///    (Turn N) is promoted from `last_turn` to `pending`. The
    ///    current turn is not cached as `last_turn` because a
    ///    correction user message is not a valid prompt.
    /// 3. **Turn N+2** (normal): if `pending` exists, the current
    ///    assistant response becomes the chosen response. A pair is
    ///    emitted after semantic validation. The current turn is then
    ///    cached as `last_turn` for potential future corrections.
    ///
    /// Chained corrections (Turn N+2 also a correction) simply
    /// overwrite `pending` with the latest rejected turn.
    ///
    /// Sensitive values in `user_message` and `assistant_response` are
    /// redacted before storage: `koina::redact::redact_sensitive` always
    /// runs, and the full nous training PII suite runs only when
    /// `pii_filter_enabled` is `true`.
    #[must_use]
    pub fn process_turn(
        &mut self,
        session_id: &str,
        turn_number: u64,
        user_message: &str,
        assistant_response: &str,
        is_correction: bool,
        pii_filter_enabled: bool,
    ) -> Option<DpoPair> {
        let (user_message, assistant_response) =
            redact_turn_text(user_message, assistant_response, pii_filter_enabled);

        if is_correction {
            if let Some(last) = self.last_turn.remove(session_id) {
                debug!(
                    session_id,
                    rejected_turn = last.turn_number,
                    "dpo.pending_correction: waiting for chosen response"
                );
                self.pending.insert(
                    session_id.to_owned(),
                    PendingCorrection {
                        prompt: last.user_message,
                        rejected: last.assistant_response,
                        rejected_turn: last.turn_number,
                    },
                );
            } else {
                // WHY: a chained correction (correction turn with no intervening
                // non-correction turn) invalidates any stale pending. Without this,
                // pending from an earlier correction could spuriously pair with a
                // much later non-correction turn.
                self.pending.remove(session_id);
            }
            // WHY: correction turns are never cached as last_turn.
            return None;
        }

        let pair = if let Some(pending) = self.pending.remove(session_id) {
            if Self::validate_semantic_match(&pending.prompt, &user_message) {
                info!(
                    session_id,
                    rejected_turn = pending.rejected_turn,
                    chosen_turn = turn_number,
                    "dpo.pair_captured"
                );
                Some(DpoPair {
                    prompt: pending.prompt,
                    chosen: assistant_response.clone(),
                    rejected: pending.rejected,
                    session_id: session_id.to_owned(),
                    rejected_turn: pending.rejected_turn,
                    chosen_turn: turn_number,
                })
            } else {
                debug!(
                    session_id,
                    rejected_turn = pending.rejected_turn,
                    chosen_turn = turn_number,
                    prompt = pending.prompt.as_str(),
                    chosen_prompt = user_message.as_str(),
                    "dpo.pair_rejected: semantic mismatch"
                );
                None
            }
        } else {
            None
        };

        self.last_turn.insert(
            session_id.to_owned(),
            TurnSnapshot {
                turn_number,
                user_message,
                assistant_response,
            },
        );

        pair
    }

    /// Check whether two user messages address the same semantic question.
    ///
    /// Uses Jaccard similarity over lowercased word sets. Very short
    /// messages (≤ [`CONTINUATION_MAX_CHARS`]) are treated as
    /// continuations and pass automatically.
    fn validate_semantic_match(original_prompt: &str, chosen_prompt: &str) -> bool {
        let chosen_trimmed = chosen_prompt.trim();
        if chosen_trimmed.len() <= CONTINUATION_MAX_CHARS {
            return true;
        }

        // WHY: normalize by stripping non-alphanumerics so that "france?" and
        // "france." collapse to the same token. Without this, trailing punctuation
        // perturbs Jaccard similarity enough to fall below the threshold.
        let tokenize = |s: &str| -> HashSet<String> {
            s.to_lowercase()
                .split_whitespace()
                .map(|w| w.trim_matches(|c: char| !c.is_alphanumeric()).to_owned())
                .filter(|w| !w.is_empty())
                .collect()
        };
        let original_words = tokenize(original_prompt);
        let chosen_words = tokenize(chosen_trimmed);

        if original_words.is_empty() || chosen_words.is_empty() {
            return false;
        }

        let intersection: HashSet<&String> = original_words.intersection(&chosen_words).collect();
        let union: HashSet<&String> = original_words.union(&chosen_words).collect();

        // WHY f64::from(u32): set cardinalities for a small-vocabulary
        // Jaccard similarity are bounded by the message word count
        // (< 2^32), so `try_from` is infallible in practice; u32→f64 is
        // an exact conversion.
        let i_u32 = u32::try_from(intersection.len()).unwrap_or(u32::MAX);
        let u_u32 = u32::try_from(union.len()).unwrap_or(u32::MAX);
        let similarity = f64::from(i_u32) / f64::from(u_u32);
        similarity >= SEMANTIC_SIMILARITY_THRESHOLD
    }
}

impl Default for DpoExtractor {
    fn default() -> Self {
        Self::new()
    }
}

/// Writer for DPO preference pairs to a dated JSONL file.
///
/// File naming: `dpo-pairs-YYYYMMDD.jsonl` in the training directory.
/// The file is opened in append mode for each write; no handle is
/// held between calls.
pub struct DpoWriter {
    path: PathBuf,
}

impl DpoWriter {
    /// Create a new DPO writer.
    ///
    /// `dir` is the training data directory (same as
    /// [`TrainingCapture`](super::TrainingCapture) uses).
    ///
    /// Creates the directory if it does not exist.
    ///
    /// # Errors
    ///
    /// Returns [`DpoError::CreateDir`] if the directory cannot be created.
    pub fn new(dir: &Path) -> Result<Self> {
        std::fs::create_dir_all(dir).context(CreateDirSnafu { path: dir })?;
        let path = dir.join(Self::file_name());
        Ok(Self { path })
    }

    /// Generate the DPO file name for today: `dpo-pairs-YYYYMMDD.jsonl`.
    fn file_name() -> String {
        let now = jiff::Timestamp::now().to_zoned(jiff::tz::TimeZone::UTC);
        let date = jiff::civil::date(now.year(), now.month(), now.day());
        format!(
            "dpo-pairs-{:04}{:02}{:02}.jsonl",
            date.year(),
            date.month(),
            date.day()
        )
    }

    /// Write a single [`DpoPair`] as a JSON line to the output file.
    ///
    /// # Errors
    ///
    /// Returns an error if the file cannot be opened, the pair cannot
    /// be serialized, or the write fails.
    pub fn write_pair(&self, pair: &DpoPair) -> Result<()> {
        let mut line = serde_json::to_string(pair).context(SerializeSnafu)?;
        line.push('\n');

        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
            .context(OpenFileSnafu { path: &self.path })?;

        file.write_all(line.as_bytes())
            .context(WritePairSnafu { path: &self.path })?;

        Ok(())
    }

    /// Path to the current DPO JSONL output file.
    #[must_use]
    pub fn file_path(&self) -> &Path {
        &self.path
    }
}

/// Global extractor shared across pipeline tasks.
///
/// WHY: The pipeline is spawned as a new task per turn with no
/// persistent actor state. Session IDs are ULID-based and globally
/// unique, so cross-session collisions are impossible. A standard
/// `Mutex` is sufficient because extractor operations are O(1) and
/// complete in microseconds.
static EXTRACTOR: std::sync::LazyLock<Mutex<DpoExtractor>> =
    std::sync::LazyLock::new(|| Mutex::new(DpoExtractor::new()));

/// Process a completed turn through the global extractor and return
/// a [`DpoPair`] if a correction sequence has finalized.
///
/// See [`DpoExtractor::process_turn`] for sequence semantics and
/// redaction behavior; `pii_filter_enabled` is forwarded unchanged.
#[must_use]
pub fn process_turn_global(
    session_id: &str,
    turn_number: u64,
    user_message: &str,
    assistant_response: &str,
    is_correction: bool,
    pii_filter_enabled: bool,
) -> Option<DpoPair> {
    let Ok(mut guard) = EXTRACTOR.lock() else {
        tracing::warn!("DPO extractor mutex poisoned; skipping pair extraction");
        return None;
    };
    guard.process_turn(
        session_id,
        turn_number,
        user_message,
        assistant_response,
        is_correction,
        pii_filter_enabled,
    )
}

/// Record a captured DPO pair in the metrics registry.
pub fn record_dpo_pair_captured(nous_id: &str) {
    crate::metrics::record_dpo_pair(nous_id);
}

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test assertions")]
mod tests {
    use super::*;

    #[test]
    fn extractor_emits_pair_on_correction_sequence() {
        let mut extractor = DpoExtractor::new();

        let p1 = extractor.process_turn(
            "ses-1",
            1,
            "What is the capital of France?",
            "London",
            false,
            false,
        );
        assert!(p1.is_none(), "single normal turn should not emit");

        let p2 = extractor.process_turn(
            "ses-1",
            2,
            "Actually, the capital of France is Paris.",
            "You are right.",
            true,
            false,
        );
        assert!(p2.is_none(), "correction turn should not emit");

        let p3 = extractor.process_turn(
            "ses-1",
            3,
            "What is the capital of France?",
            "Paris",
            false,
            false,
        );
        let pair = p3.expect("should emit pair after correction sequence");
        assert_eq!(pair.prompt, "What is the capital of France?");
        assert_eq!(pair.rejected, "London");
        assert_eq!(pair.chosen, "Paris");
        assert_eq!(pair.rejected_turn, 1);
        assert_eq!(pair.chosen_turn, 3);
        assert_eq!(pair.session_id, "ses-1");
    }

    #[test]
    fn extractor_rejects_semantic_mismatch() {
        let mut extractor = DpoExtractor::new();

        let _ = extractor.process_turn(
            "ses-1",
            1,
            "What is the capital of France?",
            "London",
            false,
            false,
        );
        let _ = extractor.process_turn(
            "ses-1",
            2,
            "Actually, the capital of France is Paris.",
            "You are right.",
            true,
            false,
        );

        let p3 = extractor.process_turn(
            "ses-1",
            3,
            "What is the weather today?",
            "Sunny",
            false,
            false,
        );
        assert!(p3.is_none(), "semantic mismatch should not emit pair");
    }

    #[test]
    fn extractor_accepts_continuation_prompt() {
        let mut extractor = DpoExtractor::new();

        let _ = extractor.process_turn(
            "ses-1",
            1,
            "What is the capital of France?",
            "London",
            false,
            false,
        );
        let _ = extractor.process_turn(
            "ses-1",
            2,
            "Actually, the capital of France is Paris.",
            "You are right.",
            true,
            false,
        );

        let p3 = extractor.process_turn("ses-1", 3, "ok", "Paris", false, false);
        let pair = p3.expect("short continuation should pass validation");
        assert_eq!(pair.chosen, "Paris");
    }

    #[test]
    fn extractor_handles_multiple_sessions() {
        let mut extractor = DpoExtractor::new();

        let _ = extractor.process_turn("ses-a", 1, "Question A?", "Wrong A", false, false);
        let _ = extractor.process_turn("ses-a", 2, "Actually...", "Sorry.", true, false);

        let _ = extractor.process_turn("ses-b", 1, "Question B?", "Wrong B", false, false);
        let _ = extractor.process_turn("ses-b", 2, "No, it's...", "My mistake.", true, false);

        let pa = extractor.process_turn("ses-a", 3, "Question A?", "Right A", false, false);
        assert!(pa.is_some(), "session A should emit");

        let pb = extractor.process_turn("ses-b", 3, "Question B?", "Right B", false, false);
        assert!(pb.is_some(), "session B should emit");
    }

    #[test]
    fn extractor_overwrites_pending_on_chained_corrections() {
        let mut extractor = DpoExtractor::new();

        let _ = extractor.process_turn("ses-1", 1, "Question?", "Wrong 1", false, false);
        let _ = extractor.process_turn("ses-1", 2, "Actually...", "Sorry.", true, false);
        let _ = extractor.process_turn("ses-1", 3, "No wait...", "I see.", true, false);

        // WHY: turn 2 was itself a correction, so no last_turn was cached and
        // the chained correction at turn 3 clears pending — turn 4 finds no
        // pending and must emit nothing.
        let p4 = extractor.process_turn("ses-1", 4, "Question?", "Right", false, false);
        assert!(
            p4.is_none(),
            "chained correction without intermediate answer should not emit"
        );
    }

    #[test]
    fn semantic_match_similar_questions() {
        assert!(DpoExtractor::validate_semantic_match(
            "What is the capital of France?",
            "Tell me the capital of France."
        ));
    }

    #[test]
    fn semantic_mismatch_different_topics() {
        assert!(!DpoExtractor::validate_semantic_match(
            "What is the capital of France?",
            "How many planets are in the solar system?"
        ));
    }

    #[test]
    fn semantic_match_short_continuation() {
        assert!(DpoExtractor::validate_semantic_match(
            "What is the capital of France?",
            "ok"
        ));
        assert!(DpoExtractor::validate_semantic_match(
            "What is the capital of France?",
            "thanks"
        ));
    }

    #[test]
    fn dpo_pair_serde_roundtrip() {
        let pair = DpoPair {
            prompt: "What is 2+2?".to_owned(),
            chosen: "4".to_owned(),
            rejected: "5".to_owned(),
            session_id: "ses-1".to_owned(),
            rejected_turn: 1,
            chosen_turn: 3,
        };

        let json = serde_json::to_string(&pair).expect("serialize");
        let back: DpoPair = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back, pair);
    }

    #[test]
    fn dpo_writer_creates_file() {
        let dir = tempfile::tempdir().expect("tempdir");
        let writer = DpoWriter::new(dir.path()).expect("new");
        assert!(writer.file_path().to_string_lossy().ends_with(".jsonl"));
    }

    #[test]
    fn dpo_writer_appends_jsonl() {
        let dir = tempfile::tempdir().expect("tempdir");
        let writer = DpoWriter::new(dir.path()).expect("new");

        let pair = DpoPair {
            prompt: "P".to_owned(),
            chosen: "C".to_owned(),
            rejected: "R".to_owned(),
            session_id: "ses-1".to_owned(),
            rejected_turn: 1,
            chosen_turn: 3,
        };
        writer.write_pair(&pair).expect("write");
        writer.write_pair(&pair).expect("write");

        let content = std::fs::read_to_string(writer.file_path()).expect("read");
        let lines: Vec<&str> = content.lines().collect();
        assert_eq!(lines.len(), 2);

        let parsed: DpoPair =
            serde_json::from_str(lines.first().expect("first line")).expect("parse");
        assert_eq!(parsed.prompt, "P");
        assert_eq!(parsed.chosen, "C");
        assert_eq!(parsed.rejected, "R");
    }

    #[test]
    fn extractor_redacts_secret_when_full_pii_suite_disabled() {
        let mut extractor = DpoExtractor::new();

        // WHY: split/concat so the full synthetic key string never appears as a
        // raw literal that credential scanners could flag.
        let secret = concat!("sk-", "ant-", "api03-", "abc123def456");
        let prompt = format!("Why does api_key={secret} fail?");
        let rejected = format!("The key {secret} is invalid");
        let correction = "Actually the key format is wrong".to_owned();
        let chosen = format!("Use {secret} with the v3 header");

        let p1 = extractor.process_turn("ses-1", 1, &prompt, &rejected, false, false);
        assert!(p1.is_none(), "single normal turn should not emit");

        let p2 = extractor.process_turn("ses-1", 2, &correction, "You are right.", true, false);
        assert!(p2.is_none(), "correction turn should not emit");

        let p3 = extractor.process_turn("ses-1", 3, &prompt, &chosen, false, false);
        let pair = p3.expect("should emit pair after correction sequence");

        assert!(
            !pair.prompt.contains(secret),
            "prompt must not contain raw secret: {}",
            pair.prompt
        );
        assert!(
            !pair.rejected.contains(secret),
            "rejected must not contain raw secret: {}",
            pair.rejected
        );
        assert!(
            !pair.chosen.contains(secret),
            "chosen must not contain raw secret: {}",
            pair.chosen
        );
        assert!(
            !pair.prompt.contains("[REDACTED:"),
            "full PII suite must not run when disabled: {}",
            pair.prompt
        );
    }
}

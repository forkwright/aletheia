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
//! directory. A versioned semantic-similarity gate ([`DPO_VALIDATOR_VERSION`])
//! validates that the prompt and the chosen-turn user message address the
//! same question, and rejects a pending correction that has aged past
//! [`PENDING_CORRECTION_MAX_AGE`].
//!
//! # Durability
//!
//! Correction-sequence state (`last_turn`, `pending`) is persisted in a
//! fjall keyspace under the writer's directory, not held in process
//! memory. A [`DpoExtractor`] opened at the same path after a restart
//! resumes exactly where the prior process left off, so a crash between
//! the correction turn and the chosen response does not silently drop the
//! pending pair. [`DpoWriter::process_and_write`] is the single entry
//! point combining extraction and the idempotent JSONL write.
//!
//! # Key schema
//!
//! All keys are UTF-8 `session_id` strings. Values are JSON-encoded.
//!
//! | Partition   | Key         | Value                    |
//! |-------------|-------------|--------------------------|
//! | `last_turn` | `session_id`| JSON [`TurnSnapshot`]    |
//! | `pending`   | `session_id`| JSON [`PendingCorrection`]|
//!
//! # Observability
//!
//! ## Events
//! | Event | Level | Fields | Condition |
//! |-------|-------|--------|-----------|
//! | `dpo.pair_captured` | info | `session_id`, `rejected_turn`, `chosen_turn` | Pair passed validation and was written |
//! | `dpo.pair_rejected` | debug | `session_id`, `reason` | Pair failed semantic validation or staleness |
//! | `dpo.pending_correction` | debug | `session_id`, `turn` | Correction detected, waiting for chosen response |
//!
//! ## Metrics
//! | Metric | Type | Labels | Condition |
//! |--------|------|--------|-----------|
//! | `aletheia_dpo_pairs_captured_total` | counter | `nous_id` | Per validated pair written |

use std::collections::HashSet;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use fjall::{KeyspaceCreateOptions, SingleWriterTxDatabase, SingleWriterTxKeyspace};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
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

    /// Failed to read the DPO JSONL file to check for an existing `pair_id`.
    #[snafu(display("failed to read DPO file {}: {source}", path.display()))]
    ReadFile {
        path: PathBuf,
        source: std::io::Error,
    },

    /// Failed to deserialize durable correction-sequence state.
    #[snafu(display("failed to deserialize DPO extractor state: {source}"))]
    DeserializeState { source: serde_json::Error },

    /// Failed to open, read, or write the durable correction-sequence state
    /// store (a fjall keyspace).
    #[snafu(display("DPO pending-state store error: {message}"))]
    PendingState { message: String },
}

/// Result alias for DPO operations.
pub type Result<T> = std::result::Result<T, DpoError>;

/// A single DPO preference pair extracted from a correction sequence.
///
/// Serialized as one JSON line in the output JSONL file.
// WHY not Eq: `similarity: Option<f64>` (#4863) has no total order.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DpoPair {
    /// Schema version that produced this pair.
    ///
    /// Defaults to `0` when deserializing rows written before the field
    /// existed, distinguishing legacy rows from version-1+ rows.
    #[serde(default)]
    pub schema_version: u32,
    /// Stable idempotency key derived from `session_id`, `rejected_turn`,
    /// and `chosen_turn` (see [`compute_pair_id`]).
    ///
    /// Deterministic: the same correction sequence always yields the same
    /// `pair_id`, so a replayed write of the same pair can be detected and
    /// deduplicated by [`DpoWriter::write_pair`]. Defaults to the empty
    /// string when deserializing rows written before the field existed; an
    /// empty `pair_id` never matches an existing row, so those rows are
    /// never treated as duplicates of anything.
    #[serde(default)]
    pub pair_id: String,
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
    /// Version tag for the semantic-similarity validator
    /// ([`DpoExtractor::validate_semantic_match`]) that admitted this pair.
    ///
    /// `None` for rows written before the field existed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub validator_version: Option<String>,
    /// Stable PII/secret redaction policy reference applied to this pair's
    /// text before persistence.
    ///
    /// `Some(pii::POLICY_REF)` when the full nous training PII suite ran
    /// (`pii_filter_enabled = true`); `None` when only the always-on
    /// secret redactor ran, and for rows written before the field existed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pii_policy_ref: Option<String>,

    // NOTE: causal provenance (#4863) groups the six fields below. Unlike
    // the fields above, these are not knowable inside
    // `DpoExtractor::process_turn` (a session-scoped correction-sequence
    // state machine with no view of the completion that produced the
    // pair) -- `DpoWriter::process_and_write` populates them on the
    // returned pair before it is written.
    /// Jaccard word-set similarity between the rejected and chosen
    /// prompts, from [`DpoExtractor::validate_semantic_match`].
    ///
    /// `None` when the pair was admitted via the continuation/reaction
    /// bypass (no similarity was computed) or for rows written before
    /// the field existed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub similarity: Option<f64>,
    /// The correction pattern that flagged the rejected turn's successor
    /// as a correction (see `episteme::extract::refinement::detect_correction`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub correction_reason: Option<String>,
    /// Reference into the prompt-audit log for the turn that produced
    /// `chosen`, when available.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt_audit_ref: Option<String>,
    /// Stable identifiers of the source turns this pair was derived from
    /// (`"{session_id}:{turn_number}"` for the rejected and chosen turns).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub source_message_ids: Vec<String>,
    /// Model that produced the chosen (corrected) response.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// Provider that served the chosen (corrected) response.
    ///
    /// A separate dimension from `model` (#4798, #4863) -- do not derive
    /// one from the other.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
}

/// Per-completion provenance for [`DpoWriter::process_and_write`] to graft
/// onto a captured [`DpoPair`] (#4863) -- borrowed inputs, mirroring
/// [`crate::training::CaptureInput`]'s pattern for the same reason: the
/// caller already owns these strings for the turn, so this borrows rather
/// than forcing an allocation per field on every processed turn (including
/// the common case where no pair results).
#[derive(Debug, Clone, Copy, Default)]
pub struct DpoPairProvenance<'a> {
    /// The correction pattern that flagged the rejected turn's successor
    /// as a correction, from `CorrectionSignal::matched_pattern`.
    pub correction_reason: Option<&'a str>,
    /// Reference into the prompt-audit log for the chosen turn.
    pub prompt_audit_ref: Option<&'a str>,
    /// Model that produced the chosen (corrected) response.
    pub model: Option<&'a str>,
    /// Provider that served the chosen (corrected) response.
    pub provider: Option<&'a str>,
}

/// Identity and content of one completed turn to run through the durable
/// correction-sequence extractor, for [`DpoWriter::process_and_write`].
///
/// WHY bundled (#4863): mirrors [`DpoExtractor::process_turn`]'s own six
/// parameters exactly -- `process_and_write` forwards every field unchanged.
/// Bundling keeps the wrapper under the workspace's `too_many_arguments`
/// threshold now that `DpoPairProvenance` is also a parameter, without
/// changing what either function does.
#[derive(Debug, Clone, Copy)]
pub struct TurnCapture<'a> {
    /// Session identifier the turn belongs to.
    pub session_id: &'a str,
    /// Turn number within the session.
    pub turn_number: u64,
    /// Raw user message for this turn.
    pub user_message: &'a str,
    /// Final assistant response for this turn.
    pub assistant_response: &'a str,
    /// Whether this turn is itself a correction of the prior turn.
    pub is_correction: bool,
    /// Whether the PII filter is enabled for redaction before matching.
    pub pii_filter_enabled: bool,
}

/// Current schema version for [`DpoPair`].
pub const DPO_PAIR_SCHEMA_VERSION: u32 = 1;

/// Version tag for [`DpoExtractor::validate_semantic_match`], recorded on
/// every captured pair via [`DpoPair::validator_version`].
///
/// Bump when the matching algorithm, threshold, continuation-bypass rule,
/// or staleness window changes, so downstream corpus consumers can tell
/// which validation semantics admitted a given pair.
pub const DPO_VALIDATOR_VERSION: &str = "dpo-jaccard-v2";

/// Compute the stable idempotency key for a DPO pair from its identity:
/// `session_id`, `rejected_turn`, and `chosen_turn`.
///
/// WHY length-prefixed field hashing rather than naive concatenation:
/// concatenating `session_id`/`rejected_turn`/`chosen_turn` directly could
/// collide across different splits (session `"a"` turns `1`,`23` vs.
/// session `"a1"` turns `2`,`3`). Hashing each field with its own length
/// prefix removes that ambiguity.
fn compute_pair_id(session_id: &str, rejected_turn: u64, chosen_turn: u64) -> String {
    let mut hasher = Sha256::new();
    hash_field(&mut hasher, session_id);
    hash_field(&mut hasher, &rejected_turn.to_string());
    hash_field(&mut hasher, &chosen_turn.to_string());
    let digest = hasher.finalize();

    let mut out = String::with_capacity(digest.len() * 2 + 5);
    out.push_str("dpo1:");
    for byte in &digest {
        use std::fmt::Write;
        // kanon:ignore RUST/no-silent-result-swallow — write! on String is infallible
        let _ = write!(out, "{byte:02x}");
    }
    out
}

/// Feed one field into `hasher`, prefixed with its byte length so that
/// field boundaries cannot be confused by concatenation ambiguity.
fn hash_field(hasher: &mut Sha256, value: &str) {
    hasher.update(value.len().to_string().as_bytes());
    hasher.update(b"\0");
    hasher.update(value.as_bytes());
    hasher.update(b"\0");
}

/// Snapshot of a single turn's data needed for DPO extraction.
#[derive(Debug, Clone, Serialize, Deserialize)]
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
#[derive(Debug, Clone, Serialize, Deserialize)]
struct PendingCorrection {
    /// User message from Turn N (the prompt).
    prompt: String,
    /// Assistant response from Turn N (the rejected response).
    rejected: String,
    /// Turn number of the rejected response.
    rejected_turn: u64,
    /// RFC 3339 timestamp when this pending correction was recorded.
    ///
    /// Used by [`is_pending_stale_at`] to reject a pairing against a chosen
    /// response that arrives long after the correction — see
    /// [`PENDING_CORRECTION_MAX_AGE`].
    pending_since: String,
}

/// Minimum Jaccard similarity for the semantic validation gate.
///
/// WHY 0.5: roughly half of the tokenized words in the shorter message
/// must reappear in the longer one, which passes rephrasings ("What is
/// the capital of France?" / "Tell me the capital of France.") while
/// rejecting topic switches ("What is the capital of France?" / "How many
/// planets are in the solar system?").
const SEMANTIC_SIMILARITY_THRESHOLD: f64 = 0.5;

/// Maximum length in characters for a continuation message that is
/// eligible for the [`CONTINUATION_PHRASES`] bypass.
///
/// WHY: short messages like "ok", "thanks", "go on" are valid
/// continuations of the prior turn and should not block pair capture.
const CONTINUATION_MAX_CHARS: usize = 20;

/// Known acknowledgement/continuation phrases that bypass semantic
/// validation when the chosen-turn message is short (see
/// [`CONTINUATION_MAX_CHARS`]).
///
/// WHY a phrase allowlist rather than "any short message passes": a short
/// message under the character cap is not necessarily a continuation of
/// the prior turn — "weather?" and "new topic" are both short but
/// semantically unrelated to a prior prompt. Requiring an exact match
/// (after lowercasing and stripping punctuation) against a known
/// acknowledgement phrase closes that gap while still passing genuine
/// continuations like "ok", "thanks", or "go on". Matched against the
/// whole trimmed message, not per-token, to avoid combinatorial
/// false-positives from common words appearing individually in the list.
const CONTINUATION_PHRASES: &[&str] = &[
    "ok",
    "okay",
    "k",
    "kk",
    "yes",
    "yeah",
    "yep",
    "sure",
    "sure thing",
    "thanks",
    "thank you",
    "thx",
    "ty",
    "got it",
    "understood",
    "noted",
    "continue",
    "go on",
    "go ahead",
    "please continue",
    "proceed",
    "roger",
    "ack",
    "acknowledged",
    "sounds good",
    "cool",
    "great",
    "perfect",
    "alright",
    "right",
    "makes sense",
];

/// Maximum age of a pending correction before it is treated as stale and
/// dropped rather than paired with a later, possibly-unrelated turn.
///
/// WHY 1 hour: pending state survives a process restart (see the
/// module-level durability note), so it can otherwise persist
/// indefinitely rather than being discarded on crash as it was before
/// durable storage. A correction and its corrected response are normally
/// exchanged within one active conversation; an hour is generous enough
/// to cover a user stepping away mid-conversation while still rejecting a
/// pairing against a conversation resumed hours or days later.
fn pending_correction_max_age() -> jiff::SignedDuration {
    jiff::SignedDuration::from_secs(3600)
}

/// Whether a pending correction recorded at `pending_since` (RFC 3339) has
/// aged past [`pending_correction_max_age`] as of `now`.
///
/// An unparseable timestamp and a `pending_since` in the future relative
/// to `now` (clock anomaly) are both treated as stale — fail closed
/// rather than pair against timing evidence that cannot be trusted.
fn is_pending_stale_at(pending_since: &str, now: jiff::Timestamp) -> bool {
    let Ok(recorded) = pending_since.parse::<jiff::Timestamp>() else {
        return true;
    };
    let age = now.duration_since(recorded);
    let zero = jiff::SignedDuration::from_secs(0);
    age > pending_correction_max_age() || age < zero
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

/// Partitions used by the extractor's durable state store.
const PARTITIONS: &[&str] = &["last_turn", "pending"];

/// Read and JSON-decode `session_id`'s row from `part` within `tx`.
///
/// Returns `Ok(None)` when no row exists for `session_id`.
fn read_state<T: serde::de::DeserializeOwned>(
    tx: &mut fjall::SingleWriterWriteTx<'_>,
    part: &SingleWriterTxKeyspace,
    session_id: &str,
) -> Result<Option<T>> {
    use fjall::Readable;
    let Some(bytes) = tx.get(part, session_id.as_bytes()).map_err(|e| {
        PendingStateSnafu {
            message: format!("fjall get: {e}"),
        }
        .build()
    })?
    else {
        return Ok(None);
    };
    let value = serde_json::from_slice(&bytes).context(DeserializeStateSnafu)?;
    Ok(Some(value))
}

/// JSON-encode `value` and insert it under `session_id` in `part` within `tx`.
fn write_state<T: Serialize>(
    tx: &mut fjall::SingleWriterWriteTx<'_>,
    part: &SingleWriterTxKeyspace,
    session_id: &str,
    value: &T,
) -> Result<()> {
    let bytes = serde_json::to_vec(value).context(SerializeSnafu)?;
    tx.insert(part, session_id.as_bytes(), bytes.as_slice());
    Ok(())
}

/// Extractor that detects correction→response sequences and produces
/// [`DpoPair`]s.
///
/// Correction-sequence state is persisted in a fjall keyspace keyed by
/// `session_id` — see the module-level durability note. State is bounded
/// per session: old pending state is overwritten if a new correction
/// arrives before the chosen response.
pub struct DpoExtractor {
    db: Arc<SingleWriterTxDatabase>,
    /// Serializes the read-decide-write sequence in [`Self::process_turn`]
    /// across concurrent callers in this process.
    ///
    /// WHY needed in addition to `SingleWriterTxDatabase`'s own writer
    /// serialization: `process_turn` reads `last_turn`/`pending` to decide
    /// what to write, and that decision must not be interleaved with
    /// another thread's read-decide-write for the same session — a lost
    /// update here would silently drop a pending correction rather than
    /// just reorder two independent writes.
    write_lock: Mutex<()>,
    /// Kept alive to auto-delete the temp directory when the store is dropped.
    _temp_dir: Option<tempfile::TempDir>,
}

impl DpoExtractor {
    /// Open (or create) a durable extractor state store at `path`.
    ///
    /// # Errors
    ///
    /// Returns [`DpoError::PendingState`] if the store cannot be opened or
    /// its partitions initialized (including when another process already
    /// holds the store open).
    pub fn open(path: &Path) -> Result<Self> {
        let fdb = koina::fjall::FjallDb::open(path, PARTITIONS).map_err(|e| {
            PendingStateSnafu {
                message: format!(
                    "failed to open DPO extractor state at {}: {e}",
                    path.display()
                ),
            }
            .build()
        })?;
        Ok(Self::from_fjall_db(fdb))
    }

    /// Open an ephemeral extractor state store (for testing).
    ///
    /// The directory and all data are deleted when the returned extractor
    /// is dropped.
    ///
    /// # Errors
    ///
    /// Returns [`DpoError::PendingState`] if the store cannot be opened.
    pub fn open_in_memory() -> Result<Self> {
        let fdb = koina::fjall::FjallDb::open_temp(PARTITIONS).map_err(|e| {
            PendingStateSnafu {
                message: format!("failed to open in-memory DPO extractor state: {e}"),
            }
            .build()
        })?;
        Ok(Self::from_fjall_db(fdb))
    }

    fn from_fjall_db(fdb: koina::fjall::FjallDb) -> Self {
        Self {
            db: Arc::new(fdb.db),
            write_lock: fdb.write_lock,
            _temp_dir: fdb._temp_dir,
        }
    }

    fn partition(&self, name: &str) -> Result<SingleWriterTxKeyspace> {
        self.db
            .keyspace(name, KeyspaceCreateOptions::default)
            .map_err(|e| {
                PendingStateSnafu {
                    message: format!("fjall partition {name}: {e}"),
                }
                .build()
            })
    }

    /// Process a completed turn and emit a [`DpoPair`] if a full
    /// correction sequence has been observed.
    ///
    /// # Sequence detection
    ///
    /// 1. **Turn N** (normal): stored as `last_turn`.
    /// 2. **Turn N+1** (`is_correction = true`): the previous turn
    ///    (Turn N) is promoted from `last_turn` to `pending`. The
    ///    current turn is not cached as `last_turn` because a
    ///    correction user message is not a valid prompt.
    /// 3. **Turn N+2** (normal): if `pending` exists and has not gone
    ///    stale (see [`pending_correction_max_age`]), the current
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
    ///
    /// # Errors
    ///
    /// Returns [`DpoError::PendingState`] or [`DpoError::DeserializeState`]
    /// if the durable state store cannot be read or written.
    pub fn process_turn(
        &self,
        session_id: &str,
        turn_number: u64,
        user_message: &str,
        assistant_response: &str,
        is_correction: bool,
        pii_filter_enabled: bool,
    ) -> Result<Option<DpoPair>> {
        let (user_message, assistant_response) =
            redact_turn_text(user_message, assistant_response, pii_filter_enabled);

        let _guard = self
            .write_lock
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);

        let last_turn_part = self.partition("last_turn")?;
        let pending_part = self.partition("pending")?;
        let mut tx = self.db.write_tx();

        if is_correction {
            let existing_last: Option<TurnSnapshot> =
                read_state(&mut tx, &last_turn_part, session_id)?;
            if let Some(last) = existing_last {
                debug!(
                    session_id,
                    rejected_turn = last.turn_number,
                    "dpo.pending_correction: waiting for chosen response"
                );
                let pending = PendingCorrection {
                    prompt: last.user_message,
                    rejected: last.assistant_response,
                    rejected_turn: last.turn_number,
                    pending_since: jiff::Timestamp::now().to_string(),
                };
                write_state(&mut tx, &pending_part, session_id, &pending)?;
                tx.remove(&last_turn_part, session_id.as_bytes());
            } else {
                // WHY: a chained correction (correction turn with no intervening
                // non-correction turn) invalidates any stale pending. Without this,
                // pending from an earlier correction could spuriously pair with a
                // much later non-correction turn.
                tx.remove(&pending_part, session_id.as_bytes());
            }
            Self::commit(tx, "process_turn(correction)")?;
            // WHY: correction turns are never cached as last_turn.
            return Ok(None);
        }

        let pending: Option<PendingCorrection> = read_state(&mut tx, &pending_part, session_id)?;
        let pair = if let Some(pending) = pending {
            tx.remove(&pending_part, session_id.as_bytes());
            if is_pending_stale_at(&pending.pending_since, jiff::Timestamp::now()) {
                debug!(
                    session_id,
                    rejected_turn = pending.rejected_turn,
                    pending_since = pending.pending_since.as_str(),
                    "dpo.pair_rejected: stale pending correction"
                );
                None
            } else if Self::validate_semantic_match(&pending.prompt, &user_message) {
                info!(
                    session_id,
                    rejected_turn = pending.rejected_turn,
                    chosen_turn = turn_number,
                    "dpo.pair_captured"
                );
                let similarity = Self::semantic_similarity_score(&pending.prompt, &user_message);
                Some(DpoPair {
                    schema_version: DPO_PAIR_SCHEMA_VERSION,
                    pair_id: compute_pair_id(session_id, pending.rejected_turn, turn_number),
                    prompt: pending.prompt,
                    chosen: assistant_response.clone(),
                    rejected: pending.rejected,
                    session_id: session_id.to_owned(),
                    rejected_turn: pending.rejected_turn,
                    chosen_turn: turn_number,
                    validator_version: Some(DPO_VALIDATOR_VERSION.to_owned()),
                    pii_policy_ref: pii_filter_enabled
                        .then(|| crate::training::pii::POLICY_REF.to_owned()),
                    similarity,
                    // WHY None here: correction_reason/prompt_audit_ref/model/
                    // provider are not knowable inside this session-scoped
                    // state machine -- `DpoWriter::process_and_write`
                    // populates them (and source_message_ids) on the pair
                    // this returns.
                    correction_reason: None,
                    prompt_audit_ref: None,
                    source_message_ids: Vec::new(),
                    model: None,
                    provider: None,
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

        let snapshot = TurnSnapshot {
            turn_number,
            user_message,
            assistant_response,
        };
        write_state(&mut tx, &last_turn_part, session_id, &snapshot)?;
        Self::commit(tx, "process_turn")?;

        Ok(pair)
    }

    fn commit(tx: fjall::SingleWriterWriteTx<'_>, op: &str) -> Result<()> {
        tx.commit().map_err(|e| {
            PendingStateSnafu {
                message: format!("fjall commit {op}: {e}"),
            }
            .build()
        })
    }

    /// Whether `chosen_trimmed` (already trimmed) bypasses semantic
    /// validation as a continuation/acknowledgement.
    ///
    /// Two independent bypasses, both bounded by
    /// [`CONTINUATION_MAX_CHARS`]: an exact match (after lowercasing and
    /// stripping punctuation) against [`CONTINUATION_PHRASES`], or a
    /// message with no alphanumeric content at all (a pure emoji/punctuation
    /// reaction, which carries no text to validate against).
    fn is_continuation_bypass(chosen_trimmed: &str) -> bool {
        if chosen_trimmed.chars().count() > CONTINUATION_MAX_CHARS {
            return false;
        }

        let normalized: String = chosen_trimmed
            .to_lowercase()
            .chars()
            .filter(|c| c.is_alphanumeric() || c.is_whitespace())
            .collect();
        let normalized = normalized.split_whitespace().collect::<Vec<_>>().join(" ");

        if normalized.is_empty() {
            // WHY: nothing alphanumeric survived normalization — a pure
            // reaction (emoji/punctuation only, e.g. "👍👍👍👍👍") carries no
            // content to validate and is treated as a continuation.
            return true;
        }
        CONTINUATION_PHRASES.contains(&normalized.as_str())
    }

    /// Check whether two user messages address the same semantic question.
    ///
    /// Short continuations/acknowledgements bypass validation (see
    /// [`Self::is_continuation_bypass`]); everything else is validated by
    /// Jaccard similarity over lowercased word sets against
    /// [`SEMANTIC_SIMILARITY_THRESHOLD`].
    fn validate_semantic_match(original_prompt: &str, chosen_prompt: &str) -> bool {
        let chosen_trimmed = chosen_prompt.trim();
        if Self::is_continuation_bypass(chosen_trimmed) {
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

    /// The raw Jaccard similarity score behind [`Self::validate_semantic_match`]'s
    /// admit/reject decision (closes the `similarity` provenance gap in
    /// #4863).
    ///
    /// A sibling function rather than changing `validate_semantic_match`'s
    /// return type: that function has direct boolean assertions in tests
    /// below, and admission logic should not have to unpack a score it
    /// doesn't need. `None` when the pair was admitted via the
    /// continuation/reaction bypass (no comparison was made) or when
    /// either message tokenized to no words.
    fn semantic_similarity_score(original_prompt: &str, chosen_prompt: &str) -> Option<f64> {
        let chosen_trimmed = chosen_prompt.trim();
        if Self::is_continuation_bypass(chosen_trimmed) {
            return None;
        }

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
            return None;
        }

        let intersection: HashSet<&String> = original_words.intersection(&chosen_words).collect();
        let union: HashSet<&String> = original_words.union(&chosen_words).collect();
        let i_u32 = u32::try_from(intersection.len()).unwrap_or(u32::MAX);
        let u_u32 = u32::try_from(union.len()).unwrap_or(u32::MAX);
        Some(f64::from(i_u32) / f64::from(u_u32))
    }
}

/// Writer for DPO preference pairs to a dated JSONL file.
///
/// File naming: `dpo-pairs-YYYYMMDD.jsonl` in the training directory.
/// The file is opened in append mode for each write; no handle is
/// held between calls. Also owns the durable [`DpoExtractor`] for this
/// directory — see [`Self::process_and_write`].
pub struct DpoWriter {
    path: PathBuf,
    extractor: DpoExtractor,
}

/// Subdirectory (under a [`DpoWriter`]'s directory) holding the durable
/// extractor state store.
const EXTRACTOR_STATE_DIRNAME: &str = "dpo-pending-state";

impl DpoWriter {
    /// Create a new DPO writer.
    ///
    /// `dir` is the training data directory (same as
    /// [`TrainingCapture`](super::TrainingCapture) uses).
    ///
    /// Creates the directory if it does not exist, and opens (or creates)
    /// the durable correction-sequence state store under it.
    ///
    /// # Errors
    ///
    /// Returns [`DpoError::CreateDir`] if the directory cannot be created,
    /// or [`DpoError::PendingState`] if the durable extractor state store
    /// cannot be opened.
    pub fn new(dir: &Path) -> Result<Self> {
        std::fs::create_dir_all(dir).context(CreateDirSnafu { path: dir })?;
        let path = dir.join(Self::file_name());
        let extractor = DpoExtractor::open(&dir.join(EXTRACTOR_STATE_DIRNAME))?;
        Ok(Self { path, extractor })
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

    /// Directory holding this writer's dated JSONL output and durable
    /// extractor state.
    ///
    /// INVARIANT: `path` is always `dir.join(Self::file_name())` (see
    /// [`Self::new`]), so `parent()` always recovers `dir` exactly.
    #[must_use]
    pub fn dir(&self) -> &Path {
        self.path.parent().unwrap_or(&self.path)
    }

    /// Process one completed turn through the durable correction-sequence
    /// extractor and, if a preference pair resulted, write it.
    ///
    /// Combines [`DpoExtractor::process_turn`] and [`Self::write_pair`]
    /// under this writer's directory so callers do not need to manage a
    /// separate extractor handle.
    ///
    /// Returns `true` if a pair was captured and appended, `false` if no
    /// pair resulted (correction turn, stale pending, or semantic
    /// mismatch) — including when the resulting pair's `pair_id` already
    /// exists in this writer's output file (idempotent replay).
    ///
    /// # Errors
    ///
    /// Returns an error if the durable extractor state cannot be read or
    /// written, or if a resulting pair cannot be serialized or appended.
    pub fn process_and_write(
        &self,
        turn: TurnCapture<'_>,
        provenance: DpoPairProvenance<'_>,
    ) -> Result<bool> {
        let Some(mut pair) = self.extractor.process_turn(
            turn.session_id,
            turn.turn_number,
            turn.user_message,
            turn.assistant_response,
            turn.is_correction,
            turn.pii_filter_enabled,
        )?
        else {
            return Ok(false);
        };
        // WHY populated here, not inside `process_turn`: these are per-
        // completion facts (which model/provider served the chosen turn,
        // why the pending turn was flagged a correction, the prompt-audit
        // row for the chosen turn) that the session-scoped correction
        // extractor has no view of (#4863).
        pair.correction_reason = provenance.correction_reason.map(ToOwned::to_owned);
        pair.prompt_audit_ref = provenance.prompt_audit_ref.map(ToOwned::to_owned);
        pair.model = provenance.model.map(ToOwned::to_owned);
        pair.provider = provenance.provider.map(ToOwned::to_owned);
        pair.source_message_ids = vec![
            format!("{}:{}", turn.session_id, pair.rejected_turn),
            format!("{}:{}", turn.session_id, pair.chosen_turn),
        ];
        self.write_pair(&pair)
    }

    /// Write a single [`DpoPair`] as a JSON line to the output file.
    ///
    /// Idempotent on `pair.pair_id`: if a row with the same `pair_id`
    /// already exists in this writer's output file, the write is skipped
    /// and this returns `Ok(false)` without appending a duplicate line. A
    /// pair with an empty `pair_id` (rows written before pair identity
    /// existed) is never treated as a duplicate and is always appended.
    ///
    /// Returns `Ok(true)` when a new line was appended.
    ///
    /// # Errors
    ///
    /// Returns an error if the existing file cannot be read for the
    /// duplicate check, the file cannot be opened for appending, the pair
    /// cannot be serialized, or the write fails.
    pub fn write_pair(&self, pair: &DpoPair) -> Result<bool> {
        if self.contains_pair_id(&pair.pair_id)? {
            debug!(
                pair_id = pair.pair_id.as_str(),
                "dpo.pair_duplicate: skipping idempotent replay"
            );
            return Ok(false);
        }

        let mut line = serde_json::to_string(pair).context(SerializeSnafu)?;
        line.push('\n');

        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
            .context(OpenFileSnafu { path: &self.path })?;

        file.write_all(line.as_bytes())
            .context(WritePairSnafu { path: &self.path })?;

        Ok(true)
    }

    /// Whether a row with the given `pair_id` has already been written to
    /// this writer's output file.
    ///
    /// An empty `pair_id` never matches (rows written before pair identity
    /// existed carry no durable identity to dedupe against).
    ///
    /// WHY re-read the file on every call rather than cache an in-memory
    /// set: `DpoWriter` holds no handle between writes (see struct docs),
    /// so a fresh read also catches pairs written by a prior process
    /// instance for the same dated file — a crash/restart replay, not just
    /// duplicates within this process's lifetime. Malformed or legacy JSON
    /// lines are treated as non-matches rather than errors, so one corrupt
    /// row cannot block every future write.
    fn contains_pair_id(&self, pair_id: &str) -> Result<bool> {
        if pair_id.is_empty() {
            return Ok(false);
        }

        let content = match std::fs::read_to_string(&self.path) {
            Ok(content) => content,
            Err(source) if source.kind() == std::io::ErrorKind::NotFound => return Ok(false),
            Err(source) => return Err(source).context(ReadFileSnafu { path: &self.path }),
        };

        Ok(content.lines().any(|line| {
            serde_json::from_str::<DpoPair>(line).is_ok_and(|existing| existing.pair_id == pair_id)
        }))
    }

    /// Path to the current DPO JSONL output file.
    #[must_use]
    pub fn file_path(&self) -> &Path {
        &self.path
    }
}

/// Record a captured DPO pair in the metrics registry.
pub fn record_dpo_pair_captured(nous_id: &str) {
    crate::metrics::record_dpo_pair(nous_id);
}

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test assertions")]
mod tests {
    use super::*;

    fn extractor() -> DpoExtractor {
        DpoExtractor::open_in_memory().expect("open in-memory extractor")
    }

    #[test]
    fn extractor_emits_pair_on_correction_sequence() {
        let extractor = extractor();

        let p1 = extractor
            .process_turn(
                "ses-1",
                1,
                "What is the capital of France?",
                "London",
                false,
                false,
            )
            .expect("process");
        assert!(p1.is_none(), "single normal turn should not emit");

        let p2 = extractor
            .process_turn(
                "ses-1",
                2,
                "Actually, the capital of France is Paris.",
                "You are right.",
                true,
                false,
            )
            .expect("process");
        assert!(p2.is_none(), "correction turn should not emit");

        let p3 = extractor
            .process_turn(
                "ses-1",
                3,
                "What is the capital of France?",
                "Paris",
                false,
                false,
            )
            .expect("process");
        let pair = p3.expect("should emit pair after correction sequence");
        assert_eq!(pair.prompt, "What is the capital of France?");
        assert_eq!(pair.rejected, "London");
        assert_eq!(pair.chosen, "Paris");
        assert_eq!(pair.rejected_turn, 1);
        assert_eq!(pair.chosen_turn, 3);
        assert_eq!(pair.session_id, "ses-1");
    }

    #[test]
    fn extractor_emitted_pair_carries_schema_and_provenance_fields() {
        let extractor = extractor();

        let _ = extractor
            .process_turn(
                "ses-1",
                1,
                "What is the capital of France?",
                "London",
                false,
                true,
            )
            .expect("process");
        let _ = extractor
            .process_turn(
                "ses-1",
                2,
                "Actually, the capital of France is Paris.",
                "You are right.",
                true,
                true,
            )
            .expect("process");
        let pair = extractor
            .process_turn(
                "ses-1",
                3,
                "What is the capital of France?",
                "Paris",
                false,
                true,
            )
            .expect("process")
            .expect("should emit pair after correction sequence");

        assert_eq!(pair.schema_version, DPO_PAIR_SCHEMA_VERSION);
        assert_eq!(pair.pair_id, compute_pair_id("ses-1", 1, 3));
        assert_ne!(pair.pair_id, "", "a captured pair must carry a durable id");
        assert_eq!(
            pair.validator_version,
            Some(DPO_VALIDATOR_VERSION.to_owned())
        );
        assert_eq!(
            pair.pii_policy_ref,
            Some(crate::training::pii::POLICY_REF.to_owned()),
            "pii_filter_enabled=true must record the policy ref"
        );
    }

    #[test]
    fn extractor_emitted_pair_omits_pii_policy_ref_when_filter_disabled() {
        let extractor = extractor();

        let _ = extractor
            .process_turn(
                "ses-1",
                1,
                "What is the capital of France?",
                "London",
                false,
                false,
            )
            .expect("process");
        let _ = extractor
            .process_turn(
                "ses-1",
                2,
                "Actually, the capital of France is Paris.",
                "You are right.",
                true,
                false,
            )
            .expect("process");
        let pair = extractor
            .process_turn(
                "ses-1",
                3,
                "What is the capital of France?",
                "Paris",
                false,
                false,
            )
            .expect("process")
            .expect("should emit pair after correction sequence");

        assert_eq!(
            pair.pii_policy_ref, None,
            "pii_filter_enabled=false must not claim the full-suite policy ran"
        );
        // WHY: the always-on secret redactor still runs regardless of
        // pii_filter_enabled, but this pair carries no secret-shaped text,
        // so provenance is correctly empty rather than falsely populated.
        assert_eq!(
            pair.validator_version,
            Some(DPO_VALIDATOR_VERSION.to_owned())
        );
    }

    #[test]
    fn extractor_rejects_semantic_mismatch() {
        let extractor = extractor();

        let _ = extractor
            .process_turn(
                "ses-1",
                1,
                "What is the capital of France?",
                "London",
                false,
                false,
            )
            .expect("process");
        let _ = extractor
            .process_turn(
                "ses-1",
                2,
                "Actually, the capital of France is Paris.",
                "You are right.",
                true,
                false,
            )
            .expect("process");

        let p3 = extractor
            .process_turn(
                "ses-1",
                3,
                "What is the weather today?",
                "Sunny",
                false,
                false,
            )
            .expect("process");
        assert!(p3.is_none(), "semantic mismatch should not emit pair");
    }

    #[test]
    fn extractor_accepts_continuation_prompt() {
        let extractor = extractor();

        let _ = extractor
            .process_turn(
                "ses-1",
                1,
                "What is the capital of France?",
                "London",
                false,
                false,
            )
            .expect("process");
        let _ = extractor
            .process_turn(
                "ses-1",
                2,
                "Actually, the capital of France is Paris.",
                "You are right.",
                true,
                false,
            )
            .expect("process");

        let p3 = extractor
            .process_turn("ses-1", 3, "ok", "Paris", false, false)
            .expect("process");
        let pair = p3.expect("short continuation should pass validation");
        assert_eq!(pair.chosen, "Paris");
    }

    #[test]
    fn extractor_handles_multiple_sessions() {
        let extractor = extractor();

        let _ = extractor
            .process_turn("ses-a", 1, "Question A?", "Wrong A", false, false)
            .expect("process");
        let _ = extractor
            .process_turn("ses-a", 2, "Actually...", "Sorry.", true, false)
            .expect("process");

        let _ = extractor
            .process_turn("ses-b", 1, "Question B?", "Wrong B", false, false)
            .expect("process");
        let _ = extractor
            .process_turn("ses-b", 2, "No, it's...", "My mistake.", true, false)
            .expect("process");

        let pa = extractor
            .process_turn("ses-a", 3, "Question A?", "Right A", false, false)
            .expect("process");
        assert!(pa.is_some(), "session A should emit");

        let pb = extractor
            .process_turn("ses-b", 3, "Question B?", "Right B", false, false)
            .expect("process");
        assert!(pb.is_some(), "session B should emit");
    }

    #[test]
    fn extractor_handles_concurrent_sessions_across_threads() {
        // WHY(#5380): the durable store is shared (Arc<SingleWriterTxDatabase>
        // + one write_lock) across all sessions in a process. This proves
        // concurrent turn processing for DIFFERENT sessions on DIFFERENT
        // threads never loses or cross-contaminates another session's state.
        let extractor = Arc::new(extractor());
        let mut handles = Vec::new();

        for i in 0..8u64 {
            let extractor = Arc::clone(&extractor);
            handles.push(std::thread::spawn(move || {
                let session_id = format!("ses-thread-{i}");
                let prompt = format!("Question {i}?");
                let _ = extractor
                    .process_turn(&session_id, 1, &prompt, "Wrong", false, false)
                    .expect("process turn 1");
                let _ = extractor
                    .process_turn(&session_id, 2, "Actually...", "Sorry.", true, false)
                    .expect("process turn 2");
                let pair = extractor
                    .process_turn(&session_id, 3, &prompt, "Right", false, false)
                    .expect("process turn 3")
                    .expect("each thread's own session must emit its own pair");
                assert_eq!(pair.session_id, session_id);
                assert_eq!(pair.prompt, prompt);
            }));
        }

        for handle in handles {
            handle.join().expect("thread should not panic");
        }
    }

    #[test]
    fn extractor_repeated_processing_of_same_chosen_turn_is_idempotent() {
        // WHY(#5380): replaying the same "chosen turn" call twice (e.g. a
        // retried task) must not emit a second pair — pending is consumed
        // on the first successful match, so the extractor itself is
        // idempotent independent of the writer's pair_id dedup.
        let extractor = extractor();

        let _ = extractor
            .process_turn("ses-1", 1, "Question?", "Wrong", false, false)
            .expect("process");
        let _ = extractor
            .process_turn("ses-1", 2, "Actually...", "Sorry.", true, false)
            .expect("process");

        let first = extractor
            .process_turn("ses-1", 3, "Question?", "Right", false, false)
            .expect("process");
        assert!(first.is_some(), "first processing of turn 3 should emit");

        let replay = extractor
            .process_turn("ses-1", 3, "Question?", "Right", false, false)
            .expect("process");
        assert!(
            replay.is_none(),
            "replaying the same chosen turn must not re-emit — pending was already consumed"
        );
    }

    #[test]
    fn extractor_survives_restart_between_correction_and_chosen_response() {
        // WHY(#5380): the literal risk this durability fix closes — a
        // process restart between the correction turn and the chosen
        // response must not lose the pending pair.
        let dir = tempfile::tempdir().expect("tempdir");

        {
            let extractor = DpoExtractor::open(dir.path()).expect("open");
            let _ = extractor
                .process_turn(
                    "ses-1",
                    1,
                    "What is the capital of France?",
                    "London",
                    false,
                    false,
                )
                .expect("process turn 1");
            let _ = extractor
                .process_turn(
                    "ses-1",
                    2,
                    "Actually, the capital of France is Paris.",
                    "You are right.",
                    true,
                    false,
                )
                .expect("process turn 2 (correction)");
            // WHY: extractor is dropped here, simulating a process exit
            // before the chosen response arrives.
        }

        let reopened = DpoExtractor::open(dir.path()).expect("reopen after restart");
        let pair = reopened
            .process_turn(
                "ses-1",
                3,
                "What is the capital of France?",
                "Paris",
                false,
                false,
            )
            .expect("process turn 3 after restart")
            .expect("pending correction must survive the restart");

        assert_eq!(pair.rejected, "London");
        assert_eq!(pair.chosen, "Paris");
        assert_eq!(pair.rejected_turn, 1);
        assert_eq!(pair.chosen_turn, 3);
    }

    #[test]
    fn extractor_overwrites_pending_on_chained_corrections() {
        let extractor = extractor();

        let _ = extractor
            .process_turn("ses-1", 1, "Question?", "Wrong 1", false, false)
            .expect("process");
        let _ = extractor
            .process_turn("ses-1", 2, "Actually...", "Sorry.", true, false)
            .expect("process");
        let _ = extractor
            .process_turn("ses-1", 3, "No wait...", "I see.", true, false)
            .expect("process");

        // WHY: turn 2 was itself a correction, so no last_turn was cached and
        // the chained correction at turn 3 clears pending — turn 4 finds no
        // pending and must emit nothing.
        let p4 = extractor
            .process_turn("ses-1", 4, "Question?", "Right", false, false)
            .expect("process");
        assert!(
            p4.is_none(),
            "chained correction without intermediate answer should not emit"
        );
    }

    #[test]
    fn pending_correction_becomes_stale_past_max_age() {
        let now = jiff::Timestamp::now();
        let long_ago = now - pending_correction_max_age() - jiff::SignedDuration::from_secs(1);
        assert!(is_pending_stale_at(&long_ago.to_string(), now));

        let recent = now - jiff::SignedDuration::from_secs(60);
        assert!(!is_pending_stale_at(&recent.to_string(), now));
    }

    #[test]
    fn pending_correction_future_timestamp_is_stale() {
        // WHY: a pending_since after `now` is a clock anomaly, not a
        // legitimate recent correction — fail closed.
        let now = jiff::Timestamp::now();
        let future = now + jiff::SignedDuration::from_secs(60);
        assert!(is_pending_stale_at(&future.to_string(), now));
    }

    #[test]
    fn pending_correction_unparseable_timestamp_is_stale() {
        assert!(is_pending_stale_at(
            "not-a-timestamp",
            jiff::Timestamp::now()
        ));
    }

    #[test]
    fn extractor_rejects_pair_when_pending_correction_is_stale() {
        // WHY(#5381): durable pending state can now outlive a single
        // process, so an old pending correction must not silently pair
        // with an unrelated later turn just because the prompts happen to
        // overlap.
        let dir = tempfile::tempdir().expect("tempdir");
        let extractor = DpoExtractor::open(dir.path()).expect("open");

        let _ = extractor
            .process_turn(
                "ses-1",
                1,
                "What is the capital of France?",
                "London",
                false,
                false,
            )
            .expect("process turn 1");
        let _ = extractor
            .process_turn(
                "ses-1",
                2,
                "Actually, the capital of France is Paris.",
                "You are right.",
                true,
                false,
            )
            .expect("process turn 2 (correction)");

        // Directly age the just-written pending row past the staleness
        // window, bypassing the extractor's own clock.
        let pending_part = extractor.partition("pending").expect("partition");
        let mut tx = extractor.db.write_tx();
        let stale_since = (jiff::Timestamp::now()
            - pending_correction_max_age()
            - jiff::SignedDuration::from_secs(1))
        .to_string();
        let aged = PendingCorrection {
            prompt: "What is the capital of France?".to_owned(),
            rejected: "London".to_owned(),
            rejected_turn: 1,
            pending_since: stale_since,
        };
        write_state(&mut tx, &pending_part, "ses-1", &aged).expect("write aged pending");
        DpoExtractor::commit(tx, "test aging").expect("commit");

        let pair = extractor
            .process_turn(
                "ses-1",
                3,
                "What is the capital of France?",
                "Paris",
                false,
                false,
            )
            .expect("process turn 3");
        assert!(
            pair.is_none(),
            "a stale pending correction must not be paired, even with a matching prompt"
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
    fn semantic_mismatch_unrelated_short_prompt_does_not_bypass() {
        // WHY(#5381): a short message is not automatically a continuation —
        // only a known acknowledgement phrase or a pure reaction bypasses.
        assert!(!DpoExtractor::validate_semantic_match(
            "What is the capital of France?",
            "weather?"
        ));
        assert!(!DpoExtractor::validate_semantic_match(
            "What is the capital of France?",
            "new topic"
        ));
    }

    #[test]
    fn semantic_match_continuation_uses_char_count_not_bytes() {
        // WHY: 5 emoji are 5 Unicode scalars but 20 bytes. A byte-count
        // check would treat them as exactly the threshold and bypass
        // validation; a char-count check correctly treats them as short
        // (5 chars) and permits the continuation.
        assert!(DpoExtractor::validate_semantic_match(
            "What is the capital of France?",
            "👍👍👍👍👍"
        ));

        // 21 ASCII characters exceed the character threshold and have no
        // semantic overlap with the prompt, so validation must fail.
        assert!(!DpoExtractor::validate_semantic_match(
            "What is the capital of France?",
            "abcdefghijklmnopqrstu"
        ));
    }

    #[test]
    fn semantic_match_threshold_boundary_is_pinned_at_one_half() {
        // WHY(#5381): pins SEMANTIC_SIMILARITY_THRESHOLD's documented value
        // (0.5) against silent drift between the constant and its comment.
        // "a b" vs "a c": intersection={a}, union={a,b,c} -> 1/3 < 0.5 -> reject.
        assert!(!DpoExtractor::validate_semantic_match("a b", "a c"));
        // "a b" vs "a b c": intersection={a,b}, union={a,b,c} -> 2/3 >= 0.5 -> accept.
        assert!(DpoExtractor::validate_semantic_match("a b", "a b c"));
    }

    /// Builds a fully-populated pair for tests, mirroring what
    /// `DpoExtractor::process_turn` produces.
    fn sample_pair(session_id: &str, rejected_turn: u64, chosen_turn: u64) -> DpoPair {
        DpoPair {
            schema_version: DPO_PAIR_SCHEMA_VERSION,
            pair_id: compute_pair_id(session_id, rejected_turn, chosen_turn),
            prompt: "What is 2+2?".to_owned(),
            chosen: "4".to_owned(),
            rejected: "5".to_owned(),
            session_id: session_id.to_owned(),
            rejected_turn,
            chosen_turn,
            validator_version: Some(DPO_VALIDATOR_VERSION.to_owned()),
            pii_policy_ref: Some(crate::training::pii::POLICY_REF.to_owned()),
            similarity: Some(1.0),
            correction_reason: Some("actually,".to_owned()),
            prompt_audit_ref: Some("audit-ref-1".to_owned()),
            source_message_ids: vec![
                format!("{session_id}:{rejected_turn}"),
                format!("{session_id}:{chosen_turn}"),
            ],
            model: Some("claude-opus-4-20250514".to_owned()),
            provider: Some("anthropic".to_owned()),
        }
    }

    #[test]
    fn dpo_pair_serde_roundtrip() {
        let pair = sample_pair("ses-1", 1, 3);

        let json = serde_json::to_string(&pair).expect("serialize");
        let back: DpoPair = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back, pair);
    }

    #[test]
    fn dpo_pair_legacy_row_deserializes_with_defaults() {
        // WHY: rows written before pair identity existed have neither the
        // provenance fields nor `pair_id`/`schema_version`. A reader must
        // accept them rather than fail closed on the whole corpus.
        let legacy = r#"{
            "prompt": "What is 2+2?",
            "chosen": "4",
            "rejected": "5",
            "session_id": "ses-1",
            "rejected_turn": 1,
            "chosen_turn": 3
        }"#;

        let parsed: DpoPair = serde_json::from_str(legacy).expect("legacy row deserializes");
        assert_eq!(parsed.schema_version, 0, "legacy rows default to version 0");
        assert_eq!(parsed.pair_id, "", "legacy rows have no durable pair_id");
        assert_eq!(parsed.validator_version, None);
        assert_eq!(parsed.pii_policy_ref, None);
        assert_eq!(parsed.prompt, "What is 2+2?");
    }

    #[test]
    fn dpo_pair_id_is_stable_and_derived_from_identity() {
        let a = compute_pair_id("ses-1", 1, 3);
        let b = compute_pair_id("ses-1", 1, 3);
        assert_eq!(a, b, "same session/turn identity must yield the same id");

        assert_ne!(
            a,
            compute_pair_id("ses-2", 1, 3),
            "different session must yield a different id"
        );
        assert_ne!(
            a,
            compute_pair_id("ses-1", 2, 3),
            "different rejected_turn must yield a different id"
        );
        assert_ne!(
            a,
            compute_pair_id("ses-1", 1, 4),
            "different chosen_turn must yield a different id"
        );

        // WHY: guards against naive string-concatenation collisions across
        // field boundaries (session "ses-1" turns 1,23 vs session "ses-11"
        // turns 2,3 must not collide just because their concatenation would).
        assert_ne!(
            compute_pair_id("ses-1", 1, 23),
            compute_pair_id("ses-11", 2, 3),
            "field boundaries must not be confusable via concatenation"
        );
    }

    #[test]
    fn dpo_writer_creates_file() {
        let dir = tempfile::tempdir().expect("tempdir");
        let writer = DpoWriter::new(dir.path()).expect("new");
        assert!(writer.file_path().to_string_lossy().ends_with(".jsonl"));
    }

    #[test]
    fn dpo_writer_dir_recovers_the_constructor_argument() {
        let dir = tempfile::tempdir().expect("tempdir");
        let writer = DpoWriter::new(dir.path()).expect("new");
        assert_eq!(writer.dir(), dir.path());
    }

    #[test]
    fn dpo_writer_appends_jsonl() {
        let dir = tempfile::tempdir().expect("tempdir");
        let writer = DpoWriter::new(dir.path()).expect("new");

        let pair = sample_pair("ses-1", 1, 3);
        let other_pair = sample_pair("ses-1", 5, 7);
        writer.write_pair(&pair).expect("write");
        writer.write_pair(&other_pair).expect("write");

        let content = std::fs::read_to_string(writer.file_path()).expect("read");
        let lines: Vec<&str> = content.lines().collect();
        assert_eq!(lines.len(), 2, "two distinct pairs must produce two lines");

        let parsed: DpoPair =
            serde_json::from_str(lines.first().expect("first line")).expect("parse");
        assert_eq!(parsed.prompt, "What is 2+2?");
        assert_eq!(parsed.chosen, "4");
        assert_eq!(parsed.rejected, "5");
    }

    #[test]
    fn dpo_writer_write_pair_is_idempotent_for_duplicate_pair_id() {
        // WHY(#5386): a replayed write of the same correction sequence (e.g.
        // after a crash-restart) must not duplicate the pair in the corpus.
        let dir = tempfile::tempdir().expect("tempdir");
        let writer = DpoWriter::new(dir.path()).expect("new");
        let pair = sample_pair("ses-1", 1, 3);

        writer.write_pair(&pair).expect("first write");
        writer.write_pair(&pair).expect("replayed write");
        writer.write_pair(&pair).expect("second replayed write");

        let content = std::fs::read_to_string(writer.file_path()).expect("read");
        let lines: Vec<&str> = content.lines().collect();
        assert_eq!(
            lines.len(),
            1,
            "three writes of the same pair_id must produce exactly one line"
        );
    }

    #[test]
    fn dpo_writer_legacy_empty_pair_id_is_never_deduped() {
        // WHY: an empty pair_id (rows written before pair identity existed,
        // or a legacy row read back) carries no durable identity, so it
        // must never suppress a write — otherwise the second real pair
        // silently vanishes.
        let dir = tempfile::tempdir().expect("tempdir");
        let writer = DpoWriter::new(dir.path()).expect("new");
        let mut pair = sample_pair("ses-1", 1, 3);
        pair.pair_id = String::new();

        writer.write_pair(&pair).expect("first write");
        writer.write_pair(&pair).expect("second write");

        let content = std::fs::read_to_string(writer.file_path()).expect("read");
        assert_eq!(content.lines().count(), 2);
    }

    #[test]
    fn dpo_writer_process_and_write_emits_and_appends_on_correction_sequence() {
        let dir = tempfile::tempdir().expect("tempdir");
        let writer = DpoWriter::new(dir.path()).expect("new");

        let wrote1 = writer
            .process_and_write(
                TurnCapture {
                    session_id: "ses-1",
                    turn_number: 1,
                    user_message: "What is the capital of France?",
                    assistant_response: "London",
                    is_correction: false,
                    pii_filter_enabled: false,
                },
                DpoPairProvenance::default(),
            )
            .expect("process turn 1");
        assert!(!wrote1, "single normal turn should not write a pair");

        let wrote2 = writer
            .process_and_write(
                TurnCapture {
                    session_id: "ses-1",
                    turn_number: 2,
                    user_message: "Actually, the capital of France is Paris.",
                    assistant_response: "You are right.",
                    is_correction: true,
                    pii_filter_enabled: false,
                },
                DpoPairProvenance::default(),
            )
            .expect("process turn 2");
        assert!(!wrote2, "correction turn should not write a pair");

        let wrote3 = writer
            .process_and_write(
                TurnCapture {
                    session_id: "ses-1",
                    turn_number: 3,
                    user_message: "What is the capital of France?",
                    assistant_response: "Paris",
                    is_correction: false,
                    pii_filter_enabled: false,
                },
                DpoPairProvenance {
                    correction_reason: Some("actually,"),
                    prompt_audit_ref: Some("audit-ref-3"),
                    model: Some("test-model"),
                    provider: Some("test-provider"),
                },
            )
            .expect("process turn 3");
        assert!(wrote3, "chosen turn should write a pair");

        let content = std::fs::read_to_string(writer.file_path()).expect("read");
        assert_eq!(content.lines().count(), 1);
        let parsed: DpoPair =
            serde_json::from_str(content.lines().next().expect("one line")).expect("parse");
        assert_eq!(parsed.chosen, "Paris");
        assert_eq!(parsed.correction_reason.as_deref(), Some("actually,"));
        assert_eq!(parsed.model.as_deref(), Some("test-model"));
        assert_eq!(parsed.provider.as_deref(), Some("test-provider"));
        assert_eq!(
            parsed.source_message_ids,
            vec!["ses-1:1".to_owned(), "ses-1:3".to_owned()]
        );
    }

    #[test]
    fn dpo_writer_process_and_write_reports_false_on_idempotent_replay() {
        // WHY: pair_id is derived only from session_id/rejected_turn/
        // chosen_turn (see compute_pair_id), so a pair already durably
        // written by a prior process instance can collide with a fresh
        // extraction that independently derives the same identity. This
        // pre-seeds that collision directly (bypassing the extractor) and
        // proves process_and_write's `was_new` check catches it rather
        // than duplicating the row.
        let dir = tempfile::tempdir().expect("tempdir");
        let writer = DpoWriter::new(dir.path()).expect("new");

        let existing = sample_pair("ses-1", 1, 3);
        writer
            .write_pair(&existing)
            .expect("pre-seed existing pair");

        let _ = writer
            .process_and_write(
                TurnCapture {
                    session_id: "ses-1",
                    turn_number: 1,
                    user_message: "What is 2+2?",
                    assistant_response: "5",
                    is_correction: false,
                    pii_filter_enabled: false,
                },
                DpoPairProvenance::default(),
            )
            .expect("process turn 1");
        let _ = writer
            .process_and_write(
                TurnCapture {
                    session_id: "ses-1",
                    turn_number: 2,
                    user_message: "Actually, 2+2 is 4.",
                    assistant_response: "You are right.",
                    is_correction: true,
                    pii_filter_enabled: false,
                },
                DpoPairProvenance::default(),
            )
            .expect("process turn 2 (correction)");
        let wrote = writer
            .process_and_write(
                TurnCapture {
                    session_id: "ses-1",
                    turn_number: 3,
                    user_message: "What is 2+2?",
                    assistant_response: "4",
                    is_correction: false,
                    pii_filter_enabled: false,
                },
                DpoPairProvenance::default(),
            )
            .expect("process turn 3");

        assert!(
            !wrote,
            "extractor emits a fresh pair, but its pair_id already exists on disk — must report false, not duplicate"
        );

        let content = std::fs::read_to_string(writer.file_path()).expect("read");
        assert_eq!(
            content.lines().count(),
            1,
            "the pre-seeded row must remain the only line"
        );
    }

    #[test]
    fn extractor_redacts_secret_when_full_pii_suite_disabled() {
        let extractor = extractor();

        // WHY: split/concat so the full synthetic key string never appears as a
        // raw literal that credential scanners could flag.
        let secret = concat!("sk-", "ant-", "api03-", "abc123def456");
        let prompt = format!("Why does api_key={secret} fail?");
        let rejected = format!("The key {secret} is invalid");
        let correction = "Actually the key format is wrong".to_owned();
        let chosen = format!("Use {secret} with the v3 header");

        let p1 = extractor
            .process_turn("ses-1", 1, &prompt, &rejected, false, false)
            .expect("process");
        assert!(p1.is_none(), "single normal turn should not emit");

        let p2 = extractor
            .process_turn("ses-1", 2, &correction, "You are right.", true, false)
            .expect("process");
        assert!(p2.is_none(), "correction turn should not emit");

        let p3 = extractor
            .process_turn("ses-1", 3, &prompt, &chosen, false, false)
            .expect("process");
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

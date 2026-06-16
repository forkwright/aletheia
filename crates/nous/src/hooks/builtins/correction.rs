//! Correction hooks: capture operator corrections and inject them into future turns.
//!
//! Two hooks work together:
//! - [`CorrectionDetector`]: runs in `on_turn_complete` (currently a no-op
//!   placeholder; detection happens in [`CorrectionInjector::before_query`]).
//! - [`CorrectionInjector`]: runs in `before_query` to detect corrections in the
//!   user message, persist them as typed [`CorrectionRecord`]s, and inject
//!   active corrections scoped to the current `nous_id`/`session_id` into the
//!   system prompt.
//!
//! Storage format: `<workspace>/corrections.json` — a JSON array of typed
//! [`CorrectionRecord`] entries. Writes are serialized with a module-local lock
//! and committed atomically via a temporary file + rename so concurrent readers
//! always see a complete file.

use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU64, Ordering};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use taxis::config::AgentBehaviorDefaults;
use tokio::sync::Mutex as TokioMutex;
use tracing::{debug, warn};

use crate::hooks::{HookResult, QueryContext, TurnContext, TurnHook};

/// Filename for the corrections store within the agent workspace.
const CORRECTIONS_FILENAME: &str = "corrections.json";

/// Module-local write lock to serialize read-modify-write cycles on the
/// corrections file. Keeps locking simple and scoped to this module.
static WRITE_LOCK: OnceLock<TokioMutex<()>> = OnceLock::new();

fn write_lock() -> &'static TokioMutex<()> {
    WRITE_LOCK.get_or_init(|| TokioMutex::new(()))
}

/// Monotonic suffix for temporary correction files.
static TMP_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Stable identifier for a correction record.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub(crate) struct CorrectionId(String);

impl CorrectionId {
    /// Derive a stable ID from the scope and source hash.
    pub(crate) fn for_scope(nous_id: &str, session_id: &str, source_hash: &str) -> Self {
        let input = format!("{nous_id}:{session_id}:{source_hash}");
        let digest = sha256_hex(input.as_bytes());
        let prefix: String = digest.chars().take(16).collect();
        Self(format!("corr-{prefix}"))
    }
}

/// Lifecycle status of a correction record.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub(crate) enum CorrectionStatus {
    /// Correction is active and should be injected.
    #[default]
    Active,
    /// Correction has been dismissed and should not be injected.
    Dismissed,
}

/// A persisted behavioral correction from the operator.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct CorrectionRecord {
    /// Stable record identifier.
    pub id: CorrectionId,
    /// Agent (`nous`) this correction belongs to.
    pub nous_id: String,
    /// Session this correction was recorded in.
    pub session_id: String,
    /// Turn number within the session when the correction was recorded.
    pub turn_number: u64,
    /// Stable hash of the full source message used for replay deduplication.
    pub source_hash: String,
    /// Monotonic revision counter, incremented on status transitions.
    pub revision: u64,
    /// Current lifecycle status.
    pub status: CorrectionStatus,
    /// Provenance: the original (possibly truncated) user message.
    pub source_message: String,
    /// The extracted correction text.
    pub text: String,
    /// ISO 8601 timestamp when the correction was recorded.
    pub created_at: String,
}

impl CorrectionRecord {
    /// Create a new active correction record.
    pub(crate) fn new(
        nous_id: impl Into<String>,
        session_id: impl Into<String>,
        turn_number: u64,
        text: impl Into<String>,
        source_message: impl Into<String>,
    ) -> Self {
        let nous_id = nous_id.into();
        let session_id = session_id.into();
        let text = text.into();
        let source_message_full = source_message.into();
        let source_hash = sha256_hex(source_message_full.as_bytes());
        let id = CorrectionId::for_scope(&nous_id, &session_id, &source_hash);

        Self {
            id,
            nous_id,
            session_id,
            turn_number,
            source_hash,
            revision: 0,
            status: CorrectionStatus::Active,
            source_message: truncate_source(&source_message_full),
            text,
            created_at: jiff::Timestamp::now().to_string(),
        }
    }

    /// Transition the record to a new status, bumping the revision when the
    /// status actually changes.
    #[cfg(test)]
    pub(crate) fn transition_to(&mut self, status: CorrectionStatus) {
        if self.status != status {
            self.status = status;
            self.revision += 1;
        }
    }
}

/// Detects correction patterns in user messages and persists them.
///
/// Runs in `on_turn_complete` (after the LLM responds). The primary detection
/// path lives in [`CorrectionInjector::before_query`] because `QueryContext`
/// carries the user message.
pub(crate) struct CorrectionDetector {
    /// Path to the agent workspace directory.
    ///
    /// WHY: Reserved for future use when the detector gains the ability to
    /// extract corrections from multi-turn patterns in `on_turn_complete`.
    #[expect(
        dead_code,
        reason = "reserved for future on_turn_complete correction extraction"
    )]
    workspace: PathBuf,
}

impl CorrectionDetector {
    /// Create a new correction detector that stores corrections in the given workspace.
    pub(crate) fn new(workspace: PathBuf) -> Self {
        Self { workspace }
    }
}

impl TurnHook for CorrectionDetector {
    fn name(&self) -> &'static str {
        "correction_detector"
    }

    fn on_turn_complete<'a>(
        &'a self,
        context: &'a TurnContext<'_>,
    ) -> Pin<Box<dyn Future<Output = HookResult> + Send + 'a>> {
        Box::pin(async move {
            debug!(
                nous_id = context.nous_id,
                "correction_detector: turn complete, no-op in on_turn_complete"
            );

            HookResult::Continue
        })
    }
}

/// Reads persisted corrections and injects them into the system prompt.
///
/// Runs in `before_query` (before the model call). Also detects new corrections
/// from the current user message and persists them before injection.
///
/// WHY: Combined detect+inject in `before_query` because:
/// 1. [`QueryContext`] has the user message ([`TurnContext`] does not)
/// 2. Corrections from this turn should apply starting from this turn
/// 3. Single file read/write per turn instead of two
pub(crate) struct CorrectionInjector {
    /// Path to the agent workspace directory.
    workspace: PathBuf,
}

impl CorrectionInjector {
    /// Create a new correction injector that reads corrections from the given workspace.
    pub(crate) fn new(workspace: PathBuf) -> Self {
        Self { workspace }
    }
}

impl TurnHook for CorrectionInjector {
    fn name(&self) -> &'static str {
        "correction_injector"
    }

    fn before_query<'a>(
        &'a self,
        context: &'a mut QueryContext<'_>,
    ) -> Pin<Box<dyn Future<Output = HookResult> + Send + 'a>> {
        Box::pin(async move {
            let user_message = context.user_message;

            if let Some(correction_text) = extract_correction(user_message) {
                debug!(
                    nous_id = context.nous_id,
                    correction = correction_text.as_str(),
                    "correction_injector: detected correction in user message"
                );

                let record = CorrectionRecord::new(
                    context.nous_id,
                    context.session_id,
                    context.turn_number,
                    correction_text,
                    user_message,
                );

                if let Err(e) = persist_correction(&self.workspace, record).await {
                    warn!(
                        nous_id = context.nous_id,
                        error = %e,
                        "correction_injector: failed to persist correction"
                    );
                    // WHY: Non-fatal — continue without persisting. The correction
                    // still applies this turn via the system prompt injection below.
                }
            }

            let corrections = match load_corrections(
                &self.workspace,
                context.nous_id,
                context.session_id,
            )
            .await
            {
                Ok(c) => c,
                Err(e) => {
                    debug!(
                        nous_id = context.nous_id,
                        error = %e,
                        "correction_injector: no corrections file or read error, skipping injection"
                    );
                    return HookResult::Continue;
                }
            };

            if corrections.is_empty() {
                return HookResult::Continue;
            }

            let section = format_corrections_section(&corrections);
            let token_estimate = section.len() / 4; // conservative: ~4 chars per token

            // WHY: Check remaining token budget before injecting. Corrections
            // should not crowd out conversation history.
            #[expect(
                clippy::cast_possible_wrap,
                clippy::as_conversions,
                reason = "usize→i64: token estimate fits in i64 for practical prompt sizes"
            )]
            let estimate_i64 = token_estimate as i64; // kanon:ignore RUST/as-cast
            if context.pipeline.remaining_tokens < estimate_i64 * 2 {
                debug!(
                    nous_id = context.nous_id,
                    remaining = context.pipeline.remaining_tokens,
                    correction_tokens = token_estimate,
                    "correction_injector: skipping injection, insufficient token budget"
                );
                return HookResult::Continue;
            }

            if let Some(ref mut prompt) = context.pipeline.system_prompt {
                prompt.push_str("\n\n");
                prompt.push_str(&section);
            }

            context.pipeline.remaining_tokens -= estimate_i64;

            debug!(
                nous_id = context.nous_id,
                correction_count = corrections.len(),
                token_estimate,
                "correction_injector: injected corrections into system prompt"
            );

            HookResult::Continue
        })
    }
}

// -- Correction detection --

/// Extract a correction from a user message, if one is detected.
///
/// Returns `Some(correction_text)` if the message contains a correction pattern,
/// or `None` if no correction is detected.
fn extract_correction(message: &str) -> Option<String> {
    let lower = message.to_lowercase();

    // WHY: Check each sentence independently. A multi-sentence message might
    // contain a correction in one sentence and a question in another.
    for sentence in split_sentences(message) {
        let sentence_lower = sentence.to_lowercase();
        let trimmed = sentence_lower.trim();

        for prefix in aletheia_lexica::prefixes::CORRECTION_PREFIXES {
            if trimmed.starts_with(prefix) {
                // WHY: return the original-case sentence, not the lowercased match.
                return Some(sentence.trim().to_owned());
            }
        }

        // WHY: Also check for mid-sentence correction patterns like
        // "I want you to never X" or "going forward, always Y".
        if (lower.contains("going forward") || lower.contains("in the future"))
            && (trimmed.contains("always ") || trimmed.contains("never "))
        {
            return Some(sentence.trim().to_owned());
        }
    }

    None
}

/// Split text into sentences on common delimiters.
///
/// WHY: Lightweight sentence splitting — no NLP dependency. Handles `.`, `!`, `?`
/// followed by whitespace or end-of-string. Good enough for correction detection.
fn split_sentences(text: &str) -> Vec<&str> {
    let mut sentences = Vec::new();
    let mut start = 0;

    for (i, c) in text.char_indices() {
        if matches!(c, '.' | '!' | '?') {
            let end = i + c.len_utf8();
            // WHY: char_indices yields valid byte boundaries, so these slices
            // are guaranteed safe. get() satisfies clippy's indexing lint.
            let rest = text.get(end..).unwrap_or("");
            if rest.is_empty() || rest.starts_with(char::is_whitespace) {
                if let Some(sentence) = text.get(start..end)
                    && !sentence.trim().is_empty()
                {
                    sentences.push(sentence);
                }
                start = end;
            }
        }
    }

    // NOTE: trailing text without terminal punctuation is still a sentence.
    if let Some(remainder) = text.get(start..)
        && !remainder.trim().is_empty()
    {
        sentences.push(remainder);
    }

    sentences
}

/// Truncate the source message for storage. Keeps approximately the first 200 characters.
///
/// WHY: Uses `char_indices` to find a char boundary near 200 bytes to avoid
/// panicking on multi-byte UTF-8 characters.
fn truncate_source(message: &str) -> String {
    if message.len() <= 200 {
        return message.to_owned();
    }

    let boundary = message
        .char_indices()
        .take_while(|(i, _)| *i <= 200)
        .last()
        .map_or(0, |(i, _)| i);

    let mut s = message.get(..boundary).unwrap_or(message).to_owned();
    s.push_str("...");
    s
}

// -- Persistence --

/// Path to the corrections file within a workspace.
fn corrections_path(workspace: &Path) -> PathBuf {
    workspace.join(CORRECTIONS_FILENAME)
}

/// Load active corrections for a specific scope from the workspace file.
///
/// Returns an empty vec if the file does not exist. Returns an error only
/// on actual I/O or parse failures. Non-active records are skipped.
async fn load_corrections(
    workspace: &Path,
    nous_id: &str,
    session_id: &str,
) -> Result<Vec<CorrectionRecord>, std::io::Error> {
    let records = load_all_records(workspace).await?;
    Ok(records
        .into_iter()
        .filter(|record| {
            record.status == CorrectionStatus::Active
                && record.nous_id == nous_id
                && record.session_id == session_id
        })
        .collect())
}

/// Load all correction records from the workspace file without filtering.
async fn load_all_records(workspace: &Path) -> Result<Vec<CorrectionRecord>, std::io::Error> {
    let path = corrections_path(workspace);

    match tokio::fs::read_to_string(&path).await {
        Ok(content) => {
            let records: Vec<CorrectionRecord> = serde_json::from_str(&content).map_err(|e| {
                std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!("invalid corrections JSON: {e}"),
                )
            })?;
            Ok(records)
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(e) => Err(e),
    }
}

/// Persist a correction, preventing replay duplicates by source hash/scope.
///
/// Holds a module-local lock while reading, deduplicating, capping, and
/// atomically writing the file.
async fn persist_correction(
    workspace: &Path,
    correction: CorrectionRecord,
) -> Result<(), std::io::Error> {
    let path = corrections_path(workspace);
    let _guard = write_lock().lock().await;

    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }

    let mut records = match load_all_records(workspace).await {
        Ok(records) => records,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Vec::new(),
        Err(e) => return Err(e),
    };

    // Prevent replay duplicates scoped to (nous_id, session_id, source_hash).
    let duplicate = records.iter().any(|record| {
        record.nous_id == correction.nous_id
            && record.session_id == correction.session_id
            && record.source_hash == correction.source_hash
    });
    if duplicate {
        return Ok(());
    }

    records.push(correction);

    // WHY: Evict oldest corrections when over the cap. Operator's most recent
    // corrections are more likely to be relevant.
    let max_corrections = AgentBehaviorDefaults::default().corrections_max_corrections;
    debug!(max_corrections, "corrections cap enforced");
    if records.len() > max_corrections {
        let excess = records.len() - max_corrections;
        records.drain(..excess);
    }

    write_corrections_atomic(&path, &records).await
}

/// Serialize records to a temporary file and atomically rename it into place.
async fn write_corrections_atomic(
    path: &Path,
    records: &[CorrectionRecord],
) -> Result<(), std::io::Error> {
    let json = serde_json::to_string_pretty(records)
        .map_err(|e| std::io::Error::other(format!("failed to serialize corrections: {e}")))?;

    let parent = path.parent().ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "corrections path has no parent directory",
        )
    })?;

    let suffix = TMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    let tmp_name = format!("{CORRECTIONS_FILENAME}.tmp.{suffix}");
    let tmp_path = parent.join(&tmp_name);

    tokio::fs::write(&tmp_path, json).await?;
    tokio::fs::rename(&tmp_path, path).await?;

    Ok(())
}

// -- Hashing --

/// Compute a stable hex-encoded SHA-256 hash of the given input.
fn sha256_hex(input: impl AsRef<[u8]>) -> String {
    let mut hasher = Sha256::new();
    hasher.update(input);
    hex_encode(&hasher.finalize())
}

/// Encode bytes as lowercase hex without external dependencies.
fn hex_encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(nibble_to_hex(byte >> 4));
        out.push(nibble_to_hex(byte & 0x0F));
    }
    out
}

fn nibble_to_hex(n: u8) -> char {
    if n < 10 {
        char::from(b'0' + n)
    } else {
        char::from(b'a' + n - 10)
    }
}

// -- System prompt formatting --

/// Format corrections into a system prompt section.
fn format_corrections_section(corrections: &[CorrectionRecord]) -> String {
    let mut section = String::from(
        "## Operator Corrections\n\n\
         The following behavioral corrections have been recorded by the operator. \
         Follow these instructions exactly:\n\n",
    );

    for (i, correction) in corrections.iter().enumerate() {
        use std::fmt::Write as _;
        // kanon:ignore RUST/no-silent-result-swallow — writeln! on String is infallible
        let _ = writeln!(section, "{}. {}", i + 1, correction.text);
    }

    section
}

#[cfg(test)]
#[path = "correction_tests.rs"]
mod correction_tests;

//! Prompt audit log: operator-visible record of outbound model requests.
//!
//! Append-only JSONL log at `{instance}/logs/prompt-audit/YYYY-MM-DD.jsonl`.
//! Rotated daily based on the UTC calendar date of the record timestamp.
//!
//! # Sovereignty contract
//!
//! The operator owns this system and must be able to audit what it sends to
//! external LLM providers. To keep the audit surface small and avoid turning
//! the log into a second copy of user content, the record schema only stores:
//!
//! - Identifiers (nous/session/turn/request/LLM request)
//! - Provider + model
//! - Loop iteration and cache/schema mode
//! - A **hash** of the system prompt (not the content)
//! - The system prompt's byte length
//! - Message count + token estimate
//! - Fact IDs included via recall (not fact content)
//! - Fact IDs filtered out by sensitivity policy
//! - Tool names and prior tool-result IDs (not tool inputs)
//!
//! Never logged: system prompt content, user message text, tool call
//! arguments. The hash lets operators correlate audit records with a known
//! prompt without storing the prompt itself.
//!
//! # Retention
//!
//! Daily files are pruned by
//! `oikonomos::maintenance::prompt_audit_rotation::PromptAuditRotator`,
//! configured via `taxis::config::PromptAuditSettings::retention_days`.

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
// kanon:ignore RUST/std-mutex-in-async — PromptAuditLog methods are synchronous; std::sync::Mutex is correct for sync code
use std::sync::Mutex;

use jiff::Timestamp;
use jiff::civil::Date;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use snafu::{ResultExt, Snafu};
use tracing::{debug, warn};

use hermeneus::types::{CompletionRequest, Content, ContentBlock};

/// Errors from prompt audit log operations.
#[derive(Debug, Snafu)]
#[snafu(visibility(pub(crate)))]
#[non_exhaustive]
#[expect(
    missing_docs,
    reason = "snafu error variant fields (source, path) are self-documenting via display format"
)]
// kanon:ignore RUST/non-exhaustive-enum — already #[non_exhaustive]; false positive from attribute ordering
pub enum PromptAuditError {
    /// Failed to create the audit log directory.
    #[snafu(display("failed to create prompt audit directory {}: {source}", path.display()))]
    CreateDir {
        path: PathBuf,
        source: std::io::Error,
    },

    /// Failed to open the daily JSONL file for appending.
    #[snafu(display("failed to open prompt audit file {}: {source}", path.display()))]
    OpenFile {
        path: PathBuf,
        source: std::io::Error,
    },

    /// Failed to serialize a record to JSON.
    #[snafu(display("failed to serialize prompt audit record: {source}"))]
    Serialize { source: serde_json::Error },

    /// Failed to write a record to the JSONL file.
    #[snafu(display("failed to write prompt audit record to {}: {source}", path.display()))]
    WriteRecord {
        path: PathBuf,
        source: std::io::Error,
    },
}

/// Result alias for prompt audit operations.
pub type Result<T> = std::result::Result<T, PromptAuditError>;

/// Deployment target classification for a request.
pub type DeploymentTarget = String;

/// Sensitivity classification of a fact that was filtered from recall.
pub type FactSensitivity = String;

/// A single fact that was filtered out by the sensitivity policy.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FilteredFact {
    /// Fact identifier from the knowledge store.
    // kanon:ignore RUST/primitive-for-domain-id — existing String-based ID; migrating to newtype requires cross-crate API changes
    pub id: String,
    /// Sensitivity tier that caused the filter to exclude the fact.
    pub sensitivity: FactSensitivity,
}

/// Prompt-audit record construction options resolved from operator config.
#[derive(Debug, Clone, Copy)]
pub struct PromptAuditRecordOptions {
    /// Whether filtered fact identifiers are persisted in the audit row.
    pub include_filtered_ids: bool,
}

impl Default for PromptAuditRecordOptions {
    fn default() -> Self {
        Self {
            include_filtered_ids: true,
        }
    }
}

impl From<&taxis::config::PromptAuditSettings> for PromptAuditRecordOptions {
    fn from(settings: &taxis::config::PromptAuditSettings) -> Self {
        Self {
            include_filtered_ids: settings.include_filtered_ids,
        }
    }
}

/// Provider-side cache controls applied to an outbound completion request.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PromptAuditCachePolicy {
    /// Whether the request asked the provider to cache the system prompt.
    #[serde(default)]
    pub cache_system: bool,
    /// Whether the request asked the provider to cache the tool declaration block.
    #[serde(default)]
    pub cache_tools: bool,
    /// Whether the request asked the provider to cache recent conversation turns.
    #[serde(default)]
    pub cache_turns: bool,
}

/// One append-only audit record per outbound completion request.
///
/// See module docs for the sovereignty contract on what is and is not logged.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromptAuditRecord {
    /// When the request was assembled (UTC).
    pub timestamp: Timestamp,
    /// Nous agent identifier (e.g. `"syn"`).
    // kanon:ignore RUST/primitive-for-domain-id — existing String-based ID; migrating to newtype requires cross-crate API changes
    pub nous_id: String,
    /// Session identifier.
    // kanon:ignore RUST/primitive-for-domain-id — existing String-based ID; migrating to newtype requires cross-crate API changes
    pub session_id: String,
    /// Turn identifier (ULID). Stable across actor restarts for a given turn.
    // kanon:ignore RUST/primitive-for-domain-id — existing String-based ID; migrating to newtype requires cross-crate API changes
    pub turn_id: String,
    /// Gateway/request correlation identifier propagated from the caller.
    #[serde(default)]
    pub request_id: Option<String>,
    /// Locally generated identifier for this outbound completion request.
    #[serde(default)]
    // kanon:ignore RUST/primitive-for-domain-id — serialization envelope; introducing a newtype would widen the public schema change
    pub completion_request_id: String,
    /// Execute-loop iteration that assembled this completion request.
    #[serde(default)]
    pub loop_iteration: u32,
    /// LLM provider name (`"anthropic"`, `"cc"`, etc.).
    pub provider: String,
    /// Deployment target (cloud/local/…). See [`DeploymentTarget`].
    pub deployment_target: DeploymentTarget,
    /// Selected model identifier that served the turn.
    pub model: String,
    /// Provider response/message identifier, when the provider returned one.
    #[serde(default)]
    pub provider_response_id: Option<String>,
    /// SHA-256 of the system prompt (hex). Empty string when no system prompt.
    pub system_prompt_hash: String,
    /// Byte length of the system prompt.
    pub system_prompt_bytes: usize,
    /// Number of conversation messages in the request.
    pub message_count: usize,
    /// Rough token count estimate for the request.
    pub token_count_estimate: u32,
    /// Fact IDs whose content was included in the recall section.
    pub fact_ids_included: Vec<String>,
    /// Facts excluded from recall by the sensitivity filter (#3404).
    #[serde(default)]
    pub fact_ids_filtered: Vec<FilteredFact>,
    /// Names of tools exposed to the model for this request.
    pub tool_names: Vec<String>,
    /// Tool-call IDs whose results were present in this outbound request.
    #[serde(default)]
    pub tool_result_ids: Vec<String>,
    /// Opaque hash of the effective tool surface exposed to the model.
    #[serde(default)]
    pub tool_surface_hash: String,
    /// Provider-side cache controls applied to this completion request.
    #[serde(default, flatten)]
    pub cache: PromptAuditCachePolicy,
    /// Whether provider tools were sent with deferred schema placeholders.
    #[serde(default)]
    pub deferred_schemas: bool,
}

/// Compute the SHA-256 hex digest of a system prompt.
///
/// Returns the empty string for `None` so the JSON field stays a consistent
/// string type across records.
#[must_use]
pub fn hash_system_prompt(prompt: Option<&str>) -> String {
    match prompt {
        Some(s) => {
            let mut hasher = Sha256::new();
            hasher.update(s.as_bytes());
            let digest = hasher.finalize();
            let mut out = String::with_capacity(digest.len() * 2);
            for byte in &digest {
                use std::fmt::Write;
                // kanon:ignore RUST/no-silent-result-swallow — write! on String is infallible
                let _ = write!(out, "{byte:02x}");
            }
            out
        }
        None => String::new(),
    }
}

/// Append-only prompt audit log with daily rotation.
///
/// # Threading
///
/// Cloneable by sharing the same `Arc<Mutex<...>>` via [`PromptAuditLog::clone`].
/// Internal state is guarded by a `Mutex`; contention is negligible because
/// each write is a serialize + append of a few KB.
#[derive(Debug)]
pub struct PromptAuditLog {
    inner: Mutex<PromptAuditLogInner>,
    /// Whether logging is active. When `false`, [`PromptAuditLog::log_request`]
    /// is a no-op that does not touch the filesystem.
    enabled: bool,
    log_dir: PathBuf,
    record_options: PromptAuditRecordOptions,
}

#[derive(Debug)]
struct PromptAuditLogInner {
    /// Currently open day file and its calendar date.
    current: Option<(Date, File)>,
}

impl PromptAuditLog {
    /// Create a new audit log writing to `log_dir`.
    ///
    /// The directory is created lazily on first write. `enabled = false`
    /// returns a log handle that drops every record — useful for operators
    /// who want to disable the feature via config without threading
    /// `Option<PromptAuditLog>` through the pipeline.
    #[must_use]
    pub fn new(log_dir: PathBuf, enabled: bool) -> Self {
        Self {
            inner: Mutex::new(PromptAuditLogInner { current: None }),
            enabled,
            log_dir,
            record_options: PromptAuditRecordOptions::default(),
        }
    }

    /// Create a new audit log from resolved prompt-audit settings.
    #[must_use]
    pub fn from_settings(log_dir: PathBuf, settings: &taxis::config::PromptAuditSettings) -> Self {
        Self {
            inner: Mutex::new(PromptAuditLogInner { current: None }),
            enabled: settings.enabled,
            log_dir,
            record_options: PromptAuditRecordOptions::from(settings),
        }
    }

    /// Return the log directory.
    #[must_use]
    pub fn log_dir(&self) -> &Path {
        &self.log_dir
    }

    /// Return whether logging is active.
    #[must_use]
    pub fn enabled(&self) -> bool {
        self.enabled
    }

    /// Return record construction options for this log.
    #[must_use]
    pub fn record_options(&self) -> PromptAuditRecordOptions {
        self.record_options
    }

    /// Append a record to today's JSONL file.
    ///
    /// Rotation happens on the calendar-date transition of `record.timestamp`.
    /// A record at 23:59 on day N goes in `N.jsonl`; a record at 00:01 on
    /// day N+1 goes in `N+1.jsonl` even if the previous file is still open.
    ///
    /// # Errors
    ///
    /// Returns an error if the directory cannot be created, the file cannot
    /// be opened, or the serialized line cannot be written. Callers in the
    /// pipeline should log the error and continue; audit failure must not
    /// break the turn.
    pub fn log_request(&self, record: &PromptAuditRecord) -> Result<()> {
        if !self.enabled {
            return Ok(());
        }

        let record_date = record.timestamp.to_zoned(jiff::tz::TimeZone::UTC).date();
        let json = serde_json::to_string(record).context(SerializeSnafu)?;

        // WHY: a poisoned mutex means a panic occurred while holding the
        // audit lock. Rather than propagate, recover the inner state and
        // continue: the audit log is observational and must not block a
        // turn. `.unwrap_or_else` with `into_inner` is the canonical
        // non-panicking recovery pattern.
        let mut inner = self.inner.lock().unwrap_or_else(|poisoned| {
            warn!("prompt audit mutex poisoned; recovering inner state");
            poisoned.into_inner()
        });

        // WHY: compute once whether the currently-open file matches
        // `record_date` so we can take the writable borrow from the right
        // arm without a later `expect` on `Option::as_mut`.
        let current_matches = matches!(inner.current.as_ref(), Some((d, _)) if *d == record_date);

        if !current_matches {
            if !self.log_dir.exists() {
                std::fs::create_dir_all(&self.log_dir).context(CreateDirSnafu {
                    path: self.log_dir.clone(),
                })?;
            }
            let path = self.log_dir.join(format!("{record_date}.jsonl"));
            let file = OpenOptions::new()
                .create(true)
                .append(true)
                .open(&path)
                .context(OpenFileSnafu { path: path.clone() })?;
            debug!(path = %path.display(), "rotated prompt audit log");
            inner.current = Some((record_date, file));
        }

        // WHY: `inner.current` is `Some` by construction -- either it
        // already matched `record_date`, or we just replaced it in the
        // rotation branch above. Use `if let` to avoid an `expect()`.
        let path = self.log_dir.join(format!("{record_date}.jsonl"));
        if let Some((_, file)) = inner.current.as_mut() {
            writeln!(file, "{json}").context(WriteRecordSnafu { path: path.clone() })?;
            file.flush().context(WriteRecordSnafu { path })?;
        }
        Ok(())
    }
}

/// Inputs needed to construct a prompt audit record.
#[derive(Clone, Copy)]
pub(crate) struct PromptAuditRecordInput<'a> {
    /// Pipeline context assembled for the turn.
    pub(crate) ctx: &'a crate::pipeline::PipelineContext,
    /// Session that owns the turn.
    pub(crate) session: &'a crate::session::SessionState,
    /// Agent config used for token-estimation settings.
    pub(crate) config: &'a crate::config::NousConfig,
    /// Concrete completion request sent to the provider.
    pub(crate) request: &'a CompletionRequest,
    /// Gateway/request correlation id for the inbound turn, when available.
    pub(crate) request_id: Option<&'a str>,
    /// Locally generated id for this outbound completion request.
    pub(crate) completion_request_id: &'a str,
    /// Execute-loop iteration that assembled this request.
    pub(crate) loop_iteration: u32,
    /// Selected model that successfully served the turn.
    pub(crate) observed_model: &'a str,
    /// Provider response/message id, when the provider returned one.
    pub(crate) provider_response_id: Option<&'a str>,
    /// Provider registry used to resolve provider/deployment metadata.
    pub(crate) providers: &'a hermeneus::provider::ProviderRegistry,
    /// Opaque hash of the effective tool surface bound for this request.
    pub(crate) tool_surface_hash: &'a str,
    /// Audit record options resolved from operator config.
    pub(crate) options: PromptAuditRecordOptions,
}

/// Build a [`PromptAuditRecord`] from the assembled completion request.
///
/// Called at the completion-request call site after a provider response has
/// reported the selected model. Never persists the system prompt content, user
/// text, or tool arguments — only hashes, counts, tool names, and correlation
/// identifiers leave this function.
pub(crate) fn build_audit_record(input: PromptAuditRecordInput<'_>) -> PromptAuditRecord {
    let PromptAuditRecordInput {
        ctx,
        session,
        config,
        request,
        request_id,
        completion_request_id,
        loop_iteration,
        observed_model,
        provider_response_id,
        providers,
        tool_surface_hash,
        options,
    } = input;
    let system_prompt = request.system.as_deref();
    let system_prompt_bytes = system_prompt.map_or(0, str::len);

    // WHY: resolve provider from the observed model so the log reports the
    // real route that served the turn. `"unknown"` keeps the log
    // writeable when the provider is mid-reconfiguration.
    let provider = providers.find_provider(observed_model);
    let provider_name = provider.map_or_else(|| "unknown".to_owned(), |p| p.name().to_owned());
    let deployment_target = provider
        .map_or(hermeneus::provider::DeploymentTarget::Cloud, |p| {
            p.deployment_target()
        })
        .as_str()
        .to_owned();

    let mut tool_names: Vec<String> = request.tools.iter().map(|tool| tool.name.clone()).collect();
    tool_names.extend(request.server_tools.iter().map(|tool| tool.name.clone()));
    let tool_result_ids = request_tool_result_ids(request);

    let fact_ids_included = ctx
        .recall_result
        .as_ref()
        .map(|r| r.fact_ids.clone())
        .unwrap_or_default();
    let fact_ids_filtered = if options.include_filtered_ids {
        ctx.recall_result.as_ref().map_or_else(Vec::new, |r| {
            r.filtered_facts
                .iter()
                .map(|fact| FilteredFact {
                    id: fact.id.clone(),
                    sensitivity: fact.sensitivity.as_str().to_owned(),
                })
                .collect()
        })
    } else {
        Vec::new()
    };

    // WHY: token estimate uses the same per-message estimate the pipeline
    // already computed, plus the system-prompt byte length divided by a
    // conservative chars-per-token heuristic. Matches the order-of-magnitude
    // figure operators see in the tracing spans.
    //
    // WHY `try_from(...).unwrap_or(u32::MAX)`: per-message token estimates
    // are positive for practical turns; if an implausible value ever
    // overflowed u32 we prefer a saturated audit figure to a silent `as`
    // truncation.
    let cpt = usize::try_from(config.generation.chars_per_token.max(1)).unwrap_or(4);
    let msg_tokens = request_message_token_estimate(request, cpt);
    let sys_tokens = u32::try_from(system_prompt_bytes / cpt).unwrap_or(u32::MAX);
    let token_count_estimate = msg_tokens.saturating_add(sys_tokens);

    PromptAuditRecord {
        timestamp: Timestamp::now(),
        nous_id: session.nous_id.clone(),
        session_id: session.id.clone(),
        turn_id: session.turn_id.to_string(),
        request_id: request_id.map(ToOwned::to_owned),
        completion_request_id: completion_request_id.to_owned(),
        loop_iteration,
        provider: provider_name,
        deployment_target,
        model: observed_model.to_owned(),
        provider_response_id: provider_response_id.map(ToOwned::to_owned),
        system_prompt_hash: hash_system_prompt(system_prompt),
        system_prompt_bytes,
        message_count: request.messages.len(),
        token_count_estimate,
        fact_ids_included,
        fact_ids_filtered,
        tool_names,
        tool_result_ids,
        tool_surface_hash: tool_surface_hash.to_owned(),
        cache: PromptAuditCachePolicy {
            cache_system: request.cache_system,
            cache_tools: request.cache_tools,
            cache_turns: request.cache_turns,
        },
        deferred_schemas: deferred_schemas_enabled(),
    }
}

fn request_tool_result_ids(request: &CompletionRequest) -> Vec<String> {
    request
        .messages
        .iter()
        .flat_map(|message| match &message.content {
            Content::Blocks(blocks) => blocks.as_slice(),
            _ => &[],
        })
        .filter_map(|block| match block {
            ContentBlock::ToolResult { tool_use_id, .. } => Some(tool_use_id.clone()),
            _ => None,
        })
        .collect()
}

fn request_message_token_estimate(request: &CompletionRequest, chars_per_token: usize) -> u32 {
    request.messages.iter().fold(0u32, |acc, message| {
        let bytes = match &message.content {
            Content::Text(text) => text.len(),
            Content::Blocks(blocks) => blocks.iter().map(content_block_text_len).sum(),
            _ => 0,
        };
        acc.saturating_add(u32::try_from(bytes / chars_per_token).unwrap_or(u32::MAX))
    })
}

fn content_block_text_len(block: &ContentBlock) -> usize {
    match block {
        ContentBlock::Text { text, .. } => text.len(),
        ContentBlock::Thinking { thinking, .. } => thinking.len(),
        ContentBlock::ToolResult { content, .. } => content.text_summary().len(),
        _ => 0,
    }
}

const fn deferred_schemas_enabled() -> bool {
    cfg!(feature = "deferred-schemas")
}

#[cfg(test)]
#[path = "audit_tests.rs"]
mod tests;

//! Graceful degradation when the LLM provider is unavailable.
//!
//! When the provider returns a transient error (rate limit, timeout, 5xx),
//! the pipeline can still serve the user with cached distillation summaries
//! and queued message promises instead of returning a raw error.
//!
//! # Degradation tiers
//!
//! 1. **Distillation cache hit** — a recent distillation summary covers the
//!    session. Return an excerpt with a status banner.
//! 2. **No cache** — no summary is available. Return an honest "can't help
//!    right now" message with a queue promise.
//!
//! In both cases the result carries [`DegradedMode`] so the TUI and API can
//! render it differently (greyed out, warning banner, etc.).
//!
//! # Hard-failing errors
//!
//! Not every error activates the degraded fallback. Only transient LLM
//! provider failures (identified by [`is_transient_llm_error`]) enter the
//! degraded path. Storage failures (session store, competence store,
//! uncertainty store, working-checkpoint store) **always propagate as hard
//! errors** — they signal that the runtime's own persistence layer is broken
//! and cannot be masked with cached content. Use [`is_storage_failure`] to
//! identify these explicitly for observability and alerting purposes.

use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};
use tracing::{info, warn};

use koina::error_class::{Classifiable, ErrorAction, ErrorClass};
use koina::redact::redact_sensitive;

use crate::error;
use crate::pipeline::{InteractionSignal, TurnResult, TurnUsage};

/// How the pipeline is operating in degraded mode.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum DegradedMode {
    /// A recent distillation summary was found and returned as the response.
    DistillationCache {
        /// Human-readable status shown alongside the response.
        status_banner: String,
        /// Durable provenance for the degraded fallback.
        provenance: DegradedProvenance,
    },
    /// No cache available; an honest "unavailable" message was returned.
    Unavailable {
        /// Human-readable status shown alongside the response.
        status_banner: String,
        /// Durable provenance for the degraded fallback.
        provenance: DegradedProvenance,
    },
    /// The turn's wall-clock budget was exhausted at a safe boundary.
    ///
    /// Tool results observed before the deadline are preserved; the response
    /// explains that the turn stopped early rather than dropping the future.
    TurnBudgetExceeded {
        /// Human-readable status shown alongside the response.
        status_banner: String,
    },
}

impl DegradedMode {
    /// Status banner text suitable for display in a TUI warning overlay.
    #[must_use]
    pub fn status_banner(&self) -> &str {
        match self {
            Self::DistillationCache { status_banner, .. }
            | Self::Unavailable { status_banner, .. }
            | Self::TurnBudgetExceeded { status_banner } => status_banner,
        }
    }

    /// Durable provenance for provider-failure degraded responses.
    #[must_use]
    pub fn provenance(&self) -> Option<&DegradedProvenance> {
        match self {
            Self::DistillationCache { provenance, .. } | Self::Unavailable { provenance, .. } => {
                Some(provenance)
            }
            Self::TurnBudgetExceeded { .. } => None,
        }
    }
}

/// Source used to synthesize a degraded response.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum DegradationSource {
    /// Response was synthesized from the distillation cache.
    DistillationCache,
    /// No cache was available; response only reports provider unavailability.
    Unavailable,
}

/// Provider/model context known before entering degraded mode.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DegradedAttemptContext {
    /// Provider instance selected for the attempted model, when known.
    pub attempted_provider: Option<String>,
    /// Configured default model for the session/agent.
    pub configured_model: String,
    /// Model selected by routing for this turn.
    pub routed_model: String,
    /// Durable id of the cache/distillation evidence used for the response.
    pub source_id: Option<String>,
}

impl DegradedAttemptContext {
    /// Build context when the caller has no routing details.
    #[must_use]
    pub fn unknown() -> Self {
        Self {
            attempted_provider: None,
            configured_model: "unknown".to_owned(),
            routed_model: "unknown".to_owned(),
            source_id: None,
        }
    }
}

/// Durable provenance for a synthetic degraded-mode assistant response.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DegradedProvenance {
    /// Provider instance selected for the attempted model, when known.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub attempted_provider: Option<String>,
    /// Model attempted for the failed upstream request.
    pub attempted_model: String,
    /// Configured default model before routing.
    pub configured_model: String,
    /// Model selected by routing for this turn.
    pub routed_model: String,
    /// Shared error class label for the original failure.
    pub original_error_class: String,
    /// Redacted display text for the original failure.
    pub original_error_message: String,
    /// SHA-256 hash of the redacted original failure text.
    pub original_error_hash: String,
    /// Degraded fallback source.
    pub degradation_source: DegradationSource,
    /// Durable id of cache/distillation evidence, when a cache was used.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_id: Option<String>,
    /// True because the assistant content was synthesized locally.
    pub synthetic_response: bool,
    /// False because no provider usage was returned for the degraded content.
    pub provider_usage_recorded: bool,
}

impl DegradedProvenance {
    fn from_error(
        original_error: &error::Error,
        source: DegradationSource,
        attempt: DegradedAttemptContext,
    ) -> Self {
        let redacted_message = redact_sensitive(&original_error.to_string());
        Self {
            attempted_provider: attempt.attempted_provider,
            attempted_model: attempt.routed_model.clone(),
            configured_model: attempt.configured_model,
            routed_model: attempt.routed_model,
            original_error_class: error_class_label(original_error).to_owned(),
            original_error_hash: sha256_hex(redacted_message.as_bytes()),
            original_error_message: redacted_message,
            degradation_source: source,
            source_id: attempt.source_id,
            synthetic_response: true,
            provider_usage_recorded: false,
        }
    }
}

fn error_class_label(err: &error::Error) -> &'static str {
    match err.class() {
        ErrorClass::Transient => "transient",
        ErrorClass::Permanent => "permanent",
        _ => "unknown",
    }
}

fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut out = String::with_capacity(digest.len() * 2);
    for byte in digest {
        use std::fmt::Write as _;
        // kanon:ignore RUST/no-silent-result-swallow — write! on String is infallible
        let _ = write!(out, "{byte:02x}");
    }
    out
}

/// Determine whether a `nous::Error` is a transient LLM failure that should
/// activate degraded-mode fallback rather than being surfaced as a hard error.
///
/// Only [`error::Error::Llm`] variants wrapping a retryable hermeneus error
/// qualify. All other error kinds (store, config, guard, panic) propagate
/// normally — they are not provider outages.
#[must_use]
pub fn is_transient_llm_error(err: &error::Error) -> bool {
    match err {
        error::Error::Llm { source, .. } => matches!(source.action(), ErrorAction::Retry { .. }),
        _ => false,
    }
}

/// Determine whether a `nous::Error` originates from a storage layer failure.
///
/// Storage failures are always hard errors — they indicate the runtime's own
/// persistence layer is broken and cannot be masked with cached LLM output.
/// Unlike transient LLM errors, storage failures must propagate immediately
/// so the operator is alerted and can take corrective action (e.g., disk
/// recovery, store rebuild).
///
/// Use this predicate in observability hooks (metrics, alerting) to
/// distinguish storage failures from provider outages.
#[must_use]
pub fn is_storage_failure(err: &error::Error) -> bool {
    matches!(
        err,
        error::Error::Store { .. }
            | error::Error::CompetenceStore { .. }
            | error::Error::UncertaintyStore { .. }
            | error::Error::WorkingCheckpointStore { .. }
    )
}

/// Attempt to build a degraded [`TurnResult`] when the LLM provider is down.
///
/// # Behaviour
///
/// 1. If `recent_distillation` is `Some`, prepend a status banner and return
///    the summary as the response content with a [`DegradedMode::DistillationCache`]
///    indicator.
/// 2. If `recent_distillation` is `None`, return a clear "can't help right now"
///    message with a [`DegradedMode::Unavailable`] indicator.
///
/// Either way the original error is logged at `warn` level so it remains visible
/// in traces without being surfaced to the caller as a hard error.
///
/// Callers should pass `recent_distillation = None` when no store is available
/// or when the session has never been distilled.
pub fn build_degraded_response(
    nous_id: &str,
    session_id: &str,
    original_error: &error::Error,
    recent_distillation: Option<&str>,
) -> TurnResult {
    build_degraded_response_with_provenance(
        nous_id,
        session_id,
        original_error,
        recent_distillation,
        DegradedAttemptContext::unknown(),
    )
}

/// Attempt to build a degraded [`TurnResult`] with durable provenance.
pub fn build_degraded_response_with_provenance(
    nous_id: &str,
    session_id: &str,
    original_error: &error::Error,
    recent_distillation: Option<&str>,
    attempt: DegradedAttemptContext,
) -> TurnResult {
    warn!(
        nous_id,
        session_id,
        error = %original_error,
        "LLM provider unavailable — entering degraded mode"
    );

    if let Some(summary) = recent_distillation {
        let provenance = DegradedProvenance::from_error(
            original_error,
            DegradationSource::DistillationCache,
            attempt,
        );
        let model_used = provenance.attempted_model.clone();
        let banner = "Operating in degraded mode — LLM unavailable. \
             Showing response based on previous conversation context."
            .to_owned();

        info!(
            nous_id,
            session_id, "degraded mode: returning cached distillation summary"
        );

        let content = format!(
            "[Degraded mode — LLM unavailable]\n\n\
             I can't reach the LLM right now, but based on our recent conversation:\n\n\
             {summary}\n\n\
             Your message has been noted. Full responses will resume when the provider recovers."
        );

        TurnResult {
            content,
            tool_calls: vec![],
            usage: TurnUsage::default(),
            signals: vec![InteractionSignal::ErrorRecovery],
            stop_reason: "degraded".to_owned(),
            degraded: Some(crate::pipeline::DegradedMode::DistillationCache {
                status_banner: banner,
                provenance,
            }),
            reasoning: String::new(),
            model_used,
            provider_used: None,
            tool_surface_hashes: Vec::new(),
        }
    } else {
        let provenance =
            DegradedProvenance::from_error(original_error, DegradationSource::Unavailable, attempt);
        let model_used = provenance.attempted_model.clone();
        let banner = "Operating in degraded mode — LLM unavailable. \
             No cached context available for this session."
            .to_owned();

        info!(
            nous_id,
            session_id, "degraded mode: no distillation cache, returning unavailable message"
        );

        let content = "I can't reach the LLM right now and have no cached context \
                       for this session. Your message has been noted and full responses \
                       will resume when the provider recovers."
            .to_owned();

        TurnResult {
            content,
            tool_calls: vec![],
            usage: TurnUsage::default(),
            signals: vec![InteractionSignal::ErrorRecovery],
            stop_reason: "degraded".to_owned(),
            degraded: Some(crate::pipeline::DegradedMode::Unavailable {
                status_banner: banner,
                provenance,
            }),
            reasoning: String::new(),
            model_used,
            provider_used: None,
            tool_surface_hashes: Vec::new(),
        }
    }
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions")]
mod tests {
    use super::*;

    fn llm_rate_limit_error() -> error::Error {
        use hermeneus::error::RateLimitedSnafu;
        use snafu::IntoError as _;

        let hermeneus_err = RateLimitedSnafu {
            retry_after_ms: 5000u64,
        }
        .build();
        error::LlmSnafu.into_error(hermeneus_err)
    }

    fn llm_auth_error() -> error::Error {
        use hermeneus::error::AuthFailedSnafu;
        use snafu::IntoError as _;

        let hermeneus_err = AuthFailedSnafu {
            message: "invalid key",
        }
        .build();
        error::LlmSnafu.into_error(hermeneus_err)
    }

    fn store_error() -> error::Error {
        use snafu::IntoError as _;
        let mneme_err = mneme::error::Error::Storage {
            message: "disk full".to_owned(),
            location: snafu::location!(),
        };
        error::StoreSnafu.into_error(mneme_err)
    }

    fn competence_store_error() -> error::Error {
        error::CompetenceStoreSnafu {
            message: "competence store unavailable",
        }
        .build()
    }

    fn uncertainty_store_error() -> error::Error {
        error::UncertaintyStoreSnafu {
            message: "uncertainty store unavailable",
        }
        .build()
    }

    fn working_checkpoint_store_error() -> error::Error {
        error::WorkingCheckpointStoreSnafu {
            message: "checkpoint store unavailable",
        }
        .build()
    }

    #[test]
    fn rate_limit_is_transient() {
        assert!(is_transient_llm_error(&llm_rate_limit_error()));
    }

    #[test]
    fn auth_error_is_not_transient() {
        assert!(!is_transient_llm_error(&llm_auth_error()));
    }

    #[test]
    fn store_error_is_not_transient() {
        assert!(!is_transient_llm_error(&store_error()));
    }

    #[test]
    fn pipeline_stage_error_is_not_transient() {
        let err = error::PipelineStageSnafu {
            stage: "execute",
            message: "no provider",
        }
        .build();
        assert!(!is_transient_llm_error(&err));
    }

    #[test]
    fn store_error_is_storage_failure() {
        assert!(is_storage_failure(&store_error()));
    }

    #[test]
    fn competence_store_error_is_storage_failure() {
        assert!(is_storage_failure(&competence_store_error()));
    }

    #[test]
    fn uncertainty_store_error_is_storage_failure() {
        assert!(is_storage_failure(&uncertainty_store_error()));
    }

    #[test]
    fn working_checkpoint_store_error_is_storage_failure() {
        assert!(is_storage_failure(&working_checkpoint_store_error()));
    }

    #[test]
    fn llm_rate_limit_is_not_storage_failure() {
        assert!(!is_storage_failure(&llm_rate_limit_error()));
    }

    #[test]
    fn llm_auth_error_is_not_storage_failure() {
        assert!(!is_storage_failure(&llm_auth_error()));
    }

    #[test]
    fn storage_failures_are_not_transient_llm_errors() {
        let storage_errors = [
            store_error(),
            competence_store_error(),
            uncertainty_store_error(),
            working_checkpoint_store_error(),
        ];
        for err in &storage_errors {
            assert!(
                !is_transient_llm_error(err),
                "storage failure must not activate degraded LLM path: {err}"
            );
        }
    }

    #[test]
    fn degraded_with_cache_uses_distillation_cache_variant() {
        let err = llm_rate_limit_error();
        let result = build_degraded_response_with_provenance(
            "alice",
            "ses-1",
            &err,
            Some("User prefers brevity."),
            DegradedAttemptContext {
                attempted_provider: Some("anthropic".to_owned()),
                configured_model: "configured-model".to_owned(),
                routed_model: "routed-model".to_owned(),
                source_id: Some("message:ses-1:7".to_owned()),
            },
        );
        let provenance = match &result.degraded {
            Some(DegradedMode::DistillationCache { provenance, .. }) => provenance,
            other => panic!("expected DistillationCache, got {other:?}"),
        };
        assert!(result.content.contains("can't reach the LLM"));
        assert!(result.content.contains("User prefers brevity."));
        assert_eq!(result.stop_reason, "degraded");
        assert_eq!(result.model_used, "routed-model");
        assert_eq!(provenance.attempted_provider.as_deref(), Some("anthropic"));
        assert_eq!(provenance.attempted_model, "routed-model");
        assert_eq!(provenance.configured_model, "configured-model");
        assert_eq!(provenance.routed_model, "routed-model");
        assert_eq!(
            provenance.degradation_source,
            DegradationSource::DistillationCache
        );
        assert_eq!(provenance.source_id.as_deref(), Some("message:ses-1:7"));
        assert_eq!(provenance.original_error_class, "transient");
        assert!(!provenance.original_error_hash.is_empty());
        assert!(provenance.synthetic_response);
        assert!(!provenance.provider_usage_recorded);
        assert!(result.signals.contains(&InteractionSignal::ErrorRecovery));
    }

    #[test]
    fn degraded_without_cache_uses_unavailable_variant() {
        let err = llm_rate_limit_error();
        let result = build_degraded_response_with_provenance(
            "alice",
            "ses-1",
            &err,
            None,
            DegradedAttemptContext {
                attempted_provider: Some("anthropic".to_owned()),
                configured_model: "configured-model".to_owned(),
                routed_model: "routed-model".to_owned(),
                source_id: None,
            },
        );
        let provenance = match &result.degraded {
            Some(DegradedMode::Unavailable { provenance, .. }) => provenance,
            other => panic!("expected Unavailable, got {other:?}"),
        };
        assert!(result.content.contains("can't reach the LLM"));
        assert_eq!(result.stop_reason, "degraded");
        assert_eq!(result.model_used, "routed-model");
        assert_eq!(
            provenance.degradation_source,
            DegradationSource::Unavailable
        );
        assert!(provenance.source_id.is_none());
    }

    #[test]
    fn status_banner_non_empty() {
        let err = llm_rate_limit_error();
        let with_cache = build_degraded_response("alice", "ses-1", &err, Some("ctx"));
        let without_cache = build_degraded_response("alice", "ses-1", &err, None);

        assert!(
            !with_cache
                .degraded
                .as_ref()
                .unwrap()
                .status_banner()
                .is_empty()
        );
        assert!(
            !without_cache
                .degraded
                .as_ref()
                .unwrap()
                .status_banner()
                .is_empty()
        );
    }

    #[test]
    fn degraded_mode_is_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<DegradedMode>();
        assert_send_sync::<DegradedProvenance>();
    }
}

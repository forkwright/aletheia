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

use std::fmt::Write as _;

use sha2::{Digest, Sha256};
use tracing::{info, warn};

use koina::error_class::{Classifiable, ErrorAction, ErrorClass};

use crate::error;
use crate::pipeline::{InteractionSignal, TurnResult, TurnUsage};

/// Provenance captured when a turn falls back to degraded mode.
///
/// WHY: the durable turn ledger needs enough context to reconstruct the failed
/// provider attempt without reading logs. This struct is intentionally flat so
/// it serializes cleanly into `TurnAttemptRecord`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DegradedProvenance {
    /// Provider/model that was selected before the failure.
    pub attempted_model: String,
    /// Routed model context when complexity routing was active.
    pub routed_model_context: Option<String>,
    /// Error class of the original provider failure.
    pub error_class: String,
    /// Stable hash of the original error display string for log correlation.
    pub error_message_hash: String,
    /// Degradation source: `distillation_cache` or `unavailable`.
    pub source: String,
    /// Distillation cache reference when a cached summary was used.
    pub distillation_id: Option<String>,
}

/// How the pipeline is operating in degraded mode.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum DegradedMode {
    /// A recent distillation summary was found and returned as the response.
    DistillationCache {
        /// Human-readable status shown alongside the response.
        status_banner: String,
        /// Provenance of the degraded fallback, including the distillation id.
        provenance: DegradedProvenance,
    },
    /// No cache available; an honest "unavailable" message was returned.
    Unavailable {
        /// Human-readable status shown alongside the response.
        status_banner: String,
        /// Provenance of the degraded fallback.
        provenance: DegradedProvenance,
    },
}

impl DegradedMode {
    /// Status banner text suitable for display in a TUI warning overlay.
    #[must_use]
    pub fn status_banner(&self) -> &str {
        match self {
            Self::DistillationCache { status_banner, .. }
            | Self::Unavailable { status_banner, .. } => status_banner,
        }
    }

    /// Provenance for this degraded turn.
    #[must_use]
    pub fn provenance(&self) -> &DegradedProvenance {
        match self {
            Self::DistillationCache { provenance, .. } | Self::Unavailable { provenance, .. } => {
                provenance
            }
        }
    }
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

/// Stable 16-character hex hash of a byte slice.
///
/// WHY: short enough for durable records and timeline displays, long enough to
/// avoid trivial collisions in a single session's degraded turns.
fn short_hash(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    // WHY: GenericArray does not implement LowerHex; iterate bytes instead.
    let mut hex = String::with_capacity(64);
    for b in &digest {
        write!(hex, "{b:02x}").ok();
    }
    // WHY: SHA-256 is always 64 hex chars; truncate to 16 for compact durable records.
    hex.truncate(16);
    hex
}

fn error_class_name(class: ErrorClass) -> &'static str {
    // WHY: `ErrorClass` is `#[non_exhaustive]` in `koina`; the wildcard arm
    // prevents a downstream crate from failing to compile when a variant is added.
    match class {
        ErrorClass::Transient => "transient",
        ErrorClass::Permanent => "permanent",
        ErrorClass::Unknown | _ => "unknown",
    }
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
/// in traces without being surfaced to the caller as a hard error. The returned
/// [`TurnResult`] carries [`DegradedProvenance`] so finalization can persist the
/// failed attempt in the turn ledger rather than treating it as a successful
/// assistant turn.
///
/// Callers should pass `recent_distillation = None` when no store is available
/// or when the session has never been distilled.
pub fn build_degraded_response(
    nous_id: &str,
    session_id: &str,
    original_error: &error::Error,
    recent_distillation: Option<&str>,
    attempted_model: &str,
    routed_model_context: Option<&str>,
) -> TurnResult {
    warn!(
        nous_id,
        session_id,
        error = %original_error,
        "LLM provider unavailable — entering degraded mode"
    );

    let error_display = original_error.to_string();
    let provenance = DegradedProvenance {
        attempted_model: attempted_model.to_owned(),
        routed_model_context: routed_model_context.map(ToOwned::to_owned),
        error_class: error_class_name(original_error.class()).to_owned(),
        error_message_hash: short_hash(error_display.as_bytes()),
        source: String::new(),
        distillation_id: None,
    };

    if let Some(summary) = recent_distillation {
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

        // WHY: degraded output is synthetic, not provider-served. `llm_calls: 0`
        // distinguishes it from a real completion in usage/cost records.
        let synthetic_output_tokens = u64::try_from(content.len() / 4).unwrap_or(u64::MAX);
        let distillation_id = format!(
            "{session_id}:distillation:seq=0:{}",
            short_hash(summary.as_bytes())
        );

        TurnResult {
            content,
            tool_calls: vec![],
            usage: TurnUsage {
                output_tokens: synthetic_output_tokens,
                llm_calls: 0,
                ..TurnUsage::default()
            },
            signals: vec![InteractionSignal::ErrorRecovery],
            stop_reason: "degraded".to_owned(),
            degraded: Some(crate::pipeline::DegradedMode::DistillationCache {
                status_banner: banner,
                provenance: DegradedProvenance {
                    source: "distillation_cache".to_owned(),
                    distillation_id: Some(distillation_id),
                    ..provenance
                },
            }),
            reasoning: String::new(),
            model_used: attempted_model.to_owned(),
            tool_surface_hashes: Vec::new(),
        }
    } else {
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

        // WHY: degraded output is synthetic, not provider-served. `llm_calls: 0`
        // distinguishes it from a real completion in usage/cost records.
        let synthetic_output_tokens = u64::try_from(content.len() / 4).unwrap_or(u64::MAX);

        TurnResult {
            content,
            tool_calls: vec![],
            usage: TurnUsage {
                output_tokens: synthetic_output_tokens,
                llm_calls: 0,
                ..TurnUsage::default()
            },
            signals: vec![InteractionSignal::ErrorRecovery],
            stop_reason: "degraded".to_owned(),
            degraded: Some(crate::pipeline::DegradedMode::Unavailable {
                status_banner: banner,
                provenance: DegradedProvenance {
                    source: "unavailable".to_owned(),
                    distillation_id: None,
                    ..provenance
                },
            }),
            reasoning: String::new(),
            model_used: attempted_model.to_owned(),
            tool_surface_hashes: Vec::new(),
        }
    }
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions")]
#[expect(clippy::expect_used, reason = "test assertions")]
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
        let result = build_degraded_response(
            "alice",
            "ses-1",
            &err,
            Some("User prefers brevity."),
            "test-model",
            None,
        );
        assert!(
            matches!(
                result.degraded,
                Some(DegradedMode::DistillationCache { .. })
            ),
            "expected DistillationCache, got {:?}",
            result.degraded
        );
        assert!(result.content.contains("can't reach the LLM"));
        assert!(result.content.contains("User prefers brevity."));
        assert_eq!(result.stop_reason, "degraded");
        assert!(result.signals.contains(&InteractionSignal::ErrorRecovery));
    }

    #[test]
    fn degraded_without_cache_uses_unavailable_variant() {
        let err = llm_rate_limit_error();
        let result = build_degraded_response("alice", "ses-1", &err, None, "test-model", None);
        assert!(
            matches!(result.degraded, Some(DegradedMode::Unavailable { .. })),
            "expected Unavailable, got {:?}",
            result.degraded
        );
        assert!(result.content.contains("can't reach the LLM"));
        assert_eq!(result.stop_reason, "degraded");
    }

    #[test]
    fn status_banner_non_empty() {
        let err = llm_rate_limit_error();
        let with_cache =
            build_degraded_response("alice", "ses-1", &err, Some("ctx"), "test-model", None);
        let without_cache =
            build_degraded_response("alice", "ses-1", &err, None, "test-model", None);

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
    }

    #[test]
    fn degraded_with_cache_includes_provenance() {
        let err = llm_rate_limit_error();
        let result = build_degraded_response(
            "alice",
            "ses-1",
            &err,
            Some("User prefers brevity."),
            "test-model",
            Some("routed-test-model"),
        );
        let degraded = result.degraded.expect("degraded");
        let provenance = degraded.provenance();
        assert_eq!(provenance.attempted_model, "test-model");
        assert_eq!(
            provenance.routed_model_context.as_deref(),
            Some("routed-test-model")
        );
        assert_eq!(provenance.error_class, "transient");
        assert!(!provenance.error_message_hash.is_empty());
        assert_eq!(provenance.source, "distillation_cache");
        assert!(
            provenance
                .distillation_id
                .as_deref()
                .expect("distillation_id")
                .starts_with("ses-1:distillation:seq=0:")
        );

        // WHY: a degraded turn must not look like a zero-cost success.
        assert_eq!(result.model_used, "test-model");
        assert_eq!(result.usage.llm_calls, 0);
        assert!(result.usage.output_tokens > 0);
    }

    #[test]
    fn degraded_without_cache_includes_provenance() {
        let err = llm_rate_limit_error();
        let result = build_degraded_response("alice", "ses-1", &err, None, "test-model", None);
        let degraded = result.degraded.expect("degraded");
        let provenance = degraded.provenance();
        assert_eq!(provenance.attempted_model, "test-model");
        assert!(provenance.routed_model_context.is_none());
        assert_eq!(provenance.error_class, "transient");
        assert!(!provenance.error_message_hash.is_empty());
        assert_eq!(provenance.source, "unavailable");
        assert!(provenance.distillation_id.is_none());

        assert_eq!(result.model_used, "test-model");
        assert_eq!(result.usage.llm_calls, 0);
        assert!(result.usage.output_tokens > 0);
    }
}

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

use tracing::{info, warn};

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
    },
    /// No cache available; an honest "unavailable" message was returned.
    Unavailable {
        /// Human-readable status shown alongside the response.
        status_banner: String,
    },
}

impl DegradedMode {
    /// Status banner text suitable for display in a TUI warning overlay.
    #[must_use]
    pub fn status_banner(&self) -> &str {
        match self {
            Self::DistillationCache { status_banner }
            | Self::Unavailable { status_banner } => status_banner,
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
        error::Error::Llm { source, .. } => source.is_retryable(),
        _ => false,
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
/// in traces without being surfaced to the caller as a hard error.
///
/// # Parameters
///
/// - `nous_id` — agent identifier used for log context.
/// - `session_id` — session identifier used for log context.
/// - `original_error` — the transient error that triggered degradation.
/// - `recent_distillation` — most recent distillation summary for this session,
///   if any. Callers should pass `None` when no store is available or when the
///   session has never been distilled.
pub fn build_degraded_response(
    nous_id: &str,
    session_id: &str,
    original_error: &error::Error,
    recent_distillation: Option<&str>,
) -> TurnResult {
    warn!(
        nous_id,
        session_id,
        error = %original_error,
        "LLM provider unavailable — entering degraded mode"
    );

    if let Some(summary) = recent_distillation {
        let banner =
            "Operating in degraded mode — LLM unavailable. \
             Showing response based on previous conversation context."
                .to_owned();

        info!(
            nous_id,
            session_id,
            "degraded mode: returning cached distillation summary"
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
            }),
        }
    } else {
        let banner =
            "Operating in degraded mode — LLM unavailable. \
             No cached context available for this session."
                .to_owned();

        info!(
            nous_id,
            session_id,
            "degraded mode: no distillation cache, returning unavailable message"
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
            }),
        }
    }
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions")]
mod tests {
    use super::*;

    fn llm_rate_limit_error() -> error::Error {
        use aletheia_hermeneus::error::RateLimitedSnafu;
        use snafu::IntoError as _;

        let hermeneus_err = RateLimitedSnafu {
            retry_after_ms: 5000u64,
        }
        .build();
        error::LlmSnafu.into_error(hermeneus_err)
    }

    fn llm_auth_error() -> error::Error {
        use aletheia_hermeneus::error::AuthFailedSnafu;
        use snafu::IntoError as _;

        let hermeneus_err = AuthFailedSnafu {
            message: "invalid key",
        }
        .build();
        error::LlmSnafu.into_error(hermeneus_err)
    }

    fn store_error() -> error::Error {
        error::PipelineStageSnafu {
            stage: "execute",
            message: "no provider",
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
    fn non_llm_error_is_not_transient() {
        assert!(!is_transient_llm_error(&store_error()));
    }

    #[test]
    fn degraded_with_cache_uses_distillation_cache_variant() {
        let err = llm_rate_limit_error();
        let result = build_degraded_response("alice", "ses-1", &err, Some("User prefers brevity."));
        assert!(
            matches!(result.degraded, Some(DegradedMode::DistillationCache { .. })),
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
        let result = build_degraded_response("alice", "ses-1", &err, None);
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
        let with_cache = build_degraded_response("alice", "ses-1", &err, Some("ctx"));
        let without_cache = build_degraded_response("alice", "ses-1", &err, None);

        assert!(!with_cache
            .degraded
            .as_ref()
            .unwrap()
            .status_banner()
            .is_empty());
        assert!(!without_cache
            .degraded
            .as_ref()
            .unwrap()
            .status_banner()
            .is_empty());
    }

    #[test]
    fn degraded_mode_is_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<DegradedMode>();
    }
}

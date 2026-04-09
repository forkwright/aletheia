//! Integration tests for hermeneus's public API surface.
//!
//! WHY: hermeneus had zero `crates/hermeneus/tests/` integration tests
//! prior to this. The crate is the LLM client used by every nous turn,
//! and its public types (Role, Message, ContentBlock, Usage, StopReason,
//! ToolDefinition, CompletionRequest/Response) form the wire contract
//! that drives the rest of the workspace.
//!
//! These tests run against the published API surface only — what nous,
//! pylon, and the dispatch path actually consume.

#![expect(clippy::expect_used, reason = "test assertions")]
#![expect(clippy::unwrap_used, reason = "test assertions")]

use aletheia_hermeneus::models::{
    BACKOFF_BASE_MS, BACKOFF_FACTOR, BACKOFF_MAX_MS, DEFAULT_API_VERSION, DEFAULT_BASE_URL,
    DEFAULT_MAX_RETRIES, SUPPORTED_MODELS, names,
};
use aletheia_hermeneus::types::{StopReason, Usage};

// --- Model constants ---

mod model_constants {
    use super::{
        BACKOFF_BASE_MS, BACKOFF_FACTOR, BACKOFF_MAX_MS, DEFAULT_API_VERSION, DEFAULT_BASE_URL,
        DEFAULT_MAX_RETRIES, SUPPORTED_MODELS, names,
    };

    #[test]
    fn default_base_url_is_anthropic_https() {
        assert_eq!(DEFAULT_BASE_URL, "https://api.anthropic.com");
        assert!(DEFAULT_BASE_URL.starts_with("https://"));
    }

    #[test]
    fn default_api_version_matches_anthropic_release() {
        // WHY: this is a date-versioned API. The exact value matters because
        // changing it shifts the entire request/response wire shape.
        assert_eq!(DEFAULT_API_VERSION, "2023-06-01");
    }

    #[test]
    fn retry_constants_are_sensible() {
        assert!(DEFAULT_MAX_RETRIES > 0, "must allow at least one retry");
        assert!(BACKOFF_BASE_MS >= 100, "base backoff should not be aggressive");
        assert!(BACKOFF_FACTOR >= 2, "exponential backoff requires factor >= 2");
        assert!(
            BACKOFF_MAX_MS >= BACKOFF_BASE_MS * BACKOFF_FACTOR,
            "max must allow at least one full backoff step"
        );
    }

    #[test]
    fn supported_models_includes_current_default_aliases() {
        // WHY: the alias constants must always be present in SUPPORTED_MODELS
        // so anything that picks an alias gets validated against the list.
        assert!(SUPPORTED_MODELS.contains(&names::OPUS));
        assert!(SUPPORTED_MODELS.contains(&names::SONNET));
        assert!(SUPPORTED_MODELS.contains(&names::HAIKU));
    }

    #[test]
    fn supported_models_are_distinct() {
        // WHY: duplicates would shadow each other in any list-based lookup.
        let mut sorted: Vec<&&str> = SUPPORTED_MODELS.iter().collect();
        sorted.sort();
        let len_before = sorted.len();
        sorted.dedup();
        assert_eq!(
            sorted.len(),
            len_before,
            "SUPPORTED_MODELS must contain no duplicates"
        );
    }

    #[test]
    fn alias_constants_use_short_form() {
        // WHY: aliases should be short non-dated names so callers can
        // upgrade through them. Dated snapshots should not be aliases.
        assert!(!names::OPUS.contains("2025"));
        assert!(!names::SONNET.contains("2025"));
    }
}

// --- StopReason enum ---

mod stop_reason {
    use super::StopReason;
    use std::str::FromStr;

    #[test]
    fn round_trips_through_as_str_and_from_str() {
        for reason in [
            StopReason::EndTurn,
            StopReason::ToolUse,
            StopReason::MaxTokens,
            StopReason::StopSequence,
        ] {
            let s = reason.as_str();
            let parsed = StopReason::from_str(s).expect("round trip");
            assert_eq!(parsed, reason);
        }
    }

    #[test]
    fn as_str_returns_canonical_snake_case() {
        // WHY: these are wire-format strings the Anthropic API uses. The
        // exact form (snake_case) matters for serde deserialization.
        assert_eq!(StopReason::EndTurn.as_str(), "end_turn");
        assert_eq!(StopReason::ToolUse.as_str(), "tool_use");
        assert_eq!(StopReason::MaxTokens.as_str(), "max_tokens");
        assert_eq!(StopReason::StopSequence.as_str(), "stop_sequence");
    }

    #[test]
    fn from_str_rejects_unknown_value() {
        assert!(StopReason::from_str("done").is_err());
        assert!(StopReason::from_str("end-turn").is_err()); // wrong case style
        assert!(StopReason::from_str("").is_err());
    }

    #[test]
    fn from_str_error_includes_unknown_value() {
        // WHY: the error must surface the unknown value so operators
        // can debug API drift quickly.
        let err = StopReason::from_str("future_reason").unwrap_err();
        assert!(
            err.contains("future_reason"),
            "error message must include the unknown value, got: {err}"
        );
    }
}

// --- Usage ---

mod usage {
    use super::Usage;

    #[test]
    fn default_is_zero() {
        let u = Usage::default();
        assert_eq!(u.input_tokens, 0);
        assert_eq!(u.output_tokens, 0);
        assert_eq!(u.cache_read_tokens, 0);
        assert_eq!(u.cache_write_tokens, 0);
        assert_eq!(u.total(), 0);
    }

    #[test]
    fn total_sums_input_and_output() {
        // WHY: cache_read/cache_write are NOT counted in total — total() is
        // strictly the billable input+output. The contract should not change.
        let u = Usage {
            input_tokens: 100,
            output_tokens: 50,
            cache_read_tokens: 999,
            cache_write_tokens: 999,
        };
        assert_eq!(u.total(), 150);
    }

    #[test]
    fn copy_is_cheap_and_independent() {
        // WHY: Usage derives Copy. A copy must be independent of the
        // original — modifying one must not affect the other if mutated.
        let original = Usage {
            input_tokens: 10,
            output_tokens: 20,
            cache_read_tokens: 0,
            cache_write_tokens: 0,
        };
        let copied = original;
        assert_eq!(original.input_tokens, copied.input_tokens);
        assert_eq!(original.total(), copied.total());
    }
}

// --- JSON serialization round-trips ---

mod serialization {
    use super::{StopReason, Usage};

    #[test]
    fn stop_reason_serializes_to_snake_case_string() {
        let json = serde_json::to_string(&StopReason::EndTurn).expect("serialize");
        assert_eq!(json, r#""end_turn""#);
    }

    #[test]
    fn stop_reason_deserializes_from_snake_case_string() {
        let r: StopReason = serde_json::from_str(r#""tool_use""#).expect("deserialize");
        assert_eq!(r, StopReason::ToolUse);
    }

    #[test]
    fn stop_reason_rejects_kebab_case() {
        // WHY: serde rename_all = "snake_case" means kebab-case must NOT
        // round-trip. Catch any future serde attribute changes.
        let result: Result<StopReason, _> = serde_json::from_str(r#""end-turn""#);
        assert!(result.is_err());
    }

    #[test]
    fn usage_round_trip() {
        let u = Usage {
            input_tokens: 1234,
            output_tokens: 567,
            cache_read_tokens: 89,
            cache_write_tokens: 12,
        };
        let json = serde_json::to_string(&u).expect("serialize");
        let back: Usage = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back.input_tokens, 1234);
        assert_eq!(back.output_tokens, 567);
        assert_eq!(back.cache_read_tokens, 89);
        assert_eq!(back.cache_write_tokens, 12);
    }
}

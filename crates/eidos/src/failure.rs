//! Canonical failure taxonomy shared across every user-facing surface.
//!
//! Aletheia previously had no single vocabulary for "why did this run fail":
//! each crate's error enum told its own story (a `hermeneus` provider error,
//! a `pylon` `ApiError`, an `organon` tool failure) with no shared field a
//! CLI/TUI/desktop/API surface could switch on to render consistent recovery
//! guidance (aletheia#4545). [`FailureCategory`] is that shared field.
//!
//! # Adding a mapping
//!
//! A crate maps its own error type onto [`FailureCategory`] with a `From`
//! impl or a small `fn category(&self) -> FailureCategory` — this module
//! deliberately has zero knowledge of any other crate's error type, so the
//! mapping lives with the error type it classifies, not here.

use serde::{Deserialize, Serialize};

/// High-level classification of why agent work failed.
///
/// Stable identifiers (`snake_case` wire form via [`FailureCategory::as_str`])
/// so a CLI/TUI/desktop/API surface can render consistent guidance without
/// parsing English error text. Sub-reasons (e.g. "provider timeout" vs
/// "provider rejected") are a follow-up refinement per surface (aletheia#4545)
/// once a category is populated broadly; this enum is the shared spine.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum FailureCategory {
    /// The LLM provider was unavailable, rejected the request, timed out, or
    /// returned a malformed response.
    Provider,
    /// A tool call was denied, timed out, or failed during execution.
    Tool,
    /// Memory/context retrieval failed, returned stale or conflicting data,
    /// or exhausted its context budget.
    Memory,
    /// Configuration failed validation, or required credentials/feature
    /// flags were missing or incompatible.
    Config,
    /// A network or SSE connection dropped, stalled, or exhausted its retry
    /// budget.
    Network,
    /// The persistence/storage layer failed to read or write.
    Persistence,
    /// The user explicitly cancelled or interrupted the run.
    Cancellation,
    /// An internal invariant was violated — a bug, not an operational state.
    InternalBug,
}

impl FailureCategory {
    /// Return the `snake_case` string representation of this category.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Provider => "provider",
            Self::Tool => "tool",
            Self::Memory => "memory",
            Self::Config => "config",
            Self::Network => "network",
            Self::Persistence => "persistence",
            Self::Cancellation => "cancellation",
            Self::InternalBug => "internal_bug",
        }
    }

    /// Whether this category is, by its nature, an expected operational
    /// state rather than a bug. `InternalBug` is the sole permanent
    /// exception; every other category can still represent a bug in a
    /// specific instance (e.g. a `Config` failure from a broken generator),
    /// which is why this is a default classification, not a guarantee.
    #[must_use]
    pub fn is_expected_operational_state(self) -> bool {
        !matches!(self, Self::InternalBug)
    }
}

impl std::fmt::Display for FailureCategory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl std::str::FromStr for FailureCategory {
    type Err = String;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        match s {
            "provider" => Ok(Self::Provider),
            "tool" => Ok(Self::Tool),
            "memory" => Ok(Self::Memory),
            "config" => Ok(Self::Config),
            "network" => Ok(Self::Network),
            "persistence" => Ok(Self::Persistence),
            "cancellation" => Ok(Self::Cancellation),
            "internal_bug" => Ok(Self::InternalBug),
            other => Err(format!("unknown failure category: {other}")),
        }
    }
}

/// Whether a failure can plausibly be resolved by retrying, needs the user
/// to act first, or cannot be recovered from at all.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum Recoverability {
    /// Retrying the same request may succeed (e.g. a transient provider
    /// timeout or network stall).
    Retryable,
    /// The user must act before this can succeed (approve a tool, fix
    /// config, provide credentials).
    UserActionRequired,
    /// This specific failure cannot be recovered from; the run must be
    /// abandoned or filed as a bug.
    NotRecoverable,
}

impl Recoverability {
    /// Return the `snake_case` string representation.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Retryable => "retryable",
            Self::UserActionRequired => "user_action_required",
            Self::NotRecoverable => "not_recoverable",
        }
    }
}

impl std::fmt::Display for Recoverability {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// What the user or a reviewing agent can concretely do next about a failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum NextAction {
    /// Retry the same request as-is.
    Retry,
    /// Fix configuration, credentials, or a feature flag and retry.
    Reconfigure,
    /// Approve or deny a pending tool/permission request.
    ApproveOrDeny,
    /// Inspect the run's trace/logs for more detail before deciding.
    InspectTrace,
    /// File a bug — this is not an expected operational state.
    FileIssue,
    /// No action is available; the failure is terminal for this run.
    None,
}

impl NextAction {
    /// Return the `snake_case` string representation.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Retry => "retry",
            Self::Reconfigure => "reconfigure",
            Self::ApproveOrDeny => "approve_or_deny",
            Self::InspectTrace => "inspect_trace",
            Self::FileIssue => "file_issue",
            Self::None => "none",
        }
    }
}

impl std::fmt::Display for NextAction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test assertions")]
mod tests {
    use super::*;

    #[test]
    fn failure_category_round_trips_through_str() {
        for category in [
            FailureCategory::Provider,
            FailureCategory::Tool,
            FailureCategory::Memory,
            FailureCategory::Config,
            FailureCategory::Network,
            FailureCategory::Persistence,
            FailureCategory::Cancellation,
            FailureCategory::InternalBug,
        ] {
            let s = category.as_str();
            assert_eq!(s.parse::<FailureCategory>(), Ok(category));
            assert_eq!(category.to_string(), s);
        }
    }

    #[test]
    fn failure_category_round_trips_through_json() {
        let json = serde_json::to_string(&FailureCategory::Provider).expect("serializes");
        assert_eq!(json, "\"provider\"");
        let back: FailureCategory = serde_json::from_str(&json).expect("deserializes");
        assert_eq!(back, FailureCategory::Provider);
    }

    #[test]
    fn unknown_category_string_is_rejected() {
        assert!("not_a_category".parse::<FailureCategory>().is_err());
    }

    #[test]
    fn internal_bug_is_the_only_non_operational_category() {
        assert!(!FailureCategory::InternalBug.is_expected_operational_state());
        for category in [
            FailureCategory::Provider,
            FailureCategory::Tool,
            FailureCategory::Memory,
            FailureCategory::Config,
            FailureCategory::Network,
            FailureCategory::Persistence,
            FailureCategory::Cancellation,
        ] {
            assert!(category.is_expected_operational_state());
        }
    }

    #[test]
    fn recoverability_and_next_action_round_trip_through_json() {
        let r = serde_json::to_string(&Recoverability::Retryable).expect("serializes");
        assert_eq!(r, "\"retryable\"");
        let a = serde_json::to_string(&NextAction::ApproveOrDeny).expect("serializes");
        assert_eq!(a, "\"approve_or_deny\"");
    }
}

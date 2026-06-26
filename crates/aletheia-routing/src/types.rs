//! Shared routing types: [`RequestFeatures`], [`RoutingDecision`], [`TurnOutcome`], [`RouterError`].

use std::collections::HashMap;
use std::sync::Arc;

use snafu::Snafu;

/// High-level category inferred from a task prompt or user message.
///
/// Used as the aggregation key for per-provider success-rate statistics.
/// Inference is heuristic (keyword matching) and intentionally coarse.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[non_exhaustive]
pub enum TaskCategory {
    /// Code restructuring without behaviour change.
    Refactor,
    /// New product feature.
    Feature,
    /// Defect correction.
    Bug,
    /// Documentation or comment changes.
    Docs,
    /// Tests and test infrastructure.
    Test,
    /// Housekeeping, dependency updates, CI.
    Chore,
}

impl std::fmt::Display for TaskCategory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Refactor => write!(f, "refactor"),
            Self::Feature => write!(f, "feature"),
            Self::Bug => write!(f, "bug"),
            Self::Docs => write!(f, "docs"),
            Self::Test => write!(f, "test"),
            Self::Chore => write!(f, "chore"),
        }
    }
}

impl std::str::FromStr for TaskCategory {
    type Err = std::convert::Infallible;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(match s {
            "refactor" => Self::Refactor,
            "bug" => Self::Bug,
            "docs" => Self::Docs,
            "test" => Self::Test,
            "chore" => Self::Chore,
            // "feature" and any unrecognised string -> Feature
            _ => Self::Feature,
        })
    }
}

impl TaskCategory {
    /// Infer a category from a prompt body or description via keyword matching.
    ///
    /// Returns [`TaskCategory::Feature`] when no keywords match.
    ///
    /// WHY heuristic: full NLP classification would require an LLM call inside
    /// the router's hot path. Keyword matching is O(n) and zero-latency.
    pub fn from_prompt(text: &str) -> Self {
        let lower = text.to_lowercase();
        let tokens = lower
            .split(|c: char| !c.is_ascii_alphanumeric())
            .filter(|token| !token.is_empty());

        let mut is_refactor = false;
        let mut is_bug = false;
        let mut is_test = false;
        let mut is_docs = false;
        let mut is_chore = false;

        for token in tokens {
            match token {
                "refactor" | "restructure" | "rename" => is_refactor = true,
                "fix" | "bug" | "defect" | "regression" => is_bug = true,
                "test" | "spec" | "coverage" => is_test = true,
                "doc" | "docs" | "documentation" | "comment" | "readme" => is_docs = true,
                "chore" | "dependency" | "dependencies" | "deps" | "ci" | "lint" => {
                    is_chore = true;
                }
                _ => {
                    // other tokens are ignored — only keyword hits affect categorisation
                }
            }
        }

        if is_refactor {
            return Self::Refactor;
        }
        if is_test {
            return Self::Test;
        }
        if is_bug {
            return Self::Bug;
        }
        if is_docs {
            return Self::Docs;
        }
        if is_chore {
            return Self::Chore;
        }
        Self::Feature
    }
}

/// Opaque provider identifier (e.g. `"claude"`, `"kimi"`, `"local"`).
///
/// Intentionally a newtype around `Arc<str>` rather than an enum so that new
/// providers can be added at runtime from configuration without code changes.
/// `Arc<str>` avoids the allocation that `String` would require for the
/// common case of comparing/cloning the same provider ID many times per turn.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ProviderId(pub Arc<str>);

impl ProviderId {
    /// Create a new provider ID from any string-like value.
    pub fn new(id: impl Into<Arc<str>>) -> Self {
        Self(id.into())
    }
}

impl serde::Serialize for ProviderId {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.0)
    }
}

impl<'de> serde::Deserialize<'de> for ProviderId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        Ok(Self::new(s.as_str()))
    }
}

impl std::fmt::Display for ProviderId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::ops::Deref for ProviderId {
    type Target = str;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl From<&str> for ProviderId {
    fn from(s: &str) -> Self {
        Self::new(s)
    }
}

impl From<String> for ProviderId {
    fn from(s: String) -> Self {
        Self::new(s.as_str())
    }
}

/// Sovereignty boundary for a routing request.
///
/// Mirrors `hermeneus::provider::DeploymentTarget` without creating a hard
/// dependency on `hermeneus`. Routers use this to filter out candidates whose
/// `deployment_target` is less private than the current request boundary.
///
/// Ordering: `Cloud < LocalHosted < Embedded` (same as the hermeneus variant).
/// A request with `RoutingBoundary::LocalHosted` allows providers at
/// `LocalHosted` *or* `Embedded`, but not `Cloud`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
#[non_exhaustive]
pub enum RoutingBoundary {
    /// External cloud provider allowed. Widest boundary; permits all providers.
    ///
    /// This is the default so routers that have not been updated to pass a
    /// boundary never accidentally restrict routing.
    #[default]
    Cloud,
    /// Only local-hosted or embedded providers (no external API calls).
    LocalHosted,
    /// Only in-process providers (fully air-gapped).
    Embedded,
}

/// Input signals used to make a routing decision.
///
/// Both dispatch and interactive paths populate this struct before calling
/// [`Router::route`]. Fields are optional so paths with less context can
/// leave them as `None`; routers degrade gracefully to fallbacks when
/// features are absent.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct RequestFeatures {
    /// Candidate provider IDs eligible for selection.
    ///
    /// An empty slice causes the router to return its configured static
    /// fallback. Dispatch paths supply all configured providers; interactive
    /// paths supply the currently-active provider from the agent config.
    pub candidates: Vec<ProviderId>,

    /// High-level category for aggregation in the success-rate store.
    ///
    /// When `None`, the store key falls back to [`TaskCategory::Feature`].
    pub task_category: Option<TaskCategory>,

    /// Free-text prompt or message that drove this request.
    ///
    /// Used by category-inference helpers when `task_category` is absent.
    pub prompt_text: Option<Arc<str>>,

    /// Maximum allowed deployment boundary for this request.
    ///
    /// Routers that respect sovereignty must select only providers whose
    /// deployment target is at least as private as this boundary. Defaults to
    /// [`RoutingBoundary::Cloud`] so existing call-sites are not broken.
    ///
    /// WHY(#3969): the Q-learner and fallthrough router need this in context
    /// so they can filter candidates by sovereignty without out-of-band state.
    #[doc(hidden)]
    pub deployment_target: RoutingBoundary,

    candidate_deployment_targets: HashMap<ProviderId, RoutingBoundary>,
}

impl RequestFeatures {
    /// Construct a new `RequestFeatures`.
    ///
    /// WHY: `#[non_exhaustive]` prevents struct-literal construction outside
    /// this crate. This constructor gives callers a stable build path.
    pub fn new(
        candidates: Vec<ProviderId>,
        task_category: Option<TaskCategory>,
        prompt_text: Option<Arc<str>>,
    ) -> Self {
        Self {
            candidates,
            task_category,
            prompt_text,
            deployment_target: RoutingBoundary::default(),
            candidate_deployment_targets: HashMap::new(),
        }
    }

    /// Set the deployment boundary for this request.
    ///
    /// Builder-style setter for call-sites that need sovereignty gating.
    #[must_use]
    pub fn with_deployment_target(mut self, boundary: RoutingBoundary) -> Self {
        self.deployment_target = boundary;
        self
    }

    /// Set the deployment boundary for one candidate provider.
    ///
    /// Routers use configured candidate boundaries to filter providers before
    /// scoring. Candidates without metadata remain eligible so existing callers
    /// do not lose their fallback route when provider config is unavailable.
    #[must_use]
    pub fn with_candidate_deployment_target(
        mut self,
        provider: impl Into<ProviderId>,
        boundary: RoutingBoundary,
    ) -> Self {
        self.candidate_deployment_targets
            .insert(provider.into(), boundary);
        self
    }

    /// Return the configured deployment boundary for `provider`, if known.
    #[must_use]
    pub fn candidate_deployment_target(&self, provider: &ProviderId) -> Option<RoutingBoundary> {
        self.candidate_deployment_targets.get(provider).copied()
    }

    /// Return whether `provider` may receive this request.
    ///
    /// Unknown provider boundaries are allowed for compatibility. Configured
    /// candidates must be at least as private as the request boundary:
    /// `Cloud` accepts every candidate, `LocalHosted` rejects cloud-only
    /// candidates, and `Embedded` accepts only embedded candidates.
    #[must_use]
    pub fn candidate_allowed_by_boundary(&self, provider: &ProviderId) -> bool {
        self.candidate_deployment_target(provider)
            .is_none_or(|boundary| boundary >= self.deployment_target)
    }

    /// Resolve the effective task category.
    ///
    /// Uses `task_category` when set, otherwise infers from `prompt_text`,
    /// and defaults to [`TaskCategory::Feature`] when both are absent.
    pub fn effective_category(&self) -> TaskCategory {
        if let Some(cat) = self.task_category {
            return cat;
        }
        self.prompt_text
            .as_deref()
            .map_or(TaskCategory::Feature, TaskCategory::from_prompt)
    }
}

/// Output of a [`Router::route`] call: selected provider and optional confidence.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct RoutingDecision {
    /// The selected provider identifier.
    pub provider: Arc<str>,

    /// Empirical confidence in the selection (0.0–1.0), if the router has
    /// enough historical data to compute one. `None` for static/fallback
    /// decisions.
    pub confidence: Option<f64>,
}

impl RoutingDecision {
    /// Construct a new routing decision.
    ///
    /// WHY: same `#[non_exhaustive]` constructor rationale as
    /// [`RequestFeatures::new`].
    pub fn new(provider: impl Into<Arc<str>>, confidence: Option<f64>) -> Self {
        Self {
            provider: provider.into(),
            confidence,
        }
    }
}

/// Real outcome dimensions for an interactive turn.
///
/// Replaces the coarse "non-degraded == success" heuristic with explicit
/// signals that can be audited and fed into the empirical router. The
/// dimensions are intentionally independent so that future routers can learn
/// from partial failure patterns rather than a single collapsed boolean.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
#[non_exhaustive]
pub struct InteractiveOutcome {
    /// Whether the turn completed normally (LLM reachable, no provider failure).
    pub completed: bool,
    /// Whether the user corrected or rejected the turn.
    pub user_correction: bool,
    /// Ratio of tool calls that errored, in [0.0, 1.0].
    pub tool_error_rate: f64,
    /// Whether a loop guard fired and replaced the response.
    pub loop_guard_intervention: bool,
    /// Whether a mistake brake fired and replaced the response.
    pub mistake_brake_intervention: bool,
    /// Whether the turn exceeded its budget/cost threshold.
    pub budget_exceeded: bool,
    /// Whether a provider-side failure occurred.
    pub provider_failure: bool,
    /// Optional explicit user rating (e.g., -1/0/+1).
    pub explicit_user_rating: Option<i8>,
}

impl InteractiveOutcome {
    /// Maximum tool-error rate still considered a successful turn.
    ///
    /// WHY: a single tool failure in a multi-tool turn can be normal recovery;
    /// routing signal should degrade only when errors dominate the turn.
    const MAX_ACCEPTABLE_TOOL_ERROR_RATE: f64 = 0.5;

    /// Collapse the outcome dimensions into a single routing success boolean.
    ///
    /// A turn is a routing success only when it completed normally, was not
    /// corrected, had few tool errors, was not interrupted by a guard/brake,
    /// stayed within budget, and had no provider failure.
    #[must_use]
    pub fn is_success(&self) -> bool {
        self.completed
            && !self.user_correction
            && self.tool_error_rate < Self::MAX_ACCEPTABLE_TOOL_ERROR_RATE
            && !self.loop_guard_intervention
            && !self.mistake_brake_intervention
            && !self.budget_exceeded
            && !self.provider_failure
            && self.explicit_user_rating.map_or(true, |r| r >= 0)
    }
}

/// Outcome of a completed turn, fed back via [`Router::after_action`].
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct TurnOutcome {
    /// The provider identifier that handled this turn.
    pub provider: ProviderId,

    /// The model identifier used for this turn, if known separately from the
    /// provider. Kept distinct from `provider` to support #4798.
    pub model: Option<Arc<str>>,

    /// Task category for the aggregation key.
    pub task_category: TaskCategory,

    /// Whether the turn completed successfully.
    ///
    /// WHY: kept as a derived, collapsed boolean so the store can continue to
    /// aggregate success rates without understanding every dimension.
    pub success: bool,

    /// Whether the response path was the interactive (nous) path.
    ///
    /// `false` means dispatch (energeia). Used for observability; the storage
    /// backend is the same regardless of path.
    pub is_interactive: bool,

    /// Interactive outcome dimensions used to derive `success` and for audit.
    ///
    /// `None` for dispatch-path outcomes or older interactive records.
    pub interactive_outcome: Option<InteractiveOutcome>,
}

impl TurnOutcome {
    /// Construct a new turn outcome.
    ///
    /// WHY: same `#[non_exhaustive]` constructor rationale as
    /// [`RequestFeatures::new`]. The collapsed `success` boolean is supplied
    /// directly; use [`Self::with_interactive_outcome`] when the underlying
    /// dimensions are known.
    pub fn new(
        provider: ProviderId,
        task_category: TaskCategory,
        success: bool,
        is_interactive: bool,
    ) -> Self {
        Self {
            provider,
            model: None,
            task_category,
            success,
            is_interactive,
            interactive_outcome: None,
        }
    }

    /// Construct an interactive outcome from its real signal dimensions.
    ///
    /// `success` is derived from `interactive_outcome.is_success()` so the
    /// empirical store cannot accidentally learn from a proxy boolean.
    #[must_use]
    pub fn with_interactive_outcome(
        provider: ProviderId,
        model: Option<Arc<str>>,
        task_category: TaskCategory,
        is_interactive: bool,
        interactive_outcome: InteractiveOutcome,
    ) -> Self {
        Self {
            provider,
            model,
            task_category,
            success: interactive_outcome.is_success(),
            is_interactive,
            interactive_outcome: Some(interactive_outcome),
        }
    }
}

/// Errors from router operations.
#[derive(Debug, Snafu)]
#[snafu(visibility(pub))]
#[non_exhaustive]
pub enum RouterError {
    /// After-action record could not be written to the store.
    #[snafu(display("router after-action write failed: {message}"))]
    AfterActionWrite {
        /// Human-readable error description.
        message: String,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_prompt_matches_keyword_tokens() {
        assert_eq!(
            TaskCategory::from_prompt("rename the parser module"),
            TaskCategory::Refactor
        );
        assert_eq!(
            TaskCategory::from_prompt("add coverage for route selection"),
            TaskCategory::Test
        );
        assert_eq!(
            TaskCategory::from_prompt("fix regression in provider choice"),
            TaskCategory::Bug
        );
        assert_eq!(
            TaskCategory::from_prompt("update README"),
            TaskCategory::Docs
        );
        assert_eq!(
            TaskCategory::from_prompt("update documentation for API"),
            TaskCategory::Docs
        );
        assert_eq!(
            TaskCategory::from_prompt("run CI lint cleanup"),
            TaskCategory::Chore
        );
    }

    #[test]
    fn from_prompt_ignores_keyword_substrings() {
        for prompt in [
            "fixture data setup",
            "prefix normalization",
            "suffix array experiment",
            "affix metadata",
            "contest ranking",
            "testament parser",
            "documentary index",
            "docile retry policy",
            "dock event stream",
            "doctor profile import",
            "splint workflow",
            "lintel metadata",
        ] {
            assert_eq!(
                TaskCategory::from_prompt(prompt),
                TaskCategory::Feature,
                "{prompt}"
            );
        }
    }

    #[test]
    fn from_prompt_prefers_test_when_bug_words_modify_test_work() {
        assert_eq!(
            TaskCategory::from_prompt("fix the test fixture"),
            TaskCategory::Test
        );
    }

    // WHY(#3969): deployment_target field must default to Cloud so existing
    // call-sites using RequestFeatures::new() are not broken.
    #[test]
    fn request_features_deployment_target_defaults_to_cloud() {
        let f = RequestFeatures::new(Vec::new(), None, None);
        assert_eq!(f.deployment_target, RoutingBoundary::Cloud);
    }

    #[test]
    fn routing_boundary_ordering_matches_sovereignty_hierarchy() {
        assert!(RoutingBoundary::Cloud < RoutingBoundary::LocalHosted);
        assert!(RoutingBoundary::LocalHosted < RoutingBoundary::Embedded);
    }

    // WHY(#3969): with_deployment_target is the builder for sovereignty gating.
    #[test]
    fn request_features_with_deployment_target_sets_boundary() {
        let f = RequestFeatures::new(Vec::new(), None, None)
            .with_deployment_target(RoutingBoundary::Embedded);
        assert_eq!(f.deployment_target, RoutingBoundary::Embedded);
    }

    #[test]
    fn request_features_candidate_deployment_targets_gate_boundaries() {
        let f = RequestFeatures::new(Vec::new(), None, None)
            .with_deployment_target(RoutingBoundary::LocalHosted)
            .with_candidate_deployment_target("cloud", RoutingBoundary::Cloud)
            .with_candidate_deployment_target("local", RoutingBoundary::LocalHosted)
            .with_candidate_deployment_target("embedded", RoutingBoundary::Embedded);

        assert!(!f.candidate_allowed_by_boundary(&ProviderId::new("cloud")));
        assert!(f.candidate_allowed_by_boundary(&ProviderId::new("local")));
        assert!(f.candidate_allowed_by_boundary(&ProviderId::new("embedded")));
        assert!(f.candidate_allowed_by_boundary(&ProviderId::new("unknown")));
    }

    #[test]
    fn interactive_outcome_success_requires_clean_completion() {
        let good = InteractiveOutcome {
            completed: true,
            user_correction: false,
            tool_error_rate: 0.0,
            loop_guard_intervention: false,
            mistake_brake_intervention: false,
            budget_exceeded: false,
            provider_failure: false,
            explicit_user_rating: None,
        };
        assert!(good.is_success());
    }

    #[test]
    fn interactive_outcome_failure_modes_do_not_count_as_success() {
        let base = InteractiveOutcome {
            completed: true,
            user_correction: false,
            tool_error_rate: 0.0,
            loop_guard_intervention: false,
            mistake_brake_intervention: false,
            budget_exceeded: false,
            provider_failure: false,
            explicit_user_rating: None,
        };

        assert!(
            !InteractiveOutcome {
                completed: false,
                ..base.clone()
            }
            .is_success()
        );
        assert!(
            !InteractiveOutcome {
                user_correction: true,
                ..base.clone()
            }
            .is_success()
        );
        assert!(
            !InteractiveOutcome {
                tool_error_rate: 1.0,
                ..base.clone()
            }
            .is_success()
        );
        assert!(
            !InteractiveOutcome {
                loop_guard_intervention: true,
                ..base.clone()
            }
            .is_success()
        );
        assert!(
            !InteractiveOutcome {
                mistake_brake_intervention: true,
                ..base.clone()
            }
            .is_success()
        );
        assert!(
            !InteractiveOutcome {
                budget_exceeded: true,
                ..base.clone()
            }
            .is_success()
        );
        assert!(
            !InteractiveOutcome {
                provider_failure: true,
                ..base.clone()
            }
            .is_success()
        );
        assert!(
            !InteractiveOutcome {
                explicit_user_rating: Some(-1),
                ..base
            }
            .is_success()
        );
    }

    #[test]
    fn turn_outcome_with_interactive_outcome_derives_success() {
        let failed = InteractiveOutcome {
            completed: true,
            user_correction: false,
            tool_error_rate: 1.0,
            loop_guard_intervention: false,
            mistake_brake_intervention: false,
            budget_exceeded: false,
            provider_failure: false,
            explicit_user_rating: None,
        };
        let outcome = TurnOutcome::with_interactive_outcome(
            ProviderId::new("p"),
            None,
            TaskCategory::Feature,
            true,
            failed,
        );
        assert!(!outcome.success);
        assert!(outcome.interactive_outcome.is_some());
    }
}

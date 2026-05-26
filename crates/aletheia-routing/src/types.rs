//! Shared routing types: [`RequestFeatures`], [`RoutingDecision`], [`TurnOutcome`], [`RouterError`].

use std::sync::Arc;

use snafu::Snafu;

// ---------------------------------------------------------------------------
// RequestFeatures
// ---------------------------------------------------------------------------

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

// ---------------------------------------------------------------------------
// RequestFeatures
// ---------------------------------------------------------------------------

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
        }
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

// ---------------------------------------------------------------------------
// RoutingDecision
// ---------------------------------------------------------------------------

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
    /// WHY: `#[non_exhaustive]` prevents struct-literal construction outside
    /// this crate. This constructor gives crates that implement or wrap the
    /// trait a stable way to build the struct.
    pub fn new(provider: impl Into<Arc<str>>, confidence: Option<f64>) -> Self {
        Self {
            provider: provider.into(),
            confidence,
        }
    }
}

// ---------------------------------------------------------------------------
// TurnOutcome
// ---------------------------------------------------------------------------

/// Outcome of a completed turn, fed back via [`Router::after_action`].
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct TurnOutcome {
    /// The provider identifier that handled this turn.
    pub provider: ProviderId,

    /// Task category for the aggregation key.
    pub task_category: TaskCategory,

    /// Whether the turn completed successfully.
    pub success: bool,

    /// Whether the response path was the interactive (nous) path.
    ///
    /// `false` means dispatch (energeia). Used for observability; the storage
    /// backend is the same regardless of path.
    pub is_interactive: bool,
}

impl TurnOutcome {
    /// Construct a new turn outcome.
    ///
    /// WHY: `#[non_exhaustive]` prevents struct-literal construction outside
    /// this crate. This constructor gives implementors a stable build path.
    pub fn new(
        provider: ProviderId,
        task_category: TaskCategory,
        success: bool,
        is_interactive: bool,
    ) -> Self {
        Self {
            provider,
            task_category,
            success,
            is_interactive,
        }
    }
}

// ---------------------------------------------------------------------------
// RouterError
// ---------------------------------------------------------------------------

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
}

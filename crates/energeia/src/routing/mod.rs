// WHY: Routing module groups all provider-selection logic — static (operator
// config) and empirical (historical success-rate). Keeping them co-located
// means the EmpiricalRouter can depend on StaticRouter as its fallback without
// introducing a cross-module cycle.

/// After-action record read-side aggregation and rolling statistics.
pub(crate) mod store;

/// Empirical provider router: selects providers by historical success rate.
pub(crate) mod empirical;

// ---------------------------------------------------------------------------
// TaskCategory
// ---------------------------------------------------------------------------

/// High-level category inferred from a task prompt.
///
/// Used as the aggregation key for per-provider success-rate statistics.
/// Inference is heuristic (keyword matching) and intentionally coarse —
/// the follow-up PR (#3455) introduces persona-aware refinement.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[non_exhaustive]
pub(crate) enum TaskCategory {
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

impl TaskCategory {
    /// Infer a category from a prompt body or description via keyword matching.
    ///
    /// Returns `TaskCategory::Feature` when no keywords match.
    ///
    /// WHY heuristic: full NLP classification would require an LLM call inside
    /// the router's hot path. Keyword matching is O(n) and zero-latency. The
    /// follow-up PR (#3455) replaces this with persona-aware routing that
    /// operates on richer metadata.
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "binary wiring consumes from_prompt (follow-up #3455)"
        )
    )]
    pub(crate) fn from_prompt(text: &str) -> Self {
        let lower = text.to_lowercase();
        if lower.contains("refactor") || lower.contains("restructure") || lower.contains("rename") {
            return Self::Refactor;
        }
        if lower.contains("fix")
            || lower.contains("bug")
            || lower.contains("defect")
            || lower.contains("regression")
        {
            return Self::Bug;
        }
        if lower.contains("test") || lower.contains("spec") || lower.contains("coverage") {
            return Self::Test;
        }
        if lower.contains("doc") || lower.contains("comment") || lower.contains("readme") {
            return Self::Docs;
        }
        if lower.contains("chore")
            || lower.contains("dependency")
            || lower.contains("deps")
            || lower.contains("ci")
            || lower.contains("lint")
        {
            return Self::Chore;
        }
        Self::Feature
    }
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

// ---------------------------------------------------------------------------
// ProviderId
// ---------------------------------------------------------------------------

/// Opaque provider identifier (e.g. `"claude"`, `"kimi"`, `"local"`).
///
/// Intentionally a newtype around `String` rather than an enum so that new
/// providers can be added at runtime from configuration without code changes.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub(crate) struct ProviderId(pub(crate) String);

impl ProviderId {
    /// Create a new provider ID from any string-like value.
    pub(crate) fn new(id: impl Into<String>) -> Self {
        Self(id.into())
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

// ---------------------------------------------------------------------------
// StaticRouter
// ---------------------------------------------------------------------------

/// Static provider router: always returns the configured default provider.
///
/// Used as the fallback when the empirical router lacks sufficient data or is
/// disabled via `[dispatch.routing] mode = "static"` (the default).
#[derive(Debug, Clone)]
pub(crate) struct StaticRouter {
    /// The default provider returned for all task categories.
    default_provider: ProviderId,
}

impl StaticRouter {
    /// Create a static router with the given default provider.
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "binary wiring constructs StaticRouter")
    )]
    pub(crate) fn new(default_provider: ProviderId) -> Self {
        Self { default_provider }
    }

    /// Return the configured default provider regardless of category.
    pub(crate) fn pick(&self, _category: TaskCategory) -> &ProviderId {
        &self.default_provider
    }
}

// ---------------------------------------------------------------------------
// RoutingMode
// ---------------------------------------------------------------------------

/// Dispatch routing mode configured by the operator.
///
/// Set via `[dispatch.routing] mode = "..."` in `taxis` configuration.
/// Defaults to `Static` for backward compatibility.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum RoutingMode {
    /// Always use the statically configured provider (default).
    #[default]
    Static,
    /// Use historical success rates to pick providers when data is sufficient.
    Empirical,
}

// ---------------------------------------------------------------------------
// DispatchRoutingConfig
// ---------------------------------------------------------------------------

/// Operator-facing routing configuration for the dispatch engine.
///
/// Placed under `[dispatch.routing]` in the instance `taxis` config.
#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "binary wiring constructs DispatchRoutingConfig (follow-up #3455)"
    )
)]
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub(crate) struct DispatchRoutingConfig {
    /// Routing mode. Defaults to `static`.
    pub(crate) mode: RoutingMode,
    /// Minimum number of historical samples required before empirical routing
    /// overrides the static choice. Defaults to 5.
    pub(crate) min_samples: usize,
    /// Rolling window in days for after-action record weighting. Defaults to 7.
    pub(crate) window_days: u64,
    /// Minimum confidence gap (`winner_rate` - `loser_rate`) required before
    /// switching away from the static provider. Defaults to 0.1 (10 pp).
    pub(crate) confidence_threshold: f64,
    /// Default provider ID returned by the static fallback.
    pub(crate) default_provider: String,
}

impl Default for DispatchRoutingConfig {
    fn default() -> Self {
        Self {
            mode: RoutingMode::Static,
            min_samples: 5,
            window_days: 7,
            confidence_threshold: 0.1,
            default_provider: "claude".to_owned(),
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_prompt_identifies_refactor() {
        assert_eq!(
            TaskCategory::from_prompt("refactor the session manager"),
            TaskCategory::Refactor
        );
    }

    #[test]
    fn from_prompt_identifies_bug() {
        assert_eq!(
            TaskCategory::from_prompt("fix crash in budget tracker"),
            TaskCategory::Bug
        );
    }

    #[test]
    fn from_prompt_identifies_test() {
        assert_eq!(
            TaskCategory::from_prompt("add test coverage for pipeline"),
            TaskCategory::Test
        );
    }

    #[test]
    fn from_prompt_identifies_docs() {
        assert_eq!(
            TaskCategory::from_prompt("update documentation for API"),
            TaskCategory::Docs
        );
    }

    #[test]
    fn from_prompt_identifies_chore() {
        assert_eq!(
            TaskCategory::from_prompt("bump dependency versions"),
            TaskCategory::Chore
        );
    }

    #[test]
    fn from_prompt_defaults_to_feature() {
        assert_eq!(
            TaskCategory::from_prompt("implement empirical router"),
            TaskCategory::Feature
        );
    }

    #[test]
    fn task_category_display() {
        assert_eq!(TaskCategory::Refactor.to_string(), "refactor");
        assert_eq!(TaskCategory::Feature.to_string(), "feature");
        assert_eq!(TaskCategory::Bug.to_string(), "bug");
        assert_eq!(TaskCategory::Docs.to_string(), "docs");
        assert_eq!(TaskCategory::Test.to_string(), "test");
        assert_eq!(TaskCategory::Chore.to_string(), "chore");
    }

    #[test]
    fn static_router_always_returns_default() {
        let router = StaticRouter::new(ProviderId::new("claude"));
        assert_eq!(router.pick(TaskCategory::Bug).0, "claude");
        assert_eq!(router.pick(TaskCategory::Refactor).0, "claude");
    }

    #[test]
    fn routing_mode_default_is_static() {
        let mode = RoutingMode::default();
        assert_eq!(mode, RoutingMode::Static);
    }

    #[test]
    fn dispatch_routing_config_default() {
        let cfg = DispatchRoutingConfig::default();
        assert_eq!(cfg.mode, RoutingMode::Static);
        assert_eq!(cfg.min_samples, 5);
        assert_eq!(cfg.window_days, 7);
        assert!((cfg.confidence_threshold - 0.1).abs() < f64::EPSILON);
        assert_eq!(cfg.default_provider, "claude");
    }

    #[test]
    fn provider_id_deref() {
        let id = ProviderId::new("kimi");
        assert_eq!(&*id, "kimi");
        assert_eq!(id.to_string(), "kimi");
    }
}

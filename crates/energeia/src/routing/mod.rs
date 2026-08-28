// WHY: Routing module groups all provider-selection logic — static (operator
// config) and empirical (historical success-rate). Keeping them co-located
// means the EmpiricalRouter can depend on StaticRouter as its fallback without
// introducing a cross-module cycle.
//
// Types shared with the interactive path (nous) live in the `aletheia-routing`
// crate; this module re-exports them under the original paths.

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use aletheia_routing::types::{RequestFeatures, RoutingBoundary};

/// After-action record read-side aggregation and rolling statistics.
///
/// Thin wrapper — the implementation lives in `aletheia-routing::store` so it
/// can be shared with the interactive path without coupling nous to energeia.
pub(crate) mod store;

/// Empirical provider router: selects providers by historical success rate.
pub(crate) mod empirical;

/// Persona-aware router: model-tier + role selection on top of empirical routing.
///
/// The classifier (`persona_classifier.rs`) supplies a `(ModelTier, PersonaRole)`
/// hint; the router carries it in `PersonaDecision`.
pub(crate) mod persona;

/// AST-style prompt classifier for persona-based routing.
///
/// Replaces the keyword heuristic in [`aletheia_routing::types::TaskCategory::from_prompt`]
/// with a markdown-structure-aware scorer. Heading keywords are weighted 2×
/// body keywords (phronesis design). Returns `None` when confidence is below
/// the threshold so callers can fall back to the keyword heuristic.
pub(crate) mod persona_classifier;

/// Expertise-affinity router: prefers providers with historical success in the
/// requested [`TaskCategory`].
///
/// Extends [`PersonaRouter`](persona::PersonaRouter) with a four-dimension
/// weighted affinity score (category match 40%, consistency 30%, breadth 20%,
/// recency 10%) that acts as a tiebreaker when the empirical confidence gap is
/// narrow.
pub(crate) mod affinity;

/// Shared `#[cfg(test)]` after-action JSONL fixture builders for the
/// empirical/persona/affinity router test modules.
#[cfg(test)]
pub(crate) mod test_support;

pub(crate) use aletheia_routing::types::{ProviderId, TaskCategory};

pub(crate) const DEFAULT_PROVIDER_ID: &str = "claude";
const SECS_PER_DAY: u64 = 24 * 60 * 60;

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
    pub(crate) fn new(default_provider: ProviderId) -> Self {
        Self { default_provider }
    }

    /// Return the configured default provider regardless of category.
    pub(crate) fn pick(&self, _category: TaskCategory) -> &ProviderId {
        &self.default_provider
    }
}

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

/// Operator-facing routing configuration for the dispatch engine.
///
/// Placed under `[dispatch.routing]` in the instance `taxis` config.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(default)]
#[serde(deny_unknown_fields)]
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
    /// Minimum affinity-score gap required before overriding the empirical
    /// selection. Defaults to 0.15.
    pub(crate) affinity_threshold: f64,
    /// Default provider ID returned by the static fallback.
    pub(crate) default_provider: String,
    /// Candidate provider/model IDs eligible for empirical routing.
    pub(crate) candidate_providers: Vec<String>,
    /// Maximum deployment boundary this dispatch instance may route to.
    ///
    /// WHY(#3969): the sovereignty half of the Q-learning router's two
    /// pulled-forward precursors — `RequestFeatures::deployment_target`
    /// (and the `candidate_allowed_by_boundary` filtering it drives inside
    /// `AffinityRouter`/`EmpiricalRouter`'s `route()`) already existed, but
    /// nothing on this — the one live production `route()` consumer —
    /// call path ever set it to anything but the always-permissive default.
    /// Defaults to [`RoutingBoundary::Cloud`] so existing operator configs
    /// are unaffected until this is explicitly tightened.
    pub(crate) sovereignty_boundary: RoutingBoundary,
    /// Per-candidate deployment boundary overrides, keyed by provider/model
    /// ID (matching `candidate_providers`/`default_provider`).
    ///
    /// A candidate absent from this map is treated as
    /// [`RoutingBoundary::Cloud`] by
    /// [`RequestFeatures::candidate_allowed_by_boundary`] — i.e. unknown
    /// candidates stay eligible rather than being silently excluded, so an
    /// operator who sets `sovereignty_boundary` without also populating
    /// this map does not lose every candidate at once.
    pub(crate) candidate_deployment_targets: HashMap<String, RoutingBoundary>,
}

impl Default for DispatchRoutingConfig {
    fn default() -> Self {
        Self {
            mode: RoutingMode::Static,
            min_samples: 5,
            window_days: 7,
            confidence_threshold: 0.1,
            affinity_threshold: 0.15,
            default_provider: DEFAULT_PROVIDER_ID.to_owned(),
            candidate_providers: Vec::new(),
            sovereignty_boundary: RoutingBoundary::default(),
            candidate_deployment_targets: HashMap::new(),
        }
    }
}

impl DispatchRoutingConfig {
    pub(crate) async fn model_for_prompt(
        &self,
        prompt_text: &str,
        after_action_log_dir: Option<&Path>,
    ) -> Option<String> {
        let category = TaskCategory::from_prompt(prompt_text);
        let provider = match self.mode {
            RoutingMode::Static => {
                let router = StaticRouter::new(ProviderId::new(self.default_provider.as_str()));
                router.pick(category).clone()
            }
            RoutingMode::Empirical => {
                self.empirical_model_for_prompt(prompt_text, category, after_action_log_dir)
                    .await
            }
        };

        if provider.0.as_ref() == DEFAULT_PROVIDER_ID {
            None
        } else {
            Some(provider.to_string())
        }
    }

    async fn empirical_model_for_prompt(
        &self,
        prompt_text: &str,
        category: TaskCategory,
        after_action_log_dir: Option<&Path>,
    ) -> ProviderId {
        let window = self.window();
        let store = Arc::new(match after_action_log_dir {
            Some(dir) => {
                let store = store::AfterActionStore::new_with_window(dir.to_owned(), window);
                if let Err(error) = store.refresh_window(window).await {
                    tracing::warn!(
                        error = %error,
                        "failed to refresh after-action store for dispatch routing"
                    );
                }
                store
            }
            None => store::AfterActionStore::in_memory(),
        });

        let fallback = StaticRouter::new(ProviderId::new(self.default_provider.as_str()));
        let empirical = empirical::EmpiricalRouter::new(
            Arc::clone(&store),
            fallback,
            u64::try_from(self.min_samples).unwrap_or(u64::MAX),
            window,
            self.confidence_threshold,
        );
        let persona = persona::PersonaRouter::new(empirical);
        let affinity =
            affinity::AffinityRouter::new(persona, store, window, self.affinity_threshold);
        let mut features = RequestFeatures::new(
            self.candidates(),
            Some(category),
            Some(Arc::<str>::from(prompt_text)),
        )
        .with_deployment_target(self.sovereignty_boundary);
        for (provider, boundary) in &self.candidate_deployment_targets {
            features = features.with_candidate_deployment_target(provider.as_str(), *boundary);
        }

        let decision = affinity.route_with_affinity(&features, None).await;
        ProviderId::new(decision.base.provider)
    }

    fn candidates(&self) -> Vec<ProviderId> {
        let mut providers = vec![ProviderId::new(self.default_provider.as_str())];
        for provider in &self.candidate_providers {
            if provider != &self.default_provider {
                providers.push(ProviderId::new(provider.as_str()));
            }
        }
        providers
    }

    fn window(&self) -> Duration {
        Duration::from_secs(self.window_days.saturating_mul(SECS_PER_DAY))
    }
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions")]
#[expect(clippy::expect_used, reason = "test assertions")]
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
    fn from_prompt_marks_unmatched_prompt_unknown() {
        assert_eq!(
            TaskCategory::from_prompt("implement empirical router"),
            TaskCategory::Unknown
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
        assert_eq!(TaskCategory::Unknown.to_string(), "unknown");
    }

    #[test]
    fn static_router_always_returns_default() {
        let router = StaticRouter::new(ProviderId::new(DEFAULT_PROVIDER_ID));
        assert_eq!(&*router.pick(TaskCategory::Bug).0, DEFAULT_PROVIDER_ID);
        assert_eq!(&*router.pick(TaskCategory::Refactor).0, DEFAULT_PROVIDER_ID);
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
        assert!((cfg.affinity_threshold - 0.15).abs() < f64::EPSILON);
        assert_eq!(cfg.default_provider, DEFAULT_PROVIDER_ID);
        assert!(cfg.candidate_providers.is_empty());
    }

    #[test]
    fn provider_id_deref() {
        let id = ProviderId::new("kimi");
        assert_eq!(&*id, "kimi");
        assert_eq!(id.to_string(), "kimi");
    }

    #[test]
    fn dispatch_routing_config_sovereignty_defaults_to_cloud() {
        let cfg = DispatchRoutingConfig::default();
        assert_eq!(cfg.sovereignty_boundary, RoutingBoundary::Cloud);
        assert!(cfg.candidate_deployment_targets.is_empty());
    }

    #[test]
    fn dispatch_routing_config_deserializes_without_sovereignty_fields() {
        // WHY(#3969): #[serde(default)] must hold for every existing operator
        // config written before these fields existed.
        let cfg: DispatchRoutingConfig = serde_yaml::from_str(
            "mode: static\n\
             default_provider: claude\n",
        )
        .expect("config without sovereignty fields must still deserialize");
        assert_eq!(cfg.sovereignty_boundary, RoutingBoundary::Cloud);
    }

    fn write_session_line(
        dir: &std::path::Path,
        filename: &str,
        model: &str,
        status: &str,
        category: &str,
    ) {
        use std::io::Write as _;
        let path = dir.join(filename);
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .unwrap();
        writeln!(
            file,
            "{}",
            serde_json::json!({
                "dispatch_id": "test",
                "ts_start": "2026-04-17T00:00:00Z",
                "ts_end": "2026-04-17T00:01:00Z",
                "duration_ms": 60000,
                "session_outcomes": [{"model": model, "status": status, "category": category}],
                "cost_total_cents": 5,
                "turns_total": 10,
                "stage_latencies_ms": {},
                "qa_verdict": "pass",
                "prompt_hash": "sha256:abc"
            })
        )
        .unwrap();
    }

    /// WHY(#3969): proves the sovereignty boundary is enforced on the one
    /// live production `route()` consumer (`empirical_model_for_prompt`),
    /// not merely at the lower-level `RequestFeatures`/`EmpiricalRouter`
    /// unit tests already covering the filtering primitive itself. A
    /// cloud-only candidate with a dominant success rate must never win when
    /// the configured boundary excludes it.
    #[tokio::test]
    async fn empirical_model_for_prompt_excludes_cloud_candidate_under_local_boundary() {
        let tmp = tempfile::tempdir().unwrap();
        // WHY(#5217): "implement a feature" has no keyword signal, so it
        // categorizes as Unknown; the stats below must be keyed "unknown"
        // for the empirical path to engage at all.
        for _ in 0..10 {
            write_session_line(
                tmp.path(),
                "2026-04-17.jsonl",
                "cloud-model",
                "success",
                "unknown",
            );
        }
        for _ in 0..6 {
            write_session_line(
                tmp.path(),
                "2026-04-17.jsonl",
                "local-model",
                "success",
                "unknown",
            );
        }
        for _ in 0..4 {
            write_session_line(
                tmp.path(),
                "2026-04-17.jsonl",
                "local-model",
                "failed",
                "unknown",
            );
        }

        let mut candidate_deployment_targets = HashMap::new();
        candidate_deployment_targets.insert("cloud-model".to_owned(), RoutingBoundary::Cloud);
        candidate_deployment_targets.insert("local-model".to_owned(), RoutingBoundary::LocalHosted);

        let cfg = DispatchRoutingConfig {
            mode: RoutingMode::Empirical,
            min_samples: 5,
            default_provider: "local-model".to_owned(),
            candidate_providers: vec!["cloud-model".to_owned(), "local-model".to_owned()],
            sovereignty_boundary: RoutingBoundary::LocalHosted,
            candidate_deployment_targets,
            ..DispatchRoutingConfig::default()
        };

        let chosen = cfg
            .model_for_prompt("implement a feature", Some(tmp.path()))
            .await;

        // WHY: `model_for_prompt` returns `None` when the chosen provider is
        // the crate's DEFAULT_PROVIDER_ID sentinel; a real, non-sentinel
        // model choice is what proves routing happened at all.
        assert_ne!(
            chosen.as_deref(),
            Some("cloud-model"),
            "sovereignty boundary must exclude the cloud-only candidate even \
             though it has the dominant success rate"
        );
    }
}

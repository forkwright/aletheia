// WHY: PersonaRouter adds model-tier and role selection on top of the empirical
// success-rate routing layer: Opus-class for architecture/multi-file tasks,
// Sonnet-class for well-scoped execution (phronesis persona design).
//
// Interaction with EmpiricalRouter:
//   PersonaRouter wraps EmpiricalRouter for provider selection, then overlays
//   model-tier + role on top. When empirical data is absent the static fallback
//   decides both provider and model tier.

use aletheia_routing::types::{RequestFeatures, TurnOutcome};
use aletheia_routing::{BoxFuture, Router, RouterError, RoutingDecision};
use tracing::instrument;

use super::empirical::EmpiricalRouter;
use super::persona_classifier;

/// LLM capability tier selected by the persona router.
///
/// Maps to a family of models (not a pinned model version) so that the
/// underlying provider config can evolve without changing routing logic.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[non_exhaustive]
pub(crate) enum ModelTier {
    /// Lightweight model for mechanical, low-complexity work.
    ///
    /// Maps to Haiku-class models. Chosen when complexity signals are absent
    /// or explicitly low (lint, fmt, typo, docs).
    Light,
    /// Standard model for well-scoped feature and bug-fix work.
    ///
    /// Maps to Sonnet-class models. The default tier for most dispatches.
    Standard,
    /// Frontier model for architecture, multi-crate, and ambiguous specs.
    ///
    /// Maps to Opus-class models. Only chosen when the complexity classifier
    /// finds strong architecture or multi-file signals.
    Frontier,
}

impl std::fmt::Display for ModelTier {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Light => write!(f, "light"),
            Self::Standard => write!(f, "standard"),
            Self::Frontier => write!(f, "frontier"),
        }
    }
}

/// Dispatch persona (role) assigned to a session.
///
/// Phronesis encoded role as part of the system prompt sent to the agent.
/// Here the role is carried in `PersonaDecision` so the orchestrator can
/// inject the appropriate role context when building the prompt prefix.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[non_exhaustive]
pub(crate) enum PersonaRole {
    /// Architect — strategic, multi-crate, cross-cutting concerns.
    ///
    /// Paired with [`ModelTier::Frontier`] for high-complexity tasks.
    Architect,
    /// Engineer — standard feature and bug-fix implementation.
    ///
    /// Paired with [`ModelTier::Standard`] for typical dispatch tasks.
    Engineer,
    /// Mechanic — mechanical, deterministic low-complexity work.
    ///
    /// Paired with [`ModelTier::Light`] for lint, fmt, and chore tasks.
    Mechanic,
}

impl std::fmt::Display for PersonaRole {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Architect => write!(f, "architect"),
            Self::Engineer => write!(f, "engineer"),
            Self::Mechanic => write!(f, "mechanic"),
        }
    }
}

/// Extended routing decision that includes model tier and persona role.
///
/// Returned by [`PersonaRouter::route_with_persona`]. The base
/// [`RoutingDecision`] (provider + confidence) is embedded so the caller can
/// use either the unified `Router` trait or the richer persona decision.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub(crate) struct PersonaDecision {
    /// Underlying provider + empirical confidence.
    pub(crate) base: RoutingDecision,
    /// Selected model tier.
    pub(crate) model_tier: ModelTier,
    /// Selected persona role.
    pub(crate) persona_role: PersonaRole,
    /// Human-readable rationale for the persona selection.
    ///
    /// Never read within this crate — carried for binary wiring and logging.
    /// WHY: The `tracing::debug!` in `route_with_persona` uses the local
    /// variable `rationale` (before `PersonaDecision::new` consumes it), not
    /// the struct field. The field exists solely for consumers outside this crate.
    #[expect(dead_code, reason = "binary wiring reads PersonaDecision::rationale")]
    pub(crate) rationale: String,
}

impl PersonaDecision {
    /// Construct a new persona decision.
    pub(crate) fn new(
        base: RoutingDecision,
        model_tier: ModelTier,
        persona_role: PersonaRole,
        rationale: impl Into<String>,
    ) -> Self {
        Self {
            base,
            model_tier,
            persona_role,
            rationale: rationale.into(),
        }
    }
}

/// Persona-aware provider router.
///
/// Wraps [`EmpiricalRouter`] for provider selection and overlays model-tier +
/// role selection on top. The empirical layer picks the provider with the best
/// historical success rate; the persona layer decides how to configure the
/// session (model tier + role prompt).
///
/// # Interaction with the classifier
///
/// `route_with_persona` accepts a pre-computed `(ModelTier, PersonaRole)` so
/// the persona classifier can inject its result without coupling
/// the router to the classifier. Call sites that have no classifier available
/// pass `None` and the router falls back to complexity-free tier selection.
///
/// # No changes to `aletheia-routing`
///
/// `PersonaRouter` implements the shared `Router` trait by delegating to
/// `EmpiricalRouter::route`. The persona extension is available via the
/// additional method `route_with_persona`, which callers invoke directly.
pub(crate) struct PersonaRouter {
    inner: EmpiricalRouter,
}

impl PersonaRouter {
    /// Create a new persona router wrapping the given empirical router.
    pub(crate) fn new(inner: EmpiricalRouter) -> Self {
        Self { inner }
    }

    /// Route with full persona decision: provider + model tier + role.
    ///
    /// If `persona_hint` is `Some`, that tier/role pair is used directly
    /// (caller-supplied, e.g. from an outer dispatch loop). If `None`, the
    /// router invokes [`persona_classifier::classify_prompt`] on `prompt_text`
    /// from `features`; when the classifier returns `None` (low confidence) the
    /// router falls back to [`ModelTier::Standard`] + [`PersonaRole::Engineer`].
    ///
    /// WHY: Wiring the classifier directly into the router means callers that
    /// have access to `RequestFeatures.prompt_text` get automatic persona
    /// selection without extra glue code. Callers that need to override the
    /// classifier output (e.g. explicit frontmatter) pass a hint.
    #[instrument(skip(self), fields(
        persona_hint_tier = ?persona_hint.as_ref().map(|(t, _)| t),
        persona_hint_role = ?persona_hint.as_ref().map(|(_, r)| r),
    ))]
    pub(crate) async fn route_with_persona(
        &self,
        features: &RequestFeatures,
        persona_hint: Option<(ModelTier, PersonaRole)>,
    ) -> PersonaDecision {
        let base = self.inner.route(features).await;

        let (model_tier, persona_role, rationale) = if let Some((tier, role)) = persona_hint {
            let rationale = format!(
                "hint-assigned tier={tier} role={role} for provider={}",
                base.provider
            );
            (tier, role, rationale)
        } else if let Some(prompt) = features.prompt_text.as_deref() {
            if let Some(classified) = persona_classifier::classify_prompt(prompt) {
                let rationale = format!(
                    "classifier-assigned tier={} role={} conf={:.2} for provider={}",
                    classified.model_tier,
                    classified.persona_role,
                    classified.confidence,
                    base.provider
                );
                (classified.model_tier, classified.persona_role, rationale)
            } else {
                let rationale = format!(
                    "classifier-deferred (low confidence) → standard/engineer for provider={}",
                    base.provider
                );
                (ModelTier::Standard, PersonaRole::Engineer, rationale)
            }
        } else {
            let rationale = format!(
                "no prompt text → default tier=standard role=engineer for provider={}",
                base.provider
            );
            (ModelTier::Standard, PersonaRole::Engineer, rationale)
        };

        tracing::debug!(
            provider = %base.provider,
            %model_tier,
            %persona_role,
            %rationale,
            "persona routing decision"
        );

        PersonaDecision::new(base, model_tier, persona_role, rationale)
    }
}

impl Router for PersonaRouter {
    /// Route using the empirical success-rate model.
    ///
    /// Returns the same decision as the inner [`EmpiricalRouter`]. Use
    /// [`route_with_persona`](Self::route_with_persona) for model-tier + role.
    fn route<'a>(&'a self, features: &'a RequestFeatures) -> BoxFuture<'a, RoutingDecision> {
        self.inner.route(features)
    }

    /// Record an after-action outcome into the shared store.
    fn after_action(
        &self,
        decision: &RoutingDecision,
        outcome: &TurnOutcome,
    ) -> Result<(), RouterError> {
        self.inner.after_action(decision, outcome)
    }
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions")]
mod tests {
    use std::io::Write as _;
    use std::sync::Arc;

    use aletheia_routing::DEFAULT_ROUTING_WINDOW;

    use super::*;
    use crate::routing::store::AfterActionStore;
    use crate::routing::{ProviderId, StaticRouter, TaskCategory};

    fn session_line(model: &str, status: &str, category: &str) -> serde_json::Value {
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
    }

    fn write_jsonl(dir: &std::path::Path, filename: &str, lines: &[serde_json::Value]) {
        let path = dir.join(filename);
        let mut file = std::fs::File::create(path).unwrap();
        for line in lines {
            writeln!(file, "{line}").unwrap();
        }
    }

    async fn make_persona_router(dir: &std::path::Path, default: &str) -> PersonaRouter {
        let store = Arc::new(AfterActionStore::new(dir.to_owned()));
        store.refresh().await.unwrap();
        let empirical = EmpiricalRouter::new(
            store,
            StaticRouter::new(ProviderId::new(default)),
            5,
            DEFAULT_ROUTING_WINDOW,
            0.1,
        );
        PersonaRouter::new(empirical)
    }

    /// Without a classifier hint, router returns Standard/Engineer.
    #[tokio::test]
    async fn defaults_to_standard_engineer_without_hint() {
        let tmp = tempfile::tempdir().unwrap();
        let router = make_persona_router(tmp.path(), "claude").await;
        let features = aletheia_routing::types::RequestFeatures::new(
            vec![ProviderId::new("claude")],
            Some(TaskCategory::Feature),
            None,
        );
        let decision = router.route_with_persona(&features, None).await;
        assert_eq!(decision.model_tier, ModelTier::Standard);
        assert_eq!(decision.persona_role, PersonaRole::Engineer);
        assert_eq!(&*decision.base.provider, "claude");
    }

    /// Classifier hint is respected: Frontier/Architect when supplied.
    #[tokio::test]
    async fn respects_classifier_hint_frontier() {
        let tmp = tempfile::tempdir().unwrap();
        let router = make_persona_router(tmp.path(), "claude").await;
        let features = aletheia_routing::types::RequestFeatures::new(
            vec![ProviderId::new("claude")],
            Some(TaskCategory::Feature),
            None,
        );
        let hint = Some((ModelTier::Frontier, PersonaRole::Architect));
        let decision = router.route_with_persona(&features, hint).await;
        assert_eq!(decision.model_tier, ModelTier::Frontier);
        assert_eq!(decision.persona_role, PersonaRole::Architect);
    }

    /// Classifier hint: Light/Mechanic.
    #[tokio::test]
    async fn respects_classifier_hint_light() {
        let tmp = tempfile::tempdir().unwrap();
        let router = make_persona_router(tmp.path(), "claude").await;
        let features = aletheia_routing::types::RequestFeatures::new(
            vec![ProviderId::new("claude")],
            Some(TaskCategory::Chore),
            None,
        );
        let hint = Some((ModelTier::Light, PersonaRole::Mechanic));
        let decision = router.route_with_persona(&features, hint).await;
        assert_eq!(decision.model_tier, ModelTier::Light);
        assert_eq!(decision.persona_role, PersonaRole::Mechanic);
    }

    /// Router trait delegation still picks empirical winner.
    #[tokio::test]
    async fn router_trait_delegates_to_empirical() {
        let tmp = tempfile::tempdir().unwrap();
        let mut lines = vec![];
        for _ in 0..9 {
            lines.push(session_line("winner", "success", "feature"));
        }
        lines.push(session_line("winner", "failed", "feature"));
        for _ in 0..2 {
            lines.push(session_line("loser", "success", "feature"));
        }
        for _ in 0..8 {
            lines.push(session_line("loser", "failed", "feature"));
        }
        write_jsonl(tmp.path(), "2026-04-17.jsonl", &lines);

        let router = make_persona_router(tmp.path(), "loser").await;
        let features = aletheia_routing::types::RequestFeatures::new(
            vec![ProviderId::new("winner"), ProviderId::new("loser")],
            Some(TaskCategory::Feature),
            None,
        );
        let decision = router.route(&features).await;
        assert_eq!(&*decision.provider, "winner");
    }

    /// `ModelTier` display values.
    #[test]
    fn model_tier_display() {
        assert_eq!(ModelTier::Light.to_string(), "light");
        assert_eq!(ModelTier::Standard.to_string(), "standard");
        assert_eq!(ModelTier::Frontier.to_string(), "frontier");
    }

    /// `PersonaRole` display values.
    #[test]
    fn persona_role_display() {
        assert_eq!(PersonaRole::Architect.to_string(), "architect");
        assert_eq!(PersonaRole::Engineer.to_string(), "engineer");
        assert_eq!(PersonaRole::Mechanic.to_string(), "mechanic");
    }
}

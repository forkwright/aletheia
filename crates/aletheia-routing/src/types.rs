//! Shared routing types: [`RequestFeatures`], [`RoutingDecision`], [`TurnOutcome`], [`RouterError`].

use std::collections::HashMap;
use std::sync::Arc;

use snafu::Snafu;

/// High-level category inferred from a task prompt or user message.
///
/// Used as the aggregation key for per-provider success-rate statistics.
/// Inference is heuristic (keyword matching) and intentionally coarse.
///
/// [`TaskCategory::Unknown`] is the explicit outcome for work the heuristic
/// cannot classify: unclassified tasks aggregate in their own bucket rather
/// than silently polluting feature-routing statistics (#5217).
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
    /// Categorization could not classify the task (no keyword match, missing
    /// task info, or an unrecognized persisted value). Explicitly *not* a
    /// feature: low-confidence categorization stays visible for review and
    /// training instead of being laundered into [`TaskCategory::Feature`].
    Unknown,
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
            Self::Unknown => write!(f, "unknown"),
        }
    }
}

impl std::str::FromStr for TaskCategory {
    type Err = std::convert::Infallible;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(match s {
            "refactor" => Self::Refactor,
            "feature" => Self::Feature,
            "bug" => Self::Bug,
            "docs" => Self::Docs,
            "test" => Self::Test,
            "chore" => Self::Chore,
            // "unknown" and any unrecognised string -> Unknown (#5217):
            // unrecognized persisted values must not masquerade as features.
            _ => Self::Unknown,
        })
    }
}

impl TaskCategory {
    /// Infer a category from a prompt body or description via keyword matching.
    ///
    /// Returns [`TaskCategory::Unknown`] when no keywords match — the absence
    /// of a signal is reported as absence, not defaulted to `Feature`.
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
        Self::Unknown
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
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Default,
    serde::Serialize,
    serde::Deserialize,
)]
#[serde(rename_all = "lowercase")]
#[non_exhaustive]
pub enum RoutingBoundary {
    /// External cloud provider allowed. Widest boundary; permits all providers.
    ///
    /// This is the default so routers that have not been updated to pass a
    /// boundary never accidentally restrict routing. External-channel ingress
    /// is the exception: see [`RequestFeatures::applied_boundary`].
    #[default]
    Cloud,
    /// Only local-hosted or embedded providers (no external API calls).
    LocalHosted,
    /// Only in-process providers (fully air-gapped).
    Embedded,
}

/// Origin a routing request's turn arrived on.
///
/// The privacy posture of a request depends on where the turn came from: an
/// operator at the local TUI/REPL is trusted input, while a turn arriving over
/// an external channel (agora: Signal, Matrix, ...) is untrusted-by-default
/// input whose routing must not silently assume the cloud-permissive boundary
/// (#5219).
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum IngressSource {
    /// Direct operator turn (TUI, REPL, local HTTP API).
    #[default]
    Operator,
    /// Turn arrived over an external channel.
    ExternalChannel {
        /// Channel identifier (e.g. `"signal"`, `"matrix"`).
        channel: Arc<str>,
    },
}

impl IngressSource {
    /// Whether this ingress is an external channel.
    #[must_use]
    pub fn is_external_channel(&self) -> bool {
        matches!(self, Self::ExternalChannel { .. })
    }

    /// The external channel identifier, when this is an external-channel
    /// ingress.
    #[must_use]
    pub fn channel(&self) -> Option<&str> {
        match self {
            Self::Operator => None,
            Self::ExternalChannel { channel } => Some(channel),
        }
    }

    /// Stable string form for durable audit records
    /// (`"operator"` or `"external_channel:<channel>"`).
    #[must_use]
    pub fn wire_name(&self) -> String {
        match self {
            Self::Operator => "operator".to_owned(),
            Self::ExternalChannel { channel } => format!("external_channel:{channel}"),
        }
    }
}

/// How the privacy boundary in force for a request was chosen (#5219).
///
/// The source matters for audit: an operator who explicitly configured
/// `Cloud` for channel traffic made an informed choice; a boundary that
/// merely *defaulted* to `Cloud` is an accident of omission.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum BoundarySource {
    /// Caller supplied the boundary explicitly
    /// ([`RequestFeatures::with_deployment_target`]).
    Explicit,
    /// No boundary supplied; the operator-ingress default
    /// ([`RoutingBoundary::Cloud`]) applied.
    OperatorDefault,
    /// No boundary supplied for an external-channel turn; policy clamped the
    /// boundary to [`RoutingBoundary::LocalHosted`] so channel-origin work
    /// never silently routes at the cloud-permissive default.
    ExternalChannelDefault,
}

/// The privacy boundary in force for a request and how it was chosen (#5219).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AppliedBoundary {
    /// Boundary actually applied to candidate filtering.
    pub boundary: RoutingBoundary,
    /// Whether the caller set it explicitly or policy derived it.
    pub source: BoundarySource,
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
    /// When `None`, the store key resolves to [`TaskCategory::Unknown`]
    /// (never silently `Feature`).
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
    /// Read [`Self::applied_boundary`] rather than this field directly: for
    /// external-channel ingress without an explicit boundary, policy clamps
    /// the applied boundary instead of silently permitting cloud (#5219).
    #[doc(hidden)]
    pub deployment_target: RoutingBoundary,

    candidate_deployment_targets: HashMap<ProviderId, RoutingBoundary>,
    /// Where the request's turn arrived from (#5219).
    ingress: IngressSource,
    /// Whether `deployment_target` was set explicitly via
    /// [`Self::with_deployment_target`] rather than left at the `Cloud`
    /// default. Distinguishes "the caller chose cloud" from "nobody chose",
    /// which [`Self::applied_boundary`] needs for the external-channel clamp.
    boundary_explicit: bool,
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
            ingress: IngressSource::default(),
            boundary_explicit: false,
        }
    }

    /// Construct features for a turn that arrived over an external channel
    /// (agora: Signal, Matrix, ...).
    ///
    /// Unlike [`Self::new`], the privacy boundary is a required argument:
    /// channel-origin turns must carry an explicit, operator-chosen boundary
    /// rather than drifting into one by default (#5219). Callers without an
    /// operator-configured boundary should pass [`RoutingBoundary::LocalHosted`].
    #[must_use]
    pub fn for_external_channel(
        channel: impl Into<Arc<str>>,
        candidates: Vec<ProviderId>,
        task_category: Option<TaskCategory>,
        prompt_text: Option<Arc<str>>,
        boundary: RoutingBoundary,
    ) -> Self {
        Self::new(candidates, task_category, prompt_text)
            .with_ingress(IngressSource::ExternalChannel {
                channel: channel.into(),
            })
            .with_deployment_target(boundary)
    }

    /// Record where this request's turn arrived from (#5219).
    #[must_use]
    pub fn with_ingress(mut self, ingress: IngressSource) -> Self {
        self.ingress = ingress;
        self
    }

    /// Where this request's turn arrived from.
    #[must_use]
    pub fn ingress(&self) -> &IngressSource {
        &self.ingress
    }

    /// Set the deployment boundary for this request.
    ///
    /// Builder-style setter for call-sites that need sovereignty gating.
    /// Marks the boundary as explicitly chosen (see [`Self::applied_boundary`]).
    #[must_use]
    pub fn with_deployment_target(mut self, boundary: RoutingBoundary) -> Self {
        self.deployment_target = boundary;
        self.boundary_explicit = true;
        self
    }

    /// The privacy boundary actually in force for this request, with its
    /// provenance (#5219).
    ///
    /// An explicitly supplied boundary always wins. When no boundary was
    /// supplied, external-channel turns clamp to [`RoutingBoundary::LocalHosted`]
    /// (channel-origin work is sensitive unless classified otherwise); only
    /// operator-direct turns keep the `Cloud` compatibility default.
    #[must_use]
    pub fn applied_boundary(&self) -> AppliedBoundary {
        if self.boundary_explicit {
            return AppliedBoundary {
                boundary: self.deployment_target,
                source: BoundarySource::Explicit,
            };
        }
        if self.ingress.is_external_channel() {
            return AppliedBoundary {
                boundary: RoutingBoundary::LocalHosted,
                source: BoundarySource::ExternalChannelDefault,
            };
        }
        AppliedBoundary {
            boundary: self.deployment_target,
            source: BoundarySource::OperatorDefault,
        }
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
    /// candidates must be at least as private as the applied request boundary
    /// (see [`Self::applied_boundary`]): `Cloud` accepts every candidate,
    /// `LocalHosted` rejects cloud-only candidates, and `Embedded` accepts
    /// only embedded candidates.
    #[must_use]
    pub fn candidate_allowed_by_boundary(&self, provider: &ProviderId) -> bool {
        self.candidate_deployment_target(provider)
            .is_none_or(|boundary| boundary >= self.applied_boundary().boundary)
    }

    /// Resolve the effective task category.
    ///
    /// Uses `task_category` when set, otherwise infers from `prompt_text`,
    /// and reports [`TaskCategory::Unknown`] when neither classifies the task.
    pub fn effective_category(&self) -> TaskCategory {
        if let Some(cat) = self.task_category {
            return cat;
        }
        self.prompt_text
            .as_deref()
            .map_or(TaskCategory::Unknown, TaskCategory::from_prompt)
    }
}

/// Which layer of a composed router stack produced a routing decision (#5218).
#[derive(Debug, Clone, Default, PartialEq)]
#[non_exhaustive]
pub enum DecisionOrigin {
    /// A single router produced the decision directly (no fallthrough
    /// composition involved).
    #[default]
    Direct,
    /// The primary router of a fallthrough stack produced the decision.
    Primary,
    /// The fallback router of a fallthrough stack produced the decision
    /// because the primary's decision was not confident enough to accept.
    Fallback {
        /// Provider the primary attempted before the fallback took over.
        primary_provider: Arc<str>,
        /// Why the primary's decision was not accepted.
        reason: FallbackReason,
    },
}

/// Why a fallthrough stack rejected the primary router's decision (#5218).
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub enum FallbackReason {
    /// The primary returned no confidence signal at all.
    ConfidenceAbsent,
    /// The primary's confidence fell below the configured fallthrough
    /// threshold.
    ConfidenceBelowThreshold {
        /// Confidence the primary reported.
        confidence: f64,
        /// Configured acceptance threshold it was measured against.
        threshold: f64,
    },
}

/// Provenance of a routing decision: which router layer made it, why a
/// fallback handled it, and under which privacy boundary and ingress it was
/// made (#5218, #5219). Durable by construction — carried on the decision
/// record itself, not emitted as a log line.
#[derive(Debug, Clone, Default)]
#[non_exhaustive]
pub struct DecisionProvenance {
    /// Which layer of the router stack made the selection.
    pub origin: DecisionOrigin,
    /// Privacy boundary applied while selecting, when the router stamped it.
    pub boundary: Option<RoutingBoundary>,
    /// How `boundary` was chosen (explicit vs policy-derived).
    pub boundary_source: Option<BoundarySource>,
    /// Ingress the request arrived on, when the router stamped it.
    pub ingress: Option<IngressSource>,
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

    /// Which router layer made this decision and under what request context
    /// (#5218, #5219).
    pub provenance: DecisionProvenance,
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
            provenance: DecisionProvenance::default(),
        }
    }

    /// Stamp the decision with the request context it was made under:
    /// applied privacy boundary (with its source) and ingress (#5219).
    #[must_use]
    pub fn with_request_provenance(mut self, features: &RequestFeatures) -> Self {
        let applied = features.applied_boundary();
        self.provenance.boundary = Some(applied.boundary);
        self.provenance.boundary_source = Some(applied.source);
        self.provenance.ingress = Some(features.ingress().clone());
        self
    }

    /// Stamp which layer of a composed router stack made this decision (#5218).
    #[must_use]
    pub fn with_origin(mut self, origin: DecisionOrigin) -> Self {
        self.provenance.origin = origin;
        self
    }
}

/// Whether an interactive turn reached a normal completion.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum CompletionStatus {
    /// The turn did not complete normally.
    #[default]
    Incomplete,
    /// The turn completed normally.
    Completed,
}

/// Whether the user corrected or rejected an interactive turn.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum CorrectionStatus {
    /// No correction or rejection was observed.
    #[default]
    Clear,
    /// The user corrected or rejected the turn.
    Corrected,
}

/// Whether a runtime guard intervened in an interactive turn.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum InterventionStatus {
    /// The guard did not intervene.
    #[default]
    Clear,
    /// The guard intervened and replaced the response.
    Triggered,
}

/// Whether an interactive turn stayed within its budget.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum BudgetStatus {
    /// The turn stayed within its budget.
    #[default]
    WithinLimit,
    /// The turn exceeded its budget or cost threshold.
    Exceeded,
}

/// Whether the provider reported a failure for an interactive turn.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum ProviderStatus {
    /// No provider-side failure was observed.
    #[default]
    Available,
    /// A provider-side failure occurred.
    Failed,
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
    /// Whether the turn completed normally.
    pub completion: CompletionStatus,
    /// Whether the user corrected or rejected the turn.
    pub user_correction: CorrectionStatus,
    /// Ratio of tool calls that errored, in [0.0, 1.0].
    pub tool_error_rate: f64,
    /// Whether a loop guard fired and replaced the response.
    pub loop_guard: InterventionStatus,
    /// Whether a mistake brake fired and replaced the response.
    pub mistake_brake: InterventionStatus,
    /// Whether the turn exceeded its budget/cost threshold.
    pub budget: BudgetStatus,
    /// Whether a provider-side failure occurred.
    pub provider: ProviderStatus,
    /// Optional explicit user rating (e.g., -1/0/+1).
    pub explicit_user_rating: Option<i8>,
}

impl InteractiveOutcome {
    /// Maximum tool-error rate still considered a successful turn.
    ///
    /// WHY: a single tool failure in a multi-tool turn can be normal recovery;
    /// routing signal should degrade only when errors dominate the turn.
    const MAX_ACCEPTABLE_TOOL_ERROR_RATE: f64 = 0.5;

    /// Construct an interactive outcome with neutral failure modifiers.
    ///
    /// Correction, guard, budget, and provider signals default to clear states;
    /// pass [`CompletionStatus::Incomplete`] when the turn itself failed.
    #[must_use]
    pub fn new(completion: CompletionStatus, tool_error_rate: f64) -> Self {
        Self {
            completion,
            tool_error_rate,
            ..Self::default()
        }
    }

    /// Attach the observed user-correction signal.
    #[must_use]
    pub fn with_user_correction(mut self, status: CorrectionStatus) -> Self {
        self.user_correction = status;
        self
    }

    /// Attach the observed loop-guard signal.
    #[must_use]
    pub fn with_loop_guard(mut self, status: InterventionStatus) -> Self {
        self.loop_guard = status;
        self
    }

    /// Attach the observed mistake-brake signal.
    #[must_use]
    pub fn with_mistake_brake(mut self, status: InterventionStatus) -> Self {
        self.mistake_brake = status;
        self
    }

    /// Attach the observed budget signal.
    #[must_use]
    pub fn with_budget(mut self, status: BudgetStatus) -> Self {
        self.budget = status;
        self
    }

    /// Attach the observed provider signal.
    #[must_use]
    pub fn with_provider(mut self, status: ProviderStatus) -> Self {
        self.provider = status;
        self
    }

    /// Attach an explicit user rating.
    #[must_use]
    pub fn with_explicit_user_rating(mut self, rating: Option<i8>) -> Self {
        self.explicit_user_rating = rating;
        self
    }

    /// Collapse the outcome dimensions into a single routing success boolean.
    ///
    /// A turn is a routing success only when it completed normally, was not
    /// corrected, had few tool errors, was not interrupted by a guard/brake,
    /// stayed within budget, and had no provider failure.
    #[must_use]
    pub fn is_success(&self) -> bool {
        self.completion == CompletionStatus::Completed
            && self.user_correction == CorrectionStatus::Clear
            && self.tool_error_rate < Self::MAX_ACCEPTABLE_TOOL_ERROR_RATE
            && self.loop_guard == InterventionStatus::Clear
            && self.mistake_brake == InterventionStatus::Clear
            && self.budget == BudgetStatus::WithinLimit
            && self.provider == ProviderStatus::Available
            && self.explicit_user_rating.is_none_or(|r| r >= 0)
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

    /// Ingress the turn arrived on (#5219). Operator-direct by default;
    /// external-channel turns carry the channel identifier so the audit
    /// trail shows which turns came in over the wire.
    pub ingress: IngressSource,

    /// Privacy boundary in force for this turn's routing context, when
    /// known (#5219). Records the posture the routing layer applied to
    /// decisions for this turn; it does not by itself gate interactive
    /// model selection.
    pub boundary: Option<RoutingBoundary>,

    /// How `boundary` was chosen (explicit vs policy-derived).
    pub boundary_source: Option<BoundarySource>,
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
            ingress: IngressSource::default(),
            boundary: None,
            boundary_source: None,
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
            ingress: IngressSource::default(),
            boundary: None,
            boundary_source: None,
        }
    }

    /// Record where this turn arrived from (#5219).
    #[must_use]
    pub fn with_ingress(mut self, ingress: IngressSource) -> Self {
        self.ingress = ingress;
        self
    }

    /// Record the privacy boundary in force for this turn's routing context
    /// (#5219).
    #[must_use]
    pub fn with_boundary_provenance(mut self, applied: AppliedBoundary) -> Self {
        self.boundary = Some(applied.boundary);
        self.boundary_source = Some(applied.source);
        self
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

    // WHY(#5217): substring traps must not keyword-match, and a prompt with
    // no keyword signal is Unknown — not silently Feature.
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
                TaskCategory::Unknown,
                "{prompt}"
            );
        }
    }

    // WHY(#5217): no keyword match is an explicit unknown categorization,
    // never a silent feature default.
    #[test]
    fn from_prompt_without_keyword_match_is_unknown() {
        assert_eq!(
            TaskCategory::from_prompt("implement empirical router"),
            TaskCategory::Unknown
        );
        assert_eq!(TaskCategory::from_prompt(""), TaskCategory::Unknown);
    }

    // WHY(#5217): "feature" itself still parses, but unrecognized persisted
    // strings land in the Unknown bucket instead of masquerading as features.
    #[test]
    fn from_str_maps_unrecognized_strings_to_unknown() {
        assert_eq!("feature".parse::<TaskCategory>(), Ok(TaskCategory::Feature));
        assert_eq!("bug".parse::<TaskCategory>(), Ok(TaskCategory::Bug));
        assert_eq!("unknown".parse::<TaskCategory>(), Ok(TaskCategory::Unknown));
        assert_eq!("quantum".parse::<TaskCategory>(), Ok(TaskCategory::Unknown));
        assert_eq!("".parse::<TaskCategory>(), Ok(TaskCategory::Unknown));
    }

    // WHY(#5217): missing task info is Unknown, not Feature.
    #[test]
    fn effective_category_without_task_info_is_unknown() {
        let f = RequestFeatures::new(Vec::new(), None, None);
        assert_eq!(f.effective_category(), TaskCategory::Unknown);

        let f = RequestFeatures::new(Vec::new(), None, Some(Arc::from("ship the thing")));
        assert_eq!(f.effective_category(), TaskCategory::Unknown);

        let f = RequestFeatures::new(Vec::new(), None, Some(Arc::from("fix the parser")));
        assert_eq!(f.effective_category(), TaskCategory::Bug);
    }

    #[test]
    fn task_category_display_covers_unknown() {
        assert_eq!(TaskCategory::Unknown.to_string(), "unknown");
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

    // WHY(#5219): an operator-direct request with no explicit boundary keeps
    // the Cloud compatibility default, and the record says so.
    #[test]
    fn applied_boundary_defaults_to_cloud_for_operator_ingress() {
        let f = RequestFeatures::new(Vec::new(), None, None);
        let applied = f.applied_boundary();
        assert_eq!(applied.boundary, RoutingBoundary::Cloud);
        assert_eq!(applied.source, BoundarySource::OperatorDefault);
        assert_eq!(f.ingress(), &IngressSource::Operator);
    }

    // WHY(#5219): an external-channel turn with no explicit boundary must NOT
    // silently route at the cloud default — policy clamps to LocalHosted and
    // the provenance says the clamp, not an operator, chose it.
    #[test]
    fn external_channel_ingress_without_boundary_clamps_to_local_hosted() {
        let f = RequestFeatures::new(Vec::new(), None, None)
            .with_ingress(IngressSource::ExternalChannel {
                channel: Arc::from("signal"),
            })
            .with_candidate_deployment_target("cloud-only", RoutingBoundary::Cloud)
            .with_candidate_deployment_target("local", RoutingBoundary::LocalHosted);

        let applied = f.applied_boundary();
        assert_eq!(applied.boundary, RoutingBoundary::LocalHosted);
        assert_eq!(applied.source, BoundarySource::ExternalChannelDefault);
        assert!(
            !f.candidate_allowed_by_boundary(&ProviderId::new("cloud-only")),
            "channel-origin work must not silently admit a cloud-only candidate"
        );
        assert!(f.candidate_allowed_by_boundary(&ProviderId::new("local")));
        assert!(f.candidate_allowed_by_boundary(&ProviderId::new("unlisted")));
    }

    // WHY(#5219): an operator who explicitly configures Cloud for channel
    // traffic made an informed choice; the provenance records it as explicit.
    #[test]
    fn external_channel_ingress_honors_explicit_cloud_boundary() {
        let f = RequestFeatures::for_external_channel(
            "matrix",
            Vec::new(),
            None,
            None,
            RoutingBoundary::Cloud,
        )
        .with_candidate_deployment_target("cloud-only", RoutingBoundary::Cloud);

        let applied = f.applied_boundary();
        assert_eq!(applied.boundary, RoutingBoundary::Cloud);
        assert_eq!(applied.source, BoundarySource::Explicit);
        assert!(f.candidate_allowed_by_boundary(&ProviderId::new("cloud-only")));
        assert_eq!(f.ingress().channel(), Some("matrix"));
    }

    #[test]
    fn ingress_source_wire_names_are_stable() {
        assert_eq!(IngressSource::Operator.wire_name(), "operator");
        assert_eq!(
            IngressSource::ExternalChannel {
                channel: Arc::from("signal"),
            }
            .wire_name(),
            "external_channel:signal"
        );
        assert!(!IngressSource::Operator.is_external_channel());
    }

    // WHY(#5218/#5219): every decision stamped from request features carries
    // the boundary (with its source) and the ingress it was made under.
    #[test]
    fn routing_decision_stamps_request_provenance() {
        let f = RequestFeatures::new(Vec::new(), None, None).with_ingress(
            IngressSource::ExternalChannel {
                channel: Arc::from("signal"),
            },
        );
        let decision = RoutingDecision::new("local-model", Some(0.5)).with_request_provenance(&f);

        assert_eq!(
            decision.provenance.boundary,
            Some(RoutingBoundary::LocalHosted)
        );
        assert_eq!(
            decision.provenance.boundary_source,
            Some(BoundarySource::ExternalChannelDefault)
        );
        assert_eq!(
            decision
                .provenance
                .ingress
                .as_ref()
                .and_then(|i| i.channel()),
            Some("signal")
        );
        assert_eq!(decision.provenance.origin, DecisionOrigin::Direct);
    }

    #[test]
    fn turn_outcome_carries_ingress_and_boundary_provenance() {
        let outcome = TurnOutcome::new(ProviderId::new("p"), TaskCategory::Unknown, true, true)
            .with_ingress(IngressSource::ExternalChannel {
                channel: Arc::from("matrix"),
            })
            .with_boundary_provenance(AppliedBoundary {
                boundary: RoutingBoundary::LocalHosted,
                source: BoundarySource::ExternalChannelDefault,
            });

        assert_eq!(outcome.ingress.channel(), Some("matrix"));
        assert_eq!(outcome.boundary, Some(RoutingBoundary::LocalHosted));
        assert_eq!(
            outcome.boundary_source,
            Some(BoundarySource::ExternalChannelDefault)
        );

        let plain = TurnOutcome::new(ProviderId::new("p"), TaskCategory::Bug, true, false);
        assert_eq!(plain.ingress, IngressSource::Operator);
        assert_eq!(plain.boundary, None);
        assert_eq!(plain.boundary_source, None);
    }

    #[test]
    fn interactive_outcome_success_requires_clean_completion() {
        let good = InteractiveOutcome {
            completion: CompletionStatus::Completed,
            user_correction: CorrectionStatus::Clear,
            tool_error_rate: 0.0,
            loop_guard: InterventionStatus::Clear,
            mistake_brake: InterventionStatus::Clear,
            budget: BudgetStatus::WithinLimit,
            provider: ProviderStatus::Available,
            explicit_user_rating: None,
        };
        assert!(good.is_success());
    }

    #[test]
    fn interactive_outcome_failure_modes_do_not_count_as_success() {
        let base = InteractiveOutcome {
            completion: CompletionStatus::Completed,
            user_correction: CorrectionStatus::Clear,
            tool_error_rate: 0.0,
            loop_guard: InterventionStatus::Clear,
            mistake_brake: InterventionStatus::Clear,
            budget: BudgetStatus::WithinLimit,
            provider: ProviderStatus::Available,
            explicit_user_rating: None,
        };

        assert!(
            !InteractiveOutcome {
                completion: CompletionStatus::Incomplete,
                ..base.clone()
            }
            .is_success()
        );
        assert!(
            !InteractiveOutcome {
                user_correction: CorrectionStatus::Corrected,
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
                loop_guard: InterventionStatus::Triggered,
                ..base.clone()
            }
            .is_success()
        );
        assert!(
            !InteractiveOutcome {
                mistake_brake: InterventionStatus::Triggered,
                ..base.clone()
            }
            .is_success()
        );
        assert!(
            !InteractiveOutcome {
                budget: BudgetStatus::Exceeded,
                ..base.clone()
            }
            .is_success()
        );
        assert!(
            !InteractiveOutcome {
                provider: ProviderStatus::Failed,
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
            completion: CompletionStatus::Completed,
            user_correction: CorrectionStatus::Clear,
            tool_error_rate: 1.0,
            loop_guard: InterventionStatus::Clear,
            mistake_brake: InterventionStatus::Clear,
            budget: BudgetStatus::WithinLimit,
            provider: ProviderStatus::Available,
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

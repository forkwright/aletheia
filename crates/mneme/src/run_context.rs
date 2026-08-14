// kanon:ignore RUST/file-too-long WHY: cohesive run-context provenance contract with co-located inspection helpers and focused tests; split when a downstream durable store implementation lands.
//! Run-context provenance and inspection records.
//!
//! A run can be trusted only when its memory influence is inspectable. The
//! types in this module form the durable contract that downstream runtime and
//! UI layers can persist with a turn: selected context, excluded context,
//! staleness and supersession state, selection reasons, run-caused memory
//! updates, and explicit export redaction behavior.

use std::fmt;

use serde::{Deserialize, Serialize};

use crate::id::FactId;
use crate::knowledge::{
    EpistemicTier, Fact, FactSensitivity, ForgetReason, MemoryScope, Visibility,
};
use crate::recall::{FactorScores, ScoredResult};
use crate::workspace::ProjectId;

/// Selection criteria that can explain why a memory item was admitted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum ContextSelectionReason {
    /// Query terms matched the memory content or metadata.
    Lexical,
    /// Embedding or semantic search matched the memory.
    Semantic,
    /// Recent creation or access increased the item score.
    Recency,
    /// A user or agent explicitly pinned the item into context.
    ManualPin,
    /// The item matched the active agent identity.
    AgentIdentity,
    /// The item matched the active project partition.
    ProjectScope,
    /// Prior access frequency increased the item score.
    AccessFrequency,
    /// Graph relationship proximity increased the item score.
    RelationshipProximity,
    /// Graph centrality increased the item score.
    GraphImportance,
    /// Evidence-gap coverage increased the item score.
    EvidenceCoverage,
    /// Serendipity scoring admitted a distant but useful item.
    Serendipity,
    /// Bayesian surprise scoring admitted a topic-shift item.
    Surprise,
    /// Consolidated evidence convergence increased the item score.
    Convergence,
    /// A caller-specific criterion not represented by a first-class variant.
    Other,
}

impl ContextSelectionReason {
    /// Stable `snake_case` label for logs, JSON summaries, and inspection text.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Lexical => "lexical",
            Self::Semantic => "semantic",
            Self::Recency => "recency",
            Self::ManualPin => "manual_pin",
            Self::AgentIdentity => "agent_identity",
            Self::ProjectScope => "project_scope",
            Self::AccessFrequency => "access_frequency",
            Self::RelationshipProximity => "relationship_proximity",
            Self::GraphImportance => "graph_importance",
            Self::EvidenceCoverage => "evidence_coverage",
            Self::Serendipity => "serendipity",
            Self::Surprise => "surprise",
            Self::Convergence => "convergence",
            Self::Other => "other",
        }
    }
}

/// Hard or soft reason a context candidate was not selected.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum ContextExclusionReason {
    /// The memory has been intentionally forgotten.
    Forgotten,
    /// The memory has been superseded by a newer item.
    Superseded,
    /// The memory exceeded its effective stability window.
    Stale,
    /// Visibility policy prevented the active run from seeing the item.
    Visibility,
    /// Provider deployment target or export policy blocked the item.
    Sensitivity,
    /// Team-memory scope did not match the active run.
    ScopeMismatch,
    /// Project partition did not match the active run.
    ProjectMismatch,
    /// No lexical match was found.
    NoLexicalMatch,
    /// No semantic match was found.
    NoSemanticMatch,
    /// The item scored below the retrieval threshold.
    LowScore,
    /// The item ranked below the returned context limit.
    LimitTruncation,
    /// A manual deny-list or operator decision excluded the item.
    ManualDeny,
    /// A caller-specific exclusion not represented by a first-class variant.
    Other,
}

impl ContextExclusionReason {
    /// Stable `snake_case` label for logs, JSON summaries, and inspection text.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Forgotten => "forgotten",
            Self::Superseded => "superseded",
            Self::Stale => "stale",
            Self::Visibility => "visibility",
            Self::Sensitivity => "sensitivity",
            Self::ScopeMismatch => "scope_mismatch",
            Self::ProjectMismatch => "project_mismatch",
            Self::NoLexicalMatch => "no_lexical_match",
            Self::NoSemanticMatch => "no_semantic_match",
            Self::LowScore => "low_score",
            Self::LimitTruncation => "limit_truncation",
            Self::ManualDeny => "manual_deny",
            Self::Other => "other",
        }
    }
}

/// Retrieval decision for a candidate context item.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum ContextDecision {
    /// Included in the run prompt or available working context.
    Selected,
    /// Considered but omitted by a hard gate or ranking cutoff.
    Excluded,
    /// Relevant but lower ranked than the selected context limit.
    LowerRanked,
}

impl ContextDecision {
    /// Stable `snake_case` label for logs, JSON summaries, and inspection text.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Selected => "selected",
            Self::Excluded => "excluded",
            Self::LowerRanked => "lower_ranked",
        }
    }
}

/// Lifecycle state used for run-context trust decisions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum MemoryLifecycleState {
    /// Current, not superseded, not forgotten, and within stability bounds.
    Active,
    /// Current but beyond its effective stability window.
    Stale,
    /// Replaced by another memory item.
    Superseded,
    /// No longer domain-valid at the observed run time.
    Invalidated,
    /// Intentionally forgotten.
    Forgotten,
}

impl MemoryLifecycleState {
    /// Stable `snake_case` label for logs, JSON summaries, and inspection text.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Stale => "stale",
            Self::Superseded => "superseded",
            Self::Invalidated => "invalidated",
            Self::Forgotten => "forgotten",
        }
    }
}

/// Evidence pointer supporting a context item.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContextEvidenceRef {
    /// Source category such as `session`, `message`, `tool`, `document`, or `fact`.
    pub source_type: String,
    /// Stable source identifier inside that source category.
    // kanon:ignore RUST/primitive-for-domain-id WHY: cross-source evidence references may point to sessions, messages, tools, documents, or facts; a single source-specific newtype would be misleading.
    pub source_id: String,
    /// Relationship between the source and the context item.
    pub relation: String,
}

impl ContextEvidenceRef {
    /// Construct an evidence pointer.
    #[must_use]
    pub fn new(
        source_type: impl Into<String>,
        source_id: impl Into<String>,
        relation: impl Into<String>,
    ) -> Self {
        Self {
            source_type: source_type.into(),
            source_id: source_id.into(),
            relation: relation.into(),
        }
    }
}

/// Lifecycle timing and trust semantics for a context item.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextLifecycle {
    /// Computed lifecycle state at the time the run observed this item.
    pub state: MemoryLifecycleState,
    /// When the fact became valid in the represented domain.
    pub valid_from: Option<jiff::Timestamp>,
    /// When the fact stopped being valid in the represented domain.
    pub valid_to: Option<jiff::Timestamp>,
    /// When Aletheia recorded the memory item.
    pub recorded_at: Option<jiff::Timestamp>,
    /// Most recent known update time, if the source record exposes one.
    pub updated_at: Option<jiff::Timestamp>,
    /// Most recent known access time, if available.
    pub last_accessed_at: Option<jiff::Timestamp>,
    /// Timestamp when the item became invalid, forgotten, or explicitly expired.
    pub invalidated_at: Option<jiff::Timestamp>,
    /// Replacement fact ID, if this item has been superseded.
    pub superseded_by: Option<String>,
    /// Forget reason when the item was intentionally removed from recall.
    pub forget_reason: Option<ForgetReason>,
    /// Effective stability window after tier multiplier, in hours.
    pub stale_after_hours: Option<f64>,
    /// Age at observation time, in hours.
    pub observed_age_hours: Option<f64>,
}

impl ContextLifecycle {
    /// Compute lifecycle semantics from a stored fact.
    #[must_use]
    pub fn from_fact(fact: &Fact, observed_at: jiff::Timestamp) -> Self {
        let stale_after_hours =
            effective_stability_hours(fact.provenance.stability_hours, fact.provenance.tier);
        let observed_age_hours = age_hours_since(observed_at, fact.temporal.recorded_at);
        let is_stale = match (observed_age_hours, stale_after_hours) {
            (Some(age), Some(stability)) => age >= stability,
            _ => false,
        };
        let is_invalidated = fact.temporal.valid_to <= observed_at;
        let state = if fact.lifecycle.is_forgotten {
            MemoryLifecycleState::Forgotten
        } else if fact.lifecycle.superseded_by.is_some() {
            MemoryLifecycleState::Superseded
        } else if is_invalidated {
            MemoryLifecycleState::Invalidated
        } else if is_stale {
            MemoryLifecycleState::Stale
        } else {
            MemoryLifecycleState::Active
        };
        let invalidated_at = if fact.lifecycle.is_forgotten {
            fact.lifecycle.forgotten_at
        } else if is_invalidated {
            Some(fact.temporal.valid_to)
        } else {
            None
        };

        Self {
            state,
            valid_from: Some(fact.temporal.valid_from),
            valid_to: Some(fact.temporal.valid_to),
            recorded_at: Some(fact.temporal.recorded_at),
            updated_at: None,
            last_accessed_at: fact.access.last_accessed_at,
            invalidated_at,
            superseded_by: fact.lifecycle.superseded_by.as_ref().map(FactId::to_string),
            forget_reason: fact.lifecycle.forget_reason,
            stale_after_hours,
            observed_age_hours,
        }
    }
}

/// Complete source and trust provenance for one context item.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextItemProvenance {
    /// Source category such as `fact`, `message`, `note`, or `document`.
    pub source_type: String,
    /// Stable source identifier within the source category.
    // kanon:ignore RUST/primitive-for-domain-id WHY: context items can originate from heterogeneous memory planes, so the stable wire contract stores the source kind and source id together.
    pub source_id: String,
    /// Agent or memory owner that owns the item.
    // kanon:ignore RUST/primitive-for-domain-id WHY: NousId is not a mneme dependency; this facade keeps the serialized cross-crate identifier as its wire string.
    pub nous_id: String,
    /// Source session that produced the item, if known.
    pub source_session_id: Option<String>,
    /// Raw context content.
    ///
    /// WARNING: use [`RunContextRecord::inspection_report`] before exposing a
    /// record outside an authorized internal inspection surface.
    pub content: String,
    /// Confidence score attached to the source memory.
    pub confidence: f64,
    /// Epistemic trust tier attached to the source memory.
    pub tier: EpistemicTier,
    /// Data-sovereignty class for provider and export policy.
    pub sensitivity: FactSensitivity,
    /// Visibility boundary for cross-agent and external surfaces.
    pub visibility: Visibility,
    /// Team-memory scope, if the source record is scoped.
    pub scope: Option<MemoryScope>,
    /// Project partition, if the source record is project-scoped.
    pub project_id: Option<ProjectId>,
    /// Lifecycle and staleness semantics observed during the run.
    pub lifecycle: ContextLifecycle,
    /// Evidence pointers supporting this context item.
    pub evidence: Vec<ContextEvidenceRef>,
}

impl ContextItemProvenance {
    /// Build provenance from a stored fact.
    #[must_use]
    pub fn from_fact(fact: &Fact, observed_at: jiff::Timestamp) -> Self {
        let evidence = match fact.provenance.source_session_id.as_ref() {
            Some(source_session_id) => vec![ContextEvidenceRef::new(
                "session",
                source_session_id.clone(),
                "source_session",
            )],
            None => Vec::new(),
        };

        Self {
            source_type: "fact".to_owned(),
            source_id: fact.id.to_string(),
            nous_id: fact.nous_id.clone(),
            source_session_id: fact.provenance.source_session_id.clone(),
            content: fact.content.clone(),
            confidence: fact.provenance.confidence,
            tier: fact.provenance.tier,
            sensitivity: fact.sensitivity,
            visibility: fact.visibility,
            scope: fact.scope,
            project_id: fact.project_id.clone(),
            lifecycle: ContextLifecycle::from_fact(fact, observed_at),
            evidence,
        }
    }
}

/// Score components captured with a selected or lower-ranked context item.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ContextScoreFactors {
    /// Cosine or semantic similarity score.
    pub vector_similarity: f64,
    /// Recency and FSRS decay score.
    pub decay: f64,
    /// Agent identity relevance score.
    pub relevance: f64,
    /// Epistemic trust-tier score.
    pub epistemic_tier: f64,
    /// Graph relationship proximity score.
    pub relationship_proximity: f64,
    /// Access-frequency score.
    pub access_frequency: f64,
    /// Graph centrality score.
    pub graph_importance: f64,
    /// Serendipity score.
    pub serendipity: f64,
    /// Bayesian-surprise score.
    pub surprise: f64,
    /// Evidence-gap coverage score.
    pub evidence_coverage: f64,
    /// Consolidated-evidence convergence score.
    pub convergence: f64,
}

impl From<&FactorScores> for ContextScoreFactors {
    fn from(factors: &FactorScores) -> Self {
        Self {
            vector_similarity: factors.vector_similarity,
            decay: factors.decay,
            relevance: factors.relevance,
            epistemic_tier: factors.epistemic_tier,
            relationship_proximity: factors.relationship_proximity,
            access_frequency: factors.access_frequency,
            graph_importance: factors.graph_importance,
            serendipity: factors.serendipity,
            surprise: factors.surprise,
            evidence_coverage: factors.evidence_coverage,
            convergence: factors.convergence,
        }
    }
}

/// One memory or context candidate considered for a run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextItem {
    /// Source, lifecycle, trust, and evidence metadata.
    pub provenance: ContextItemProvenance,
    /// Retrieval decision applied to the item.
    pub decision: ContextDecision,
    /// Positive selection criteria that affected ranking or admission.
    pub selection_reasons: Vec<ContextSelectionReason>,
    /// Exclusion criteria for omitted or lower-ranked items.
    pub exclusion_reasons: Vec<ContextExclusionReason>,
    /// One-based rank after scoring, if ranked.
    pub rank: Option<usize>,
    /// Final composite score, if scored.
    pub score: Option<f64>,
    /// Raw scoring factors, if the selector exposed them.
    pub factors: Option<ContextScoreFactors>,
}

impl ContextItem {
    /// Build a selected context item.
    #[must_use]
    pub fn selected(
        provenance: ContextItemProvenance,
        rank: usize,
        score: f64,
        selection_reasons: Vec<ContextSelectionReason>,
    ) -> Self {
        Self {
            provenance,
            decision: ContextDecision::Selected,
            selection_reasons,
            exclusion_reasons: Vec::new(),
            rank: Some(rank),
            score: Some(score),
            factors: None,
        }
    }

    /// Build an excluded context item.
    #[must_use]
    pub fn excluded(
        provenance: ContextItemProvenance,
        exclusion_reasons: Vec<ContextExclusionReason>,
    ) -> Self {
        Self {
            provenance,
            decision: ContextDecision::Excluded,
            selection_reasons: Vec::new(),
            exclusion_reasons,
            rank: None,
            score: None,
            factors: None,
        }
    }

    /// Build a lower-ranked context item retained for retrieval debugging.
    #[must_use]
    pub fn lower_ranked(
        provenance: ContextItemProvenance,
        rank: usize,
        score: f64,
        exclusion_reasons: Vec<ContextExclusionReason>,
    ) -> Self {
        Self {
            provenance,
            decision: ContextDecision::LowerRanked,
            selection_reasons: Vec::new(),
            exclusion_reasons,
            rank: Some(rank),
            score: Some(score),
            factors: None,
        }
    }

    /// Attach raw scoring factors.
    #[must_use]
    pub fn with_factors(mut self, factors: ContextScoreFactors) -> Self {
        self.factors = Some(factors);
        self
    }

    /// Attach an additional selection reason.
    #[must_use]
    pub fn with_selection_reason(mut self, reason: ContextSelectionReason) -> Self {
        if !self.selection_reasons.contains(&reason) {
            self.selection_reasons.push(reason);
        }
        self
    }

    /// Build a selected context item from a scored recall result.
    #[must_use]
    pub fn selected_from_scored_result(
        result: &ScoredResult,
        provenance: ContextItemProvenance,
        rank: usize,
    ) -> Self {
        Self::selected(
            provenance,
            rank,
            result.score,
            selection_reasons_from_factors(&result.factors, result.project_id.is_some()),
        )
        .with_factors(ContextScoreFactors::from(&result.factors))
    }
}

/// Memory update action caused by a run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum MemoryUpdateKind {
    /// A new memory item was created.
    Created,
    /// An existing memory item was updated.
    Updated,
    /// A memory item was superseded by another item.
    Superseded,
    /// A memory item was marked no longer valid.
    Invalidated,
    /// A memory item was intentionally forgotten.
    Forgotten,
    /// A previously forgotten item was restored.
    Restored,
    /// Confidence or tier changed without content replacement.
    ConfidenceChanged,
}

impl MemoryUpdateKind {
    /// Stable `snake_case` label for logs, JSON summaries, and inspection text.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Created => "created",
            Self::Updated => "updated",
            Self::Superseded => "superseded",
            Self::Invalidated => "invalidated",
            Self::Forgotten => "forgotten",
            Self::Restored => "restored",
            Self::ConfidenceChanged => "confidence_changed",
        }
    }
}

/// Link between a run record and a memory mutation it caused.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunMemoryUpdate {
    /// Run record that caused the update.
    // kanon:ignore RUST/primitive-for-domain-id WHY: run identifiers are supplied by runtime crates outside mneme; keeping the string preserves the facade boundary.
    pub run_id: String,
    /// Session that owns the run.
    // kanon:ignore RUST/primitive-for-domain-id WHY: SessionId lives outside mneme; this record mirrors existing session-store wire IDs.
    pub session_id: String,
    /// Updated fact or memory item identifier.
    // kanon:ignore RUST/primitive-for-domain-id WHY: memory updates can target fact IDs or non-fact context IDs, so the record carries the source-plane wire ID.
    pub memory_id: String,
    /// Update action applied to the memory item.
    pub action: MemoryUpdateKind,
    /// When the update occurred.
    pub occurred_at: jiff::Timestamp,
    /// Source session that produced the update, if different from `session_id`.
    pub source_session_id: Option<String>,
    /// Fact replaced by this update, when known.
    pub supersedes: Option<String>,
    /// Fact replacing this memory, when known.
    pub superseded_by: Option<String>,
    /// Human-readable operator or system reason for the update.
    pub reason: Option<String>,
}

impl RunMemoryUpdate {
    /// Construct a memory update linked to a run record.
    #[must_use]
    pub fn new(
        run_id: impl Into<String>,
        session_id: impl Into<String>,
        memory_id: impl Into<String>,
        action: MemoryUpdateKind,
        occurred_at: jiff::Timestamp,
    ) -> Self {
        Self {
            run_id: run_id.into(),
            session_id: session_id.into(),
            memory_id: memory_id.into(),
            action,
            occurred_at,
            source_session_id: None,
            supersedes: None,
            superseded_by: None,
            reason: None,
        }
    }

    /// Return a copy linked to the provided run and session.
    #[must_use]
    pub fn linked_to(mut self, run_id: &str, session_id: &str) -> Self {
        run_id.clone_into(&mut self.run_id);
        session_id.clone_into(&mut self.session_id);
        self
    }

    /// Attach a human-readable update reason.
    #[must_use]
    pub fn with_reason(mut self, reason: impl Into<String>) -> Self {
        self.reason = Some(reason.into());
        self
    }
}

/// Redaction mode for inspection reports and exports.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum RedactionPolicy {
    /// Authorized internal inspection: content is shown with trust labels.
    InternalInspection,
    /// Public artifacts and exports: only public, published content is shown.
    ///
    /// NOTE: `Internal`, `Confidential`, `Private`, `Shared`, and
    /// `Restricted` memory content is replaced with a redaction reason while
    /// source IDs, scores, lifecycle, and selection reasons remain visible.
    PublicArtifact,
}

impl RedactionPolicy {
    /// Stable `snake_case` label for logs, JSON summaries, and inspection text.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::InternalInspection => "internal_inspection",
            Self::PublicArtifact => "public_artifact",
        }
    }

    /// Decide whether raw content can be shown under this policy.
    #[must_use]
    pub fn content_redaction(
        self,
        sensitivity: FactSensitivity,
        visibility: Visibility,
    ) -> ContentRedaction {
        match self {
            Self::InternalInspection => ContentRedaction::Visible,
            Self::PublicArtifact => {
                if sensitivity == FactSensitivity::Public && visibility == Visibility::Published {
                    ContentRedaction::Visible
                } else {
                    ContentRedaction::Redacted {
                        reason: format!(
                            "redacted for public artifact: sensitivity={} visibility={}",
                            sensitivity.as_str(),
                            visibility.as_str()
                        ),
                    }
                }
            }
        }
    }
}

/// Content visibility decision under a redaction policy.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum ContentRedaction {
    /// Raw content may be shown.
    Visible,
    /// Raw content must be hidden, with a stable explanation.
    Redacted {
        /// Human-readable redaction reason.
        reason: String,
    },
}

/// Durable record of the memory/context considered by one run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunContextRecord {
    /// Stable run identifier.
    // kanon:ignore RUST/primitive-for-domain-id WHY: run identifiers are runtime-owned and not defined in mneme; this is the cross-crate wire boundary.
    pub run_id: String,
    /// Session that owns the run.
    // kanon:ignore RUST/primitive-for-domain-id WHY: SessionId lives outside mneme; this record mirrors existing session-store wire IDs.
    pub session_id: String,
    /// Agent that executed the run.
    // kanon:ignore RUST/primitive-for-domain-id WHY: NousId is not a mneme dependency; this facade keeps the serialized cross-crate identifier as its wire string.
    pub nous_id: String,
    /// Turn identifier, if distinct from `run_id`.
    pub turn_id: Option<String>,
    /// When this record was generated.
    pub recorded_at: jiff::Timestamp,
    /// Query or prompt fragment used for context selection, if retained.
    pub query: Option<String>,
    /// Context admitted into the run.
    pub selected_context: Vec<ContextItem>,
    /// Context inspected but omitted, including lower-ranked candidates.
    pub excluded_context: Vec<ContextItem>,
    /// Memory mutations caused by the run.
    pub memory_updates: Vec<RunMemoryUpdate>,
}

impl RunContextRecord {
    /// Construct an empty run-context record.
    #[must_use]
    pub fn new(
        run_id: impl Into<String>,
        session_id: impl Into<String>,
        nous_id: impl Into<String>,
        recorded_at: jiff::Timestamp,
    ) -> Self {
        Self {
            run_id: run_id.into(),
            session_id: session_id.into(),
            nous_id: nous_id.into(),
            turn_id: None,
            recorded_at,
            query: None,
            selected_context: Vec::new(),
            excluded_context: Vec::new(),
            memory_updates: Vec::new(),
        }
    }

    /// Attach the query or prompt fragment used for retrieval.
    #[must_use]
    pub fn with_query(mut self, query: impl Into<String>) -> Self {
        self.query = Some(query.into());
        self
    }

    /// Attach a turn identifier distinct from the run identifier.
    #[must_use]
    pub fn with_turn_id(mut self, turn_id: impl Into<String>) -> Self {
        self.turn_id = Some(turn_id.into());
        self
    }

    /// Add a selected context item.
    pub fn add_selected_context(&mut self, mut item: ContextItem) {
        item.decision = ContextDecision::Selected;
        self.selected_context.push(item);
    }

    /// Add an excluded or lower-ranked context item.
    pub fn add_excluded_context(&mut self, item: ContextItem) {
        self.excluded_context.push(item);
    }

    /// Add a memory update and force-link it back to this run record.
    pub fn add_memory_update(&mut self, update: RunMemoryUpdate) {
        self.memory_updates
            .push(update.linked_to(&self.run_id, &self.session_id));
    }

    /// Iterate over all selected context items.
    pub fn selected_items(&self) -> impl Iterator<Item = &ContextItem> {
        self.selected_context.iter()
    }

    /// Iterate over all excluded context items.
    pub fn excluded_items(&self) -> impl Iterator<Item = &ContextItem> {
        self.excluded_context.iter()
    }

    /// Iterate over lower-ranked candidates retained for retrieval debugging.
    pub fn lower_ranked_items(&self) -> impl Iterator<Item = &ContextItem> {
        self.excluded_context
            .iter()
            .filter(|item| item.decision == ContextDecision::LowerRanked)
    }

    /// Build a user-facing inspection report under a redaction policy.
    #[must_use]
    pub fn inspection_report(&self, policy: RedactionPolicy) -> ContextInspectionReport {
        let selected_context = self
            .selected_context
            .iter()
            .map(|item| InspectableContextItem::from_context_item(item, policy))
            .collect();
        let excluded_context = self
            .excluded_context
            .iter()
            .map(|item| InspectableContextItem::from_context_item(item, policy))
            .collect();

        ContextInspectionReport {
            run_id: self.run_id.clone(),
            session_id: self.session_id.clone(),
            nous_id: self.nous_id.clone(),
            redaction_policy: policy,
            selected_context,
            excluded_context,
            memory_updates: self.memory_updates.clone(),
        }
    }
}

/// User-facing inspection report generated from a run-context record.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextInspectionReport {
    /// Stable run identifier.
    // kanon:ignore RUST/primitive-for-domain-id WHY: inspection reports are serialized from RunContextRecord and keep the same runtime-owned run ID.
    pub run_id: String,
    /// Session that owns the run.
    // kanon:ignore RUST/primitive-for-domain-id WHY: inspection reports mirror session-store wire IDs instead of introducing a duplicate mneme newtype.
    pub session_id: String,
    /// Agent that executed the run.
    // kanon:ignore RUST/primitive-for-domain-id WHY: inspection reports mirror the serialized cross-crate nous identifier.
    pub nous_id: String,
    /// Redaction mode applied to all content fields.
    pub redaction_policy: RedactionPolicy,
    /// Selected context with content redaction applied.
    pub selected_context: Vec<InspectableContextItem>,
    /// Excluded and lower-ranked context with content redaction applied.
    pub excluded_context: Vec<InspectableContextItem>,
    /// Memory updates linked back to this run.
    pub memory_updates: Vec<RunMemoryUpdate>,
}

impl ContextInspectionReport {
    /// Render the report as Markdown for a diagnostics panel or export.
    #[must_use]
    pub fn to_markdown(&self) -> String {
        let mut out = String::new();
        out.push_str("# Memory Context Inspection\n\n");
        push_fmt(&mut out, format_args!("Run: `{}`\n\n", self.run_id));
        push_fmt(&mut out, format_args!("Session: `{}`\n\n", self.session_id));
        push_fmt(&mut out, format_args!("Agent: `{}`\n\n", self.nous_id));
        push_fmt(
            &mut out,
            format_args!("Redaction: `{}`\n\n", self.redaction_policy.as_str()),
        );

        out.push_str("## Selected Context\n\n");
        append_items(&mut out, &self.selected_context);
        out.push_str("## Excluded Context\n\n");
        append_items(&mut out, &self.excluded_context);
        out.push_str("## Memory Updates\n\n");
        if self.memory_updates.is_empty() {
            out.push_str("No memory updates recorded.\n");
        } else {
            for update in &self.memory_updates {
                push_fmt(
                    &mut out,
                    format_args!(
                        "- `{}` `{}` linked to run `{}`",
                        update.action.as_str(),
                        update.memory_id,
                        update.run_id
                    ),
                );
                if let Some(reason) = &update.reason {
                    push_fmt(&mut out, format_args!(" ({})", one_line(reason)));
                }
                out.push('\n');
            }
        }

        out
    }
}

/// Inspection-ready context item with redaction already applied.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InspectableContextItem {
    /// Source category such as `fact`, `message`, `note`, or `document`.
    pub source_type: String,
    /// Stable source identifier within the source category.
    // kanon:ignore RUST/primitive-for-domain-id WHY: inspection items preserve heterogeneous source IDs alongside source_type for debugging.
    pub source_id: String,
    /// Retrieval decision applied to the item.
    pub decision: ContextDecision,
    /// Content when visible under the report redaction policy.
    pub content: Option<String>,
    /// Whether content was redacted.
    pub content_redacted: bool,
    /// Redaction reason when content was hidden.
    pub redaction_reason: Option<String>,
    /// Confidence score attached to the source memory.
    pub confidence: f64,
    /// Epistemic trust tier attached to the source memory.
    pub tier: EpistemicTier,
    /// Data-sovereignty class for provider and export policy.
    pub sensitivity: FactSensitivity,
    /// Visibility boundary for cross-agent and external surfaces.
    pub visibility: Visibility,
    /// Lifecycle state observed during the run.
    pub lifecycle_state: MemoryLifecycleState,
    /// Positive selection criteria that affected ranking or admission.
    pub selection_reasons: Vec<ContextSelectionReason>,
    /// Exclusion criteria for omitted or lower-ranked items.
    pub exclusion_reasons: Vec<ContextExclusionReason>,
    /// One-based rank after scoring, if ranked.
    pub rank: Option<usize>,
    /// Final composite score, if scored.
    pub score: Option<f64>,
    /// Evidence pointers supporting this context item.
    pub evidence: Vec<ContextEvidenceRef>,
}

impl InspectableContextItem {
    /// Build an inspection item from raw context and apply redaction.
    #[must_use]
    pub fn from_context_item(item: &ContextItem, policy: RedactionPolicy) -> Self {
        let redaction =
            policy.content_redaction(item.provenance.sensitivity, item.provenance.visibility);
        let (content, content_redacted, redaction_reason) = match redaction {
            ContentRedaction::Visible => (Some(item.provenance.content.clone()), false, None),
            ContentRedaction::Redacted { reason } => (None, true, Some(reason)),
        };

        Self {
            source_type: item.provenance.source_type.clone(),
            source_id: item.provenance.source_id.clone(),
            decision: item.decision,
            content,
            content_redacted,
            redaction_reason,
            confidence: item.provenance.confidence,
            tier: item.provenance.tier,
            sensitivity: item.provenance.sensitivity,
            visibility: item.provenance.visibility,
            lifecycle_state: item.provenance.lifecycle.state,
            selection_reasons: item.selection_reasons.clone(),
            exclusion_reasons: item.exclusion_reasons.clone(),
            rank: item.rank,
            score: item.score,
            evidence: item.provenance.evidence.clone(),
        }
    }
}

fn selection_reasons_from_factors(
    factors: &FactorScores,
    has_project_scope: bool,
) -> Vec<ContextSelectionReason> {
    let mut reasons = Vec::new();
    if factors.vector_similarity > 0.0 {
        reasons.push(ContextSelectionReason::Semantic);
    }
    if factors.decay > 0.0 {
        reasons.push(ContextSelectionReason::Recency);
    }
    if factors.relevance > 0.0 {
        reasons.push(ContextSelectionReason::AgentIdentity);
    }
    if has_project_scope {
        reasons.push(ContextSelectionReason::ProjectScope);
    }
    if factors.relationship_proximity > 0.0 {
        reasons.push(ContextSelectionReason::RelationshipProximity);
    }
    if factors.access_frequency > 0.0 {
        reasons.push(ContextSelectionReason::AccessFrequency);
    }
    if factors.graph_importance > 0.0 {
        reasons.push(ContextSelectionReason::GraphImportance);
    }
    if factors.evidence_coverage > 0.0 {
        reasons.push(ContextSelectionReason::EvidenceCoverage);
    }
    if factors.serendipity > 0.0 {
        reasons.push(ContextSelectionReason::Serendipity);
    }
    if factors.surprise > 0.0 {
        reasons.push(ContextSelectionReason::Surprise);
    }
    if factors.convergence > 0.0 {
        reasons.push(ContextSelectionReason::Convergence);
    }
    if reasons.is_empty() {
        reasons.push(ContextSelectionReason::Other);
    }
    reasons
}

fn append_items(out: &mut String, items: &[InspectableContextItem]) {
    if items.is_empty() {
        out.push_str("No context recorded.\n\n");
        return;
    }

    for item in items {
        push_fmt(
            out,
            format_args!(
                "- `{}` `{}`: {}",
                item.decision.as_str(),
                item.source_id,
                item.lifecycle_state.as_str()
            ),
        );
        if let Some(rank) = item.rank {
            push_fmt(out, format_args!(" rank={rank}"));
        }
        if let Some(score) = item.score {
            push_fmt(out, format_args!(" score={score:.3}"));
        }
        out.push('\n');
        push_fmt(
            out,
            format_args!(
                "  - trust: tier={} confidence={:.3} sensitivity={} visibility={}\n",
                item.tier.as_str(),
                item.confidence,
                item.sensitivity.as_str(),
                item.visibility.as_str()
            ),
        );
        if !item.selection_reasons.is_empty() {
            push_fmt(
                out,
                format_args!(
                    "  - selected_by: {}\n",
                    selection_reason_list(&item.selection_reasons)
                ),
            );
        }
        if !item.exclusion_reasons.is_empty() {
            push_fmt(
                out,
                format_args!(
                    "  - excluded_by: {}\n",
                    exclusion_reason_list(&item.exclusion_reasons)
                ),
            );
        }
        match (&item.content, &item.redaction_reason) {
            (Some(content), _) => {
                push_fmt(out, format_args!("  - content: {}\n", one_line(content)));
            }
            (None, Some(reason)) => {
                push_fmt(out, format_args!("  - content: [{}]\n", one_line(reason)));
            }
            (None, None) => {
                out.push_str("  - content: [unavailable]\n");
            }
        }
    }
    out.push('\n');
}

fn selection_reason_list(reasons: &[ContextSelectionReason]) -> String {
    reasons
        .iter()
        .map(|reason| reason.as_str())
        .collect::<Vec<_>>()
        .join(", ")
}

fn exclusion_reason_list(reasons: &[ContextExclusionReason]) -> String {
    reasons
        .iter()
        .map(|reason| reason.as_str())
        .collect::<Vec<_>>()
        .join(", ")
}

fn one_line(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn push_fmt(out: &mut String, args: fmt::Arguments<'_>) {
    if fmt::Write::write_fmt(out, args).is_err() {
        out.push_str("[formatting error]");
    }
}

fn effective_stability_hours(stability_hours: f64, tier: EpistemicTier) -> Option<f64> {
    let effective = stability_hours * tier.stability_multiplier();
    if effective.is_finite() && effective >= 0.0 {
        Some(effective)
    } else {
        None
    }
}

fn age_hours_since(now: jiff::Timestamp, then: jiff::Timestamp) -> Option<f64> {
    let hours = now.duration_since(then).as_secs_f64() / 3_600.0;
    if hours.is_finite() {
        Some(hours.max(0.0))
    } else {
        None
    }
}

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test setup and assertions")]
mod tests {
    use super::*;
    use crate::knowledge::{FactAccess, FactLifecycle, FactProvenance, FactTemporal, ForgetReason};

    fn make_fact(id: &str, content: &str, recorded_at: jiff::Timestamp) -> Fact {
        Fact {
            id: FactId::new(id).expect("valid test fact id"),
            nous_id: "alice".to_owned(),
            fact_type: "preference".to_owned(),
            content: content.to_owned(),
            scope: Some(MemoryScope::Project),
            project_id: None,
            sensitivity: FactSensitivity::Public,
            visibility: Visibility::Published,
            temporal: FactTemporal {
                valid_from: recorded_at,
                valid_to: crate::knowledge::far_future(),
                recorded_at,
            },
            provenance: FactProvenance {
                confidence: 0.82,
                tier: EpistemicTier::Inferred,
                source_session_id: Some("session-source".to_owned()),
                stability_hours: 24.0,
            },
            lifecycle: FactLifecycle {
                superseded_by: None,
                is_forgotten: false,
                forgotten_at: None,
                forget_reason: None,
            },
            access: FactAccess {
                access_count: 3,
                last_accessed_at: None,
            },
        }
    }

    #[test]
    fn run_record_inspection_shows_selected_excluded_and_update_links() {
        let now = jiff::Timestamp::now();
        let fact = make_fact("fact_a", "alice prefers concise answers", now);
        let provenance = ContextItemProvenance::from_fact(&fact, now);
        let selected = ContextItem::selected(
            provenance,
            1,
            0.91,
            vec![
                ContextSelectionReason::Lexical,
                ContextSelectionReason::AgentIdentity,
            ],
        );

        let mut stale_fact = make_fact("fact_b", "alice used an old deploy flow", now);
        stale_fact.lifecycle.superseded_by =
            Some(FactId::new("fact_c").expect("valid replacement id"));
        let excluded = ContextItem::excluded(
            ContextItemProvenance::from_fact(&stale_fact, now),
            vec![ContextExclusionReason::Superseded],
        );

        let mut record = RunContextRecord::new("run-1", "session-1", "alice", now)
            .with_query("concise answer preference");
        record.add_selected_context(selected);
        record.add_excluded_context(excluded);
        record.add_memory_update(
            RunMemoryUpdate::new(
                "wrong-run",
                "wrong-session",
                "fact_new",
                MemoryUpdateKind::Created,
                now,
            )
            .with_reason("assistant extracted a durable preference"),
        );

        let report = record.inspection_report(RedactionPolicy::InternalInspection);
        assert_eq!(report.selected_context.len(), 1);
        assert_eq!(report.excluded_context.len(), 1);
        let update = report
            .memory_updates
            .first()
            .expect("memory update present");
        assert_eq!(update.run_id, "run-1");
        assert_eq!(update.session_id, "session-1");

        let markdown = report.to_markdown();
        assert!(markdown.contains("selected_by: lexical, agent_identity"));
        assert!(markdown.contains("excluded_by: superseded"));
        assert!(markdown.contains("assistant extracted a durable preference"));
    }

    #[test]
    fn public_report_redacts_non_public_or_unpublished_content() {
        let now = jiff::Timestamp::now();
        let mut fact = make_fact("fact_private", "alice private detail", now);
        fact.sensitivity = FactSensitivity::Internal;
        fact.visibility = Visibility::Private;
        let item = ContextItem::selected(
            ContextItemProvenance::from_fact(&fact, now),
            1,
            0.9,
            vec![ContextSelectionReason::Semantic],
        );
        let mut record = RunContextRecord::new("run-2", "session-2", "alice", now);
        record.add_selected_context(item);

        let report = record.inspection_report(RedactionPolicy::PublicArtifact);
        let redacted = report
            .selected_context
            .first()
            .expect("selected context present");

        assert!(redacted.content_redacted);
        assert!(redacted.content.is_none());
        assert!(
            redacted
                .redaction_reason
                .as_deref()
                .unwrap_or_default()
                .contains("sensitivity=internal visibility=private")
        );
        assert!(!report.to_markdown().contains("alice private detail"));
    }

    #[test]
    fn lifecycle_marks_stale_superseded_invalidated_and_forgotten() {
        let now = jiff::Timestamp::now();
        let old = now
            .checked_sub(jiff::SignedDuration::from_hours(48))
            .expect("valid timestamp");
        let mut stale = make_fact("stale", "old fact", old);
        stale.provenance.stability_hours = 1.0;

        let mut superseded = make_fact("superseded", "old deploy fact", now);
        superseded.lifecycle.superseded_by =
            Some(FactId::new("replacement").expect("valid replacement"));

        let mut invalidated = make_fact("invalidated", "temporary fact", now);
        invalidated.temporal.valid_to = now
            .checked_sub(jiff::SignedDuration::from_hours(1))
            .expect("valid timestamp");

        let mut forgotten = make_fact("forgotten", "forgotten fact", now);
        forgotten.lifecycle.is_forgotten = true;
        forgotten.lifecycle.forgotten_at = Some(now);
        forgotten.lifecycle.forget_reason = Some(ForgetReason::UserRequested);

        assert_eq!(
            ContextLifecycle::from_fact(&stale, now).state,
            MemoryLifecycleState::Stale
        );
        assert_eq!(
            ContextLifecycle::from_fact(&superseded, now).state,
            MemoryLifecycleState::Superseded
        );
        assert_eq!(
            ContextLifecycle::from_fact(&invalidated, now).state,
            MemoryLifecycleState::Invalidated
        );
        assert_eq!(
            ContextLifecycle::from_fact(&forgotten, now).state,
            MemoryLifecycleState::Forgotten
        );
    }

    #[test]
    fn scored_result_selection_records_factor_reasons() {
        let now = jiff::Timestamp::now();
        let fact = make_fact("fact_score", "rust project uses tokio", now);
        let factors = FactorScores {
            vector_similarity: 0.7,
            decay: 0.4,
            relevance: 1.0,
            epistemic_tier: 0.8,
            relationship_proximity: 0.0,
            access_frequency: 0.2,
            graph_importance: 0.0,
            serendipity: 0.0,
            surprise: 0.0,
            evidence_coverage: 0.0,
            convergence: 0.0,
        };
        let result = ScoredResult {
            content: fact.content.clone(),
            source_type: "fact".to_owned(),
            source_id: fact.id.to_string(),
            nous_id: fact.nous_id.clone(),
            factors,
            score: 0.77,
            sensitivity: FactSensitivity::Public,
            visibility: Visibility::Published,
            scope: Some(MemoryScope::Project),
            project_id: None,
        };

        let item = ContextItem::selected_from_scored_result(
            &result,
            ContextItemProvenance::from_fact(&fact, now),
            2,
        );

        assert!(
            item.selection_reasons
                .contains(&ContextSelectionReason::Semantic)
        );
        assert!(
            item.selection_reasons
                .contains(&ContextSelectionReason::Recency)
        );
        assert!(
            item.selection_reasons
                .contains(&ContextSelectionReason::AgentIdentity)
        );
        assert!(
            item.selection_reasons
                .contains(&ContextSelectionReason::AccessFrequency)
        );
        assert!(item.factors.is_some());
    }
}

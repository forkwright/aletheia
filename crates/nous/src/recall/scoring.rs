//! Recall configuration, weights, and token estimation.

use std::collections::HashMap;

use mneme::id::FactId;
use mneme::knowledge::MemoryScope;
use serde::{Deserialize, Serialize};

use mneme::recall::ScoredResult;

/// Per-factor base scores for the recall pipeline.
///
/// These values are placed directly into the non-vector
/// [`mneme::recall::FactorScores`] fields. Only vector similarity is computed
/// from the actual embedding distance; decay, relevance, tier, proximity, frequency,
/// and graph importance use these configured values as their scores. All values default
/// to the previously hardcoded constants, preserving existing behaviour unless an
/// operator overrides them in taxis config.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecallWeights {
    /// Temporal decay weight (0.0-1.0).
    pub decay: f64,
    /// Content relevance weight (0.0-1.0).
    pub relevance: f64,
    /// Epistemic tier weight (0.0-1.0).
    pub epistemic_tier: f64,
    /// Knowledge-graph relationship proximity weight (0.0-1.0).
    pub relationship_proximity: f64,
    /// Access frequency weight (0.0-1.0).
    pub access_frequency: f64,
    /// Graph `PageRank` importance weight (0.0-1.0).
    pub graph_importance: f64,
}

impl Default for RecallWeights {
    fn default() -> Self {
        Self {
            decay: 0.5,
            relevance: 0.5,
            epistemic_tier: 0.3,
            relationship_proximity: 0.1,
            access_frequency: 0.0,
            graph_importance: 0.1,
        }
    }
}

/// Configuration for the recall stage.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "recall controls are independent operator knobs (enabled, iterative, inject_metadata, late_inject_anchor); not a state machine"
)]
pub struct RecallConfig {
    /// Whether recall is enabled.
    pub enabled: bool,
    /// Maximum number of recalled items to inject.
    pub max_results: usize,
    /// Minimum score threshold to include a result.
    pub min_score: f64,
    /// Maximum tokens to allocate for recalled knowledge.
    pub max_recall_tokens: u64,
    /// Enable iterative 2-cycle retrieval with terminology discovery.
    pub iterative: bool,
    /// Maximum retrieval cycles (only used when `iterative` is true).
    pub max_cycles: usize,
    /// Per-factor scoring weights applied when building candidates.
    #[serde(default)]
    pub weights: RecallWeights,
    /// Inject factor metadata into recalled knowledge prompts.
    ///
    /// When enabled, each recalled fact includes its factor scores so the
    /// LLM can weight its reasoning by provenance quality.
    #[serde(default)]
    pub inject_metadata: bool,
    /// Fact IDs that should be recalled first when they appear in candidates.
    #[serde(default)]
    pub pinned_facts: Vec<FactId>,
    /// When true, append recalled knowledge as a system message at the end of
    /// the conversation context instead of injecting it into the system prompt.
    #[serde(default)]
    pub late_inject_anchor: bool,
    /// Per-scope minimum result counts with slack-fill.
    #[serde(default)]
    pub scope_quotas: HashMap<MemoryScope, usize>,
    /// URL for an HTTP cross-encoder reranker.
    #[serde(default)]
    pub reranker_url: Option<String>,
    /// Filesystem path to a local ONNX cross-encoder model for in-process reranking.
    #[serde(default)]
    pub reranker_model_path: Option<String>,
    /// Characters per token for recall budget estimation.
    ///
    /// Wired from `agents.defaults.chars_per_token` at startup.
    /// Default: 4 (1 token ≈ 4 chars).
    #[serde(default = "default_chars_per_token")]
    pub chars_per_token: u64,
}

pub(super) fn default_chars_per_token() -> u64 {
    4
}

impl Default for RecallConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            max_results: 5,
            min_score: 0.3,
            max_recall_tokens: 2000,
            iterative: false,
            max_cycles: 2,
            weights: RecallWeights::default(),
            inject_metadata: false,
            pinned_facts: Vec::new(),
            late_inject_anchor: false,
            scope_quotas: HashMap::new(),
            reranker_url: None,
            reranker_model_path: None,
            chars_per_token: default_chars_per_token(),
        }
    }
}

impl From<taxis::config::RecallSettings> for RecallConfig {
    fn from(s: taxis::config::RecallSettings) -> Self {
        Self {
            enabled: s.enabled,
            max_results: s.max_results,
            min_score: s.min_score,
            max_recall_tokens: s.max_recall_tokens,
            iterative: s.iterative,
            max_cycles: s.max_cycles,
            weights: RecallWeights {
                decay: s.weights.decay,
                relevance: s.weights.relevance,
                epistemic_tier: s.weights.epistemic_tier,
                relationship_proximity: s.weights.relationship_proximity,
                access_frequency: s.weights.access_frequency,
                graph_importance: s.weights.graph_importance,
            },
            inject_metadata: s.inject_metadata,
            pinned_facts: s.pinned_facts,
            late_inject_anchor: s.late_inject_anchor,
            scope_quotas: s.scope_quotas,
            reranker_url: s.reranker_url,
            reranker_model_path: s.reranker_model_path,
            // NOTE: chars_per_token is forwarded separately from AgentDefaults
            //       via NousConfig; the From conversion cannot carry it since
            //       RecallSettings does not own that field.
            chars_per_token: default_chars_per_token(),
        }
    }
}

/// Format scored results as a markdown section.
#[must_use]
pub(crate) fn format_section(results: &[&ScoredResult], inject_metadata: bool) -> String {
    use std::fmt::Write;

    let mut out = String::from(
        "## Recalled Knowledge\n\nThe following facts were recalled from memory (relevance score in brackets):\n",
    );

    for r in results {
        // kanon:ignore RUST/no-silent-result-swallow — write! on String is infallible
        let _ = write!(out, "\n- [{:.2}] {}", r.score, r.content);
        if inject_metadata {
            let _ = write!(
                out,
                "\n  (factors: vector={:.2}, decay={:.2}, relevance={:.2}, tier={:.2}, proximity={:.2}, freq={:.2})",
                r.factors.vector_similarity,
                r.factors.decay,
                r.factors.relevance,
                r.factors.epistemic_tier,
                r.factors.relationship_proximity,
                r.factors.access_frequency,
            );
        }
    }

    out
}

/// Estimate token count from text length using a configurable character divisor.
///
/// `chars_per_token` is the number of characters assumed per token. Use
/// `RecallConfig::chars_per_token` for operator-configurable behaviour, or
/// pass `4` directly in tests and contexts without a live config.
#[must_use]
pub(crate) fn estimate_tokens(text: &str, chars_per_token: u64) -> u64 {
    #[expect(
        clippy::as_conversions,
        reason = "usize->u64: text length always fits in u64"
    )]
    let len = text.len() as u64; // kanon:ignore RUST/as-cast
    len.div_ceil(chars_per_token.max(1))
}

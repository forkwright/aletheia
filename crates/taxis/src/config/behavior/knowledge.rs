//! Episteme knowledge configuration.

use serde::{Deserialize, Serialize};

/// Episteme knowledge conflict resolution, decay, and extraction parameters.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(default)]
#[serde(deny_unknown_fields)]
pub struct KnowledgeConfig {
    /// Maximum LLM calls per fact during conflict resolution. Default: 3.
    pub conflict_max_llm_calls_per_fact: usize,
    /// Similarity threshold above which intra-batch candidates are merged. Default: 0.95.
    pub conflict_intra_batch_dedup_threshold: f64,
    /// Maximum vector distance for a fact to be a conflict candidate. Default: 0.28.
    pub conflict_candidate_distance_threshold: f64,
    /// Maximum conflict candidates evaluated per fact. Default: 5.
    pub conflict_max_candidates: usize,
    /// Confidence boost per reinforcement event. Default: 0.02.
    pub decay_reinforcement_boost: f64,
    /// Maximum cumulative reinforcement bonus. Default: 1.0.
    pub decay_max_reinforcement_bonus: f64,
    /// Confidence bonus per additional corroborating agent. Default: 0.15.
    pub decay_cross_agent_bonus_per_agent: f64,
    /// Cap on total cross-agent multiplier. Default: 1.75.
    pub decay_max_cross_agent_multiplier: f64,
    /// Minimum confidence for a fact to pass extraction filtering. Default: 0.3.
    pub extraction_confidence_threshold: f64,
    /// Minimum character length for an extracted fact. Default: 10.
    pub extraction_min_fact_length: usize,
    /// Maximum character length for an extracted fact. Default: 500.
    pub extraction_max_fact_length: usize,
    /// Provider selection for the extraction bookkeeping pass.
    pub extraction: ExtractionConfig,
    /// Minimum tool calls before operational instinct scoring fires. Default: 5.
    pub instinct_min_tool_calls: u64,
    /// Maximum length for parameter values before truncation. Default: 200.
    pub instinct_max_param_value_len: usize,
    /// Maximum length for context summaries. Default: 100.
    pub instinct_max_context_summary_len: usize,
    /// Maximum byte length for fact content strings.
    pub max_content_length: usize,
    /// Default maximum entries returned by a single side-query.
    pub side_query_max_results: usize,
    /// Default cache time-to-live in seconds for side-query.
    pub side_query_cache_ttl_secs: u64,
    /// Default maximum cache entries for side-query.
    pub side_query_cache_capacity: usize,
    /// Decay score below which a skill is flagged for review.
    pub skill_decay_needs_review_threshold: f64,
    /// Decay score below which a skill is auto-retired.
    pub skill_decay_retire_threshold: f64,
    /// Days of inactivity before decay reaches review threshold (low-usage skills).
    pub skill_decay_stale_days: u32,
    /// Usage count above which a skill decays slower.
    pub skill_decay_high_usage_threshold: u32,
    /// Multiplier applied to decay half-life for high-usage skills.
    pub skill_decay_high_usage_factor: f64,
    /// Surprise threshold (nats) for episode boundary detection.
    pub surprise_threshold: f64,
    /// EMA alpha for surprise baseline adaptation.
    pub surprise_ema_alpha: f64,
    /// Recall weight for Bayesian surprise contribution. Default: 0.0 (inert).
    ///
    /// Non-zero values blend the session `SurpriseCalculator`'s KL-divergence
    /// signal (via `RecallEngine::score_surprise`) into recall scoring, so
    /// candidates whose content diverges from the running session topic rank
    /// higher. Threaded into `RecallWeights::surprise` at engine construction
    /// (`aletheia::runtime::nous_config` → `RecallConfig::surprise_weight`).
    ///
    /// WARNING: this is a novelty/serendipity signal, not a relevance booster —
    /// it surfaces cross-topic memories (high topic-shift surprise), trading
    /// relevance for diversity. Keep it small relative to `vector_similarity`.
    pub recall_surprise_weight: f64,
    /// Recall weight for evidence-gap coverage. Default: 0.0 (inert).
    ///
    /// Non-zero values boost candidates whose `source_id` answers a decomposed
    /// query gap (via `RecallEngine::score_evidence_coverage`) during the
    /// iterative-retrieval path. Threaded into `RecallWeights::evidence_coverage`
    /// at engine construction.
    pub recall_evidence_coverage_weight: f64,
    /// Recall weight for consolidated-fact convergence. Default: 0.0 (inert).
    ///
    /// Non-zero values boost facts assembled from more independent converging
    /// observations, scored as `log(1 + source_count)` from the
    /// `fact_multiplicity` side-index (via `RecallEngine::score_convergence`).
    /// Threaded into `RecallWeights::convergence` at engine construction.
    pub recall_convergence_weight: f64,
    /// Recall weight for serendipity. Default: 0.0 (inert).
    ///
    /// Non-zero values boost candidates that are both graph-obscure and
    /// farther away in semantic distance, using existing recall fields only.
    /// Threaded into `RecallWeights::serendipity` at engine construction.
    ///
    /// WARNING: this is a novelty/serendipity signal, not a relevance booster
    /// — keep it small relative to `vector_similarity`.
    pub recall_serendipity_weight: f64,
    /// Admission policy applied to every `insert_fact` call. Default: `default` (admit-all).
    ///
    /// Set to `structured` to activate the five-factor A-MAC gate
    /// (`StructuredAdmissionPolicy`). Use `admission_threshold`,
    /// `admission_min_confidence`, and `admission_content_hash_dedup` to tune
    /// the gate without recompiling.
    pub admission_policy: AdmissionPolicyKind,
    /// Minimum combined A-MAC score for a fact to be admitted under the
    /// `structured` policy (0.0..=1.0). Default: 0.3.
    ///
    /// Ignored when `admission_policy = "default"`.
    pub admission_threshold: f64,
    /// Minimum source confidence for a fact to pass the fast-reject gate under
    /// the `structured` policy (0.0..=1.0). Default: 0.1.
    ///
    /// Facts whose `confidence` is below this value are rejected immediately
    /// without computing the full five-factor score. Ignored when
    /// `admission_policy = "default"`.
    pub admission_min_confidence: f64,
    /// Enable SHA-256 content-hash deduplication under the `structured` policy.
    /// Default: `true`.
    ///
    /// When `true`, facts whose normalized content is identical to a previously
    /// admitted fact are rejected with `low_novelty`. Disable if the knowledge
    /// store already performs its own deduplication. Ignored when
    /// `admission_policy = "default"`.
    pub admission_content_hash_dedup: bool,
}

impl Default for KnowledgeConfig {
    fn default() -> Self {
        Self {
            conflict_max_llm_calls_per_fact: 3,
            conflict_intra_batch_dedup_threshold: 0.95,
            conflict_candidate_distance_threshold: 0.28,
            conflict_max_candidates: 5,
            decay_reinforcement_boost: 0.02,
            decay_max_reinforcement_bonus: 1.0,
            decay_cross_agent_bonus_per_agent: 0.15,
            decay_max_cross_agent_multiplier: 1.75,
            max_content_length: eidos::knowledge::fact::MAX_CONTENT_LENGTH,
            extraction_confidence_threshold: 0.3,
            extraction_min_fact_length: 10,
            extraction_max_fact_length: 500,
            extraction: ExtractionConfig::default(),
            instinct_min_tool_calls: 5,
            instinct_max_param_value_len: 200,
            instinct_max_context_summary_len: 100,
            side_query_max_results: episteme::side_query::DEFAULT_MAX_RESULTS,
            side_query_cache_ttl_secs: episteme::side_query::DEFAULT_CACHE_TTL_SECS,
            side_query_cache_capacity: episteme::side_query::DEFAULT_CACHE_CAPACITY,
            skill_decay_needs_review_threshold: episteme::skill::decay::NEEDS_REVIEW_THRESHOLD,
            skill_decay_retire_threshold: episteme::skill::decay::RETIRE_THRESHOLD,
            skill_decay_stale_days: episteme::skill::decay::DEFAULT_STALE_DAYS,
            skill_decay_high_usage_threshold: episteme::skill::decay::HIGH_USAGE_THRESHOLD,
            skill_decay_high_usage_factor: episteme::skill::decay::HIGH_USAGE_DECAY_FACTOR,
            surprise_threshold: episteme::surprise::DEFAULT_THRESHOLD,
            surprise_ema_alpha: episteme::surprise::DEFAULT_EMA_ALPHA,
            recall_surprise_weight: 0.0,
            recall_evidence_coverage_weight: 0.0,
            recall_convergence_weight: 0.0,
            recall_serendipity_weight: 0.0,
            admission_policy: AdmissionPolicyKind::Default,
            admission_threshold: 0.3,
            admission_min_confidence: 0.1,
            admission_content_hash_dedup: true,
        }
    }
}

/// Provider-specific extraction configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(default)]
#[serde(deny_unknown_fields)]
pub struct ExtractionConfig {
    /// Bookkeeping provider implementation. Default: `llm`.
    pub provider: BookkeepingProviderKind,
}

impl Default for ExtractionConfig {
    fn default() -> Self {
        Self {
            provider: BookkeepingProviderKind::Llm,
        }
    }
}

/// Bookkeeping provider selected for extraction.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
#[non_exhaustive]
pub enum BookkeepingProviderKind {
    /// Compatibility LLM prompt + parser path.
    #[default]
    Llm,
    /// `GLiNER` ONNX entity adapter with LLM fallback.
    Gliner,
}

/// Preserved-tail compaction strategy for full context compaction.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum CompactionStrategyKind {
    /// Keep the preserved tail as whole messages.
    #[default]
    UniformTail,
    /// Keep the last two steps full and compact earlier preserved steps.
    StepPositional,
}

/// Which admission policy the knowledge store uses for fact insertion.
///
/// Default is `Default` (admit-all), preserving existing behavior unless
/// the operator explicitly sets `admission_policy = "structured"` in the
/// knowledge section of the config.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
#[non_exhaustive]
pub enum AdmissionPolicyKind {
    /// Admit-all policy: every fact that passes basic validation is stored.
    ///
    /// This is the pre-admission-control behavior. Use this when the
    /// extraction pipeline is already well-filtered or when behavioral
    /// compatibility with existing deployments is required.
    #[default]
    Default,
    /// Five-factor A-MAC policy (arxiv 2603.04549): utility, confidence,
    /// novelty, recency, and content-type prior. Facts whose combined
    /// weighted score falls below the configured threshold are rejected.
    Structured,
}

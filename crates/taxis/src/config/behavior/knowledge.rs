//! Episteme knowledge configuration.

use serde::{Deserialize, Serialize};

/// Episteme knowledge conflict resolution, decay, and extraction parameters.
///
/// All defaults match the current hardcoded constants in the `episteme` crate.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(default)]
#[serde(deny_unknown_fields)]
pub struct KnowledgeConfig {
    /// Maximum LLM calls per fact during conflict resolution. Default: 3.
    /// Mirrors `episteme::conflict::MAX_LLM_CALLS_PER_FACT`.
    pub conflict_max_llm_calls_per_fact: usize,
    /// Similarity threshold above which intra-batch candidates are merged. Default: 0.95.
    /// Mirrors `episteme::conflict::INTRA_BATCH_DEDUP_THRESHOLD`.
    pub conflict_intra_batch_dedup_threshold: f64,
    /// Maximum vector distance for a fact to be a conflict candidate. Default: 0.28.
    /// Mirrors `episteme::conflict::CANDIDATE_DISTANCE_THRESHOLD`.
    pub conflict_candidate_distance_threshold: f64,
    /// Maximum conflict candidates evaluated per fact. Default: 5.
    /// Mirrors `episteme::conflict::MAX_CANDIDATES`.
    pub conflict_max_candidates: usize,
    /// Confidence boost per reinforcement event. Default: 0.02.
    /// Mirrors `episteme::decay::REINFORCEMENT_BOOST`.
    pub decay_reinforcement_boost: f64,
    /// Maximum cumulative reinforcement bonus. Default: 1.0.
    /// Mirrors `episteme::decay::MAX_REINFORCEMENT_BONUS`.
    pub decay_max_reinforcement_bonus: f64,
    /// Confidence bonus per additional corroborating agent. Default: 0.15.
    /// Mirrors `episteme::decay::CROSS_AGENT_BONUS_PER_AGENT`.
    pub decay_cross_agent_bonus_per_agent: f64,
    /// Cap on total cross-agent multiplier. Default: 1.75.
    /// Mirrors `episteme::decay::MAX_CROSS_AGENT_MULTIPLIER`.
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
    /// Mirrors `episteme::ops_facts::MIN_TOOL_CALLS`.
    pub instinct_min_tool_calls: u64,
    /// Maximum length for parameter values before truncation. Default: 200.
    /// Mirrors `episteme::instinct::MAX_PARAM_VALUE_LEN`.
    pub instinct_max_param_value_len: usize,
    /// Maximum length for context summaries. Default: 100.
    /// Mirrors `episteme::instinct::MAX_CONTEXT_SUMMARY_LEN`.
    pub instinct_max_context_summary_len: usize,
    /// Maximum byte length for fact content strings. Default: 102400 (100 KiB).
    /// Mirrors `eidos::knowledge::fact::MAX_CONTENT_LENGTH`.
    pub max_content_length: usize,
    /// Default maximum entries returned by a single side-query. Default: 5.
    /// Mirrors `episteme::side_query::DEFAULT_MAX_RESULTS`.
    pub side_query_max_results: usize,
    /// Default cache time-to-live in seconds for side-query. Default: 300.
    /// Mirrors `episteme::side_query::DEFAULT_CACHE_TTL_SECS`.
    pub side_query_cache_ttl_secs: u64,
    /// Default maximum cache entries for side-query. Default: 64.
    /// Mirrors `episteme::side_query::DEFAULT_CACHE_CAPACITY`.
    pub side_query_cache_capacity: usize,
    /// Decay score below which a skill is flagged for review. Default: 0.3.
    /// Mirrors `episteme::skill::decay::NEEDS_REVIEW_THRESHOLD`.
    pub skill_decay_needs_review_threshold: f64,
    /// Decay score below which a skill is auto-retired. Default: 0.08.
    /// Mirrors `episteme::skill::decay::RETIRE_THRESHOLD`.
    pub skill_decay_retire_threshold: f64,
    /// Days of inactivity before decay reaches review threshold (low-usage skills). Default: 28.
    /// Mirrors `episteme::skill::decay::DEFAULT_STALE_DAYS`.
    pub skill_decay_stale_days: u32,
    /// Usage count above which a skill decays slower. Default: 10.
    /// Mirrors `episteme::skill::decay::HIGH_USAGE_THRESHOLD`.
    pub skill_decay_high_usage_threshold: u32,
    /// Multiplier applied to decay half-life for high-usage skills. Default: 3.0.
    /// Mirrors `episteme::skill::decay::HIGH_USAGE_DECAY_FACTOR`.
    pub skill_decay_high_usage_factor: f64,
    /// Surprise threshold (nats) for episode boundary detection. Default: 2.0.
    /// Mirrors `episteme::surprise::DEFAULT_THRESHOLD`.
    pub surprise_threshold: f64,
    /// EMA alpha for surprise baseline adaptation. Default: 0.3.
    /// Mirrors `episteme::surprise::DEFAULT_EMA_ALPHA`.
    pub surprise_ema_alpha: f64,
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
            max_content_length: 102_400,
            extraction_confidence_threshold: 0.3,
            extraction_min_fact_length: 10,
            extraction_max_fact_length: 500,
            extraction: ExtractionConfig::default(),
            instinct_min_tool_calls: 5,
            instinct_max_param_value_len: 200,
            instinct_max_context_summary_len: 100,
            side_query_max_results: 5,
            side_query_cache_ttl_secs: 300,
            side_query_cache_capacity: 64,
            skill_decay_needs_review_threshold: 0.3,
            skill_decay_retire_threshold: 0.08,
            skill_decay_stale_days: 28,
            skill_decay_high_usage_threshold: 10,
            skill_decay_high_usage_factor: 3.0,
            surprise_threshold: 2.0,
            surprise_ema_alpha: 0.3,
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

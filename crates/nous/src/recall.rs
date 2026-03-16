//! Recall pipeline stage: retrieves relevant knowledge and injects into context.

use std::collections::{HashMap, HashSet};
use std::sync::LazyLock;

use serde::{Deserialize, Serialize};
use tracing::{debug, instrument};

use aletheia_mneme::embedding::EmbeddingProvider;
use aletheia_mneme::knowledge::RecallResult as KnowledgeRecallResult;
use aletheia_mneme::recall::{FactorScores, RecallEngine, ScoredResult};

#[cfg(feature = "knowledge-store")]
use std::sync::Arc;

use crate::error;

#[cfg(feature = "knowledge-store")]
use aletheia_mneme::knowledge_store::KnowledgeStore;

/// Abstracts BM25 text search for recall when no embedding provider is available.
///
/// Used as fallback when the embedding provider is mock or unavailable.
/// `KnowledgeStore` implements this when the `mneme-engine` feature is available.
pub trait TextSearch: Send + Sync {
    /// Search by text (BM25) and return the `k` best-matching results.
    fn search_text(&self, query: &str, k: usize) -> error::Result<Vec<KnowledgeRecallResult>>;
}

/// Bridges [`aletheia_mneme::knowledge_store::KnowledgeStore::search_text_for_recall`] to [`TextSearch`].
#[cfg(feature = "knowledge-store")]
pub struct KnowledgeTextSearch {
    store: Arc<KnowledgeStore>,
}

#[cfg(feature = "knowledge-store")]
impl KnowledgeTextSearch {
    #[must_use]
    pub fn new(store: Arc<KnowledgeStore>) -> Self {
        Self { store }
    }
}

#[cfg(feature = "knowledge-store")]
impl TextSearch for KnowledgeTextSearch {
    fn search_text(&self, query: &str, k: usize) -> error::Result<Vec<KnowledgeRecallResult>> {
        let k_i64 = i64::try_from(k).unwrap_or(i64::MAX);
        self.store
            .search_text_for_recall(query, k_i64)
            .map_err(|e| {
                error::RecallSearchSnafu {
                    message: e.to_string(),
                }
                .build()
            })
    }
}

/// Abstracts vector knowledge search.
///
/// `KnowledgeStore` implements this when the `mneme-engine` feature is available.
/// For tests, use `MockVectorSearch`.
pub trait VectorSearch: Send + Sync {
    /// Search for the `k` nearest vectors with HNSW `ef` parameter.
    fn search_vectors(
        &self,
        query_vec: Vec<f32>,
        k: usize,
        ef: usize,
    ) -> error::Result<Vec<KnowledgeRecallResult>>;
}

/// Bridges [`KnowledgeStore::search_vectors`] to the [`VectorSearch`] trait.
#[cfg(feature = "knowledge-store")]
pub struct KnowledgeVectorSearch {
    store: Arc<KnowledgeStore>,
}

#[cfg(feature = "knowledge-store")]
impl KnowledgeVectorSearch {
    #[must_use]
    pub fn new(store: Arc<KnowledgeStore>) -> Self {
        Self { store }
    }
}

#[cfg(feature = "knowledge-store")]
impl VectorSearch for KnowledgeVectorSearch {
    fn search_vectors(
        &self,
        query_vec: Vec<f32>,
        k: usize,
        ef: usize,
    ) -> error::Result<Vec<KnowledgeRecallResult>> {
        let k_i64 = i64::try_from(k).unwrap_or(i64::MAX);
        let ef_i64 = i64::try_from(ef).unwrap_or(i64::MAX);
        self.store
            .search_vectors(query_vec, k_i64, ef_i64)
            .map_err(|e| {
                error::RecallSearchSnafu {
                    message: e.to_string(),
                }
                .build()
            })
    }
}

/// Per-factor base scores for the recall pipeline.
///
/// These values are placed directly into the non-vector
/// [`aletheia_mneme::recall::FactorScores`] fields. Only vector similarity is computed
/// from the actual embedding distance; decay, relevance, tier, proximity, and frequency
/// use these configured values as their scores. All values default to the previously
/// hardcoded constants, preserving existing behaviour unless an operator overrides them
/// in taxis config.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecallWeights {
    /// Temporal decay weight (0.0–1.0).
    pub decay: f64,
    /// Content relevance weight (0.0–1.0).
    pub relevance: f64,
    /// Epistemic tier weight (0.0–1.0).
    pub epistemic_tier: f64,
    /// Knowledge-graph relationship proximity weight (0.0–1.0).
    pub relationship_proximity: f64,
    /// Access frequency weight (0.0–1.0).
    pub access_frequency: f64,
}

impl Default for RecallWeights {
    fn default() -> Self {
        Self {
            decay: 0.5,
            relevance: 0.5,
            epistemic_tier: 0.3,
            relationship_proximity: 0.0,
            access_frequency: 0.0,
        }
    }
}

/// Configuration for the recall stage.
#[derive(Debug, Clone, Serialize, Deserialize)]
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
    /// Per-factor engine scoring weights for the mneme `RecallEngine`.
    ///
    /// Controls the weighted combination of retrieval signals. Wired from
    /// `agents.defaults.recall.engine_weights` at startup; defaults match
    /// the mneme engine built-in values for zero behavioural change.
    #[serde(default)]
    pub engine_weights: aletheia_taxis::config::RecallEngineWeights,
    /// Characters per token for recall budget estimation.
    ///
    /// Wired from `agents.defaults.chars_per_token` at startup.
    /// Default: 4 (1 token ≈ 4 chars).
    #[serde(default = "default_chars_per_token")]
    pub chars_per_token: u64,
}

fn default_chars_per_token() -> u64 {
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
            engine_weights: aletheia_taxis::config::RecallEngineWeights::default(),
            chars_per_token: default_chars_per_token(),
        }
    }
}

impl From<aletheia_taxis::config::RecallSettings> for RecallConfig {
    fn from(s: aletheia_taxis::config::RecallSettings) -> Self {
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
            },
            engine_weights: s.engine_weights,
            // NOTE: chars_per_token is forwarded separately from AgentDefaults
            //       via NousConfig; the From conversion cannot carry it since
            //       RecallSettings does not own that field.
            chars_per_token: default_chars_per_token(),
        }
    }
}

/// Output of the recall pipeline stage.
#[derive(Debug, Clone)]
pub struct RecallStageResult {
    /// Number of candidates retrieved from knowledge store.
    pub candidates_found: usize,
    /// Number that passed scoring threshold.
    pub results_injected: usize,
    /// Tokens consumed by injected knowledge.
    pub tokens_consumed: u64,
    /// The formatted recall section (appended to system prompt).
    pub recall_section: Option<String>,
}

impl RecallStageResult {
    fn empty() -> Self {
        Self {
            candidates_found: 0,
            results_injected: 0,
            tokens_consumed: 0,
            recall_section: None,
        }
    }
}

/// Recall stage: scores and formats knowledge for injection into the system prompt.
pub struct RecallStage {
    engine: RecallEngine,
    config: RecallConfig,
}

impl RecallStage {
    /// Create a recall stage, wiring operator-configured engine weights.
    #[must_use]
    pub fn new(config: RecallConfig) -> Self {
        let ew = &config.engine_weights;
        let engine_weights = aletheia_mneme::recall::RecallWeights {
            vector_similarity: ew.vector_similarity,
            decay: ew.decay,
            relevance: ew.relevance,
            epistemic_tier: ew.epistemic_tier,
            relationship_proximity: ew.relationship_proximity,
            access_frequency: ew.access_frequency,
        };
        Self {
            engine: RecallEngine::with_weights(engine_weights),
            config,
        }
    }

    /// Run recall using BM25 text search only (no vector embeddings required).
    ///
    /// Used as a fallback when the embedding provider is in mock mode.
    /// Scores, ranks, and formats results the same way as [`run`](Self::run).
    pub fn run_bm25(
        &self,
        query: &str,
        nous_id: &str,
        text_search: &dyn TextSearch,
        remaining_budget: u64,
    ) -> error::Result<RecallStageResult> {
        if !self.config.enabled {
            debug!("recall disabled");
            return Ok(RecallStageResult::empty());
        }

        let k = self.config.max_results * 3;
        let raw = text_search.search_text(query, k)?;

        if raw.is_empty() {
            debug!("no BM25 recall candidates found");
            return Ok(RecallStageResult::empty());
        }

        let candidates = self.build_candidates(raw, nous_id);
        let ranked = self.engine.rank(candidates);
        Ok(self.finalize_results(ranked, remaining_budget))
    }

    /// Run the recall stage.
    ///
    /// Embeds the query, searches for nearest vectors, scores and ranks results,
    /// then formats the top results as a markdown section for the system prompt.
    /// When `iterative` is enabled, runs a second cycle with terminology-refined queries.
    ///
    /// Non-fatal errors are returned as `Err`. The caller should catch and continue.
    #[instrument(skip_all, fields(nous_id = %nous_id))]
    pub fn run(
        &self,
        query: &str,
        nous_id: &str,
        embedding_provider: &dyn EmbeddingProvider,
        vector_search: &dyn VectorSearch,
        remaining_budget: u64,
    ) -> error::Result<RecallStageResult> {
        if !self.config.enabled {
            debug!("recall disabled");
            return Ok(RecallStageResult::empty());
        }

        if self.config.iterative && self.config.max_cycles > 1 {
            self.run_iterative(
                query,
                nous_id,
                embedding_provider,
                vector_search,
                remaining_budget,
            )
        } else {
            self.run_single(
                query,
                nous_id,
                embedding_provider,
                vector_search,
                remaining_budget,
            )
        }
    }

    fn run_single(
        &self,
        query: &str,
        nous_id: &str,
        embedding_provider: &dyn EmbeddingProvider,
        vector_search: &dyn VectorSearch,
        remaining_budget: u64,
    ) -> error::Result<RecallStageResult> {
        let k = self.config.max_results * 3;
        let query_vec = embed(query, embedding_provider)?;
        let raw = search(vector_search, query_vec, k)?;

        if raw.is_empty() {
            debug!("no recall candidates found");
            return Ok(RecallStageResult::empty());
        }

        let candidates = self.build_candidates(raw, nous_id);
        let ranked = self.engine.rank(candidates);
        Ok(self.finalize_results(ranked, remaining_budget))
    }

    fn run_iterative(
        &self,
        query: &str,
        nous_id: &str,
        embedding_provider: &dyn EmbeddingProvider,
        vector_search: &dyn VectorSearch,
        remaining_budget: u64,
    ) -> error::Result<RecallStageResult> {
        let k = self.config.max_results * 3;

        let query_vec = embed(query, embedding_provider)?;
        let raw_cycle1 = search(vector_search, query_vec, k)?;

        if raw_cycle1.is_empty() {
            debug!("no recall candidates in cycle 1");
            return Ok(RecallStageResult::empty());
        }

        let candidates_c1 = self.build_candidates(raw_cycle1.clone(), nous_id);
        let ranked_c1 = self.engine.rank(candidates_c1);

        let terms = discover_terminology(&ranked_c1, query);
        let gaps = detect_gaps(&ranked_c1);

        if terms.is_empty() && gaps.is_empty() {
            debug!("no novel terms or gaps discovered, skipping cycle 2");
            return Ok(self.finalize_results(ranked_c1, remaining_budget));
        }

        let mut refined = String::from(query);
        for term in &terms {
            refined.push(' ');
            refined.push_str(term);
        }
        for gap in &gaps {
            refined.push(' ');
            refined.push_str(gap);
        }

        debug!(
            new_terms = terms.len(),
            gaps = gaps.len(),
            refined = refined.as_str(),
            "cycle 2 with refined query"
        );

        let refined_vec = embed(&refined, embedding_provider)?;
        let raw_cycle2 = search(vector_search, refined_vec, k)?;

        let mut seen: HashSet<String> = HashSet::new();
        let mut merged: Vec<KnowledgeRecallResult> = Vec::new();
        for r in raw_cycle1 {
            if seen.insert(r.source_id.clone()) {
                merged.push(r);
            }
        }
        for r in raw_cycle2 {
            if seen.insert(r.source_id.clone()) {
                merged.push(r);
            }
        }

        debug!(
            unique_candidates = merged.len(),
            "merged results from 2 cycles"
        );

        let candidates = self.build_candidates(merged, nous_id);
        let ranked = self.engine.rank(candidates);
        Ok(self.finalize_results(ranked, remaining_budget))
    }

    fn finalize_results(
        &self,
        ranked: Vec<ScoredResult>,
        remaining_budget: u64,
    ) -> RecallStageResult {
        let candidates_found = ranked.len();
        let filtered = self.filter(ranked);

        if filtered.is_empty() {
            debug!(candidates_found, "all candidates below min_score");
            return RecallStageResult {
                candidates_found,
                ..RecallStageResult::empty()
            };
        }

        let budget = remaining_budget.min(self.config.max_recall_tokens);
        let (results_injected, section, tokens) = self.format_within_budget(&filtered, budget);

        debug!(
            candidates_found,
            results_injected,
            tokens_consumed = tokens,
            "recall complete"
        );

        RecallStageResult {
            candidates_found,
            results_injected,
            tokens_consumed: tokens,
            recall_section: if section.is_empty() {
                None
            } else {
                Some(section)
            },
        }
    }

    fn build_candidates(
        &self,
        raw: Vec<KnowledgeRecallResult>,
        _nous_id: &str,
    ) -> Vec<ScoredResult> {
        let w = &self.config.weights;
        raw.into_iter()
            .map(|r| ScoredResult {
                content: r.content,
                source_type: r.source_type,
                source_id: r.source_id,
                nous_id: String::new(),
                factors: FactorScores {
                    vector_similarity: self.engine.score_vector_similarity(r.distance),
                    decay: w.decay,
                    relevance: w.relevance,
                    epistemic_tier: w.epistemic_tier,
                    relationship_proximity: w.relationship_proximity,
                    access_frequency: w.access_frequency,
                },
                score: 0.0,
            })
            .collect()
    }

    fn filter(&self, ranked: Vec<ScoredResult>) -> Vec<ScoredResult> {
        ranked
            .into_iter()
            .filter(|r| r.score >= self.config.min_score)
            .take(self.config.max_results)
            .collect()
    }

    fn format_within_budget(&self, results: &[ScoredResult], budget: u64) -> (usize, String, u64) {
        let cpt = self.config.chars_per_token;
        let mut included = Vec::with_capacity(results.len());

        for result in results {
            included.push(result);
            let section = format_section(&included);
            let tokens = estimate_tokens(&section, cpt);
            if tokens > budget {
                included.pop();
                break;
            }
        }

        if included.is_empty() {
            return (0, String::new(), 0);
        }

        let section = format_section(&included);
        let tokens = estimate_tokens(&section, cpt);
        (included.len(), section, tokens)
    }
}

fn embed(query: &str, provider: &dyn EmbeddingProvider) -> error::Result<Vec<f32>> {
    provider.embed(query).map_err(|e| {
        error::RecallEmbeddingSnafu {
            message: e.to_string(),
        }
        .build()
    })
}

fn search(
    vector_search: &dyn VectorSearch,
    query_vec: Vec<f32>,
    k: usize,
) -> error::Result<Vec<KnowledgeRecallResult>> {
    vector_search.search_vectors(query_vec, k, 50).map_err(|e| {
        error::RecallSearchSnafu {
            message: e.to_string(),
        }
        .build()
    })
}

static STOPWORDS: LazyLock<HashSet<&str>> = LazyLock::new(|| {
    HashSet::from([
        "a",
        "an",
        "the",
        "and",
        "but",
        "or",
        "nor",
        "for",
        "yet",
        "so",
        "in",
        "on",
        "at",
        "to",
        "from",
        "by",
        "with",
        "about",
        "into",
        "through",
        "during",
        "before",
        "after",
        "above",
        "below",
        "between",
        "out",
        "off",
        "over",
        "under",
        "again",
        "further",
        "then",
        "once",
        "is",
        "am",
        "are",
        "was",
        "were",
        "be",
        "been",
        "being",
        "have",
        "has",
        "had",
        "having",
        "do",
        "does",
        "did",
        "doing",
        "will",
        "would",
        "shall",
        "should",
        "may",
        "might",
        "must",
        "can",
        "could",
        "need",
        "dare",
        "ought",
        "used",
        "i",
        "me",
        "my",
        "myself",
        "we",
        "our",
        "ours",
        "ourselves",
        "you",
        "your",
        "yours",
        "yourself",
        "yourselves",
        "he",
        "him",
        "his",
        "himself",
        "she",
        "her",
        "hers",
        "herself",
        "it",
        "its",
        "itself",
        "they",
        "them",
        "their",
        "theirs",
        "themselves",
        "what",
        "which",
        "who",
        "whom",
        "this",
        "that",
        "these",
        "those",
        "here",
        "there",
        "when",
        "where",
        "why",
        "how",
        "all",
        "each",
        "every",
        "both",
        "few",
        "more",
        "most",
        "other",
        "some",
        "such",
        "only",
        "own",
        "same",
        "than",
        "too",
        "very",
        "just",
        "also",
        "not",
        "no",
    ])
});

/// Check if a word is a common English stopword.
fn is_stopword(word: &str) -> bool {
    STOPWORDS.contains(word)
}

/// Extract domain-specific terms from first-pass results not present in the original query.
///
/// Splits result content on whitespace, filters stopwords and short words,
/// then returns the top-5 most frequent novel terms.
fn discover_terminology(results: &[ScoredResult], original_query: &str) -> Vec<String> {
    let query_words: HashSet<String> = original_query
        .split_whitespace()
        .map(str::to_lowercase)
        .collect();

    let mut term_freq: HashMap<String, usize> = HashMap::new();
    for result in results {
        for word in result.content.split_whitespace() {
            let cleaned = word
                .trim_matches(|c: char| !c.is_alphanumeric())
                .to_lowercase();
            if cleaned.len() > 3 && !query_words.contains(&cleaned) && !is_stopword(&cleaned) {
                *term_freq.entry(cleaned).or_default() += 1;
            }
        }
    }

    let mut terms: Vec<_> = term_freq.into_iter().collect();
    terms.sort_by(|a, b| b.1.cmp(&a.1));
    terms.into_iter().take(5).map(|(t, _)| t).collect()
}

/// Detect entity references in results that aren't captured as result IDs.
///
/// Scans for capitalized multi-word phrases (2+ consecutive capitalized words)
/// and quoted strings. These represent referenced-but-unretrieved entities.
fn detect_gaps(results: &[ScoredResult]) -> Vec<String> {
    let source_ids: HashSet<&str> = results.iter().map(|r| r.source_id.as_str()).collect();
    let mut gaps: Vec<String> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();

    for result in results {
        let words: Vec<&str> = result.content.split_whitespace().collect();
        let mut i = 0;
        while i < words.len() {
            if starts_with_uppercase(words[i]) {
                let start = i;
                while i < words.len() && starts_with_uppercase(words[i]) {
                    i += 1;
                }
                if i - start >= 2 {
                    let phrase = words[start..i].join(" ");
                    if !source_ids.contains(phrase.as_str()) && seen.insert(phrase.clone()) {
                        gaps.push(phrase);
                    }
                }
            } else {
                i += 1;
            }
        }

        for quoted in extract_quoted_strings(&result.content) {
            if !source_ids.contains(quoted.as_str()) && seen.insert(quoted.clone()) {
                gaps.push(quoted);
            }
        }
    }

    debug!(count = gaps.len(), "detected gaps in recall results");
    gaps
}

fn starts_with_uppercase(word: &str) -> bool {
    word.chars().next().is_some_and(char::is_uppercase)
}

fn extract_quoted_strings(text: &str) -> Vec<String> {
    let parts: Vec<&str> = text.split('"').collect();
    parts
        .iter()
        .enumerate()
        .filter(|(i, part)| i % 2 == 1 && !part.is_empty() && part.len() < 100)
        .map(|(_, part)| (*part).to_owned())
        .collect()
}

/// Format scored results as a markdown section.
#[must_use]
pub fn format_section(results: &[&ScoredResult]) -> String {
    use std::fmt::Write;

    let mut out = String::from(
        "## Recalled Knowledge\n\nThe following facts were recalled from memory (relevance score in brackets):\n",
    );

    for r in results {
        let _ = write!(out, "\n- [{:.2}] {}", r.score, r.content);
    }

    out
}

/// Estimate token count from text length using a configurable character divisor.
///
/// `chars_per_token` is the number of characters assumed per token. Use
/// `RecallConfig::chars_per_token` for operator-configurable behaviour, or
/// pass `4` directly in tests and contexts without a live config.
#[must_use]
pub fn estimate_tokens(text: &str, chars_per_token: u64) -> u64 {
    let len = text.len() as u64;
    len.div_ceil(chars_per_token.max(1))
}

#[cfg(test)]
#[path = "recall_tests.rs"]
mod tests;

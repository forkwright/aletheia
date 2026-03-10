//! Recall pipeline stage — retrieves relevant knowledge and injects into context.

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

/// Recall stage — scores and formats knowledge for injection into the system prompt.
pub struct RecallStage {
    engine: RecallEngine,
    config: RecallConfig,
}

impl RecallStage {
    /// Create a recall stage with default scoring weights.
    #[must_use]
    pub fn new(config: RecallConfig) -> Self {
        Self {
            engine: RecallEngine::new(),
            config,
        }
    }

    /// Run the recall stage.
    ///
    /// Embeds the query, searches for nearest vectors, scores and ranks results,
    /// then formats the top results as a markdown section for the system prompt.
    /// When `iterative` is enabled, runs a second cycle with terminology-refined queries.
    ///
    /// Non-fatal errors are returned as `Err` — the caller should catch and continue.
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

        // Cycle 1: embed and search with original query
        let query_vec = embed(query, embedding_provider)?;
        let raw_cycle1 = search(vector_search, query_vec, k)?;

        if raw_cycle1.is_empty() {
            debug!("no recall candidates in cycle 1");
            return Ok(RecallStageResult::empty());
        }

        // Rank cycle 1 for terminology discovery (clone raw for later merge)
        let candidates_c1 = self.build_candidates(raw_cycle1.clone(), nous_id);
        let ranked_c1 = self.engine.rank(candidates_c1);

        let terms = discover_terminology(&ranked_c1, query);
        let gaps = detect_gaps(&ranked_c1);

        if terms.is_empty() && gaps.is_empty() {
            debug!("no novel terms or gaps discovered, skipping cycle 2");
            return Ok(self.finalize_results(ranked_c1, remaining_budget));
        }

        // Build refined query: original + discovered terms + gap entities
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

        // Cycle 2: embed and search with refined query
        let refined_vec = embed(&refined, embedding_provider)?;
        let raw_cycle2 = search(vector_search, refined_vec, k)?;

        // Merge and deduplicate by source_id
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
        raw.into_iter()
            .map(|r| ScoredResult {
                content: r.content,
                source_type: r.source_type,
                source_id: r.source_id,
                nous_id: String::new(),
                factors: FactorScores {
                    vector_similarity: self.engine.score_vector_similarity(r.distance),
                    decay: 0.5,
                    relevance: 0.5,
                    epistemic_tier: 0.3,
                    relationship_proximity: 0.0,
                    access_frequency: 0.0,
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

    #[expect(
        clippy::unused_self,
        reason = "will use config fields when budget strategy is extended"
    )]
    fn format_within_budget(&self, results: &[ScoredResult], budget: u64) -> (usize, String, u64) {
        let mut included = Vec::with_capacity(results.len());

        for result in results {
            included.push(result);
            let section = format_section(&included);
            let tokens = estimate_tokens(&section);
            if tokens > budget {
                included.pop();
                break;
            }
        }

        if included.is_empty() {
            return (0, String::new(), 0);
        }

        let section = format_section(&included);
        let tokens = estimate_tokens(&section);
        (included.len(), section, tokens)
    }
}

// --- Helpers ---

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

// --- Stopword list ---

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

// --- Terminology discovery ---

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

// --- Gap detection ---

/// Detect entity references in results that aren't captured as result IDs.
///
/// Scans for capitalized multi-word phrases (2+ consecutive capitalized words)
/// and quoted strings. These represent referenced-but-unretrieved entities.
fn detect_gaps(results: &[ScoredResult]) -> Vec<String> {
    let source_ids: HashSet<&str> = results.iter().map(|r| r.source_id.as_str()).collect();
    let mut gaps: Vec<String> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();

    for result in results {
        // Capitalized multi-word phrases
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

        // Quoted strings
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

// --- Formatting ---

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

/// Estimate token count from text length (~4 chars per token, ceiling).
#[must_use]
pub fn estimate_tokens(text: &str) -> u64 {
    let len = text.len() as u64;
    len.div_ceil(4)
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use aletheia_mneme::embedding::MockEmbeddingProvider;

    use super::*;

    struct MockVectorSearch {
        results: Vec<KnowledgeRecallResult>,
    }

    impl MockVectorSearch {
        fn new(results: Vec<KnowledgeRecallResult>) -> Self {
            Self { results }
        }

        fn empty() -> Self {
            Self::new(vec![])
        }
    }

    impl VectorSearch for MockVectorSearch {
        fn search_vectors(
            &self,
            _query_vec: Vec<f32>,
            _k: usize,
            _ef: usize,
        ) -> error::Result<Vec<KnowledgeRecallResult>> {
            Ok(self.results.clone())
        }
    }

    /// Mock that returns different results on successive search calls.
    struct CycledMockSearch {
        cycles: Vec<Vec<KnowledgeRecallResult>>,
        call_index: AtomicUsize,
    }

    impl CycledMockSearch {
        fn new(cycles: Vec<Vec<KnowledgeRecallResult>>) -> Self {
            Self {
                cycles,
                call_index: AtomicUsize::new(0),
            }
        }

        fn call_count(&self) -> usize {
            self.call_index.load(Ordering::Relaxed)
        }
    }

    impl VectorSearch for CycledMockSearch {
        fn search_vectors(
            &self,
            _query_vec: Vec<f32>,
            _k: usize,
            _ef: usize,
        ) -> error::Result<Vec<KnowledgeRecallResult>> {
            let idx = self.call_index.fetch_add(1, Ordering::Relaxed);
            Ok(self.cycles.get(idx).cloned().unwrap_or_default())
        }
    }

    fn mock_embed() -> MockEmbeddingProvider {
        MockEmbeddingProvider::new(384)
    }

    fn make_knowledge_result(content: &str, distance: f64) -> KnowledgeRecallResult {
        KnowledgeRecallResult {
            content: content.to_owned(),
            distance,
            source_type: "fact".to_owned(),
            source_id: format!("fact-{}", content.len()),
        }
    }

    fn make_knowledge_result_with_id(
        content: &str,
        distance: f64,
        source_id: &str,
    ) -> KnowledgeRecallResult {
        KnowledgeRecallResult {
            content: content.to_owned(),
            distance,
            source_type: "fact".to_owned(),
            source_id: source_id.to_owned(),
        }
    }

    fn make_scored(content: &str, score: f64) -> ScoredResult {
        ScoredResult {
            content: content.to_owned(),
            source_type: "fact".to_owned(),
            source_id: "f1".to_owned(),
            nous_id: "syn".to_owned(),
            factors: FactorScores::default(),
            score,
        }
    }

    // --- Existing tests ---

    #[test]
    fn recall_disabled_returns_empty() {
        let config = RecallConfig {
            enabled: false,
            ..Default::default()
        };
        let stage = RecallStage::new(config);
        let result = stage
            .run(
                "query",
                "syn",
                &mock_embed(),
                &MockVectorSearch::empty(),
                10000,
            )
            .unwrap();
        assert_eq!(result.candidates_found, 0);
        assert_eq!(result.results_injected, 0);
        assert!(result.recall_section.is_none());
    }

    #[test]
    fn recall_empty_candidates_returns_empty() {
        let config = RecallConfig::default();
        let stage = RecallStage::new(config);
        let result = stage
            .run(
                "query",
                "syn",
                &mock_embed(),
                &MockVectorSearch::empty(),
                10000,
            )
            .unwrap();
        assert_eq!(result.candidates_found, 0);
        assert!(result.recall_section.is_none());
    }

    #[test]
    fn recall_formats_section_correctly() {
        let a = make_scored("User prefers dark mode", 0.87);
        let b = make_scored("Project deadline is March 15", 0.72);
        let refs: Vec<&ScoredResult> = vec![&a, &b];
        let section = format_section(&refs);

        assert!(section.starts_with("## Recalled Knowledge"));
        assert!(section.contains("[0.87] User prefers dark mode"));
        assert!(section.contains("[0.72] Project deadline is March 15"));
    }

    #[test]
    fn recall_respects_min_score() {
        let results = vec![
            make_knowledge_result("close match", 0.1),
            make_knowledge_result("medium match", 0.8),
            make_knowledge_result("distant match", 1.5),
        ];
        let config = RecallConfig {
            min_score: 0.4,
            ..Default::default()
        };
        let stage = RecallStage::new(config);
        let result = stage
            .run(
                "query",
                "syn",
                &mock_embed(),
                &MockVectorSearch::new(results),
                10000,
            )
            .unwrap();

        assert_eq!(result.candidates_found, 3);
        assert!(result.results_injected <= 3);
        if let Some(ref section) = result.recall_section {
            assert!(!section.contains("distant match"));
        }
    }

    #[test]
    fn recall_respects_max_results() {
        let results: Vec<KnowledgeRecallResult> = (0..10)
            .map(|i| make_knowledge_result(&format!("fact {i}"), 0.1 + f64::from(i) * 0.05))
            .collect();
        let config = RecallConfig {
            max_results: 3,
            min_score: 0.0,
            ..Default::default()
        };
        let stage = RecallStage::new(config);
        let result = stage
            .run(
                "query",
                "syn",
                &mock_embed(),
                &MockVectorSearch::new(results),
                50000,
            )
            .unwrap();

        assert_eq!(result.candidates_found, 10);
        assert!(result.results_injected <= 3);
    }

    #[test]
    fn recall_respects_token_budget() {
        let long_content = "x".repeat(400);
        let results: Vec<KnowledgeRecallResult> = (0..5)
            .map(|i| make_knowledge_result(&format!("{long_content} {i}"), 0.1))
            .collect();
        let config = RecallConfig {
            max_results: 5,
            min_score: 0.0,
            max_recall_tokens: 200,
            ..Default::default()
        };
        let stage = RecallStage::new(config);
        let result = stage
            .run(
                "query",
                "syn",
                &mock_embed(),
                &MockVectorSearch::new(results),
                200,
            )
            .unwrap();

        assert!(result.tokens_consumed <= 200);
        assert!(result.results_injected < 5);
    }

    #[test]
    fn estimate_tokens_heuristic() {
        assert_eq!(estimate_tokens(""), 0);
        assert_eq!(estimate_tokens("abcd"), 1);
        assert_eq!(estimate_tokens("abcde"), 2);
        let text = "x".repeat(400);
        assert_eq!(estimate_tokens(&text), 100);
    }

    #[test]
    fn vector_search_trait_is_object_safe() {
        fn _assert_object_safe(_: &dyn VectorSearch) {}
    }

    #[cfg(feature = "knowledge-store")]
    mod knowledge_bridge_tests {
        use aletheia_mneme::knowledge::EmbeddedChunk;
        use aletheia_mneme::knowledge_store::{KnowledgeConfig, KnowledgeStore};

        use super::super::*;

        const DIM: usize = 4;

        fn make_store() -> Arc<KnowledgeStore> {
            KnowledgeStore::open_mem_with_config(KnowledgeConfig { dim: DIM })
                .expect("open in-memory store")
        }

        fn make_chunk(id: &str, content: &str, embedding: Vec<f32>) -> EmbeddedChunk {
            EmbeddedChunk {
                id: id.into(),
                content: content.to_owned(),
                source_type: "fact".to_owned(),
                source_id: format!("fact-{id}"),
                nous_id: String::new(),
                embedding,
                created_at: jiff::Timestamp::from_second(1_735_689_600).expect("valid epoch"),
            }
        }

        #[test]
        fn empty_store_returns_empty_vec() {
            let store = make_store();
            let search = KnowledgeVectorSearch::new(store);
            let results = search
                .search_vectors(vec![0.0; DIM], 5, 10)
                .expect("search should not error on empty store");
            assert!(results.is_empty());
        }

        #[test]
        fn returns_matching_results() {
            let store = make_store();
            let chunk = make_chunk("c1", "Rust is a systems language", vec![1.0, 0.0, 0.0, 0.0]);
            store.insert_embedding(&chunk).expect("insert embedding");

            let search = KnowledgeVectorSearch::new(Arc::clone(&store));
            let results = search
                .search_vectors(vec![1.0, 0.0, 0.0, 0.0], 5, 10)
                .expect("search");
            assert_eq!(results.len(), 1);
            assert_eq!(results[0].content, "Rust is a systems language");
            assert_eq!(results[0].source_type, "fact");
        }

        #[test]
        fn closer_vectors_rank_first() {
            let store = make_store();
            let close = make_chunk("c1", "close", vec![1.0, 0.0, 0.0, 0.0]);
            let far = make_chunk("c2", "far", vec![0.0, 0.0, 0.0, 1.0]);
            store.insert_embedding(&close).expect("insert close");
            store.insert_embedding(&far).expect("insert far");

            let search = KnowledgeVectorSearch::new(Arc::clone(&store));
            let results = search
                .search_vectors(vec![1.0, 0.0, 0.0, 0.0], 5, 10)
                .expect("search");
            assert_eq!(results.len(), 2);
            assert!(
                results[0].distance <= results[1].distance,
                "closer vector should have smaller distance"
            );
            assert_eq!(results[0].content, "close");
        }

        #[test]
        fn respects_k_limit() {
            let store = make_store();
            for i in 0..5 {
                let mut emb = vec![0.0; DIM];
                emb[i % DIM] = 1.0;
                let chunk = make_chunk(&format!("c{i}"), &format!("fact {i}"), emb);
                store.insert_embedding(&chunk).expect("insert");
            }

            let search = KnowledgeVectorSearch::new(Arc::clone(&store));
            let results = search
                .search_vectors(vec![1.0, 0.0, 0.0, 0.0], 2, 10)
                .expect("search");
            assert!(results.len() <= 2, "should return at most k=2 results");
        }
    }
    // --- Terminology discovery tests ---

    #[test]
    fn terminology_discovery_finds_novel_terms() {
        let results = vec![
            ScoredResult {
                content: "quantum entanglement enables teleportation protocols".to_owned(),
                source_type: "fact".to_owned(),
                source_id: "f1".to_owned(),
                nous_id: String::new(),
                factors: FactorScores::default(),
                score: 0.8,
            },
            ScoredResult {
                content: "quantum computing leverages superposition states".to_owned(),
                source_type: "fact".to_owned(),
                source_id: "f2".to_owned(),
                nous_id: String::new(),
                factors: FactorScores::default(),
                score: 0.7,
            },
        ];

        let terms = discover_terminology(&results, "physics research");
        assert!(!terms.is_empty());
        assert!(terms.contains(&"quantum".to_owned()));
    }

    #[test]
    fn terminology_discovery_ignores_stopwords() {
        let results = vec![ScoredResult {
            content: "the and with from that have been this their those".to_owned(),
            source_type: "fact".to_owned(),
            source_id: "f1".to_owned(),
            nous_id: String::new(),
            factors: FactorScores::default(),
            score: 0.5,
        }];

        let terms = discover_terminology(&results, "test query");
        assert!(
            terms.is_empty(),
            "stopwords should be filtered: got {terms:?}"
        );
    }

    #[test]
    fn terminology_discovery_empty_results() {
        let terms = discover_terminology(&[], "some query");
        assert!(terms.is_empty());
    }

    #[test]
    fn terminology_discovery_skips_short_words() {
        let results = vec![ScoredResult {
            content: "big cat ran far low set quantum".to_owned(),
            source_type: "fact".to_owned(),
            source_id: "f1".to_owned(),
            nous_id: String::new(),
            factors: FactorScores::default(),
            score: 0.5,
        }];

        let terms = discover_terminology(&results, "test");
        // "big", "cat", "ran", "far", "low", "set" are all <= 3 chars, only "quantum" passes
        assert_eq!(terms, vec!["quantum"]);
    }

    // --- Gap detection tests ---

    #[test]
    fn gap_detection_finds_capitalized_phrases() {
        let results = vec![ScoredResult {
            content: "Research on Machine Learning shows promising results".to_owned(),
            source_type: "fact".to_owned(),
            source_id: "f1".to_owned(),
            nous_id: String::new(),
            factors: FactorScores::default(),
            score: 0.8,
        }];

        let gaps = detect_gaps(&results);
        assert!(
            gaps.iter()
                .any(|g| g == "Machine Learning" || g == "Research"),
            "should detect capitalized phrases: got {gaps:?}"
        );
    }

    #[test]
    fn gap_detection_finds_quoted_strings() {
        let results = vec![ScoredResult {
            content: r#"The concept of "neural plasticity" was studied"#.to_owned(),
            source_type: "fact".to_owned(),
            source_id: "f1".to_owned(),
            nous_id: String::new(),
            factors: FactorScores::default(),
            score: 0.7,
        }];

        let gaps = detect_gaps(&results);
        assert!(
            gaps.contains(&"neural plasticity".to_owned()),
            "should detect quoted strings: got {gaps:?}"
        );
    }

    // --- Stopword tests ---

    #[test]
    fn stopword_is_stopword() {
        assert!(is_stopword("the"));
        assert!(is_stopword("and"));
        assert!(is_stopword("but"));
        assert!(is_stopword("with"));
        assert!(!is_stopword("quantum"));
        assert!(!is_stopword("neural"));
        assert!(!is_stopword("database"));
    }

    // --- Iterative recall tests ---

    #[test]
    fn iterative_recall_deduplicates() {
        // Cycle 1: results with domain terms to trigger cycle 2
        let cycle1 = vec![
            make_knowledge_result_with_id(
                "quantum entanglement enables communication",
                0.1,
                "fact-a",
            ),
            make_knowledge_result_with_id("quantum computing research paper", 0.2, "fact-b"),
        ];
        // Cycle 2: overlapping fact-b, plus new fact-c
        let cycle2 = vec![
            make_knowledge_result_with_id("quantum computing research paper", 0.15, "fact-b"),
            make_knowledge_result_with_id("entanglement measurement protocols", 0.3, "fact-c"),
        ];

        let search = CycledMockSearch::new(vec![cycle1, cycle2]);
        let config = RecallConfig {
            iterative: true,
            max_cycles: 2,
            min_score: 0.0,
            max_results: 10,
            ..Default::default()
        };
        let stage = RecallStage::new(config);
        let result = stage
            .run("physics", "syn", &mock_embed(), &search, 50000)
            .unwrap();

        // fact-b should appear only once in the merged set
        assert_eq!(
            result.candidates_found, 3,
            "should have 3 unique candidates"
        );
        assert_eq!(search.call_count(), 2, "should have searched twice");
    }

    #[test]
    fn iterative_recall_disabled_by_default() {
        let cycle1 = vec![make_knowledge_result("quantum research findings", 0.1)];
        let cycle2 = vec![make_knowledge_result("additional results", 0.2)];

        let search = CycledMockSearch::new(vec![cycle1, cycle2]);
        let config = RecallConfig::default(); // iterative: false
        let stage = RecallStage::new(config);
        let _result = stage
            .run("test query", "syn", &mock_embed(), &search, 50000)
            .unwrap();

        assert_eq!(
            search.call_count(),
            1,
            "default config should only search once"
        );
    }
}

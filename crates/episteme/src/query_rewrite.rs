#![allow(
    dead_code,
    reason = "module not yet wired into recall pipeline; lint fires in lib but not test target"
)]
//! LLM-powered query rewriting for the recall pipeline.
//!
//! Rewrites natural language queries into multiple search variants before
//! hybrid search, improving recall for queries that use different terminology
//! than the stored knowledge (e.g., "Cody's truck" vs "Cummins diesel").
use std::time::Instant;

use tracing::instrument;

/// Minimal LLM completion interface for query rewriting.
///
/// Keeps mneme independent of hermeneus. The nous layer bridges this trait
/// to the full `LlmProvider` + `CompletionRequest` API.
pub trait RewriteProvider: Send + Sync {
    /// Generate a completion from a system prompt and user message.
    fn complete(&self, system: &str, user_message: &str) -> Result<String, RewriteError>;
}

/// Errors from the query rewriting pipeline.
#[derive(Debug)]
#[non_exhaustive]
pub enum RewriteError {
    /// The LLM provider returned an error.
    LlmCall(String),
    /// The LLM response could not be parsed.
    ParseResponse(String),
}

impl std::fmt::Display for RewriteError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::LlmCall(msg) => write!(f, "LLM call failed: {msg}"),
            Self::ParseResponse(msg) => write!(f, "failed to parse rewrite response: {msg}"),
        }
    }
}

/// Configuration for query rewriting behavior.
#[derive(Debug, Clone)]
pub struct RewriteConfig {
    /// Maximum number of variant queries to generate (2-4).
    pub max_variants: usize,
    /// Whether to always include the original query in the variant set.
    pub include_original: bool,
}

impl Default for RewriteConfig {
    fn default() -> Self {
        Self {
            max_variants: 4,
            include_original: true,
        }
    }
}

/// Result of a query rewrite operation.
#[derive(Debug, Clone)]
pub struct RewriteResult {
    /// The original query string.
    pub original: String,
    /// Generated search variant queries (may include the original).
    pub variants: Vec<String>,
    /// Time spent on the rewrite operation in milliseconds.
    pub latency_ms: u64,
}

/// LLM-powered query rewriter for the recall pipeline.
pub struct QueryRewriter {
    config: RewriteConfig,
}

impl QueryRewriter {
    /// Create a new query rewriter with the given configuration.
    #[must_use]
    pub fn new(config: RewriteConfig) -> Self {
        Self { config }
    }

    /// Create a query rewriter with default configuration.
    #[must_use]
    pub fn with_defaults() -> Self {
        Self::new(RewriteConfig::default())
    }

    /// Rewrite a query into multiple search variants using an LLM.
    ///
    /// Returns the original query plus generated variants. Never fails;
    /// falls back to the original query on any error.
    #[instrument(skip(self, provider, context))]
    pub fn rewrite(
        &self,
        query: &str,
        context: Option<&str>,
        provider: &dyn RewriteProvider,
    ) -> RewriteResult {
        let start = Instant::now();

        let variants = match self.try_rewrite(query, context, provider) {
            Ok(v) => v,
            Err(e) => {
                tracing::warn!(error = %e, "query rewrite failed, falling back to original");
                vec![query.to_owned()]
            }
        };

        let latency_ms = start.elapsed().as_millis().try_into().unwrap_or(u64::MAX);

        tracing::debug!(
            original = query,
            variant_count = variants.len(),
            latency_ms,
            "query rewrite complete"
        );

        RewriteResult {
            original: query.to_owned(),
            variants,
            latency_ms,
        }
    }

    /// Attempt the rewrite, returning an error on failure.
    fn try_rewrite(
        &self,
        query: &str,
        context: Option<&str>,
        provider: &dyn RewriteProvider,
    ) -> Result<Vec<String>, RewriteError> {
        let system = build_system_prompt(self.config.max_variants);
        let user_message = build_user_message(query, context);

        let response = provider.complete(&system, &user_message)?;
        let mut variants = parse_rewrite_response(&response)?;

        variants.truncate(self.config.max_variants);
        if self.config.include_original && !variants.iter().any(|v| v == query) {
            variants.insert(0, query.to_owned());
        }
        let mut seen = std::collections::HashSet::new();
        variants.retain(|v| seen.insert(v.clone()));

        Ok(variants)
    }
}

/// Build the system prompt for query rewriting.
fn build_system_prompt(max_variants: usize) -> String {
    format!(
        r#"You are a search query expansion engine. Given a search query and optional conversation context, generate {max_variants} alternative search queries that would find relevant facts in a knowledge graph.

For each variant, consider:
- Synonyms and related terms
- Specific entities that might be referenced
- Broader category terms
- Technical names vs colloquial names

Respond with ONLY a JSON array of strings. No commentary, no markdown fences.
Example: ["query variant 1", "query variant 2", "query variant 3"]"#
    )
}

/// Build the user message containing the query and optional context.
fn build_user_message(query: &str, context: Option<&str>) -> String {
    match context {
        Some(ctx) => format!("Query: {query}\nRecent context: {ctx}"),
        None => format!("Query: {query}"),
    }
}

/// Parse the LLM response as a JSON array of query strings.
///
/// Strips markdown code fences if present. Returns an error if parsing fails.
fn parse_rewrite_response(response: &str) -> Result<Vec<String>, RewriteError> {
    let trimmed = strip_code_fences(response);

    let variants: Vec<String> =
        serde_json::from_str(trimmed).map_err(|e| RewriteError::ParseResponse(e.to_string()))?;
    let variants: Vec<String> = variants.into_iter().filter(|v| !v.is_empty()).collect();

    if variants.is_empty() {
        return Err(RewriteError::ParseResponse(
            "LLM returned empty variant list".to_owned(),
        ));
    }

    Ok(variants)
}

/// Strip markdown code fences from an LLM response.
fn strip_code_fences(s: &str) -> &str {
    let trimmed = s.trim();
    if let Some(rest) = trimmed.strip_prefix("```json") {
        rest.strip_suffix("```").unwrap_or(rest).trim()
    } else if let Some(rest) = trimmed.strip_prefix("```") {
        rest.strip_suffix("```").unwrap_or(rest).trim()
    } else {
        trimmed
    }
}

/// Configuration for multi-tier search behavior.
#[derive(Debug, Clone)]
pub struct TieredSearchConfig {
    /// Minimum results from fast path before escalating to enhanced search.
    pub fast_path_min_results: usize,
    /// Minimum RRF score threshold for fast path results to be considered sufficient.
    pub fast_path_score_threshold: f64,
    /// Minimum results from enhanced search before escalating to graph-enhanced.
    pub enhanced_min_results: usize,
    /// Minimum RRF score threshold for enhanced results.
    pub enhanced_score_threshold: f64,
    /// Maximum entities to expand via graph neighborhood in tier 3.
    pub graph_expansion_limit: usize,
}

impl Default for TieredSearchConfig {
    fn default() -> Self {
        Self {
            fast_path_min_results: 3,
            fast_path_score_threshold: 0.5,
            enhanced_min_results: 3,
            enhanced_score_threshold: 0.3,
            graph_expansion_limit: 5,
        }
    }
}

/// Which search tier produced the final results.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum SearchTier {
    /// Single-query hybrid search (BM25 + vector).
    Fast,
    /// LLM query rewrite + multi-query hybrid search.
    Enhanced,
    /// Graph neighborhood expansion on top of enhanced results.
    GraphEnhanced,
}

impl std::fmt::Display for SearchTier {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Fast => f.write_str("fast"),
            Self::Enhanced => f.write_str("enhanced"),
            Self::GraphEnhanced => f.write_str("graph-enhanced"),
        }
    }
}

/// Results from a tiered search operation.
#[derive(Debug, Clone)]
pub struct TieredSearchResult<T> {
    /// Which tier produced the final results.
    pub tier: SearchTier,
    /// The merged, deduplicated results.
    pub results: Vec<T>,
    /// Query variants used (if enhanced tier was reached).
    pub query_variants: Option<Vec<String>>,
    /// Total latency across all tiers in milliseconds.
    pub total_latency_ms: u64,
}

/// Merge multiple sets of hybrid results using reciprocal rank fusion.
///
/// Deduplicates by ID, combining RRF scores from all query variants.
/// Results from multiple queries for the same document get boosted.
pub fn rrf_merge<T: HasId + HasRrfScore + Clone>(results_per_query: &[Vec<T>], k: f64) -> Vec<T> {
    use std::collections::HashMap;

    let mut score_map: HashMap<String, (f64, T)> = HashMap::new();

    for query_results in results_per_query {
        for (rank, result) in query_results.iter().enumerate() {
            #[expect(
                clippy::cast_precision_loss,
                clippy::as_conversions,
                reason = "usize→f64: rank is small enough for f64"
            )]
            let rrf_contribution = 1.0 / (k + rank as f64 + 1.0);
            let entry = score_map
                .entry(result.id().to_owned())
                .or_insert_with(|| (0.0, result.clone()));
            entry.0 += rrf_contribution;
        }
    }

    let mut merged: Vec<(f64, T)> = score_map.into_values().collect();
    merged.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));

    merged
        .into_iter()
        .map(|(score, mut item)| {
            item.set_rrf_score(score);
            item
        })
        .collect()
}

/// Trait for types that have an ID field for deduplication.
pub trait HasId {
    fn id(&self) -> &str;
}

/// Trait for types that have a mutable RRF score.
pub trait HasRrfScore {
    #[expect(
        dead_code,
        reason = "trait API completeness; only set_rrf_score used by merge"
    )]
    fn rrf_score(&self) -> f64;
    fn set_rrf_score(&mut self, score: f64);
}

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test assertions")]
#[expect(
    clippy::indexing_slicing,
    reason = "test: vec indices are valid after asserting len"
)]
mod tests {
    use super::*;

    struct MockProvider {
        response: String,
    }

    impl MockProvider {
        fn with_response(response: &str) -> Self {
            Self {
                response: response.to_owned(),
            }
        }
    }

    impl RewriteProvider for MockProvider {
        fn complete(&self, _system: &str, _user_message: &str) -> Result<String, RewriteError> {
            Ok(self.response.clone())
        }
    }

    struct FailingProvider;

    impl RewriteProvider for FailingProvider {
        fn complete(&self, _: &str, _: &str) -> Result<String, RewriteError> {
            Err(RewriteError::LlmCall("rate limited".to_owned()))
        }
    }

    #[test]
    fn rewrite_produces_variants() {
        let rewriter = QueryRewriter::with_defaults();
        let provider = MockProvider::with_response(
            r#"["Cody truck vehicle", "Cummins diesel specifications", "vehicle equipment modifications"]"#,
        );

        let result = rewriter.rewrite("What's Cody's truck?", None, &provider);

        assert_eq!(result.original, "What's Cody's truck?");
        assert_eq!(result.variants.len(), 4);
        assert_eq!(result.variants[0], "What's Cody's truck?");
        assert!(
            result
                .variants
                .contains(&"Cummins diesel specifications".to_owned())
        );
    }

    #[test]
    fn rewrite_deduplicates_variants() {
        let rewriter = QueryRewriter::with_defaults();
        let provider = MockProvider::with_response(
            r#"["What's Cody's truck?", "Cody truck vehicle", "Cody truck vehicle"]"#,
        );

        let result = rewriter.rewrite("What's Cody's truck?", None, &provider);

        assert_eq!(result.variants.len(), 2);
    }

    #[test]
    fn rewrite_fallback_on_llm_failure() {
        let rewriter = QueryRewriter::with_defaults();
        let result = rewriter.rewrite("test query", None, &FailingProvider);

        assert_eq!(result.variants.len(), 1);
        assert_eq!(result.variants[0], "test query");
    }

    #[test]
    fn rewrite_fallback_on_invalid_json() {
        let rewriter = QueryRewriter::with_defaults();
        let provider = MockProvider::with_response("this is not json");

        let result = rewriter.rewrite("test query", None, &provider);

        assert_eq!(result.variants.len(), 1);
        assert_eq!(result.variants[0], "test query");
    }

    #[test]
    fn rewrite_fallback_on_empty_array() {
        let rewriter = QueryRewriter::with_defaults();
        let provider = MockProvider::with_response("[]");

        let result = rewriter.rewrite("test query", None, &provider);

        assert_eq!(result.variants.len(), 1);
        assert_eq!(result.variants[0], "test query");
    }

    #[test]
    fn rewrite_with_context() {
        let rewriter = QueryRewriter::with_defaults();
        let provider = MockProvider::with_response(r#"["Cody truck", "vehicle maintenance"]"#);

        let result = rewriter.rewrite(
            "What's the truck?",
            Some("We were discussing Cody's vehicles"),
            &provider,
        );

        assert!(!result.variants.is_empty());
        assert!(result.latency_ms < 5000); // validation: latency is within acceptable bounds
    }

    #[test]
    fn rewrite_respects_max_variants() {
        let config = RewriteConfig {
            max_variants: 2,
            include_original: true,
        };
        let rewriter = QueryRewriter::new(config);
        let provider =
            MockProvider::with_response(r#"["variant 1", "variant 2", "variant 3", "variant 4"]"#);

        let result = rewriter.rewrite("query", None, &provider);

        assert!(result.variants.len() <= 3);
    }

    #[test]
    fn rewrite_strips_code_fences() {
        let rewriter = QueryRewriter::with_defaults();
        let provider = MockProvider::with_response("```json\n[\"variant 1\", \"variant 2\"]\n```");

        let result = rewriter.rewrite("query", None, &provider);

        assert!(result.variants.len() >= 2);
    }

    #[test]
    fn rewrite_without_include_original() {
        let config = RewriteConfig {
            max_variants: 4,
            include_original: false,
        };
        let rewriter = QueryRewriter::new(config);
        let provider = MockProvider::with_response(r#"["variant 1", "variant 2"]"#);

        let result = rewriter.rewrite("original query", None, &provider);

        assert_eq!(result.variants.len(), 2);
        assert!(!result.variants.contains(&"original query".to_owned()));
    }

    #[test]
    fn rewrite_filters_empty_strings() {
        let rewriter = QueryRewriter::with_defaults();
        let provider = MockProvider::with_response(r#"["", "variant 1", "", "variant 2"]"#);

        let result = rewriter.rewrite("query", None, &provider);

        assert!(result.variants.iter().all(|v| !v.is_empty()));
    }

    #[test]
    fn rewrite_latency_tracked() {
        let rewriter = QueryRewriter::with_defaults();
        let provider = MockProvider::with_response(r#"["v1", "v2"]"#);

        let result = rewriter.rewrite("query", None, &provider);

        assert!(result.latency_ms < 1000);
    }

    #[test]
    fn parse_valid_response() {
        let variants =
            parse_rewrite_response(r#"["a", "b", "c"]"#).expect("valid JSON array parses");
        assert_eq!(variants, vec!["a", "b", "c"]);
    }

    #[test]
    fn parse_response_with_fences() {
        let variants = parse_rewrite_response("```json\n[\"a\", \"b\"]\n```")
            .expect("JSON array with code fences parses");
        assert_eq!(variants, vec!["a", "b"]);
    }

    #[test]
    fn parse_invalid_json() {
        let result = parse_rewrite_response("not json");
        assert!(result.is_err());
    }

    #[test]
    fn parse_empty_array() {
        let result = parse_rewrite_response("[]");
        assert!(result.is_err());
    }

    #[test]
    fn parse_filters_empty_strings() {
        let variants = parse_rewrite_response(r#"["a", "", "b"]"#)
            .expect("JSON array with empty strings parses");
        assert_eq!(variants, vec!["a", "b"]);
    }

    #[derive(Debug, Clone)]
    struct TestResult {
        doc_id: String,
        score: f64,
    }

    impl HasId for TestResult {
        fn id(&self) -> &str {
            &self.doc_id
        }
    }

    impl HasRrfScore for TestResult {
        fn rrf_score(&self) -> f64 {
            self.score
        }
        fn set_rrf_score(&mut self, score: f64) {
            self.score = score;
        }
    }

    #[test]
    fn rrf_merge_deduplicates_by_id() {
        let q1 = vec![
            TestResult {
                doc_id: "f1".to_owned(),
                score: 0.0,
            },
            TestResult {
                doc_id: "f2".to_owned(),
                score: 0.0,
            },
        ];
        let q2 = vec![
            TestResult {
                doc_id: "f2".to_owned(),
                score: 0.0,
            },
            TestResult {
                doc_id: "f3".to_owned(),
                score: 0.0,
            },
        ];

        let merged = rrf_merge(&[q1, q2], 60.0);

        assert_eq!(merged.len(), 3);
        let ids: Vec<&str> = merged.iter().map(super::HasId::id).collect();
        assert!(ids.contains(&"f1"));
        assert!(ids.contains(&"f2"));
        assert!(ids.contains(&"f3"));
    }

    #[test]
    fn rrf_merge_boosts_duplicate_results() {
        let q1 = vec![
            TestResult {
                doc_id: "f1".to_owned(),
                score: 0.0,
            },
            TestResult {
                doc_id: "f2".to_owned(),
                score: 0.0,
            },
        ];
        let q2 = vec![
            TestResult {
                doc_id: "f1".to_owned(),
                score: 0.0,
            },
            TestResult {
                doc_id: "f3".to_owned(),
                score: 0.0,
            },
        ];

        let merged = rrf_merge(&[q1, q2], 60.0);

        assert_eq!(merged[0].doc_id, "f1");
        assert!(merged[0].score > merged[1].score);
    }

    #[test]
    fn rrf_merge_empty_input() {
        let merged: Vec<TestResult> = rrf_merge(&[], 60.0);
        assert!(merged.is_empty());
    }

    #[test]
    fn rrf_merge_single_query() {
        let q1 = vec![
            TestResult {
                doc_id: "f1".to_owned(),
                score: 0.0,
            },
            TestResult {
                doc_id: "f2".to_owned(),
                score: 0.0,
            },
        ];

        let merged = rrf_merge(&[q1], 60.0);

        assert_eq!(merged.len(), 2);
        assert!(merged[0].score > merged[1].score);
    }

    #[test]
    fn rrf_merge_preserves_order_by_score() {
        let q1 = vec![
            TestResult {
                doc_id: "f1".to_owned(),
                score: 0.0,
            },
            TestResult {
                doc_id: "f2".to_owned(),
                score: 0.0,
            },
            TestResult {
                doc_id: "f3".to_owned(),
                score: 0.0,
            },
        ];
        let q2 = vec![
            TestResult {
                doc_id: "f3".to_owned(),
                score: 0.0,
            },
            TestResult {
                doc_id: "f1".to_owned(),
                score: 0.0,
            },
        ];

        let merged = rrf_merge(&[q1, q2], 60.0);

        for window in merged.windows(2) {
            assert!(window[0].score >= window[1].score);
        }
    }

    #[test]
    fn tiered_config_defaults() {
        let config = TieredSearchConfig::default();
        assert_eq!(config.fast_path_min_results, 3);
        assert!((config.fast_path_score_threshold - 0.5).abs() < f64::EPSILON);
        assert_eq!(config.enhanced_min_results, 3);
        assert!((config.enhanced_score_threshold - 0.3).abs() < f64::EPSILON);
        assert_eq!(config.graph_expansion_limit, 5);
    }

    #[test]
    fn search_tier_display() {
        assert_eq!(SearchTier::Fast.to_string(), "fast");
        assert_eq!(SearchTier::Enhanced.to_string(), "enhanced");
        assert_eq!(SearchTier::GraphEnhanced.to_string(), "graph-enhanced");
    }

    #[test]
    fn system_prompt_contains_instructions() {
        let prompt = build_system_prompt(4);
        assert!(prompt.contains("JSON array"));
        assert!(prompt.contains('4'));
        assert!(prompt.contains("Synonyms"));
    }

    #[test]
    fn user_message_without_context() {
        let msg = build_user_message("test query", None);
        assert!(msg.contains("test query"));
        assert!(!msg.contains("context"));
    }

    #[test]
    fn user_message_with_context() {
        let msg = build_user_message("test query", Some("recent discussion"));
        assert!(msg.contains("test query"));
        assert!(msg.contains("recent discussion"));
    }
}

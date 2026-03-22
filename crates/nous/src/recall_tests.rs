#![expect(clippy::expect_used, reason = "test assertions may panic on failure")]
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
        .expect("recall should succeed when disabled");
    assert_eq!(
        result.candidates_found, 0,
        "disabled recall should find zero candidates"
    );
    assert_eq!(
        result.results_injected, 0,
        "disabled recall should inject zero results"
    );
    assert!(
        result.recall_section.is_none(),
        "disabled recall should have no section"
    );
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
        .expect("recall should succeed with empty candidates");
    assert_eq!(
        result.candidates_found, 0,
        "empty store should find zero candidates"
    );
    assert!(
        result.recall_section.is_none(),
        "empty results should have no section"
    );
}

#[test]
fn recall_formats_section_correctly() {
    let a = make_scored("User prefers dark mode", 0.87);
    let b = make_scored("Project deadline is March 15", 0.72);
    let refs: Vec<&ScoredResult> = vec![&a, &b];
    let section = format_section(&refs);

    assert!(
        section.starts_with("## Recalled Knowledge"),
        "section should start with header"
    );
    assert!(
        section.contains("[0.87] User prefers dark mode"),
        "section should contain first result"
    );
    assert!(
        section.contains("[0.72] Project deadline is March 15"),
        "section should contain second result"
    );
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
        .expect("recall should succeed");

    assert_eq!(result.candidates_found, 3, "should find all 3 candidates");
    assert!(
        result.results_injected <= 3,
        "should inject at most 3 results"
    );
    if let Some(ref section) = result.recall_section {
        assert!(
            !section.contains("distant match"),
            "distant match should be filtered by min_score"
        );
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
        .expect("recall should succeed");

    assert_eq!(result.candidates_found, 10, "should find all 10 candidates");
    assert!(
        result.results_injected <= 3,
        "should inject at most max_results=3"
    );
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
        .expect("recall should succeed");

    assert!(
        result.tokens_consumed <= 200,
        "should not exceed token budget"
    );
    assert!(
        result.results_injected < 5,
        "budget should limit injected results"
    );
}

#[test]
fn estimate_tokens_heuristic() {
    assert_eq!(estimate_tokens("", 4), 0, "empty string should be 0 tokens");
    assert_eq!(estimate_tokens("abcd", 4), 1, "4 chars should be 1 token");
    assert_eq!(
        estimate_tokens("abcde", 4),
        2,
        "5 chars should round up to 2 tokens"
    );
    let text = "x".repeat(400);
    assert_eq!(estimate_tokens(&text, 4), 100, "400 chars / 4 = 100 tokens");
}

#[test]
fn estimate_tokens_custom_divisor() {
    assert_eq!(estimate_tokens("abcdefgh", 2), 4, "8 chars / 2 = 4 tokens");
    assert_eq!(estimate_tokens("hello", 3), 2, "5 chars / 3 rounds up to 2");
}

#[test]
fn estimate_tokens_divisor_clamp() {
    assert_eq!(estimate_tokens("a", 0), 1, "divisor 0 should clamp to 1");
}

#[test]
fn vector_search_trait_is_object_safe() {
    fn _assert_object_safe(_: &dyn VectorSearch) {}
}

#[cfg(feature = "knowledge-store")]
#[expect(
    clippy::indexing_slicing,
    reason = "test: vec indices are valid after asserting len"
)]
#[expect(clippy::expect_used, reason = "test assertions")]
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
            id: aletheia_mneme::id::EmbeddingId::new(id).expect("valid test id"),
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
        assert!(results.is_empty(), "empty store should return no results");
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
        assert_eq!(results.len(), 1, "should find one matching result");
        assert_eq!(
            results[0].content, "Rust is a systems language",
            "content should match"
        );
        assert_eq!(results[0].source_type, "fact", "source_type should be fact");
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
        assert_eq!(results.len(), 2, "should find both results");
        assert!(
            results[0].distance <= results[1].distance,
            "closer vector should have smaller distance"
        );
        assert_eq!(
            results[0].content, "close",
            "closest result should be first"
        );
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
    assert!(!terms.is_empty(), "should discover novel terms");
    assert!(
        terms.contains(&"quantum".to_owned()),
        "should find quantum as novel term"
    );
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
    assert!(terms.is_empty(), "empty results should produce no terms");
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
    assert_eq!(
        terms,
        vec!["quantum"],
        "only words >3 chars should be included"
    );
}

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

#[test]
fn stopword_is_stopword() {
    assert!(is_stopword("the"), "the should be a stopword");
    assert!(is_stopword("and"), "and should be a stopword");
    assert!(is_stopword("but"), "but should be a stopword");
    assert!(is_stopword("with"), "with should be a stopword");
    assert!(!is_stopword("quantum"), "quantum should not be a stopword");
    assert!(!is_stopword("neural"), "neural should not be a stopword");
    assert!(
        !is_stopword("database"),
        "database should not be a stopword"
    );
}

#[test]
fn iterative_recall_deduplicates() {
    let cycle1 = vec![
        make_knowledge_result_with_id("quantum entanglement enables communication", 0.1, "fact-a"),
        make_knowledge_result_with_id("quantum computing research paper", 0.2, "fact-b"),
    ];
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
        .expect("recall should succeed");

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
        .expect("recall should succeed");

    assert_eq!(
        search.call_count(),
        1,
        "default config should only search once"
    );
}

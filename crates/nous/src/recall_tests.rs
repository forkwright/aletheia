#![expect(clippy::expect_used, reason = "test assertions may panic on failure")]
use std::sync::atomic::{AtomicUsize, Ordering};

use mneme::embedding::MockEmbeddingProvider;

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
        sensitivity: mneme::knowledge::FactSensitivity::Public,
        graph_importance: 0.0,
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
        sensitivity: mneme::knowledge::FactSensitivity::Public,
        graph_importance: 0.0,
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
        sensitivity: mneme::knowledge::FactSensitivity::Public,
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
    let section = format_section(&refs, false);

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
    assert!(
        !section.contains("factors:"),
        "disabled metadata should not contain factors"
    );
}

#[test]
fn recall_formats_section_with_metadata() {
    let a = ScoredResult {
        content: "User prefers dark mode".to_owned(),
        source_type: "fact".to_owned(),
        source_id: "f1".to_owned(),
        nous_id: "syn".to_owned(),
        factors: FactorScores {
            vector_similarity: 0.91,
            decay: 0.75,
            relevance: 0.8,
            epistemic_tier: 1.0,
            relationship_proximity: 0.5,
            access_frequency: 0.3,
            graph_importance: 0.5,
        },
        score: 0.87,
        sensitivity: mneme::knowledge::FactSensitivity::Public,
    };
    let b = ScoredResult {
        content: "Project deadline is March 15".to_owned(),
        source_type: "fact".to_owned(),
        source_id: "f2".to_owned(),
        nous_id: "syn".to_owned(),
        factors: FactorScores {
            vector_similarity: 0.82,
            decay: 0.6,
            relevance: 0.7,
            epistemic_tier: 0.6,
            relationship_proximity: 1.0,
            access_frequency: 0.2,
            graph_importance: 0.3,
        },
        score: 0.72,
        sensitivity: mneme::knowledge::FactSensitivity::Public,
    };
    let refs: Vec<&ScoredResult> = vec![&a, &b];
    let section = format_section(&refs, true);

    assert!(
        section.starts_with("## Recalled Knowledge"),
        "section should start with header"
    );
    assert!(
        section.contains("[0.87] User prefers dark mode"),
        "section should contain first result"
    );
    assert!(
        section.contains("(factors: vector=0.91, decay=0.75, relevance=0.80, tier=1.00, proximity=0.50, freq=0.30)"),
        "section should contain first result metadata: {section}"
    );
    assert!(
        section.contains("(factors: vector=0.82, decay=0.60, relevance=0.70, tier=0.60, proximity=1.00, freq=0.20)"),
        "section should contain second result metadata: {section}"
    );
}

#[test]
fn recall_disabled_metadata_returns_plain_bullets() {
    let a = make_scored("Fact one", 0.9);
    let refs: Vec<&ScoredResult> = vec![&a];
    let section = format_section(&refs, false);
    assert!(
        !section.contains("factors:"),
        "disabled metadata should not emit factor line"
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
    use std::sync::Arc;

    use mneme::knowledge::EmbeddedChunk;
    use mneme::knowledge_store::{KnowledgeConfig, KnowledgeStore};

    use super::super::*;

    const DIM: usize = 4;

    fn make_store() -> Arc<KnowledgeStore> {
        KnowledgeStore::open_mem_with_config(KnowledgeConfig {
            dim: DIM,
            ..Default::default()
        })
        .expect("open in-memory store")
    }

    fn make_chunk(id: &str, content: &str, embedding: Vec<f32>) -> EmbeddedChunk {
        EmbeddedChunk {
            id: mneme::id::EmbeddingId::new(id).expect("valid test id"),
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
            sensitivity: mneme::knowledge::FactSensitivity::Public,
        },
        ScoredResult {
            content: "quantum computing leverages superposition states".to_owned(),
            source_type: "fact".to_owned(),
            source_id: "f2".to_owned(),
            nous_id: String::new(),
            factors: FactorScores::default(),
            score: 0.7,
            sensitivity: mneme::knowledge::FactSensitivity::Public,
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
        sensitivity: mneme::knowledge::FactSensitivity::Public,
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
        sensitivity: mneme::knowledge::FactSensitivity::Public,
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
        sensitivity: mneme::knowledge::FactSensitivity::Public,
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
        sensitivity: mneme::knowledge::FactSensitivity::Public,
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

// ── Sovereignty filter tests (#3404, #3413) ──────────────────────────────

fn make_knowledge_result_sensitive(
    content: &str,
    distance: f64,
    source_id: &str,
    sensitivity: mneme::knowledge::FactSensitivity,
) -> KnowledgeRecallResult {
    KnowledgeRecallResult {
        content: content.to_owned(),
        distance,
        source_type: "fact".to_owned(),
        source_id: source_id.to_owned(),
        sensitivity,
        graph_importance: 0.0,
    }
}

#[test]
fn sovereignty_filter_cloud_drops_internal_and_confidential() {
    use hermeneus::provider::DeploymentTarget;
    use mneme::knowledge::FactSensitivity;

    let results = vec![
        make_knowledge_result_sensitive("public A", 0.1, "f-pub", FactSensitivity::Public),
        make_knowledge_result_sensitive("internal B", 0.1, "f-int", FactSensitivity::Internal),
        make_knowledge_result_sensitive(
            "confidential C",
            0.1,
            "f-conf",
            FactSensitivity::Confidential,
        ),
    ];
    let search = MockVectorSearch::new(results);
    let config = RecallConfig {
        min_score: 0.0,
        max_results: 10,
        ..Default::default()
    };
    let stage = RecallStage::new(config).with_deployment_target(DeploymentTarget::Cloud);
    let result = stage
        .run("query", "syn", &mock_embed(), &search, 50000)
        .expect("recall runs");

    let section = result
        .recall_section
        .expect("public result should yield a section");
    assert!(
        section.contains("public A"),
        "Cloud target must keep Public; section = {section}"
    );
    assert!(
        !section.contains("internal B"),
        "Cloud target must drop Internal; section = {section}"
    );
    assert!(
        !section.contains("confidential C"),
        "Cloud target must drop Confidential; section = {section}"
    );
    assert_eq!(
        result.results_injected, 1,
        "only Public fact should be injected on Cloud target"
    );
}

#[test]
fn sovereignty_filter_local_hosted_drops_only_confidential() {
    use hermeneus::provider::DeploymentTarget;
    use mneme::knowledge::FactSensitivity;

    let results = vec![
        make_knowledge_result_sensitive("public A", 0.1, "f-pub", FactSensitivity::Public),
        make_knowledge_result_sensitive("internal B", 0.15, "f-int", FactSensitivity::Internal),
        make_knowledge_result_sensitive(
            "confidential C",
            0.2,
            "f-conf",
            FactSensitivity::Confidential,
        ),
    ];
    let search = MockVectorSearch::new(results);
    let config = RecallConfig {
        min_score: 0.0,
        max_results: 10,
        ..Default::default()
    };
    let stage = RecallStage::new(config).with_deployment_target(DeploymentTarget::LocalHosted);
    let result = stage
        .run("query", "syn", &mock_embed(), &search, 50000)
        .expect("recall runs");

    let section = result
        .recall_section
        .expect("public+internal should yield a section");
    assert!(section.contains("public A"));
    assert!(section.contains("internal B"));
    assert!(!section.contains("confidential C"));
    assert_eq!(result.results_injected, 2);
}

#[test]
fn sovereignty_filter_embedded_keeps_all() {
    use hermeneus::provider::DeploymentTarget;
    use mneme::knowledge::FactSensitivity;

    let results = vec![
        make_knowledge_result_sensitive("public A", 0.1, "f-pub", FactSensitivity::Public),
        make_knowledge_result_sensitive("internal B", 0.15, "f-int", FactSensitivity::Internal),
        make_knowledge_result_sensitive(
            "confidential C",
            0.2,
            "f-conf",
            FactSensitivity::Confidential,
        ),
    ];
    let search = MockVectorSearch::new(results);
    let config = RecallConfig {
        min_score: 0.0,
        max_results: 10,
        ..Default::default()
    };
    let stage = RecallStage::new(config).with_deployment_target(DeploymentTarget::Embedded);
    let result = stage
        .run("query", "syn", &mock_embed(), &search, 50000)
        .expect("recall runs");

    let section = result.recall_section.expect("section present");
    assert!(section.contains("public A"));
    assert!(section.contains("internal B"));
    assert!(section.contains("confidential C"));
    assert_eq!(result.results_injected, 3);
}

#[test]
fn test_internal_fact_admitted_to_local_hosted_provider_but_stripped_from_cloud() {
    // WHY (#3736): end-to-end regression for the OpenAI-compatible
    // provider's sovereignty wiring. The bug: OpenAiProviderConfig had no
    // `deployment_target` field and OpenAiProvider did not override the
    // LlmProvider trait, so operator TOML `deployment_target = "local_hosted"`
    // was logged at startup then silently discarded. Every OpenAI-compat
    // provider — including loopback llama.cpp / logismos — reported `Cloud`
    // via the trait default and the recall filter stripped `Internal` facts
    // from a locally-hosted model's system prompt.
    //
    // This test exercises the wiring from the provider instance through to
    // the admission filter: if either half regresses (config field removed,
    // trait override dropped, or `with_deployment_target` call site drops
    // the provider's value), the assertion fails.
    use hermeneus::openai::{OpenAiProvider, OpenAiProviderConfig};
    use hermeneus::provider::{DeploymentTarget, LlmProvider};
    use mneme::knowledge::FactSensitivity;

    let results = vec![
        make_knowledge_result_sensitive("public A", 0.1, "f-pub", FactSensitivity::Public),
        make_knowledge_result_sensitive("internal B", 0.15, "f-int", FactSensitivity::Internal),
    ];

    // Build an OpenAI-compat provider pointing at loopback llama.cpp with
    // deployment_target = LocalHosted (the operator-intended configuration
    // that the bug silently ignored).
    let local_provider = OpenAiProvider::new(OpenAiProviderConfig {
        name: "local-llama".to_owned(),
        base_url: "http://127.0.0.1:8088/v1".to_owned(),
        models: vec!["qwen-local".to_owned()],
        deployment_target: DeploymentTarget::LocalHosted,
        ..Default::default()
    })
    .expect("local OpenAiProvider init");
    assert_eq!(
        local_provider.deployment_target(),
        DeploymentTarget::LocalHosted,
        "provider must report LocalHosted — the regression point"
    );

    // Admission path: plug the provider's reported target into the recall
    // stage (mirroring pipeline/stages.rs:108-112) and verify `Internal`
    // facts survive the sovereignty filter for a local provider.
    let search_local = MockVectorSearch::new(results.clone());
    let config = RecallConfig {
        min_score: 0.0,
        max_results: 10,
        ..Default::default()
    };
    let local_stage =
        RecallStage::new(config.clone()).with_deployment_target(local_provider.deployment_target());
    let local_result = local_stage
        .run("query", "syn", &mock_embed(), &search_local, 50000)
        .expect("local recall runs");
    let local_section = local_result
        .recall_section
        .expect("public+internal yields a section on LocalHosted target");
    assert!(
        local_section.contains("public A"),
        "LocalHosted must keep Public; section = {local_section}"
    );
    assert!(
        local_section.contains("internal B"),
        "LocalHosted must ADMIT Internal — this is the sovereignty guarantee; section = {local_section}"
    );
    assert_eq!(
        local_result.results_injected, 2,
        "both Public and Internal must be injected on LocalHosted target"
    );

    // Control: a Cloud-default provider (no deployment_target field in
    // TOML) still strips Internal — proves the filter itself is wired and
    // that LocalHosted is not a no-op pass-through.
    let cloud_provider = OpenAiProvider::new(OpenAiProviderConfig {
        name: "cloud-openai".to_owned(),
        base_url: "https://api.openai.com/v1".to_owned(),
        models: vec!["gpt-4o".to_owned()],
        ..Default::default()
    })
    .expect("cloud OpenAiProvider init");
    assert_eq!(
        cloud_provider.deployment_target(),
        DeploymentTarget::Cloud,
        "omitted field must default to Cloud (safe)"
    );

    let search_cloud = MockVectorSearch::new(results);
    let cloud_stage =
        RecallStage::new(config).with_deployment_target(cloud_provider.deployment_target());
    let cloud_result = cloud_stage
        .run("query", "syn", &mock_embed(), &search_cloud, 50000)
        .expect("cloud recall runs");
    let cloud_section = cloud_result
        .recall_section
        .expect("public yields a section on Cloud target");
    assert!(
        cloud_section.contains("public A"),
        "Cloud must keep Public; section = {cloud_section}"
    );
    assert!(
        !cloud_section.contains("internal B"),
        "Cloud must STRIP Internal — the pre-existing sovereignty invariant; section = {cloud_section}"
    );
    assert_eq!(
        cloud_result.results_injected, 1,
        "only Public must be injected on Cloud target"
    );
}

#[test]
fn sovereignty_filter_default_is_cloud() {
    use mneme::knowledge::FactSensitivity;

    // WHY: an unconfigured `RecallStage::new` defaults to Cloud so callers
    // who forget to thread `with_deployment_target` still get the safest
    // behaviour (no Internal/Confidential leaks).
    let results = vec![
        make_knowledge_result_sensitive("public A", 0.1, "f-pub", FactSensitivity::Public),
        make_knowledge_result_sensitive("secret B", 0.1, "f-sec", FactSensitivity::Confidential),
    ];
    let search = MockVectorSearch::new(results);
    let config = RecallConfig {
        min_score: 0.0,
        max_results: 10,
        ..Default::default()
    };
    let stage = RecallStage::new(config);
    let result = stage
        .run("query", "syn", &mock_embed(), &search, 50000)
        .expect("recall runs");

    let section = result.recall_section.expect("public yields a section");
    assert!(section.contains("public A"));
    assert!(
        !section.contains("secret B"),
        "default (Cloud) must drop Confidential"
    );
    assert_eq!(result.results_injected, 1);
}

#[test]
fn recall_injects_metadata_when_enabled() {
    let results = vec![make_knowledge_result("verified fact about Rust", 0.1)];
    let config = RecallConfig {
        inject_metadata: true,
        min_score: 0.0,
        max_results: 10,
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

    let section = result.recall_section.expect("should have recall section");
    assert!(
        section.contains("factors:"),
        "metadata injection should include factors: {section}"
    );
    assert!(
        section.contains("vector="),
        "metadata should include vector similarity: {section}"
    );
}

#[test]
fn recall_omits_metadata_when_disabled() {
    let results = vec![make_knowledge_result("plain fact about Rust", 0.1)];
    let config = RecallConfig {
        inject_metadata: false,
        min_score: 0.0,
        max_results: 10,
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

    let section = result.recall_section.expect("should have recall section");
    assert!(
        !section.contains("factors:"),
        "disabled metadata should omit factors: {section}"
    );
}

#[test]
fn sovereignty_filter_reports_filtered_count_in_result() {
    use hermeneus::provider::DeploymentTarget;
    use mneme::knowledge::FactSensitivity;

    // WHY: `candidates_found` is pre-filter and `results_injected` is
    // post-filter; the delta quantifies what the sovereignty filter
    // removed (audited alongside the info-level log).
    let results = vec![
        make_knowledge_result_sensitive("public A", 0.1, "f-pub", FactSensitivity::Public),
        make_knowledge_result_sensitive("internal B", 0.1, "f-int-42", FactSensitivity::Internal),
        make_knowledge_result_sensitive("secret C", 0.1, "f-sec", FactSensitivity::Confidential),
    ];
    let search = MockVectorSearch::new(results);
    let config = RecallConfig {
        min_score: 0.0,
        max_results: 10,
        ..Default::default()
    };
    let stage = RecallStage::new(config).with_deployment_target(DeploymentTarget::Cloud);
    let result = stage
        .run("query", "syn", &mock_embed(), &search, 50000)
        .expect("recall runs");

    assert_eq!(
        result.candidates_found, 3,
        "candidates_found is pre-filter total"
    );
    assert_eq!(
        result.results_injected, 1,
        "only Public fact survives Cloud sovereignty filter"
    );
}

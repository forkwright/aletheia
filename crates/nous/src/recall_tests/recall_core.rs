//! (Split from `recall_tests.rs` — see parent mod.)

#![expect(clippy::expect_used, reason = "test assertions may panic on failure")]

use super::super::*;
use super::*;

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
        visibility: mneme::knowledge::Visibility::Private,
        scope: None,
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
        visibility: mneme::knowledge::Visibility::Private,
        scope: None,
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

#[test]
fn recall_config_defaults() {
    let config = RecallConfig::default();
    assert!(
        config.pinned_facts.is_empty(),
        "default pinned_facts should be empty"
    );
    assert!(
        !config.late_inject_anchor,
        "default late_inject_anchor should be false"
    );
    assert!(
        config.scope_quotas.is_empty(),
        "default scope_quotas should be empty"
    );
    assert!(
        config.reranker_url.is_none(),
        "default reranker_url should be None"
    );
}

#[test]
fn recall_from_settings_propagation() {
    use eidos::id::FactId;
    use eidos::knowledge::MemoryScope;

    let settings = taxis::config::RecallSettings {
        pinned_facts: vec![
            FactId::new("fact-a").expect("valid"),
            FactId::new("fact-b").expect("valid"),
        ],
        late_inject_anchor: true,
        scope_quotas: {
            let mut m = std::collections::HashMap::new();
            m.insert(MemoryScope::Project, 2);
            m
        },
        reranker_url: Some("http://localhost:9999/rerank".to_owned()),
        ..taxis::config::RecallSettings::default()
    };
    let config = RecallConfig::from(settings);
    assert_eq!(config.pinned_facts.len(), 2);
    assert!(
        config
            .pinned_facts
            .first()
            .is_some_and(|f| f.as_str() == "fact-a")
    );
    assert!(config.late_inject_anchor);
    assert_eq!(config.scope_quotas.get(&MemoryScope::Project), Some(&2));
    assert_eq!(
        config.reranker_url,
        Some("http://localhost:9999/rerank".to_owned())
    );
}

#[test]
fn recall_pinned_facts_before_budgeted() {
    let pinned_id = "pinned-1";
    let results: Vec<KnowledgeRecallResult> = vec![
        make_knowledge_result_with_id("budgeted A", 0.1, "other-1"),
        make_knowledge_result_with_id("budgeted B", 0.1, "other-2"),
        make_knowledge_result_with_id("pinned fact", 0.1, pinned_id),
    ];
    let config = RecallConfig {
        max_results: 2,
        min_score: 0.0,
        pinned_facts: vec![eidos::id::FactId::new(pinned_id).expect("valid")],
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

    assert_eq!(result.results_injected, 3, "pinned + 2 budgeted should fit");
    let section = result.recall_section.expect("should have section");
    let pinned_pos = section
        .find("pinned fact")
        .expect("pinned should be present");
    let a_pos = section.find("budgeted A").expect("A should be present");
    let b_pos = section.find("budgeted B").expect("B should be present");
    assert!(
        pinned_pos < a_pos && pinned_pos < b_pos,
        "pinned fact should appear before budgeted results"
    );
}

#[test]
fn recall_pinned_facts_deduplicated() {
    let pinned_id = "pinned-dup";
    let results: Vec<KnowledgeRecallResult> = vec![
        make_knowledge_result_with_id("first", 0.1, pinned_id),
        make_knowledge_result_with_id("duplicate", 0.1, pinned_id),
    ];
    let config = RecallConfig {
        max_results: 5,
        min_score: 0.0,
        pinned_facts: vec![eidos::id::FactId::new(pinned_id).expect("valid")],
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

    assert_eq!(
        result.results_injected, 1,
        "duplicate pinned facts should be deduplicated"
    );
    let section = result.recall_section.expect("should have section");
    assert!(
        section.matches("pinned-dup").count() <= 2,
        "should only reference the pinned fact once or twice (score + source)"
    );
}

#[test]
fn recall_scope_quotas_reserves_minimum_per_scope() {
    // Build candidates across three scopes with distinct scores.
    let results: Vec<KnowledgeRecallResult> = vec![
        make_knowledge_result_with_scope("project-a", 0.95, Some(MemoryScope::Project)),
        make_knowledge_result_with_scope("project-b", 0.90, Some(MemoryScope::Project)),
        make_knowledge_result_with_scope("project-c", 0.85, Some(MemoryScope::Project)),
        make_knowledge_result_with_scope("user-a", 0.80, Some(MemoryScope::User)),
        make_knowledge_result_with_scope("user-b", 0.75, Some(MemoryScope::User)),
        make_knowledge_result_with_scope("ref-a", 0.70, Some(MemoryScope::Reference)),
    ];

    let mut quotas = std::collections::HashMap::new();
    quotas.insert(MemoryScope::Project, 2);
    quotas.insert(MemoryScope::User, 1);
    // Reference has no quota — falls to general pool.

    let config = RecallConfig {
        max_results: 10,
        min_score: 0.0,
        scope_quotas: quotas,
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

    let section = result.recall_section.expect("should have section");
    // Project quota = 2 → project-a and project-b (higher scores)
    assert!(
        section.contains("project-a"),
        "Project quota should include highest-scoring project fact"
    );
    assert!(
        section.contains("project-b"),
        "Project quota should include second project fact"
    );
    // User quota = 1 → user-a
    assert!(
        section.contains("user-a"),
        "User quota should include highest-scoring user fact"
    );
    // Remaining pool ordered by score: project-c (0.85), user-b (0.75), ref-a (0.70)
    assert!(
        section.contains("project-c"),
        "project-c should fill from general pool"
    );
}

#[test]
fn recall_scope_quotas_underfilled_scope_slack_absorbed() {
    // User scope has only 1 candidate but quota is 5 — slack goes to general pool.
    let results: Vec<KnowledgeRecallResult> = vec![
        make_knowledge_result_with_scope("user-only", 0.60, Some(MemoryScope::User)),
        make_knowledge_result_with_scope("proj-a", 0.95, Some(MemoryScope::Project)),
        make_knowledge_result_with_scope("proj-b", 0.90, Some(MemoryScope::Project)),
    ];

    let mut quotas = std::collections::HashMap::new();
    quotas.insert(MemoryScope::User, 5); // underfilled
    quotas.insert(MemoryScope::Project, 1);

    let config = RecallConfig {
        max_results: 10,
        min_score: 0.0,
        scope_quotas: quotas,
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

    let section = result.recall_section.expect("should have section");
    assert!(
        section.contains("user-only"),
        "underfilled User scope should still appear"
    );
    assert!(
        section.contains("proj-a"),
        "Project quota should include top project fact"
    );
    assert!(
        section.contains("proj-b"),
        "proj-b should appear from slack/general pool"
    );
}

#[test]
fn recall_scope_quotas_noop_when_empty_quotas() {
    let results: Vec<KnowledgeRecallResult> = (0..3)
        .map(|i| {
            make_knowledge_result_with_scope(&format!("fact {i}"), 0.5 + f64::from(i) * 0.1, None)
        })
        .collect();

    let config = RecallConfig {
        max_results: 5,
        min_score: 0.0,
        scope_quotas: std::collections::HashMap::new(),
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

    assert_eq!(
        result.results_injected, 3,
        "empty scope quotas should not alter results"
    );
}

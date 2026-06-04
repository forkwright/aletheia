//! (Split from `recall_tests.rs` — see parent mod.)

#![expect(clippy::expect_used, reason = "test assertions may panic on failure")]

use super::super::*;
use super::*;

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
            visibility: mneme::knowledge::Visibility::Private,
            scope: None,
            project_id: None,
        },
        ScoredResult {
            content: "quantum computing leverages superposition states".to_owned(),
            source_type: "fact".to_owned(),
            source_id: "f2".to_owned(),
            nous_id: String::new(),
            factors: FactorScores::default(),
            score: 0.7,
            sensitivity: mneme::knowledge::FactSensitivity::Public,
            visibility: mneme::knowledge::Visibility::Private,
            scope: None,
            project_id: None,
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
        visibility: mneme::knowledge::Visibility::Private,
        scope: None,
        project_id: None,
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
        visibility: mneme::knowledge::Visibility::Private,
        scope: None,
        project_id: None,
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
        visibility: mneme::knowledge::Visibility::Private,
        scope: None,
        project_id: None,
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
        visibility: mneme::knowledge::Visibility::Private,
        scope: None,
        project_id: None,
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
        nous_id: "syn".to_owned(),
        sensitivity,
        graph_importance: 0.0,
        scope: None,
        project_id: None,
        visibility: mneme::knowledge::Visibility::Private,
        source_count: 0,
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
    assert_eq!(result.filtered_facts.len(), 2);
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
    assert_eq!(result.filtered_facts.len(), 1);
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
    assert!(result.filtered_facts.is_empty());
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
    assert_eq!(
        result
            .filtered_facts
            .iter()
            .map(|fact| fact.id.as_str())
            .collect::<Vec<_>>(),
        vec!["f-int-42", "f-sec"],
        "filtered fact IDs should be preserved for prompt audit"
    );
}

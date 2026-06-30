#![expect(clippy::expect_used, reason = "test assertions")]

use super::*;
use crate::client::EvalClient;
use crate::scenario::{Scenario, ScenarioClassification};
use std::sync::{Arc, Mutex};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, Request, ResponseTemplate};

#[test]
fn canary_provider_returns_scenarios() {
    let provider = CanaryProvider;
    let scenarios = provider.provide();
    assert!(
        !scenarios.is_empty(),
        "canary provider should return scenarios"
    );
    assert_eq!(provider.name(), "canary");
}

#[test]
fn canary_scenarios_have_unique_ids() {
    let scenarios = canary_scenarios();
    let mut ids: Vec<&str> = scenarios.iter().map(|s| s.meta().id).collect();
    let total = ids.len();
    ids.sort_unstable();
    ids.dedup();
    assert_eq!(ids.len(), total, "duplicate canary scenario IDs detected");
}

#[test]
fn canary_scenarios_count() {
    let scenarios = canary_scenarios();
    assert_eq!(scenarios.len(), 25, "expected 25 canary scenarios");
}

#[test]
fn canary_scenarios_have_valid_categories() {
    let scenarios = canary_scenarios();
    let valid_categories = [
        "canary-recall",
        "canary-tool",
        "canary-session",
        "canary-knowledge",
        "canary-conflict",
    ];
    for s in &scenarios {
        let meta = s.meta();
        assert!(
            valid_categories.contains(&meta.category),
            "scenario {} has invalid category: {}",
            meta.id,
            meta.category
        );
    }
}

#[test]
fn recall_and_knowledge_assertive_canaries_state_backend_invariants() {
    for scenario in canary_scenarios() {
        let meta = scenario.meta();
        if matches!(meta.category, "canary-recall" | "canary-knowledge")
            && meta.classification == ScenarioClassification::Assertive
        {
            assert!(
                meta.description
                    .starts_with(support::BACKEND_INVARIANT_PREFIX),
                "assertive {} should state its backend invariant, got {:?}",
                meta.id,
                meta.description
            );
            assert!(
                meta.expected_contains.is_none() && meta.expected_pattern.is_none(),
                "assertive {} should not rely on response-text criteria",
                meta.id
            );
        }
    }
}

#[test]
fn tool_canaries_are_smoke_not_assertive_capability_proofs() {
    for scenario in canary_scenarios() {
        let meta = scenario.meta();
        if meta.category == "canary-tool" {
            assert_eq!(
                meta.classification,
                ScenarioClassification::Smoke,
                "tool canary {} should be smoke unless it observes tool-call invariants",
                meta.id
            );
        }
    }
}

#[tokio::test]
async fn recall_roundtrip_canary_reads_back_durable_fact() {
    init_crypto();
    let server = setup_knowledge_mock(true).await;
    let client = EvalClient::new(server.uri(), Some("token".to_owned()));

    let outcome = RecallInsertQueryRoundtrip.run(&client).await;
    assert!(
        outcome.result.is_ok(),
        "recall roundtrip should pass through durable knowledge APIs: {:?}",
        outcome.result
    );

    let requests = server
        .received_requests()
        .await
        .expect("mock server should record requests");
    assert!(
        !requests
            .iter()
            .any(|request| request.url.path().contains("/messages")),
        "durable recall canary should not pass by asking the model for response text"
    );
}

#[tokio::test]
async fn recall_roundtrip_canary_fails_when_search_does_not_return_ingested_fact() {
    init_crypto();
    let server = setup_knowledge_mock(false).await;
    let client = EvalClient::new(server.uri(), Some("token".to_owned()));

    let outcome = RecallInsertQueryRoundtrip.run(&client).await;
    assert!(
        outcome.result.is_err(),
        "recall roundtrip should fail when durable search omits the ingested fact"
    );
}

fn init_crypto() {
    if rustls::crypto::ring::default_provider()
        .install_default()
        .is_err()
    {
        // Already installed by another test in this process.
    }
}

async fn setup_knowledge_mock(return_search_fact: bool) -> MockServer {
    let server = MockServer::start().await;
    let facts = Arc::new(Mutex::new(Vec::<serde_json::Value>::new()));

    Mock::given(method("GET"))
        .and(path("/api/v1/nous"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "nous": [
                {
                    "id": "nous-canary",
                    "model": "mock",
                    "status": "ready"
                }
            ]
        })))
        .mount(&server)
        .await;

    let ingest_facts = Arc::clone(&facts);
    Mock::given(method("POST"))
        .and(path("/api/v1/knowledge/ingest"))
        .respond_with(move |request: &Request| {
            let body = request
                .body_json::<serde_json::Value>()
                .expect("ingest request should be JSON");
            let content = body
                .get("content")
                .and_then(serde_json::Value::as_str)
                .expect("ingest body should include content string");
            let parsed = serde_json::from_str::<Vec<serde_json::Value>>(content)
                .expect("ingest content should be JSON facts");
            let inserted = parsed.len();
            *ingest_facts.lock().expect("facts mutex") = parsed;
            ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "inserted": inserted,
                "skipped": 0,
                "errors": []
            }))
        })
        .mount(&server)
        .await;

    let search_facts = Arc::clone(&facts);
    Mock::given(method("GET"))
        .and(path("/api/v1/knowledge/search"))
        .respond_with(move |_request: &Request| {
            let results = if return_search_fact {
                search_facts
                    .lock()
                    .expect("facts mutex")
                    .iter()
                    .map(search_result_from_fact)
                    .collect::<Vec<_>>()
            } else {
                Vec::new()
            };
            ResponseTemplate::new(200).set_body_json(serde_json::json!({ "results": results }))
        })
        .mount(&server)
        .await;

    let list_facts = Arc::clone(&facts);
    Mock::given(method("GET"))
        .and(path("/api/v1/knowledge/facts"))
        .respond_with(move |_request: &Request| {
            let facts = list_facts.lock().expect("facts mutex").clone();
            ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "facts": facts,
                "total": facts.len()
            }))
        })
        .mount(&server)
        .await;

    server
}

fn search_result_from_fact(fact: &serde_json::Value) -> serde_json::Value {
    serde_json::json!({
        "id": fact.get("id").cloned().unwrap_or(serde_json::Value::Null),
        "content": fact.get("content").cloned().unwrap_or(serde_json::Value::Null),
        "confidence": fact.get("confidence").cloned().unwrap_or(serde_json::json!(0.0)),
        "tier": fact.get("tier").cloned().unwrap_or(serde_json::json!("")),
        "fact_type": fact.get("fact_type").cloned().unwrap_or(serde_json::json!("")),
        "score": 1.0
    })
}

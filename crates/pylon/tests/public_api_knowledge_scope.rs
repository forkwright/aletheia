#![expect(clippy::expect_used, reason = "test assertions use expect")]
#![expect(
    clippy::indexing_slicing,
    reason = "test: JSON indices are valid after asserting response shape"
)]

use std::sync::Arc;

use axum::body::Body;
use axum::http::{Method, Request, StatusCode};
use tower::ServiceExt;

use pylon::router::build_router;
use symbolon::types::Role;

mod common;
use common::{TestEnv, bearer, issue_test_token_scoped, permissive_security, read_body_json};

#[tokio::test]
async fn scoped_token_rejects_foreign_bulk_import_fact() {
    let env = TestEnv::new().await;
    let token = issue_test_token_scoped(&env.state, Role::Operator, "syn");
    let router = build_router(Arc::clone(&env.state), &permissive_security());
    let fact = make_fact(
        "fact-bob-import",
        "bob",
        "bob owns this imported memory",
        0.7,
    );

    let response = router
        .oneshot(authed_json_request(
            Method::POST,
            "/api/v1/knowledge/facts/import",
            &token,
            &serde_json::json!({ "facts": [fact] }),
        ))
        .await
        .expect("router response");

    assert_scope_forbidden(response).await;
}

#[tokio::test]
async fn scoped_token_rejects_foreign_ingest_target_and_json_fact() {
    let env = TestEnv::new().await;
    let token = issue_test_token_scoped(&env.state, Role::Operator, "syn");
    let router = build_router(Arc::clone(&env.state), &permissive_security());

    let response = router
        .clone()
        .oneshot(authed_json_request(
            Method::POST,
            "/api/v1/knowledge/ingest",
            &token,
            &serde_json::json!({
                "content": "bob remembers the deployment window",
                "format": "text",
                "nous_id": "bob"
            }),
        ))
        .await
        .expect("router response");
    assert_scope_forbidden(response).await;

    let fact = make_fact("fact-bob-ingest", "bob", "bob owns this JSON memory", 0.7);
    let content = serde_json::to_string(&fact).expect("serialize fact");
    let response = router
        .oneshot(authed_json_request(
            Method::POST,
            "/api/v1/knowledge/ingest",
            &token,
            &serde_json::json!({
                "content": content,
                "format": "json",
                "nous_id": "syn"
            }),
        ))
        .await
        .expect("router response");
    assert_scope_forbidden(response).await;
}

#[tokio::test]
async fn scoped_token_rejects_foreign_webhook_target_and_fact() {
    let env = TestEnv::new().await;
    let token = issue_test_token_scoped(&env.state, Role::Operator, "syn");
    let router = build_router(Arc::clone(&env.state), &permissive_security());

    let response = router
        .clone()
        .oneshot(authed_json_request(
            Method::POST,
            "/api/v1/knowledge/ingest/webhook",
            &token,
            &serde_json::json!({
                "nous_id": "bob",
                "facts": [],
                "source": "acme.corp"
            }),
        ))
        .await
        .expect("router response");
    assert_scope_forbidden(response).await;

    let fact = make_fact(
        "fact-bob-webhook",
        "bob",
        "bob owns this webhook memory",
        0.7,
    );
    let response = router
        .oneshot(authed_json_request(
            Method::POST,
            "/api/v1/knowledge/ingest/webhook",
            &token,
            &serde_json::json!({
                "nous_id": "syn",
                "facts": [fact],
                "source": "acme.corp"
            }),
        ))
        .await
        .expect("router response");
    assert_scope_forbidden(response).await;
}

#[cfg(feature = "knowledge-store")]
#[tokio::test]
async fn scoped_ingest_without_target_uses_token_scope() {
    let store = mneme::knowledge_store::KnowledgeStore::open_mem().expect("open knowledge store");
    let env = TestEnv::builder()
        .knowledge_store(Arc::clone(&store))
        .build()
        .await;
    let token = issue_test_token_scoped(&env.state, Role::Operator, "syn");
    let router = build_router(Arc::clone(&env.state), &permissive_security());

    let response = router
        .oneshot(authed_json_request(
            Method::POST,
            "/api/v1/knowledge/ingest",
            &token,
            &serde_json::json!({
                "content": "alice remembers the scoped target can be implicit",
                "format": "text"
            }),
        ))
        .await
        .expect("router response");

    assert_eq!(response.status(), StatusCode::OK);
    let body = read_body_json(response).await;
    assert_eq!(body["inserted"], 1);
    let facts = store.list_all_facts(10).expect("list facts");
    let fact = facts.first().expect("one ingested fact");
    assert_eq!(fact.nous_id, "syn");
}

#[cfg(feature = "knowledge-store")]
#[tokio::test]
async fn scoped_token_rejects_foreign_fact_mutations() {
    let store = mneme::knowledge_store::KnowledgeStore::open_mem().expect("open knowledge store");
    for fact in [
        make_fact("fact-bob-forget", "bob", "bob forget target", 0.4),
        make_fact("fact-bob-restore", "bob", "bob restore target", 0.4),
        make_fact("fact-bob-confidence", "bob", "bob confidence target", 0.2),
        make_fact("fact-bob-sensitivity", "bob", "bob sensitivity target", 0.6),
    ] {
        store.insert_fact(&fact).expect("insert fact");
    }
    let restore_id = mneme::id::FactId::new("fact-bob-restore").expect("valid fact id");
    store
        .forget_fact(&restore_id, mneme::knowledge::ForgetReason::UserRequested)
        .expect("pre-forget restore target");

    let env = TestEnv::builder()
        .knowledge_store(Arc::clone(&store))
        .build()
        .await;
    let token = issue_test_token_scoped(&env.state, Role::Operator, "syn");
    let router = build_router(Arc::clone(&env.state), &permissive_security());

    let response = router
        .clone()
        .oneshot(authed_json_request(
            Method::POST,
            "/api/v1/knowledge/facts/fact-bob-forget/forget",
            &token,
            &serde_json::json!({}),
        ))
        .await
        .expect("router response");
    assert_scope_forbidden(response).await;
    assert!(
        !stored_fact(&store, "fact-bob-forget")
            .lifecycle
            .is_forgotten
    );

    let response = router
        .clone()
        .oneshot(authed_empty_request(
            Method::POST,
            "/api/v1/knowledge/facts/fact-bob-restore/restore",
            &token,
        ))
        .await
        .expect("router response");
    assert_scope_forbidden(response).await;
    assert!(
        stored_fact(&store, "fact-bob-restore")
            .lifecycle
            .is_forgotten
    );

    let response = router
        .clone()
        .oneshot(authed_json_request(
            Method::PUT,
            "/api/v1/knowledge/facts/fact-bob-confidence/confidence",
            &token,
            &serde_json::json!({ "confidence": 0.9 }),
        ))
        .await
        .expect("router response");
    assert_scope_forbidden(response).await;
    assert!(
        (stored_fact(&store, "fact-bob-confidence")
            .provenance
            .confidence
            - 0.2)
            .abs()
            < f64::EPSILON
    );

    let response = router
        .oneshot(authed_json_request(
            Method::PUT,
            "/api/v1/knowledge/facts/fact-bob-sensitivity/sensitivity",
            &token,
            &serde_json::json!({ "sensitivity": "confidential" }),
        ))
        .await
        .expect("router response");
    assert_scope_forbidden(response).await;
    assert_eq!(
        stored_fact(&store, "fact-bob-sensitivity")
            .sensitivity
            .as_str(),
        "public"
    );
}

fn authed_json_request(
    method: Method,
    uri: &str,
    token: &str,
    body: &serde_json::Value,
) -> Request<Body> {
    Request::builder()
        .method(method)
        .uri(uri)
        .header("content-type", "application/json")
        .header("authorization", bearer(token))
        .body(Body::from(
            serde_json::to_vec(&body).expect("serialize json body"),
        ))
        .expect("build request")
}

#[cfg(feature = "knowledge-store")]
fn authed_empty_request(method: Method, uri: &str, token: &str) -> Request<Body> {
    Request::builder()
        .method(method)
        .uri(uri)
        .header("authorization", bearer(token))
        .body(Body::empty())
        .expect("build request")
}

async fn assert_scope_forbidden(response: axum::response::Response) {
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    let body = read_body_json(response).await;
    assert_eq!(body["error"]["code"], "forbidden");
    let message = body["error"]["message"]
        .as_str()
        .expect("error message must be a string");
    assert!(
        message.contains("access denied for this agent"),
        "scope mismatch must not look like an insufficient-role failure: {message}"
    );
}

fn make_fact(id: &str, nous_id: &str, content: &str, confidence: f64) -> mneme::knowledge::Fact {
    mneme::knowledge::Fact {
        id: mneme::id::FactId::new(id).expect("valid fact id"),
        nous_id: nous_id.to_owned(),
        fact_type: "knowledge".to_owned(),
        content: content.to_owned(),
        scope: None,
        project_id: None,
        sensitivity: mneme::knowledge::FactSensitivity::Public,
        visibility: mneme::knowledge::Visibility::Private,
        temporal: mneme::knowledge::FactTemporal {
            valid_from: jiff::Timestamp::UNIX_EPOCH,
            valid_to: jiff::Timestamp::UNIX_EPOCH,
            recorded_at: jiff::Timestamp::UNIX_EPOCH,
        },
        provenance: mneme::knowledge::FactProvenance {
            confidence,
            tier: mneme::knowledge::EpistemicTier::Inferred,
            source_session_id: None,
            stability_hours: 24.0,
        },
        lifecycle: mneme::knowledge::FactLifecycle {
            superseded_by: None,
            is_forgotten: false,
            forgotten_at: None,
            forget_reason: None,
        },
        access: mneme::knowledge::FactAccess {
            access_count: 0,
            last_accessed_at: None,
        },
    }
}

#[cfg(feature = "knowledge-store")]
fn stored_fact(
    store: &Arc<mneme::knowledge_store::KnowledgeStore>,
    id: &str,
) -> mneme::knowledge::Fact {
    store
        .read_facts_by_id(id)
        .expect("read fact")
        .into_iter()
        .next()
        .expect("stored fact")
}

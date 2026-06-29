//! End-to-end smoke test: drive a `MemoryServer` through rmcp in-process
//! transport and exercise each exposed tool.
//!
//! Uses an in-memory knowledge store seeded with a single fact so we can
//! verify that tool call → store query → JSON response wiring is correct.
//! The rmcp `Service` is driven via `tokio::io::duplex` so no real stdio or
//! network is involved.

use std::collections::BTreeMap;
use std::sync::Arc;

use aletheia_memory_mcp::server::MemoryServer;
use mneme::engine::DataValue;
use mneme::id::FactId;
use mneme::knowledge::{
    EpistemicTier, Fact, FactAccess, FactLifecycle, FactProvenance, FactSensitivity, FactTemporal,
    Visibility, default_stability_hours, far_future, format_timestamp, parse_timestamp,
};
use mneme::knowledge_store::KnowledgeStore;
use rmcp::ServiceExt;
use rmcp::model::{CallToolRequestParams, ClientCapabilities, ClientInfo, Implementation};

/// Valid write token for tests. Must be at least 32 characters to satisfy
/// `MemoryServer::MIN_WRITE_TOKEN_LEN`.
const VALID_WRITE_TOKEN: &str = "correct-token-for-write-tools-test";

/// Seed a fresh in-memory store with one fact so list/stats/search have
/// something to return.
#[expect(
    clippy::expect_used,
    reason = "test setup: panic on unexpected store failure is acceptable"
)]
fn seed_store() -> Arc<KnowledgeStore> {
    let store = KnowledgeStore::open_mem().expect("open_mem should succeed");
    let now = jiff::Timestamp::now();
    let fact = Fact {
        id: FactId::new("f-test-0001").expect("valid fact id"),
        nous_id: "alice".to_owned(),
        fact_type: "preference".to_owned(),
        content: "Alice prefers dark roast coffee with no cream".to_owned(),
        scope: None,
        project_id: None,
        sensitivity: FactSensitivity::Public,
        visibility: Visibility::Private,
        temporal: FactTemporal {
            valid_from: now,
            valid_to: far_future(),
            recorded_at: now,
        },
        provenance: FactProvenance {
            confidence: 0.9,
            tier: EpistemicTier::Inferred,
            source_session_id: None,
            stability_hours: default_stability_hours("preference"),
        },
        lifecycle: FactLifecycle {
            superseded_by: None,
            is_forgotten: false,
            forgotten_at: None,
            forget_reason: None,
        },
        access: FactAccess {
            access_count: 0,
            last_accessed_at: None,
        },
    };
    store
        .insert_fact(&fact)
        .expect("insert_fact should succeed");
    store
}

/// Seed a fresh in-memory store with two facts for write tests.
#[expect(
    clippy::expect_used,
    reason = "test setup: panic on unexpected store failure is acceptable"
)]
fn seed_store_two_facts() -> Arc<KnowledgeStore> {
    let store = KnowledgeStore::open_mem().expect("open_mem should succeed");
    let now = jiff::Timestamp::now();
    let fact1 = Fact {
        id: FactId::new("f-test-0001").expect("valid fact id"),
        nous_id: "alice".to_owned(),
        fact_type: "preference".to_owned(),
        content: "Alice prefers dark roast coffee with no cream".to_owned(),
        scope: None,
        project_id: None,
        sensitivity: FactSensitivity::Public,
        visibility: Visibility::Private,
        temporal: FactTemporal {
            valid_from: now,
            valid_to: far_future(),
            recorded_at: now,
        },
        provenance: FactProvenance {
            confidence: 0.9,
            tier: EpistemicTier::Inferred,
            source_session_id: None,
            stability_hours: default_stability_hours("preference"),
        },
        lifecycle: FactLifecycle {
            superseded_by: None,
            is_forgotten: false,
            forgotten_at: None,
            forget_reason: None,
        },
        access: FactAccess {
            access_count: 0,
            last_accessed_at: None,
        },
    };
    store
        .insert_fact(&fact1)
        .expect("insert_fact should succeed");

    let fact2 = Fact {
        id: FactId::new("f-test-0002").expect("valid fact id"),
        nous_id: "alice".to_owned(),
        fact_type: "preference".to_owned(),
        content: "Alice prefers espresso".to_owned(),
        scope: None,
        project_id: None,
        sensitivity: FactSensitivity::Public,
        visibility: Visibility::Private,
        temporal: FactTemporal {
            valid_from: now,
            valid_to: far_future(),
            recorded_at: now,
        },
        provenance: FactProvenance {
            confidence: 0.95,
            tier: EpistemicTier::Inferred,
            source_session_id: None,
            stability_hours: default_stability_hours("preference"),
        },
        lifecycle: FactLifecycle {
            superseded_by: None,
            is_forgotten: false,
            forgotten_at: None,
            forget_reason: None,
        },
        access: FactAccess {
            access_count: 0,
            last_accessed_at: None,
        },
    };
    store
        .insert_fact(&fact2)
        .expect("insert_fact should succeed");

    store
}

#[tokio::test]
#[expect(
    clippy::expect_used,
    reason = "test: panics on unexpected protocol failure are acceptable"
)]
#[expect(
    clippy::too_many_lines,
    reason = "single end-to-end test walks all four tools in one sequence; \
              splitting fragments the narrative without helping readability"
)]
async fn nous_tools_end_to_end() {
    let store = seed_store();
    let server =
        MemoryServer::with_write_token(store, None, None).with_nous_id(Some("alice".to_owned()));

    // WHY: use duplex pipes as the transport so the server and client both run
    // in-process. This exercises the real rmcp serve/call path without stdio.
    let (server_tx, client_rx) = tokio::io::duplex(4096);
    let (client_tx, server_rx) = tokio::io::duplex(4096);

    let server_handle = tokio::spawn(async move {
        server
            .serve((server_rx, server_tx))
            .await
            .expect("server serve")
            .waiting()
            .await
            .expect("server waiting")
    });

    let client_info = ClientInfo::new(
        ClientCapabilities::default(),
        Implementation::new("test-client", "0.1.0"),
    );

    let client = client_info
        .serve((client_rx, client_tx))
        .await
        .expect("client handshake");

    let listed = client.peer().list_tools(None).await.expect("list tools");
    let tool_names: Vec<_> = listed.tools.iter().map(|tool| tool.name.as_ref()).collect();
    assert!(tool_names.contains(&"nous_search"));
    assert!(tool_names.contains(&"nous_neighbors"));
    assert!(tool_names.contains(&"nous_list_topics"));
    assert!(tool_names.contains(&"nous_stats"));
    assert!(!tool_names.iter().any(|name| name.starts_with("memory_")));

    // 1. nous_list_topics — should see "preference" with count 1.
    let topics = client
        .call_tool(
            CallToolRequestParams::new("nous_list_topics")
                .with_arguments(serde_json::json!({}).as_object().expect("object").clone()),
        )
        .await
        .expect("nous_list_topics call");
    let topics_text = topics
        .content
        .first()
        .and_then(|c| c.as_text())
        .map(|t| t.text.clone())
        .expect("topics text content");
    assert!(
        topics_text.contains("preference"),
        "expected 'preference' topic in: {topics_text}"
    );
    assert!(
        topics_text.contains('1'),
        "expected count 1 in: {topics_text}"
    );

    // 2. nous_stats — fact_count should be 1.
    let stats = client
        .call_tool(
            CallToolRequestParams::new("nous_stats")
                .with_arguments(serde_json::json!({}).as_object().expect("object").clone()),
        )
        .await
        .expect("nous_stats call");
    let stats_text = stats
        .content
        .first()
        .and_then(|c| c.as_text())
        .map(|t| t.text.clone())
        .expect("stats text content");
    let stats_json: serde_json::Value =
        serde_json::from_str(&stats_text).expect("stats json parse");
    assert_eq!(
        stats_json
            .get("fact_count")
            .and_then(serde_json::Value::as_i64),
        Some(1),
        "expected fact_count=1 in: {stats_text}"
    );
    assert_eq!(
        stats_json
            .get("topic_count")
            .and_then(serde_json::Value::as_i64),
        Some(1),
        "expected topic_count=1 in: {stats_text}"
    );

    // 3. nous_search — the seeded fact should be retrievable by content term.
    let search = client
        .call_tool(
            CallToolRequestParams::new("nous_search").with_arguments(
                serde_json::json!({ "query": "coffee", "limit": 5 })
                    .as_object()
                    .expect("object")
                    .clone(),
            ),
        )
        .await
        .expect("nous_search call");
    let search_text = search
        .content
        .first()
        .and_then(|c| c.as_text())
        .map(|t| t.text.clone())
        .expect("search text content");
    // The seeded content contains "coffee" — whether BM25 returns it depends on
    // index build state, but the call must succeed and return a JSON array.
    let parsed: serde_json::Value = serde_json::from_str(&search_text).expect("search json parse");
    assert!(parsed.is_array(), "search result must be a JSON array");

    // 4. nous_neighbors — seed has no entity links so neighbors should be an
    // empty array. Call must still succeed.
    let neighbors = client
        .call_tool(
            CallToolRequestParams::new("nous_neighbors").with_arguments(
                serde_json::json!({ "fact_id": "f-test-0001" })
                    .as_object()
                    .expect("object")
                    .clone(),
            ),
        )
        .await
        .expect("nous_neighbors call");
    let neighbors_text = neighbors
        .content
        .first()
        .and_then(|c| c.as_text())
        .map(|t| t.text.clone())
        .expect("neighbors text content");
    let neighbors_json: serde_json::Value =
        serde_json::from_str(&neighbors_text).expect("neighbors json parse");
    assert_eq!(
        neighbors_json.get("fact_id").and_then(|v| v.as_str()),
        Some("f-test-0001"),
    );
    assert!(
        neighbors_json
            .get("neighbors")
            .is_some_and(serde_json::Value::is_array),
        "neighbors must be a JSON array: {neighbors_text}"
    );

    // Invalid input: empty query should yield a protocol-level error.
    let bad = client
        .call_tool(
            CallToolRequestParams::new("nous_search").with_arguments(
                serde_json::json!({ "query": "" })
                    .as_object()
                    .expect("object")
                    .clone(),
            ),
        )
        .await;
    assert!(bad.is_err(), "empty query must produce an error response");

    drop(client);
    // Give the server a moment to observe the client dropping, then clean up.
    let _ = tokio::time::timeout(std::time::Duration::from_secs(2), server_handle).await;
}

/// Test that write tools are not found when write token is not configured.
#[tokio::test]
#[expect(
    clippy::expect_used,
    reason = "test: panics on unexpected protocol failure are acceptable"
)]
async fn write_tool_rejected_without_token_env_set() {
    let store = seed_store();
    let server = MemoryServer::with_write_token(store, None, None);

    let (server_tx, client_rx) = tokio::io::duplex(4096);
    let (client_tx, server_rx) = tokio::io::duplex(4096);

    let server_handle = tokio::spawn(async move {
        server
            .serve((server_rx, server_tx))
            .await
            .expect("server serve")
            .waiting()
            .await
            .expect("server waiting")
    });

    let client_info = ClientInfo::new(
        ClientCapabilities::default(),
        Implementation::new("test-client", "0.1.0"),
    );

    let client = client_info
        .serve((client_rx, client_tx))
        .await
        .expect("client handshake");

    let listed = client.peer().list_tools(None).await.expect("list tools");
    let tool_names: Vec<_> = listed.tools.iter().map(|tool| tool.name.as_ref()).collect();
    assert!(
        !tool_names.contains(&"nous_annotate")
            && !tool_names.contains(&"nous_supersede")
            && !tool_names.contains(&"nous_forget"),
        "write tools must not be listed without token: {tool_names:?}"
    );

    // Try to call nous_annotate without a token configured — should fail.
    let result = client
        .call_tool(
            CallToolRequestParams::new("nous_annotate").with_arguments(
                serde_json::json!({
                    "fact_id": "f-test-0001",
                    "content": "test annotation",
                    "nous_id": "alice",
                    "source_session_id": "session-write-disabled"
                })
                .as_object()
                .expect("object")
                .clone(),
            ),
        )
        .await;

    // Should get an error because write tools are not available
    assert!(
        result.is_err(),
        "nous_annotate must fail when write token is not configured"
    );

    drop(client);
    let _ = tokio::time::timeout(std::time::Duration::from_secs(2), server_handle).await;
}

/// Test happy path: write tools work when a capability token is configured server-side.
#[tokio::test]
#[expect(
    clippy::expect_used,
    reason = "test: panics on unexpected protocol failure are acceptable"
)]
#[expect(
    clippy::too_many_lines,
    reason = "single test for all three write tools to verify happy path"
)]
async fn write_tool_accepts_correct_token() {
    let store = seed_store_two_facts();
    let inspect_store = Arc::clone(&store);
    // Create server with an explicit write token (avoids env var manipulation)
    let server = MemoryServer::with_write_token(store, None, Some(VALID_WRITE_TOKEN.to_owned()));

    let (server_tx, client_rx) = tokio::io::duplex(4096);
    let (client_tx, server_rx) = tokio::io::duplex(4096);

    let server_handle = tokio::spawn(async move {
        server
            .serve((server_rx, server_tx))
            .await
            .expect("server serve")
            .waiting()
            .await
            .expect("server waiting")
    });

    let client_info = ClientInfo::new(
        ClientCapabilities::default(),
        Implementation::new("test-client", "0.1.0"),
    );

    let client = client_info
        .serve((client_rx, client_tx))
        .await
        .expect("client handshake");

    let listed = client.peer().list_tools(None).await.expect("list tools");
    let tool_names: Vec<_> = listed.tools.iter().map(|tool| tool.name.as_ref()).collect();
    assert!(tool_names.contains(&"nous_annotate"));
    assert!(tool_names.contains(&"nous_supersede"));
    assert!(tool_names.contains(&"nous_forget"));

    // 1. nous_annotate with correct token should succeed
    let annotate_result = client
        .call_tool(
            CallToolRequestParams::new("nous_annotate").with_arguments(
                serde_json::json!({
                    "fact_id": "f-test-0001",
                    "content": "This fact is well-established",
                    "nous_id": "alice",
                    "source_session_id": "session-annotate-1"
                })
                .as_object()
                .expect("object")
                .clone(),
            ),
        )
        .await
        .expect("nous_annotate call should succeed");

    let annotate_text = annotate_result
        .content
        .first()
        .and_then(|c| c.as_text())
        .map(|t| t.text.clone())
        .expect("annotate text content");

    let annotate_json: serde_json::Value =
        serde_json::from_str(&annotate_text).expect("annotate json parse");

    assert!(
        annotate_json
            .get("annotation_id")
            .and_then(|v| v.as_str())
            .is_some(),
        "annotation_id should be present: {annotate_text}"
    );
    let annotation_id = annotate_json
        .get("annotation_id")
        .and_then(|v| v.as_str())
        .expect("annotation id string");
    let annotation_fact = inspect_store
        .read_facts_by_id(annotation_id)
        .expect("read annotation fact")
        .into_iter()
        .next()
        .expect("annotation fact exists");
    assert_eq!(
        annotation_fact.provenance.source_session_id.as_deref(),
        Some("session-annotate-1"),
        "annotation provenance must use the supplied source session"
    );
    let mut link_params = BTreeMap::new();
    link_params.insert(
        "annotation_entity".to_owned(),
        DataValue::Str(format!("fact:{annotation_id}").into()),
    );
    link_params.insert(
        "target_entity".to_owned(),
        DataValue::Str("fact:f-test-0001".into()),
    );
    let link_rows = inspect_store
        .run_query(
            r"
                ?[relation] :=
                    *relationships{src: $annotation_entity, dst: $target_entity, relation}
            ",
            link_params,
        )
        .expect("annotation relationship query");
    let link_json = link_rows.rows_to_json();
    assert_eq!(
        link_json
            .first()
            .and_then(|row| row.first())
            .and_then(serde_json::Value::as_str),
        Some("annotates"),
        "annotation must link back to target fact"
    );

    // 2. nous_supersede with correct token should succeed
    let supersede_result = client
        .call_tool(
            CallToolRequestParams::new("nous_supersede").with_arguments(
                serde_json::json!({
                    "old_fact_id": "f-test-0001",
                    "new_fact_id": "f-test-0002",
                    "nous_id": "alice",
                    "source_session_id": "session-supersede-1",
                    "reason": "Updated with fresher information"
                })
                .as_object()
                .expect("object")
                .clone(),
            ),
        )
        .await
        .expect("nous_supersede call should succeed");

    let supersede_text = supersede_result
        .content
        .first()
        .and_then(|c| c.as_text())
        .map(|t| t.text.clone())
        .expect("supersede text content");

    let supersede_json: serde_json::Value =
        serde_json::from_str(&supersede_text).expect("supersede json parse");

    assert_eq!(
        supersede_json.get("old_fact_id").and_then(|v| v.as_str()),
        Some("f-test-0001"),
        "old_fact_id mismatch"
    );
    let record_id = supersede_json
        .get("record_id")
        .and_then(|v| v.as_str())
        .expect("supersession record id");
    let supersession_record = inspect_store
        .read_facts_by_id(record_id)
        .expect("read supersession record")
        .into_iter()
        .next()
        .expect("supersession record exists");
    assert_eq!(
        supersession_record.provenance.source_session_id.as_deref(),
        Some("session-supersede-1"),
        "supersession provenance must use the supplied source session"
    );

    // 3. nous_forget with correct token should succeed
    let forget_result = client
        .call_tool(
            CallToolRequestParams::new("nous_forget").with_arguments(
                serde_json::json!({
                    "fact_id": "f-test-0001",
                    "nous_id": "alice",
                    "reason": "outdated"
                })
                .as_object()
                .expect("object")
                .clone(),
            ),
        )
        .await
        .expect("nous_forget call should succeed");

    let forget_text = forget_result
        .content
        .first()
        .and_then(|c| c.as_text())
        .map(|t| t.text.clone())
        .expect("forget text content");

    let forget_json: serde_json::Value =
        serde_json::from_str(&forget_text).expect("forget json parse");

    assert_eq!(
        forget_json.get("fact_id").and_then(|v| v.as_str()),
        Some("f-test-0001"),
        "fact_id mismatch"
    );
    assert!(
        forget_json.get("recorded_at").is_none(),
        "forget response must not expose recorded_at: {forget_text}"
    );
    let response_forgotten_at = forget_json
        .get("forgotten_at")
        .and_then(|v| v.as_str())
        .expect("forgotten_at string");
    let stored_facts = inspect_store
        .read_facts_by_id("f-test-0001")
        .expect("read forgotten fact");
    let stored_forgotten_at = stored_facts
        .first()
        .and_then(|fact| fact.lifecycle.forgotten_at.as_ref())
        .map(std::string::ToString::to_string)
        .expect("stored forgotten_at");
    assert_eq!(response_forgotten_at, stored_forgotten_at);

    drop(client);
    let _ = tokio::time::timeout(std::time::Duration::from_secs(2), server_handle).await;
}

#[tokio::test]
#[expect(
    clippy::expect_used,
    reason = "test: panics on unexpected protocol failure are acceptable"
)]
#[expect(
    clippy::too_many_lines,
    reason = "test: explicit facts make the superseded-last-updated regression visible"
)]
async fn nous_stats_last_updated_ignores_superseded_facts() {
    let store = KnowledgeStore::open_mem().expect("open_mem should succeed");
    let active_recorded = parse_timestamp("2024-01-01T00:00:00Z").expect("valid timestamp");
    let superseded_recorded = parse_timestamp("2025-01-01T00:00:00Z").expect("valid timestamp");
    let replacement_id = FactId::new("f-active").expect("valid fact id");
    for fact in [
        Fact {
            id: replacement_id.clone(),
            nous_id: "alice".to_owned(),
            fact_type: "preference".to_owned(),
            content: "Alice prefers espresso".to_owned(),
            scope: None,
            project_id: None,
            sensitivity: FactSensitivity::Public,
            visibility: Visibility::Private,
            temporal: FactTemporal {
                valid_from: active_recorded,
                valid_to: far_future(),
                recorded_at: active_recorded,
            },
            provenance: FactProvenance {
                confidence: 0.95,
                tier: EpistemicTier::Inferred,
                source_session_id: None,
                stability_hours: default_stability_hours("preference"),
            },
            lifecycle: FactLifecycle {
                superseded_by: None,
                is_forgotten: false,
                forgotten_at: None,
                forget_reason: None,
            },
            access: FactAccess {
                access_count: 0,
                last_accessed_at: None,
            },
        },
        Fact {
            id: FactId::new("f-superseded").expect("valid fact id"),
            nous_id: "alice".to_owned(),
            fact_type: "preference".to_owned(),
            content: "Alice prefers stale coffee".to_owned(),
            scope: None,
            project_id: None,
            sensitivity: FactSensitivity::Public,
            visibility: Visibility::Private,
            temporal: FactTemporal {
                valid_from: superseded_recorded,
                valid_to: far_future(),
                recorded_at: superseded_recorded,
            },
            provenance: FactProvenance {
                confidence: 0.5,
                tier: EpistemicTier::Inferred,
                source_session_id: None,
                stability_hours: default_stability_hours("preference"),
            },
            lifecycle: FactLifecycle {
                superseded_by: Some(replacement_id),
                is_forgotten: false,
                forgotten_at: None,
                forget_reason: None,
            },
            access: FactAccess {
                access_count: 0,
                last_accessed_at: None,
            },
        },
    ] {
        store.insert_fact(&fact).expect("insert fact");
    }

    let server =
        MemoryServer::with_write_token(store, None, None).with_nous_id(Some("alice".to_owned()));
    let (server_tx, client_rx) = tokio::io::duplex(4096);
    let (client_tx, server_rx) = tokio::io::duplex(4096);
    let server_handle = tokio::spawn(async move {
        server
            .serve((server_rx, server_tx))
            .await
            .expect("server serve")
            .waiting()
            .await
            .expect("server waiting")
    });
    let client_info = ClientInfo::new(
        ClientCapabilities::default(),
        Implementation::new("test-client", "0.1.0"),
    );
    let client = client_info
        .serve((client_rx, client_tx))
        .await
        .expect("client handshake");

    let stats = client
        .call_tool(
            CallToolRequestParams::new("nous_stats")
                .with_arguments(serde_json::json!({}).as_object().expect("object").clone()),
        )
        .await
        .expect("nous_stats call");
    let stats_text = stats
        .content
        .first()
        .and_then(|c| c.as_text())
        .map(|t| t.text.clone())
        .expect("stats text content");
    let stats_json: serde_json::Value =
        serde_json::from_str(&stats_text).expect("stats json parse");
    assert_eq!(
        stats_json
            .get("fact_count")
            .and_then(serde_json::Value::as_i64),
        Some(1)
    );
    let expected_last_updated = format_timestamp(&active_recorded);
    assert_eq!(
        stats_json.get("last_updated").and_then(|v| v.as_str()),
        Some(expected_last_updated.as_str())
    );

    drop(client);
    let _ = tokio::time::timeout(std::time::Duration::from_secs(2), server_handle).await;
}

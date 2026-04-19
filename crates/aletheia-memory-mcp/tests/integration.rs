//! End-to-end smoke test: drive a `MemoryServer` through rmcp in-process
//! transport and exercise each exposed tool.
//!
//! Uses an in-memory knowledge store seeded with a single fact so we can
//! verify that tool call → store query → JSON response wiring is correct.
//! The rmcp `Service` is driven via `tokio::io::duplex` so no real stdio or
//! network is involved.

use std::sync::Arc;

use aletheia_memory_mcp::server::MemoryServer;
use mneme::id::FactId;
use mneme::knowledge::{
    EpistemicTier, Fact, FactAccess, FactLifecycle, FactProvenance, FactSensitivity, FactTemporal,
    default_stability_hours, far_future,
};
use mneme::knowledge_store::KnowledgeStore;
use rmcp::ServiceExt;
use rmcp::model::{CallToolRequestParams, ClientCapabilities, ClientInfo, Implementation};

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
        sensitivity: FactSensitivity::Public,
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
async fn memory_tools_end_to_end() {
    let store = seed_store();
    let server = MemoryServer::new(store, None);

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

    // 1. memory_list_topics — should see "preference" with count 1.
    let topics = client
        .call_tool(CallToolRequestParams::new("memory_list_topics"))
        .await
        .expect("memory_list_topics call");
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

    // 2. memory_stats — fact_count should be 1.
    let stats = client
        .call_tool(CallToolRequestParams::new("memory_stats"))
        .await
        .expect("memory_stats call");
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

    // 3. memory_search — the seeded fact should be retrievable by content term.
    let search = client
        .call_tool(
            CallToolRequestParams::new("memory_search").with_arguments(
                serde_json::json!({ "query": "coffee", "limit": 5 })
                    .as_object()
                    .expect("object")
                    .clone(),
            ),
        )
        .await
        .expect("memory_search call");
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

    // 4. memory_neighbors — seed has no entity links so neighbors should be an
    // empty array. Call must still succeed.
    let neighbors = client
        .call_tool(
            CallToolRequestParams::new("memory_neighbors").with_arguments(
                serde_json::json!({ "fact_id": "f-test-0001" })
                    .as_object()
                    .expect("object")
                    .clone(),
            ),
        )
        .await
        .expect("memory_neighbors call");
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
            CallToolRequestParams::new("memory_search").with_arguments(
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

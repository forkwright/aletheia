//! Integration tests: organon tool executors → mneme session store.
//!
//! Tests the `note` and `blackboard` tools with real `SessionStore` adapters,
//! and memory search/correct/audit tools with a real `KnowledgeSearchService`.
//!
//! What is NOT tested here (already covered in organon unit tests):
//! - Mock-backed note/blackboard tool internals (see organon/src/builtins/memory.rs)
//!
//! What IS new here:
//! - Real `SessionStore` ↔ `SessionNoteAdapter` ↔ note tool executor path
//! - Real `SessionStore` ↔ `SessionBlackboardAdapter` ↔ blackboard tool executor path
//! - `KnowledgeSearchService` → memory tool executor wiring

#![expect(clippy::expect_used, reason = "test assertions")]
use std::collections::{BTreeMap, HashSet};
use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::{Arc, RwLock};

use tokio::sync::Mutex;

use koina::id::ToolName;
use koina::id::{NousId, SessionId};
use mneme::embedding::{EmbeddingProvider, MockEmbeddingProvider};
use mneme::id::{EmbeddingId, FactId};
use mneme::knowledge::{
    EmbeddedChunk, EpistemicTier, Fact, FactAccess, FactLifecycle, FactProvenance, FactSensitivity,
    FactTemporal, Visibility,
};
use mneme::knowledge_store::{KnowledgeConfig, KnowledgeStore};
use mneme::store::SessionStore;
use nous::adapters::{SessionBlackboardAdapter, SessionNoteAdapter};
use organon::builtins;
use organon::error::KnowledgeAdapterError;
use organon::registry::ToolRegistry;
use organon::testing::install_crypto_provider;
use organon::types::{
    FactSummary, KnowledgeSearchService, MemoryResult, ServerToolConfig, ToolContext, ToolInput,
    ToolServices,
};

const KNOWLEDGE_DIM: usize = 384;
const RECORDED_AT: &str = "2026-03-01T00:00:00Z";

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn test_store() -> Arc<Mutex<SessionStore>> {
    Arc::new(Mutex::new(
        SessionStore::open_in_memory().expect("in-memory store"),
    ))
}

fn ctx_with_notes_bb(store: &Arc<Mutex<SessionStore>>) -> ToolContext {
    install_crypto_provider();
    let session_id = SessionId::new();
    {
        let s = store.try_lock().expect("lock not contended in test setup");
        s.create_session(&session_id.to_string(), "alice", "test-key", None, None)
            .expect("create session");
    }
    let note_adapter = Arc::new(SessionNoteAdapter(Arc::clone(store)));
    let bb_adapter = Arc::new(SessionBlackboardAdapter(Arc::clone(store)));
    ToolContext {
        nous_id: NousId::new("alice").expect("valid"),
        session_id,
        turn_number: 0,
        workspace: PathBuf::from("/tmp/test"),
        allowed_roots: vec![PathBuf::from("/tmp")],
        services: Some(Arc::new(ToolServices {
            working_checkpoint_store: None,
            cross_nous: None,
            messenger: None,
            note_store: Some(note_adapter),
            blackboard_store: Some(bb_adapter),
            spawn: None,
            planning: None,
            knowledge: None,
            http_client: reqwest::Client::new(),
            secret_vault: hermeneus::secret::SecretVault::new(),
            lazy_tool_catalog: vec![],
            server_tool_config: ServerToolConfig::default(),
        })),
        active_tools: Arc::new(RwLock::new(HashSet::new())),
        tool_config: Arc::new(taxis::config::ToolLimitsConfig::default()),
    }
}

fn registry() -> ToolRegistry {
    let mut reg = ToolRegistry::new();
    builtins::register_all(&mut reg).expect("register builtins");
    reg
}

fn tool_input(name: &str, args: serde_json::Value) -> ToolInput {
    ToolInput {
        name: ToolName::new(name).expect("valid tool name"),
        tool_use_id: "tu_test".to_owned(),
        arguments: args,
    }
}

// ---------------------------------------------------------------------------
// Note tool → real SessionStore
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread")]
async fn note_add_and_list_uses_real_store() {
    let store = test_store();
    let reg = registry();
    let ctx = ctx_with_notes_bb(&store);

    // Add a note
    let add = tool_input(
        "note",
        serde_json::json!({"action": "add", "content": "remember the vow", "category": "task"}),
    );
    let r = reg.execute(&add, &ctx).await.expect("execute");
    assert!(
        !r.is_error,
        "add should succeed: {}",
        r.content.text_summary()
    );
    assert!(r.content.text_summary().contains("#1"));

    // List notes: should show the note
    let list = tool_input("note", serde_json::json!({"action": "list"}));
    let r = reg.execute(&list, &ctx).await.expect("execute");
    assert!(!r.is_error);
    assert!(r.content.text_summary().contains("remember the vow"));
}

#[tokio::test(flavor = "multi_thread")]
async fn note_delete_removes_from_real_store() {
    let store = test_store();
    let reg = registry();
    let ctx = ctx_with_notes_bb(&store);

    // Add then delete
    let add = tool_input(
        "note",
        serde_json::json!({"action": "add", "content": "to be deleted"}),
    );
    reg.execute(&add, &ctx).await.expect("execute");

    let del = tool_input("note", serde_json::json!({"action": "delete", "id": 1}));
    let r = reg.execute(&del, &ctx).await.expect("execute");
    assert!(!r.is_error, "delete should succeed");
    assert!(r.content.text_summary().contains("deleted"));

    // Verify gone
    let list = tool_input("note", serde_json::json!({"action": "list"}));
    let r = reg.execute(&list, &ctx).await.expect("execute");
    assert!(r.content.text_summary().contains("No session notes"));
}

// ---------------------------------------------------------------------------
// Blackboard tool → real SessionStore
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread")]
async fn blackboard_write_and_read_uses_real_store() {
    let store = test_store();
    let reg = registry();
    let ctx = ctx_with_notes_bb(&store);

    let write = tool_input(
        "blackboard",
        serde_json::json!({"action": "write", "key": "status", "value": "ready", "ttl_seconds": 3600}),
    );
    let r = reg.execute(&write, &ctx).await.expect("execute");
    assert!(
        !r.is_error,
        "write should succeed: {}",
        r.content.text_summary()
    );
    assert!(r.content.text_summary().contains("status"));

    let read = tool_input(
        "blackboard",
        serde_json::json!({"action": "read", "key": "status"}),
    );
    let r = reg.execute(&read, &ctx).await.expect("execute");
    assert!(!r.is_error);
    assert!(r.content.text_summary().contains("ready"));
}

#[tokio::test(flavor = "multi_thread")]
async fn blackboard_delete_uses_real_store() {
    let store = test_store();
    let reg = registry();
    let ctx = ctx_with_notes_bb(&store);

    // Write then delete
    let write = tool_input(
        "blackboard",
        serde_json::json!({"action": "write", "key": "temp", "value": "gone", "ttl_seconds": 60}),
    );
    reg.execute(&write, &ctx).await.expect("execute");

    let del = tool_input(
        "blackboard",
        serde_json::json!({"action": "delete", "key": "temp"}),
    );
    let r = reg.execute(&del, &ctx).await.expect("execute");
    assert!(!r.is_error);
    assert!(r.content.text_summary().contains("deleted"));

    // Should be gone
    let read = tool_input(
        "blackboard",
        serde_json::json!({"action": "read", "key": "temp"}),
    );
    let r = reg.execute(&read, &ctx).await.expect("execute");
    assert!(r.content.text_summary().contains("No entry"));
}

// ---------------------------------------------------------------------------
// Real KnowledgeSearchService backed by mneme KnowledgeStore
// ---------------------------------------------------------------------------

struct RealKnowledgeFixture {
    service: Arc<RealKnowledgeService>,
    _tmp: tempfile::TempDir,
}

struct RealKnowledgeService {
    store: Arc<KnowledgeStore>,
    embedder: Arc<dyn EmbeddingProvider>,
}

impl RealKnowledgeFixture {
    fn new() -> Self {
        let tmp = tempfile::TempDir::new().expect("tmpdir");
        let store = KnowledgeStore::open_fjall(
            tmp.path().join("knowledge").join("shared"),
            KnowledgeConfig {
                dim: KNOWLEDGE_DIM,
                ..KnowledgeConfig::default()
            },
        )
        .expect("open tempfile fjall knowledge store");
        let embedder = Arc::new(MockEmbeddingProvider::new(KNOWLEDGE_DIM));
        Self {
            service: Arc::new(RealKnowledgeService { store, embedder }),
            _tmp: tmp,
        }
    }

    fn seed_fact(&self, id: &str, content: &str) {
        self.service.seed_fact(id, "alice", content);
    }
}

impl RealKnowledgeService {
    fn seed_fact(&self, id: &str, nous_id: &str, content: &str) {
        self.store
            .insert_fact(&fact(id, nous_id, content))
            .expect("insert fact");
        self.store
            .insert_embedding(&embedded_chunk(
                self.embedder.as_ref(),
                &format!("emb-{id}"),
                id,
                nous_id,
                content,
            ))
            .expect("insert embedding");
    }
}

impl KnowledgeSearchService for RealKnowledgeService {
    fn search(
        &self,
        query: &str,
        _nous_id: &str,
        limit: usize,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<MemoryResult>, KnowledgeAdapterError>> + Send + '_>>
    {
        let query = query.to_owned();
        Box::pin(async move {
            let embedding = self.embedder.embed(&query).map_err(|e| {
                organon::error::EmbeddingSnafu {
                    message: e.to_string(),
                }
                .build()
            })?;
            let results = self
                .store
                .search_vectors_async(embedding, i64::try_from(limit).unwrap_or(i64::MAX), 50)
                .await
                .map_err(|e| {
                    organon::error::SearchSnafu {
                        message: e.to_string(),
                    }
                    .build()
                })?;
            Ok(results
                .into_iter()
                .map(|r| MemoryResult {
                    id: r.source_id,
                    content: r.content,
                    score: 1.0 / (1.0 + r.distance),
                    source_type: r.source_type,
                })
                .collect())
        })
    }

    fn correct_fact(
        &self,
        _fact_id: &str,
        new_content: &str,
        nous_id: &str,
    ) -> Pin<Box<dyn Future<Output = Result<String, KnowledgeAdapterError>> + Send + '_>> {
        let new_content = new_content.to_owned();
        let nous_id = nous_id.to_owned();
        Box::pin(async move {
            let new_id = format!("fact-corrected-{}", koina::ulid::Ulid::new());
            self.seed_fact(&new_id, &nous_id, &new_content);
            Ok(new_id)
        })
    }

    fn retract_fact(
        &self,
        fact_id: &str,
        reason: Option<&str>,
    ) -> Pin<Box<dyn Future<Output = Result<(), KnowledgeAdapterError>> + Send + '_>> {
        let fact_id = fact_id.to_owned();
        let reason = reason.unwrap_or("mistake").to_owned();
        Box::pin(async move {
            let fact_id = FactId::new(fact_id).map_err(|e| {
                organon::error::MutateStoreSnafu {
                    message: e.to_string(),
                }
                .build()
            })?;
            let reason = reason
                .parse()
                .map_err(|e: String| organon::error::InvalidReasonSnafu { reason: e }.build())?;
            self.store
                .forget_fact_async(fact_id, reason)
                .await
                .map(|_| ())
                .map_err(|e| {
                    organon::error::MutateStoreSnafu {
                        message: e.to_string(),
                    }
                    .build()
                })
        })
    }

    fn audit_facts(
        &self,
        nous_id: Option<&str>,
        since: Option<&str>,
        limit: usize,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<FactSummary>, KnowledgeAdapterError>> + Send + '_>>
    {
        let nous_id = nous_id.unwrap_or("alice").to_owned();
        let since = since
            .and_then(mneme::knowledge::parse_timestamp)
            .unwrap_or(jiff::Timestamp::UNIX_EPOCH);
        Box::pin(async move {
            let facts = self
                .store
                .audit_all_facts_async(nous_id, i64::try_from(limit).unwrap_or(i64::MAX))
                .await
                .map_err(|e| {
                    organon::error::FactQuerySnafu {
                        message: e.to_string(),
                    }
                    .build()
                })?;
            Ok(facts
                .into_iter()
                .filter(|f| f.temporal.recorded_at >= since)
                .map(fact_summary)
                .collect())
        })
    }

    fn forget_fact(
        &self,
        fact_id: &str,
        reason: &str,
    ) -> Pin<
        Box<
            dyn Future<Output = Result<organon::types::FactSummary, KnowledgeAdapterError>>
                + Send
                + '_,
        >,
    > {
        let fact_id = fact_id.to_owned();
        let reason = reason.to_owned();
        Box::pin(async move {
            let fact_id = FactId::new(fact_id).map_err(|e| {
                organon::error::MutateStoreSnafu {
                    message: e.to_string(),
                }
                .build()
            })?;
            let reason = reason
                .parse()
                .map_err(|e: String| organon::error::InvalidReasonSnafu { reason: e }.build())?;
            self.store
                .forget_fact_async(fact_id, reason)
                .await
                .map(fact_summary)
                .map_err(|e| {
                    organon::error::MutateStoreSnafu {
                        message: e.to_string(),
                    }
                    .build()
                })
        })
    }

    fn unforget_fact(
        &self,
        fact_id: &str,
    ) -> Pin<
        Box<
            dyn Future<Output = Result<organon::types::FactSummary, KnowledgeAdapterError>>
                + Send
                + '_,
        >,
    > {
        let fact_id = fact_id.to_owned();
        Box::pin(async move {
            let fact_id = FactId::new(fact_id).map_err(|e| {
                organon::error::MutateStoreSnafu {
                    message: e.to_string(),
                }
                .build()
            })?;
            self.store
                .unforget_fact_async(fact_id)
                .await
                .map(fact_summary)
                .map_err(|e| {
                    organon::error::MutateStoreSnafu {
                        message: e.to_string(),
                    }
                    .build()
                })
        })
    }

    fn find_skill_by_name(
        &self,
        nous_id: &str,
        skill_name: &str,
    ) -> Pin<Box<dyn Future<Output = Result<Option<String>, KnowledgeAdapterError>> + Send + '_>>
    {
        let nous_id = nous_id.to_owned();
        let skill_name = skill_name.to_owned();
        Box::pin(async move {
            let facts = self
                .store
                .query_facts_async(nous_id, "9999-12-31T00:00:00Z".to_owned(), 1000)
                .await
                .map_err(|e| {
                    organon::error::FactQuerySnafu {
                        message: e.to_string(),
                    }
                    .build()
                })?;
            Ok(facts
                .into_iter()
                .find(|f| f.content.contains(&skill_name))
                .map(|f| f.content))
        })
    }

    fn datalog_query(
        &self,
        query: &str,
        params: Option<serde_json::Value>,
        timeout_secs: Option<f64>,
        row_limit: Option<usize>,
    ) -> Pin<
        Box<
            dyn Future<Output = Result<organon::types::DatalogResult, KnowledgeAdapterError>>
                + Send
                + '_,
        >,
    > {
        let query = query.to_owned();
        Box::pin(async move {
            let rows = self
                .store
                .run_query_with_timeout(
                    &query,
                    json_params(params),
                    timeout_secs.map(std::time::Duration::from_secs_f64),
                )
                .map_err(|e| {
                    organon::error::DatalogQuerySnafu {
                        message: e.to_string(),
                    }
                    .build()
                })?;
            let limit = row_limit.unwrap_or(100);
            let truncated = rows.row_count() > limit;
            let columns = rows.headers.iter().map(ToString::to_string).collect();
            let result_rows = rows.rows_to_json().into_iter().take(limit).collect();
            Ok(organon::types::DatalogResult {
                columns,
                rows: result_rows,
                truncated,
            })
        })
    }
}

fn ts() -> jiff::Timestamp {
    RECORDED_AT.parse().expect("valid timestamp")
}

fn fact(id: &str, nous_id: &str, content: &str) -> Fact {
    Fact {
        id: FactId::new(id).expect("valid fact id"),
        nous_id: nous_id.to_owned(),
        content: content.to_owned(),
        fact_type: "observation".to_owned(),
        scope: None,
        temporal: FactTemporal {
            valid_from: ts(),
            valid_to: mneme::knowledge::far_future(),
            recorded_at: ts(),
        },
        provenance: FactProvenance {
            confidence: 0.95,
            tier: EpistemicTier::Verified,
            source_session_id: Some("ses-organon".to_owned()),
            stability_hours: mneme::knowledge::default_stability_hours("observation"),
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
        sensitivity: FactSensitivity::Public,
        visibility: Visibility::Private,
    }
}

fn embedded_chunk(
    embedder: &dyn EmbeddingProvider,
    embedding_id: &str,
    fact_id: &str,
    nous_id: &str,
    content: &str,
) -> EmbeddedChunk {
    EmbeddedChunk {
        id: EmbeddingId::new(embedding_id).expect("valid embedding id"),
        content: content.to_owned(),
        source_type: "fact".to_owned(),
        source_id: fact_id.to_owned(),
        nous_id: nous_id.to_owned(),
        embedding: embedder.embed(content).expect("embed fixture"),
        created_at: ts(),
    }
}

fn fact_summary(f: Fact) -> FactSummary {
    FactSummary {
        id: f.id.to_string(),
        content: f.content,
        confidence: f.provenance.confidence,
        tier: f.provenance.tier.to_string(),
        recorded_at: mneme::knowledge::format_timestamp(&f.temporal.recorded_at),
        is_forgotten: f.lifecycle.is_forgotten,
        forgotten_at: f.lifecycle.forgotten_at.map(|t| t.to_string()),
        forget_reason: f.lifecycle.forget_reason.map(|r| r.to_string()),
    }
}

fn json_params(params: Option<serde_json::Value>) -> BTreeMap<String, mneme::engine::DataValue> {
    let Some(serde_json::Value::Object(map)) = params else {
        return BTreeMap::new();
    };
    map.into_iter()
        .map(|(key, value)| (key, json_to_datavalue(&value)))
        .collect()
}

fn json_to_datavalue(value: &serde_json::Value) -> mneme::engine::DataValue {
    match value {
        serde_json::Value::Null => mneme::engine::DataValue::Null,
        serde_json::Value::Bool(v) => mneme::engine::DataValue::Bool(*v),
        serde_json::Value::Number(n) => n
            .as_i64()
            .map(mneme::engine::DataValue::from)
            .or_else(|| n.as_f64().map(mneme::engine::DataValue::from))
            .unwrap_or(mneme::engine::DataValue::Null),
        serde_json::Value::String(s) => mneme::engine::DataValue::Str(s.as_str().into()),
        _ => mneme::engine::DataValue::Str(value.to_string().into()),
    }
}

fn ctx_with_knowledge(svc: Arc<dyn KnowledgeSearchService>) -> ToolContext {
    install_crypto_provider();
    ToolContext {
        nous_id: NousId::new("alice").expect("valid"),
        session_id: SessionId::new(),
        turn_number: 0,
        workspace: PathBuf::from("/tmp/test"),
        allowed_roots: vec![PathBuf::from("/tmp")],
        services: Some(Arc::new(ToolServices {
            working_checkpoint_store: None,
            cross_nous: None,
            messenger: None,
            note_store: None,
            blackboard_store: None,
            spawn: None,
            planning: None,
            knowledge: Some(svc),
            http_client: reqwest::Client::new(),
            secret_vault: hermeneus::secret::SecretVault::new(),
            lazy_tool_catalog: vec![],
            server_tool_config: ServerToolConfig::default(),
        })),
        active_tools: Arc::new(RwLock::new(HashSet::new())),
        tool_config: Arc::new(taxis::config::ToolLimitsConfig::default()),
    }
}

// ---------------------------------------------------------------------------
// Memory search tool executor wiring
// ---------------------------------------------------------------------------

#[tokio::test]
async fn memory_search_tool_returns_results() {
    let fixture = RealKnowledgeFixture::new();
    fixture.seed_fact("fact-1", "Alice works on the Aletheia project");

    let reg = registry();
    let ctx = ctx_with_knowledge(fixture.service.clone());

    let input = tool_input(
        "memory_search",
        serde_json::json!({"query": "Aletheia", "limit": 5}),
    );
    let r = reg.execute(&input, &ctx).await.expect("execute");
    assert!(
        !r.is_error,
        "search should succeed: {}",
        r.content.text_summary()
    );
    assert!(
        r.content
            .text_summary()
            .contains("Alice works on the Aletheia project"),
        "result should include seeded fact: {}",
        r.content.text_summary()
    );
}

#[tokio::test]
async fn memory_search_returns_empty_message_when_no_results() {
    let fixture = RealKnowledgeFixture::new();
    let reg = registry();
    let ctx = ctx_with_knowledge(fixture.service.clone());

    let input = tool_input(
        "memory_search",
        serde_json::json!({"query": "nonexistent topic"}),
    );
    let r = reg.execute(&input, &ctx).await.expect("execute");
    assert!(!r.is_error);
    assert!(
        r.content.text_summary().contains("No memories found"),
        "empty result message: {}",
        r.content.text_summary()
    );
}

#[tokio::test]
async fn memory_correct_tool_reports_new_id() {
    let fixture = RealKnowledgeFixture::new();
    fixture.seed_fact("fact-42", "Bob prefers Python");

    let reg = registry();
    let ctx = ctx_with_knowledge(fixture.service.clone());

    let input = tool_input(
        "memory_correct",
        serde_json::json!({"fact_id": "fact-42", "new_content": "Bob prefers Rust"}),
    );
    let r = reg.execute(&input, &ctx).await.expect("execute");
    assert!(
        !r.is_error,
        "correct should succeed: {}",
        r.content.text_summary()
    );
    assert!(
        r.content.text_summary().contains("fact-42"),
        "response should reference original fact: {}",
        r.content.text_summary()
    );
    assert!(
        r.content.text_summary().contains("fact-corrected-"),
        "response should include new fact id: {}",
        r.content.text_summary()
    );

    let memories = fixture
        .service
        .search("Bob prefers Rust", "alice", 5)
        .await
        .expect("search corrected fact");
    let memory_contents: Vec<&str> = memories.iter().map(|m| m.content.as_str()).collect();
    assert!(
        memories
            .iter()
            .any(|m| m.content.contains("Bob prefers Rust")),
        "specific invariant: correction should insert searchable content through KnowledgeStore; got: {memory_contents:?}"
    );
}

#[tokio::test]
async fn memory_audit_tool_returns_facts() {
    let fixture = RealKnowledgeFixture::new();
    fixture.seed_fact("fact-a", "Alice is an engineer");
    fixture.seed_fact("fact-b", "Bob works at acme.corp");

    let reg = registry();
    let ctx = ctx_with_knowledge(fixture.service.clone());

    let input = tool_input("memory_audit", serde_json::json!({"limit": 10}));
    let r = reg.execute(&input, &ctx).await.expect("execute");
    assert!(
        !r.is_error,
        "audit should succeed: {}",
        r.content.text_summary()
    );
    let text = r.content.text_summary();
    assert!(
        text.contains("Alice is an engineer"),
        "fact-a in output: {text}"
    );
    assert!(
        text.contains("Bob works at acme.corp"),
        "fact-b in output: {text}"
    );
}

#[tokio::test]
async fn datalog_query_tool_hits_real_knowledge_store() {
    let fixture = RealKnowledgeFixture::new();
    fixture.seed_fact("fact-datalog", "Datalog query reaches the real store");

    let reg = registry();
    let ctx = ctx_with_knowledge(fixture.service.clone());

    let input = tool_input(
        "datalog_query",
        serde_json::json!({"query": "?[id, content] := *facts{id, content}", "row_limit": 10}),
    );
    let r = reg.execute(&input, &ctx).await.expect("execute");
    assert!(
        !r.is_error,
        "specific invariant: read-only Datalog query should succeed against KnowledgeStore: {}",
        r.content.text_summary()
    );
    let text = r.content.text_summary();
    assert!(
        text.contains("fact-datalog") && text.contains("Datalog query reaches the real store"),
        "specific invariant: datalog_query output must come from the real facts relation; got: {text}"
    );
}

#[tokio::test]
async fn datalog_query_tool_rejects_mutations_before_store() {
    let fixture = RealKnowledgeFixture::new();
    let reg = registry();
    let ctx = ctx_with_knowledge(fixture.service.clone());

    let input = tool_input(
        "datalog_query",
        serde_json::json!({"query": "?[id] := id = \"x\" :put facts {id}"}),
    );
    let r = reg.execute(&input, &ctx).await.expect("execute");
    assert!(
        r.is_error,
        "specific invariant: mutating datalog_query must return tool error"
    );
    assert!(
        r.content.text_summary().contains("mutation keyword"),
        "specific invariant: mutation rejection should name the read-only guard; got: {}",
        r.content.text_summary()
    );
}

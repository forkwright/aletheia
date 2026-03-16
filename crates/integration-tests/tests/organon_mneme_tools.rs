//! Integration tests: organon tool executors → mneme session store.
//!
//! Tests the `note` and `blackboard` tools with real `SessionStore` adapters,
//! and memory search/correct/audit tools with a stub `KnowledgeSearchService`.
//!
//! What is NOT tested here (already covered in organon unit tests):
//! - Mock-backed note/blackboard tool internals (see organon/src/builtins/memory.rs)
//!
//! What IS new here:
//! - Real `SessionStore` ↔ `SessionNoteAdapter` ↔ note tool executor path
//! - Real `SessionStore` ↔ `SessionBlackboardAdapter` ↔ blackboard tool executor path
//! - `KnowledgeSearchService` → memory tool executor wiring

#![expect(clippy::unwrap_used, reason = "test assertions")]
#![expect(clippy::expect_used, reason = "test assertions")]

use aletheia_organon::error::KnowledgeAdapterError;
use std::collections::HashSet;
use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::Arc;
use tokio::sync::Mutex;

fn install_crypto_provider() {
    let _ = rustls::crypto::ring::default_provider().install_default();
}

use aletheia_koina::id::ToolName;
use aletheia_koina::id::{NousId, SessionId};
use aletheia_mneme::store::SessionStore;
use aletheia_nous::adapters::{SessionBlackboardAdapter, SessionNoteAdapter};
use aletheia_organon::builtins;
use aletheia_organon::registry::ToolRegistry;
use aletheia_organon::types::{
    FactSummary, KnowledgeSearchService, MemoryResult, ServerToolConfig, ToolContext, ToolInput,
    ToolServices,
};
use std::sync::RwLock;

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
        workspace: PathBuf::from("/tmp/test"),
        allowed_roots: vec![PathBuf::from("/tmp")],
        services: Some(Arc::new(ToolServices {
            cross_nous: None,
            messenger: None,
            note_store: Some(note_adapter),
            blackboard_store: Some(bb_adapter),
            spawn: None,
            planning: None,
            knowledge: None,
            http_client: reqwest::Client::new(),
            lazy_tool_catalog: vec![],
            server_tool_config: ServerToolConfig::default(),
        })),
        active_tools: Arc::new(RwLock::new(HashSet::new())),
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
// Stub KnowledgeSearchService for testing executor wiring
// ---------------------------------------------------------------------------

struct StubKnowledgeService {
    facts: std::sync::Mutex<Vec<(String, String)>>, // (id, content)
    next_id: std::sync::Mutex<u32>,
    corrected: std::sync::Mutex<Vec<(String, String)>>, // (old_id, new_id)
    retracted: std::sync::Mutex<Vec<String>>,
    audited: std::sync::Mutex<Vec<(String, String)>>, // (id, content): FactSummary not Clone
}

impl StubKnowledgeService {
    fn new() -> Self {
        Self {
            facts: std::sync::Mutex::new(Vec::new()),
            next_id: std::sync::Mutex::new(1),
            corrected: std::sync::Mutex::new(Vec::new()),
            retracted: std::sync::Mutex::new(Vec::new()),
            audited: std::sync::Mutex::new(Vec::new()),
        }
    }

    fn seed_fact(&self, id: &str, content: &str) {
        self.facts
            .lock()
            .unwrap()
            .push((id.to_owned(), content.to_owned()));
        self.audited
            .lock()
            .unwrap()
            .push((id.to_owned(), content.to_owned()));
    }
}

impl KnowledgeSearchService for StubKnowledgeService {
    fn search(
        &self,
        query: &str,
        _nous_id: &str,
        _limit: usize,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<MemoryResult>, KnowledgeAdapterError>> + Send + '_>>
    {
        let results: Vec<MemoryResult> = self
            .facts
            .lock()
            .unwrap()
            .iter()
            .filter(|(_, content)| content.contains(query))
            .map(|(id, content)| MemoryResult {
                id: id.clone(),
                content: content.clone(),
                score: 0.95,
                source_type: "fact".to_owned(),
            })
            .collect();
        Box::pin(std::future::ready(Ok(results)))
    }

    fn correct_fact(
        &self,
        fact_id: &str,
        _new_content: &str,
        _nous_id: &str,
    ) -> Pin<Box<dyn Future<Output = Result<String, KnowledgeAdapterError>> + Send + '_>> {
        let mut n = self.next_id.lock().unwrap();
        let new_id = format!("fact-corrected-{n}");
        *n += 1;
        self.corrected
            .lock()
            .unwrap()
            .push((fact_id.to_owned(), new_id.clone()));
        Box::pin(std::future::ready(Ok(new_id)))
    }

    fn retract_fact(
        &self,
        fact_id: &str,
        _reason: Option<&str>,
    ) -> Pin<Box<dyn Future<Output = Result<(), KnowledgeAdapterError>> + Send + '_>> {
        self.retracted.lock().unwrap().push(fact_id.to_owned());
        Box::pin(std::future::ready(Ok(())))
    }

    fn audit_facts(
        &self,
        _nous_id: Option<&str>,
        _since: Option<&str>,
        _limit: usize,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<FactSummary>, KnowledgeAdapterError>> + Send + '_>>
    {
        let facts: Vec<FactSummary> = self
            .audited
            .lock()
            .unwrap()
            .iter()
            .map(|(id, content)| FactSummary {
                id: id.clone(),
                content: content.clone(),
                confidence: 0.9,
                tier: "verified".to_owned(),
                recorded_at: "2026-01-01T00:00:00Z".to_owned(),
                is_forgotten: false,
                forgotten_at: None,
                forget_reason: None,
            })
            .collect();
        Box::pin(std::future::ready(Ok(facts)))
    }

    fn forget_fact(
        &self,
        fact_id: &str,
        reason: &str,
    ) -> Pin<
        Box<
            dyn Future<Output = Result<aletheia_organon::types::FactSummary, KnowledgeAdapterError>>
                + Send
                + '_,
        >,
    > {
        let summary = aletheia_organon::types::FactSummary {
            id: fact_id.to_owned(),
            content: "mock fact".to_owned(),
            confidence: 1.0,
            tier: "established".to_owned(),
            recorded_at: "2026-01-01T00:00:00Z".to_owned(),
            is_forgotten: true,
            forgotten_at: Some("2026-01-02T00:00:00Z".to_owned()),
            forget_reason: Some(reason.to_owned()),
        };
        Box::pin(std::future::ready(Ok(summary)))
    }

    fn unforget_fact(
        &self,
        fact_id: &str,
    ) -> Pin<
        Box<
            dyn Future<Output = Result<aletheia_organon::types::FactSummary, KnowledgeAdapterError>>
                + Send
                + '_,
        >,
    > {
        let summary = aletheia_organon::types::FactSummary {
            id: fact_id.to_owned(),
            content: "mock fact".to_owned(),
            confidence: 1.0,
            tier: "established".to_owned(),
            recorded_at: "2026-01-01T00:00:00Z".to_owned(),
            is_forgotten: false,
            forgotten_at: None,
            forget_reason: None,
        };
        Box::pin(std::future::ready(Ok(summary)))
    }

    fn datalog_query(
        &self,
        _query: &str,
        _params: Option<serde_json::Value>,
        _timeout_secs: Option<f64>,
        _row_limit: Option<usize>,
    ) -> Pin<
        Box<
            dyn Future<
                    Output = Result<aletheia_organon::types::DatalogResult, KnowledgeAdapterError>,
                > + Send
                + '_,
        >,
    > {
        Box::pin(std::future::ready(Ok(
            aletheia_organon::types::DatalogResult {
                columns: vec!["stub".to_owned()],
                rows: vec![],
                truncated: false,
            },
        )))
    }
}

fn ctx_with_knowledge(svc: Arc<StubKnowledgeService>) -> ToolContext {
    install_crypto_provider();
    ToolContext {
        nous_id: NousId::new("alice").expect("valid"),
        session_id: SessionId::new(),
        workspace: PathBuf::from("/tmp/test"),
        allowed_roots: vec![PathBuf::from("/tmp")],
        services: Some(Arc::new(ToolServices {
            cross_nous: None,
            messenger: None,
            note_store: None,
            blackboard_store: None,
            spawn: None,
            planning: None,
            knowledge: Some(svc),
            http_client: reqwest::Client::new(),
            lazy_tool_catalog: vec![],
            server_tool_config: ServerToolConfig::default(),
        })),
        active_tools: Arc::new(RwLock::new(HashSet::new())),
    }
}

// ---------------------------------------------------------------------------
// Memory search tool executor wiring
// ---------------------------------------------------------------------------

#[tokio::test]
async fn memory_search_tool_returns_results() {
    let svc = Arc::new(StubKnowledgeService::new());
    svc.seed_fact("fact-1", "Alice works on the Aletheia project");

    let reg = registry();
    let ctx = ctx_with_knowledge(Arc::clone(&svc));

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
    let svc = Arc::new(StubKnowledgeService::new());
    let reg = registry();
    let ctx = ctx_with_knowledge(Arc::clone(&svc));

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
    let svc = Arc::new(StubKnowledgeService::new());
    svc.seed_fact("fact-42", "Bob prefers Python");

    let reg = registry();
    let ctx = ctx_with_knowledge(Arc::clone(&svc));

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

    // Verify the stub recorded the correction
    let corrected = svc.corrected.lock().unwrap();
    assert_eq!(corrected.len(), 1);
    assert_eq!(corrected[0].0, "fact-42");
}

#[tokio::test]
async fn memory_audit_tool_returns_facts() {
    let svc = Arc::new(StubKnowledgeService::new());
    svc.seed_fact("fact-a", "Alice is an engineer");
    svc.seed_fact("fact-b", "Bob works at acme.corp");

    let reg = registry();
    let ctx = ctx_with_knowledge(Arc::clone(&svc));

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

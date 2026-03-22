#![expect(clippy::expect_used, reason = "test assertions")]
#![expect(
    clippy::as_conversions,
    reason = "test: coercions to dyn trait objects in test setup"
)]
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, RwLock};

use aletheia_koina::id::{NousId, SessionId, ToolName};

use crate::registry::ToolRegistry;
use crate::types::{
    BlackboardEntry, BlackboardStore, NoteEntry, NoteStore, ServerToolConfig, ToolContext,
    ToolInput, ToolServices,
};

use crate::error::StoreError;

fn install_crypto_provider() {
    let _ = rustls::crypto::ring::default_provider().install_default();
}

struct MockNoteStore {
    notes: Mutex<Vec<NoteEntry>>,
    next_id: Mutex<i64>,
}

impl MockNoteStore {
    fn new() -> Self {
        Self {
            notes: Mutex::new(Vec::new()),
            next_id: Mutex::new(1),
        }
    }
}

impl NoteStore for MockNoteStore {
    fn add_note(
        &self,
        _session_id: &str,
        _nous_id: &str,
        category: &str,
        content: &str,
    ) -> Result<i64, StoreError> {
        let mut id = self
            .next_id
            .lock()
            .expect("next_id mutex should not be poisoned");
        let note_id = *id;
        *id += 1;
        self.notes
            .lock()
            .expect("notes mutex should not be poisoned")
            .push(NoteEntry {
                id: note_id,
                category: category.to_owned(),
                content: content.to_owned(),
                created_at: "2026-01-01T00:00:00Z".to_owned(),
            });
        Ok(note_id)
    }

    fn get_notes(&self, _session_id: &str) -> Result<Vec<NoteEntry>, StoreError> {
        Ok(self
            .notes
            .lock()
            .expect("notes mutex should not be poisoned")
            .clone())
    }

    fn delete_note(&self, note_id: i64) -> Result<bool, StoreError> {
        let mut notes = self
            .notes
            .lock()
            .expect("notes mutex should not be poisoned");
        let len_before = notes.len();
        notes.retain(|n| n.id != note_id);
        Ok(notes.len() < len_before)
    }
}

struct MockBlackboardStore {
    entries: Mutex<Vec<BlackboardEntry>>,
}

impl MockBlackboardStore {
    fn new() -> Self {
        Self {
            entries: Mutex::new(Vec::new()),
        }
    }
}

impl BlackboardStore for MockBlackboardStore {
    fn write(
        &self,
        key: &str,
        value: &str,
        author: &str,
        ttl_seconds: i64,
    ) -> Result<(), StoreError> {
        let mut entries = self
            .entries
            .lock()
            .expect("entries mutex should not be poisoned");
        entries.retain(|e| e.key != key);
        entries.push(BlackboardEntry {
            key: key.to_owned(),
            value: value.to_owned(),
            author_nous_id: author.to_owned(),
            ttl_seconds,
            created_at: "2026-01-01T00:00:00Z".to_owned(),
            expires_at: None,
        });
        Ok(())
    }

    fn read(&self, key: &str) -> Result<Option<BlackboardEntry>, StoreError> {
        Ok(self
            .entries
            .lock()
            .expect("entries mutex should not be poisoned")
            .iter()
            .find(|e| e.key == key)
            .cloned())
    }

    fn list(&self) -> Result<Vec<BlackboardEntry>, StoreError> {
        Ok(self
            .entries
            .lock()
            .expect("entries mutex should not be poisoned")
            .clone())
    }

    fn delete(&self, key: &str, author: &str) -> Result<bool, StoreError> {
        let mut entries = self
            .entries
            .lock()
            .expect("entries mutex should not be poisoned");
        let len_before = entries.len();
        entries.retain(|e| !(e.key == key && e.author_nous_id == author));
        Ok(entries.len() < len_before)
    }
}

fn test_ctx() -> ToolContext {
    ToolContext {
        nous_id: NousId::new("test-agent").expect("valid"),
        session_id: SessionId::new(),
        workspace: PathBuf::from("/tmp/test"),
        allowed_roots: vec![PathBuf::from("/tmp")],
        services: None,
        active_tools: Arc::new(RwLock::new(HashSet::new())),
    }
}

fn ctx_with_services(
    note_store: Arc<dyn NoteStore>,
    bb_store: Arc<dyn BlackboardStore>,
) -> ToolContext {
    install_crypto_provider();
    ToolContext {
        nous_id: NousId::new("test-agent").expect("valid"),
        session_id: SessionId::new(),
        workspace: PathBuf::from("/tmp/test"),
        allowed_roots: vec![PathBuf::from("/tmp")],
        services: Some(Arc::new(ToolServices {
            cross_nous: None,
            messenger: None,
            note_store: Some(note_store),
            blackboard_store: Some(bb_store),
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

#[tokio::test]
async fn register_memory_tools() {
    let mut reg = ToolRegistry::new();
    super::register(&mut reg).expect("register");
    assert_eq!(
        reg.definitions().len(),
        8,
        "expected reg.definitions().len() to equal 8"
    );
}

#[tokio::test]
async fn memory_search_def_requires_query() {
    let mut reg = ToolRegistry::new();
    super::register(&mut reg).expect("register");
    let name = ToolName::new("memory_search").expect("valid");
    let def = reg.get_def(&name).expect("found");
    assert!(
        def.input_schema.required.contains(&"query".to_owned()),
        "expected def.input_schema.required.contains(&\"query\".to_owned()) to be true"
    );
}

#[tokio::test]
async fn memory_search_no_knowledge_returns_error() {
    let mut reg = ToolRegistry::new();
    super::register(&mut reg).expect("register");
    let note_store = Arc::new(MockNoteStore::new());
    let bb_store = Arc::new(MockBlackboardStore::new());
    let ctx = ctx_with_services(note_store, bb_store);

    let input = ToolInput {
        name: ToolName::new("memory_search").expect("valid"),
        tool_use_id: "tu_1".to_owned(),
        arguments: serde_json::json!({"query": "test"}),
    };
    let result = reg.execute(&input, &ctx).await.expect("execute");
    assert!(result.is_error, "expected result.is_error to be true");
    assert!(
        result
            .content
            .text_summary()
            .contains("knowledge store not configured"),
        "expected knowledge store error: {}",
        result.content.text_summary()
    );
}

#[tokio::test]
async fn memory_search_no_services_returns_error() {
    let mut reg = ToolRegistry::new();
    super::register(&mut reg).expect("register");
    let input = ToolInput {
        name: ToolName::new("memory_search").expect("valid"),
        tool_use_id: "tu_1".to_owned(),
        arguments: serde_json::json!({"query": "test"}),
    };
    let result = reg.execute(&input, &test_ctx()).await.expect("execute");
    assert!(result.is_error, "expected result.is_error to be true");
    assert!(
        result.content.text_summary().contains("not configured"),
        "expected result.content.text_summary().contains(\"not configured\") to be true"
    );
}

#[tokio::test]
async fn note_add_and_list() {
    let mut reg = ToolRegistry::new();
    super::register(&mut reg).expect("register");
    let note_store = Arc::new(MockNoteStore::new());
    let bb_store = Arc::new(MockBlackboardStore::new());
    let ctx = ctx_with_services(Arc::clone(&note_store) as Arc<dyn NoteStore>, bb_store);

    let add1 = ToolInput {
        name: ToolName::new("note").expect("valid"),
        tool_use_id: "tu_1".to_owned(),
        arguments: serde_json::json!({"action": "add", "content": "first note", "category": "task"}),
    };
    let r1 = reg.execute(&add1, &ctx).await.expect("execute");
    assert!(!r1.is_error, "expected r1.is_error to be false");
    assert!(
        r1.content.text_summary().contains("#1"),
        "expected r1.content.text_summary().contains(\"#1\") to be true"
    );

    let add2 = ToolInput {
        name: ToolName::new("note").expect("valid"),
        tool_use_id: "tu_2".to_owned(),
        arguments: serde_json::json!({"action": "add", "content": "second note"}),
    };
    let r2 = reg.execute(&add2, &ctx).await.expect("execute");
    assert!(!r2.is_error, "expected r2.is_error to be false");
    assert!(
        r2.content.text_summary().contains("#2"),
        "expected r2.content.text_summary().contains(\"#2\") to be true"
    );

    let list = ToolInput {
        name: ToolName::new("note").expect("valid"),
        tool_use_id: "tu_3".to_owned(),
        arguments: serde_json::json!({"action": "list"}),
    };
    let r3 = reg.execute(&list, &ctx).await.expect("execute");
    assert!(!r3.is_error, "expected r3.is_error to be false");
    let text = r3.content.text_summary();
    assert!(
        text.contains("first note"),
        "expected text.contains(\"first note\") to be true"
    );
    assert!(
        text.contains("second note"),
        "expected text.contains(\"second note\") to be true"
    );
}

#[tokio::test]
async fn note_delete() {
    let mut reg = ToolRegistry::new();
    super::register(&mut reg).expect("register");
    let note_store = Arc::new(MockNoteStore::new());
    let bb_store = Arc::new(MockBlackboardStore::new());
    let ctx = ctx_with_services(Arc::clone(&note_store) as Arc<dyn NoteStore>, bb_store);

    let add = ToolInput {
        name: ToolName::new("note").expect("valid"),
        tool_use_id: "tu_1".to_owned(),
        arguments: serde_json::json!({"action": "add", "content": "to delete"}),
    };
    reg.execute(&add, &ctx).await.expect("execute");

    let del = ToolInput {
        name: ToolName::new("note").expect("valid"),
        tool_use_id: "tu_2".to_owned(),
        arguments: serde_json::json!({"action": "delete", "id": 1}),
    };
    let r = reg.execute(&del, &ctx).await.expect("execute");
    assert!(!r.is_error, "expected r.is_error to be false");
    assert!(
        r.content.text_summary().contains("deleted"),
        "expected r.content.text_summary().contains(\"deleted\") to be true"
    );

    let list = ToolInput {
        name: ToolName::new("note").expect("valid"),
        tool_use_id: "tu_3".to_owned(),
        arguments: serde_json::json!({"action": "list"}),
    };
    let r3 = reg.execute(&list, &ctx).await.expect("execute");
    assert!(
        r3.content.text_summary().contains("No session notes"),
        "expected r3.content.text_summary().contains(\"No session notes\") to be true"
    );
}

#[tokio::test]
async fn note_rejects_over_500_chars() {
    let mut reg = ToolRegistry::new();
    super::register(&mut reg).expect("register");
    let note_store = Arc::new(MockNoteStore::new());
    let bb_store = Arc::new(MockBlackboardStore::new());
    let ctx = ctx_with_services(note_store, bb_store);

    let long_content = "x".repeat(501);
    let input = ToolInput {
        name: ToolName::new("note").expect("valid"),
        tool_use_id: "tu_1".to_owned(),
        arguments: serde_json::json!({"action": "add", "content": long_content}),
    };
    let result = reg.execute(&input, &ctx).await.expect("execute");
    assert!(result.is_error, "expected result.is_error to be true");
    assert!(
        result.content.text_summary().contains("500"),
        "expected result.content.text_summary().contains(\"500\") to be true"
    );
}

#[tokio::test]
async fn blackboard_write_and_read() {
    let mut reg = ToolRegistry::new();
    super::register(&mut reg).expect("register");
    let note_store = Arc::new(MockNoteStore::new());
    let bb_store = Arc::new(MockBlackboardStore::new());
    let ctx = ctx_with_services(
        note_store,
        Arc::clone(&bb_store) as Arc<dyn BlackboardStore>,
    );

    let write = ToolInput {
        name: ToolName::new("blackboard").expect("valid"),
        tool_use_id: "tu_1".to_owned(),
        arguments: serde_json::json!({"action": "write", "key": "goal", "value": "ship M0b"}),
    };
    let r1 = reg.execute(&write, &ctx).await.expect("execute");
    assert!(!r1.is_error, "expected r1.is_error to be false");
    assert!(
        r1.content.text_summary().contains("[goal] written"),
        "expected r1.content.text_summary().contains(\"[goal] written\") to be true"
    );

    let read = ToolInput {
        name: ToolName::new("blackboard").expect("valid"),
        tool_use_id: "tu_2".to_owned(),
        arguments: serde_json::json!({"action": "read", "key": "goal"}),
    };
    let r2 = reg.execute(&read, &ctx).await.expect("execute");
    assert!(!r2.is_error, "expected r2.is_error to be false");
    assert!(
        r2.content.text_summary().contains("ship M0b"),
        "expected r2.content.text_summary().contains(\"ship M0b\") to be true"
    );
}

#[tokio::test]
async fn blackboard_list() {
    let mut reg = ToolRegistry::new();
    super::register(&mut reg).expect("register");
    let note_store = Arc::new(MockNoteStore::new());
    let bb_store = Arc::new(MockBlackboardStore::new());
    let ctx = ctx_with_services(
        note_store,
        Arc::clone(&bb_store) as Arc<dyn BlackboardStore>,
    );

    for (k, v) in [("a", "1"), ("b", "2")] {
        let write = ToolInput {
            name: ToolName::new("blackboard").expect("valid"),
            tool_use_id: "tu_w".to_owned(),
            arguments: serde_json::json!({"action": "write", "key": k, "value": v}),
        };
        reg.execute(&write, &ctx).await.expect("execute");
    }

    let list = ToolInput {
        name: ToolName::new("blackboard").expect("valid"),
        tool_use_id: "tu_l".to_owned(),
        arguments: serde_json::json!({"action": "list"}),
    };
    let r = reg.execute(&list, &ctx).await.expect("execute");
    assert!(!r.is_error, "expected r.is_error to be false");
    let text = r.content.text_summary();
    assert!(
        text.contains("[a] = 1"),
        "expected text.contains(\"[a] = 1\") to be true"
    );
    assert!(
        text.contains("[b] = 2"),
        "expected text.contains(\"[b] = 2\") to be true"
    );
}

#[tokio::test]
async fn blackboard_delete_only_author() {
    let mut reg = ToolRegistry::new();
    super::register(&mut reg).expect("register");
    let note_store = Arc::new(MockNoteStore::new());
    let bb_store = Arc::new(MockBlackboardStore::new());
    let ctx = ctx_with_services(
        note_store,
        Arc::clone(&bb_store) as Arc<dyn BlackboardStore>,
    );

    let write = ToolInput {
        name: ToolName::new("blackboard").expect("valid"),
        tool_use_id: "tu_1".to_owned(),
        arguments: serde_json::json!({"action": "write", "key": "secret", "value": "data"}),
    };
    reg.execute(&write, &ctx).await.expect("execute");

    let other_ctx = ToolContext {
        nous_id: NousId::new("other-agent").expect("valid"),
        session_id: SessionId::new(),
        workspace: PathBuf::from("/tmp/test"),
        allowed_roots: vec![PathBuf::from("/tmp")],
        services: ctx.services.clone(),
        active_tools: Arc::new(RwLock::new(HashSet::new())),
    };
    let del = ToolInput {
        name: ToolName::new("blackboard").expect("valid"),
        tool_use_id: "tu_2".to_owned(),
        arguments: serde_json::json!({"action": "delete", "key": "secret"}),
    };
    let r = reg.execute(&del, &other_ctx).await.expect("execute");
    assert!(
        r.content.text_summary().contains("not your entry"),
        "expected r.content.text_summary().contains(\"not your entry\") to be true"
    );

    let del2 = ToolInput {
        name: ToolName::new("blackboard").expect("valid"),
        tool_use_id: "tu_3".to_owned(),
        arguments: serde_json::json!({"action": "delete", "key": "secret"}),
    };
    let r2 = reg.execute(&del2, &ctx).await.expect("execute");
    assert!(
        r2.content.text_summary().contains("deleted"),
        "expected r2.content.text_summary().contains(\"deleted\") to be true"
    );
}

#[tokio::test]
async fn memory_correct_no_knowledge_returns_error() {
    let mut reg = ToolRegistry::new();
    super::register(&mut reg).expect("register");
    let note_store = Arc::new(MockNoteStore::new());
    let bb_store = Arc::new(MockBlackboardStore::new());
    let ctx = ctx_with_services(note_store, bb_store);

    let input = ToolInput {
        name: ToolName::new("memory_correct").expect("valid"),
        tool_use_id: "tu_1".to_owned(),
        arguments: serde_json::json!({"fact_id": "f-1", "new_content": "corrected"}),
    };
    let result = reg.execute(&input, &ctx).await.expect("execute");
    assert!(result.is_error, "expected result.is_error to be true");
    assert!(
        result
            .content
            .text_summary()
            .contains("knowledge store not configured")
    );
}

#[tokio::test]
async fn memory_retract_no_knowledge_returns_error() {
    let mut reg = ToolRegistry::new();
    super::register(&mut reg).expect("register");
    let note_store = Arc::new(MockNoteStore::new());
    let bb_store = Arc::new(MockBlackboardStore::new());
    let ctx = ctx_with_services(note_store, bb_store);

    let input = ToolInput {
        name: ToolName::new("memory_retract").expect("valid"),
        tool_use_id: "tu_1".to_owned(),
        arguments: serde_json::json!({"fact_id": "f-1"}),
    };
    let result = reg.execute(&input, &ctx).await.expect("execute");
    assert!(result.is_error, "expected result.is_error to be true");
    assert!(
        result
            .content
            .text_summary()
            .contains("knowledge store not configured")
    );
}

#[tokio::test]
async fn memory_audit_no_knowledge_returns_error() {
    let mut reg = ToolRegistry::new();
    super::register(&mut reg).expect("register");
    let note_store = Arc::new(MockNoteStore::new());
    let bb_store = Arc::new(MockBlackboardStore::new());
    let ctx = ctx_with_services(note_store, bb_store);

    let input = ToolInput {
        name: ToolName::new("memory_audit").expect("valid"),
        tool_use_id: "tu_1".to_owned(),
        arguments: serde_json::json!({}),
    };
    let result = reg.execute(&input, &ctx).await.expect("execute");
    assert!(result.is_error, "expected result.is_error to be true");
    assert!(
        result
            .content
            .text_summary()
            .contains("knowledge store not configured")
    );
}

#[tokio::test]
async fn memory_correct_not_auto_activated() {
    let mut reg = ToolRegistry::new();
    super::register(&mut reg).expect("register");
    let name = ToolName::new("memory_correct").expect("valid");
    let def = reg.get_def(&name).expect("found");
    assert!(!def.auto_activate, "expected def.auto_activate to be false");
}

#[tokio::test]
async fn memory_retract_not_auto_activated() {
    let mut reg = ToolRegistry::new();
    super::register(&mut reg).expect("register");
    let name = ToolName::new("memory_retract").expect("valid");
    let def = reg.get_def(&name).expect("found");
    assert!(!def.auto_activate, "expected def.auto_activate to be false");
}

#[tokio::test]
async fn memory_audit_not_auto_activated() {
    let mut reg = ToolRegistry::new();
    super::register(&mut reg).expect("register");
    let name = ToolName::new("memory_audit").expect("valid");
    let def = reg.get_def(&name).expect("found");
    assert!(!def.auto_activate, "expected def.auto_activate to be false");
}

#[tokio::test]
async fn memory_forget_no_knowledge_returns_error() {
    let mut reg = ToolRegistry::new();
    super::register(&mut reg).expect("register");
    let note_store = Arc::new(MockNoteStore::new());
    let bb_store = Arc::new(MockBlackboardStore::new());
    let ctx = ctx_with_services(note_store, bb_store);

    let input = ToolInput {
        name: ToolName::new("memory_forget").expect("valid"),
        tool_use_id: "tu_1".to_owned(),
        arguments: serde_json::json!({"fact_id": "f-1", "reason": "privacy"}),
    };
    let result = reg.execute(&input, &ctx).await.expect("execute");
    assert!(result.is_error, "expected result.is_error to be true");
    assert!(
        result
            .content
            .text_summary()
            .contains("knowledge store not configured")
    );
}

#[tokio::test]
async fn memory_forget_not_auto_activated() {
    let mut reg = ToolRegistry::new();
    super::register(&mut reg).expect("register");
    let name = ToolName::new("memory_forget").expect("valid");
    let def = reg.get_def(&name).expect("found");
    assert!(!def.auto_activate, "expected def.auto_activate to be false");
}

#[tokio::test]
async fn datalog_query_rejects_mutation_keywords() {
    let mut reg = ToolRegistry::new();
    super::register(&mut reg).expect("register");
    let note_store = Arc::new(MockNoteStore::new());
    let bb_store = Arc::new(MockBlackboardStore::new());
    let ctx = ctx_with_services(note_store, bb_store);

    let mutations = vec![
        (":put facts {}", ":put"),
        (":rm facts {}", ":rm"),
        (":replace facts {}", ":replace"),
        (":create facts {}", ":create"),
        (":ensure facts {}", ":ensure"),
    ];

    for (query, keyword) in mutations {
        let input = ToolInput {
            name: ToolName::new("datalog_query").expect("valid"),
            tool_use_id: "tu_1".to_owned(),
            arguments: serde_json::json!({"query": query}),
        };
        let result = reg.execute(&input, &ctx).await.expect("execute");
        assert!(
            result.is_error,
            "query containing '{keyword}' should be rejected"
        );
        assert!(
            result.content.text_summary().contains("mutation keyword"),
            "error should mention mutation keyword for '{keyword}': {}",
            result.content.text_summary()
        );
    }
}

#[tokio::test]
async fn datalog_query_no_knowledge_returns_error() {
    let mut reg = ToolRegistry::new();
    super::register(&mut reg).expect("register");
    let note_store = Arc::new(MockNoteStore::new());
    let bb_store = Arc::new(MockBlackboardStore::new());
    let ctx = ctx_with_services(note_store, bb_store);

    let input = ToolInput {
        name: ToolName::new("datalog_query").expect("valid"),
        tool_use_id: "tu_1".to_owned(),
        arguments: serde_json::json!({"query": "?[x] := x = 42"}),
    };
    let result = reg.execute(&input, &ctx).await.expect("execute");
    assert!(result.is_error, "expected result.is_error to be true");
    assert!(
        result
            .content
            .text_summary()
            .contains("knowledge store not configured")
    );
}

#[tokio::test]
async fn datalog_query_not_auto_activated() {
    let mut reg = ToolRegistry::new();
    super::register(&mut reg).expect("register");
    let name = ToolName::new("datalog_query").expect("valid");
    let def = reg.get_def(&name).expect("found");
    assert!(!def.auto_activate, "expected def.auto_activate to be false");
}

#[tokio::test]
async fn datalog_query_rejects_case_insensitive_mutations() {
    let mut reg = ToolRegistry::new();
    super::register(&mut reg).expect("register");
    let note_store = Arc::new(MockNoteStore::new());
    let bb_store = Arc::new(MockBlackboardStore::new());
    let ctx = ctx_with_services(note_store, bb_store);

    let input = ToolInput {
        name: ToolName::new("datalog_query").expect("valid"),
        tool_use_id: "tu_1".to_owned(),
        arguments: serde_json::json!({"query": ":PUT facts {}"}),
    };
    let result = reg.execute(&input, &ctx).await.expect("execute");
    assert!(result.is_error, "uppercase mutation should be rejected");
}

#[test]
fn markdown_table_empty_result() {
    let result = crate::types::DatalogResult {
        columns: vec![],
        rows: vec![],
        truncated: false,
    };
    let table = super::datalog::format_as_markdown_table(&result);
    assert_eq!(
        table, "No results.",
        "expected table to equal \"No results.\""
    );
}

#[test]
fn markdown_table_formats_correctly() {
    let result = crate::types::DatalogResult {
        columns: vec!["id".to_owned(), "name".to_owned()],
        rows: vec![
            vec![
                serde_json::Value::String("1".to_owned()),
                serde_json::Value::String("alice".to_owned()),
            ],
            vec![
                serde_json::Value::Number(serde_json::Number::from(2)),
                serde_json::Value::Null,
            ],
        ],
        truncated: false,
    };
    let table = super::datalog::format_as_markdown_table(&result);
    assert!(
        table.contains("| id | name |"),
        "expected table.contains(\"| id | name |\") to be true"
    );
    assert!(
        table.contains("| --- | --- |"),
        "expected table.contains(\"| --- | --- |\") to be true"
    );
    assert!(
        table.contains("| 1 | alice |"),
        "expected table.contains(\"| 1 | alice |\") to be true"
    );
    assert!(
        table.contains("| 2 | null |"),
        "expected table.contains(\"| 2 | null |\") to be true"
    );
}

#[tokio::test]
async fn datalog_query_missing_query_param() {
    let mut reg = ToolRegistry::new();
    super::register(&mut reg).expect("register");
    let note_store = Arc::new(MockNoteStore::new());
    let bb_store = Arc::new(MockBlackboardStore::new());
    let ctx = ctx_with_services(note_store, bb_store);

    let input = ToolInput {
        name: ToolName::new("datalog_query").expect("valid"),
        tool_use_id: "tu_1".to_owned(),
        arguments: serde_json::json!({}),
    };
    let result = reg.execute(&input, &ctx).await;
    assert!(result.is_err(), "missing required param should error");
}

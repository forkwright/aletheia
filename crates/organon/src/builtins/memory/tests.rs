#![expect(clippy::expect_used, reason = "test assertions")]
#![expect(
    clippy::as_conversions,
    reason = "test: coercions to dyn trait objects in test setup"
)]
use std::sync::{Arc, Mutex};

use graphe::store::SessionStore;
use graphe::types::BlackboardVisibility;
use koina::id::{NousId, ToolName};

use crate::registry::ToolRegistry;
use crate::types::{
    BlackboardEntry, BlackboardStore, BlackboardViewer, NoteEntry, NoteStore, ToolContext,
    ToolInput, ToolServices,
};

use crate::error::StoreError;

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

    /// Test-only: seed an entry with an explicit visibility/session scope,
    /// bypassing `BlackboardStore::write` (which always writes `Shared`) —
    /// mirrors how the real `ws:` working-state path writes scoped rows
    /// directly against the concrete backend store rather than through this
    /// general-purpose trait (aletheia#5032).
    fn insert_scoped(&self, entry: BlackboardEntry) {
        let mut entries = self
            .entries
            .lock()
            .expect("entries mutex should not be poisoned");
        entries.retain(|e| e.key != entry.key);
        entries.push(entry);
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
        self.insert_scoped(BlackboardEntry {
            key: key.to_owned(),
            value: value.to_owned(),
            author_nous_id: author.to_owned(),
            ttl_seconds,
            created_at: "2026-01-01T00:00:00Z".to_owned(),
            expires_at: None,
            session_id: None,
            visibility: BlackboardVisibility::Shared,
        });
        Ok(())
    }

    fn read(
        &self,
        key: &str,
        viewer: &BlackboardViewer,
    ) -> Result<Option<BlackboardEntry>, StoreError> {
        Ok(self
            .entries
            .lock()
            .expect("entries mutex should not be poisoned")
            .iter()
            .find(|e| e.key == key)
            .filter(|e| viewer.can_see(e))
            .cloned())
    }

    fn list(&self, viewer: &BlackboardViewer) -> Result<Vec<BlackboardEntry>, StoreError> {
        Ok(self
            .entries
            .lock()
            .expect("entries mutex should not be poisoned")
            .iter()
            .filter(|e| viewer.can_see(e))
            .cloned()
            .collect())
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
    crate::testing::make_test_context_without_services()
}

fn ctx_with_services(
    note_store: Arc<dyn NoteStore>,
    bb_store: Arc<dyn BlackboardStore>,
) -> ToolContext {
    crate::testing::make_test_context_with(ToolServices {
        note_store: Some(note_store),
        blackboard_store: Some(bb_store),
        ..Default::default()
    })
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
async fn note_category_schema_derived_from_session_store() {
    let mut reg = ToolRegistry::new();
    super::register(&mut reg).expect("register");
    let name = ToolName::new("note").expect("valid");
    let def = reg.get_def(&name).expect("found");
    let category = def
        .input_schema
        .properties
        .get("category")
        .expect("category property");

    let expected: Vec<String> = SessionStore::VALID_CATEGORIES
        .iter()
        .map(|&category| category.to_owned())
        .collect();
    assert_eq!(
        category.enum_values,
        Some(expected),
        "expected category enum_values to match SessionStore::VALID_CATEGORIES"
    );

    assert!(
        !category
            .description
            .contains("task, decision, preference, correction, context"),
        "category description should not enumerate valid categories inline: {}",
        category.description
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
        services: ctx.services.clone(),
        ..crate::testing::make_test_context_without_services()
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

/// Pins aletheia#5032 closed: the general-purpose `blackboard` tool's
/// `list`/`read` actions must never surface `SessionPrivate` rows scoped to
/// another session — the exact shape of an internal `ws:` working-state key
/// — even when the row's author matches the viewer's own agent.
#[tokio::test]
async fn blackboard_list_and_read_exclude_other_session_private_rows() {
    let mut reg = ToolRegistry::new();
    super::register(&mut reg).expect("register");
    let note_store = Arc::new(MockNoteStore::new());
    let bb_store = Arc::new(MockBlackboardStore::new());
    let ctx = ctx_with_services(
        note_store,
        Arc::clone(&bb_store) as Arc<dyn BlackboardStore>,
    );

    // A row belonging to the SAME agent but a DIFFERENT session — the
    // shape of a `ws:` working-state key written by the export/import
    // path for a session other than the one currently running.
    bb_store.insert_scoped(BlackboardEntry {
        key: "ws:test-agent:other-session".to_owned(),
        value: "leaked-task-stack".to_owned(),
        author_nous_id: ctx.nous_id.as_str().to_owned(),
        ttl_seconds: 86_400,
        created_at: "2026-01-01T00:00:00Z".to_owned(),
        expires_at: None,
        session_id: Some("other-session".to_owned()),
        visibility: BlackboardVisibility::SessionPrivate,
    });
    // A NousPrivate row belonging to a different agent entirely.
    bb_store.insert_scoped(BlackboardEntry {
        key: "someone-elses-secret".to_owned(),
        value: "leaked-private-note".to_owned(),
        author_nous_id: "other-agent".to_owned(),
        ttl_seconds: 3600,
        created_at: "2026-01-01T00:00:00Z".to_owned(),
        expires_at: None,
        session_id: None,
        visibility: BlackboardVisibility::NousPrivate,
    });
    // An ordinary Shared row, written through the tool as any user would.
    let write = ToolInput {
        name: ToolName::new("blackboard").expect("valid"),
        tool_use_id: "tu_w".to_owned(),
        arguments: serde_json::json!({"action": "write", "key": "goal", "value": "ship it"}),
    };
    reg.execute(&write, &ctx).await.expect("execute");

    let list = ToolInput {
        name: ToolName::new("blackboard").expect("valid"),
        tool_use_id: "tu_l".to_owned(),
        arguments: serde_json::json!({"action": "list"}),
    };
    let list_result = reg.execute(&list, &ctx).await.expect("execute");
    let list_text = list_result.content.text_summary();
    assert!(
        list_text.contains("[goal]"),
        "the general list path must still show ordinary Shared entries: {list_text}"
    );
    assert!(
        !list_text.contains("leaked-task-stack"),
        "the general list path must not surface a SessionPrivate ws: row from another session: {list_text}"
    );
    assert!(
        !list_text.contains("leaked-private-note"),
        "the general list path must not surface another agent's NousPrivate row: {list_text}"
    );

    let read_ws = ToolInput {
        name: ToolName::new("blackboard").expect("valid"),
        tool_use_id: "tu_r1".to_owned(),
        arguments: serde_json::json!({"action": "read", "key": "ws:test-agent:other-session"}),
    };
    let read_ws_result = reg.execute(&read_ws, &ctx).await.expect("execute");
    assert!(
        !read_ws_result
            .content
            .text_summary()
            .contains("leaked-task-stack"),
        "reading a ws: key directly by name must not bypass session scoping: {}",
        read_ws_result.content.text_summary()
    );

    let read_secret = ToolInput {
        name: ToolName::new("blackboard").expect("valid"),
        tool_use_id: "tu_r2".to_owned(),
        arguments: serde_json::json!({"action": "read", "key": "someone-elses-secret"}),
    };
    let read_secret_result = reg.execute(&read_secret, &ctx).await.expect("execute");
    assert!(
        !read_secret_result
            .content
            .text_summary()
            .contains("leaked-private-note"),
        "reading another agent's NousPrivate key directly by name must not bypass ownership scoping: {}",
        read_secret_result.content.text_summary()
    );
}

/// The positive case for the test above: a `SessionPrivate` row scoped to
/// the viewer's OWN session is visible through the same general list/read
/// path — enforcement must not be a blanket deny.
#[tokio::test]
async fn blackboard_list_and_read_include_own_session_private_row() {
    let mut reg = ToolRegistry::new();
    super::register(&mut reg).expect("register");
    let note_store = Arc::new(MockNoteStore::new());
    let bb_store = Arc::new(MockBlackboardStore::new());
    let ctx = ctx_with_services(
        note_store,
        Arc::clone(&bb_store) as Arc<dyn BlackboardStore>,
    );

    let own_session_id = ctx.session_id.to_string();
    bb_store.insert_scoped(BlackboardEntry {
        key: "ws:test-agent:own-session".to_owned(),
        value: "own-task-stack".to_owned(),
        author_nous_id: ctx.nous_id.as_str().to_owned(),
        ttl_seconds: 86_400,
        created_at: "2026-01-01T00:00:00Z".to_owned(),
        expires_at: None,
        session_id: Some(own_session_id),
        visibility: BlackboardVisibility::SessionPrivate,
    });

    let read = ToolInput {
        name: ToolName::new("blackboard").expect("valid"),
        tool_use_id: "tu_1".to_owned(),
        arguments: serde_json::json!({"action": "read", "key": "ws:test-agent:own-session"}),
    };
    let result = reg.execute(&read, &ctx).await.expect("execute");
    assert!(
        result.content.text_summary().contains("own-task-stack"),
        "a viewer must see its own SessionPrivate row from inside the matching session: {}",
        result.content.text_summary()
    );

    let list = ToolInput {
        name: ToolName::new("blackboard").expect("valid"),
        tool_use_id: "tu_2".to_owned(),
        arguments: serde_json::json!({"action": "list"}),
    };
    let list_result = reg.execute(&list, &ctx).await.expect("execute");
    assert!(
        list_result
            .content
            .text_summary()
            .contains("own-task-stack"),
        "list must include the viewer's own SessionPrivate row from inside the matching session: {}",
        list_result.content.text_summary()
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

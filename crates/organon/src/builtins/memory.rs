//! Memory tool executors: `mem0_search`, `note`, `blackboard`.

use std::future::Future;
use std::pin::Pin;

use aletheia_koina::id::ToolName;
use indexmap::IndexMap;

use crate::error::Result;
use crate::registry::{ToolExecutor, ToolRegistry};
use crate::types::{
    InputSchema, PropertyDef, PropertyType, ToolCategory, ToolContext, ToolDef, ToolInput,
    ToolResult,
};

use super::workspace::{extract_opt_u64, extract_str};

fn require_services(ctx: &ToolContext) -> std::result::Result<&crate::types::ToolServices, ToolResult> {
    ctx.services
        .as_deref()
        .ok_or_else(|| ToolResult::error("memory services not configured"))
}

// --- Mem0 Search ---

struct Mem0SearchExecutor;

impl ToolExecutor for Mem0SearchExecutor {
    fn execute<'a>(
        &'a self,
        input: &'a ToolInput,
        ctx: &'a ToolContext,
    ) -> Pin<Box<dyn Future<Output = Result<ToolResult>> + Send + 'a>> {
        Box::pin(async {
            let services = match require_services(ctx) {
                Ok(s) => s,
                Err(e) => return Ok(e),
            };

            let query = extract_str(&input.arguments, "query", &input.name)?;
            let limit = extract_opt_u64(&input.arguments, "limit").unwrap_or(10);

            let base_url =
                std::env::var("MEM0_URL").unwrap_or_else(|_| "http://localhost:8230".to_owned());

            let response = services
                .http_client
                .post(format!("{base_url}/v1/memories/search/"))
                .json(&serde_json::json!({
                    "query": query,
                    "agent_id": ctx.nous_id.as_str(),
                    "limit": limit
                }))
                .send()
                .await;

            match response {
                Ok(resp) if resp.status().is_success() => {
                    let body: serde_json::Value =
                        resp.json().await.unwrap_or(serde_json::json!({"results": []}));
                    let results = body
                        .get("results")
                        .and_then(|r| r.as_array())
                        .cloned()
                        .unwrap_or_default();
                    if results.is_empty() {
                        Ok(ToolResult::text("No memories found."))
                    } else {
                        Ok(ToolResult::text(format_mem0_results(&results)))
                    }
                }
                Ok(resp) => Ok(ToolResult::error(format!(
                    "Mem0 search failed: HTTP {}",
                    resp.status()
                ))),
                Err(e) => Ok(ToolResult::error(format!(
                    "Mem0 sidecar unreachable: {e}"
                ))),
            }
        })
    }
}

fn format_mem0_results(results: &[serde_json::Value]) -> String {
    results
        .iter()
        .map(|r| {
            let memory = r
                .get("memory")
                .and_then(|m| m.as_str())
                .unwrap_or("(no content)");
            let score = r
                .get("score")
                .and_then(serde_json::Value::as_f64)
                .unwrap_or(0.0);
            format!("- {memory} (score: {score:.2})")
        })
        .collect::<Vec<_>>()
        .join("\n")
}

// --- Note ---

struct NoteExecutor;

impl ToolExecutor for NoteExecutor {
    fn execute<'a>(
        &'a self,
        input: &'a ToolInput,
        ctx: &'a ToolContext,
    ) -> Pin<Box<dyn Future<Output = Result<ToolResult>> + Send + 'a>> {
        Box::pin(async {
            let services = match require_services(ctx) {
                Ok(s) => s,
                Err(e) => return Ok(e),
            };
            let Some(note_store) = services.note_store.as_ref() else {
                return Ok(ToolResult::error("note store not configured"));
            };

            let action = extract_str(&input.arguments, "action", &input.name)?;

            match action {
                "add" => {
                    let content = extract_str(&input.arguments, "content", &input.name)?;
                    let category = input
                        .arguments
                        .get("category")
                        .and_then(|v| v.as_str())
                        .unwrap_or("context");

                    if content.len() > 500 {
                        return Ok(ToolResult::error(
                            "Note content exceeds 500 character limit",
                        ));
                    }

                    match note_store.add_note(
                        &ctx.session_id.to_string(),
                        ctx.nous_id.as_str(),
                        category,
                        content,
                    ) {
                        Ok(id) => Ok(ToolResult::text(format!(
                            "Note #{id} saved ({category}): \"{content}\""
                        ))),
                        Err(e) => Ok(ToolResult::error(format!("Failed to save note: {e}"))),
                    }
                }
                "list" => {
                    match note_store.get_notes(&ctx.session_id.to_string()) {
                        Ok(notes) if notes.is_empty() => {
                            Ok(ToolResult::text("No session notes."))
                        }
                        Ok(notes) => {
                            let lines: Vec<String> = notes
                                .iter()
                                .map(|n| format!("#{} [{}] {}", n.id, n.category, n.content))
                                .collect();
                            Ok(ToolResult::text(lines.join("\n")))
                        }
                        Err(e) => Ok(ToolResult::error(format!("Failed to list notes: {e}"))),
                    }
                }
                "delete" => {
                    let id = input
                        .arguments
                        .get("id")
                        .and_then(serde_json::Value::as_i64)
                        .ok_or_else(|| {
                            crate::error::InvalidInputSnafu {
                                name: input.name.clone(),
                                reason: "missing or invalid field: id".to_owned(),
                            }
                            .build()
                        })?;
                    match note_store.delete_note(id) {
                        Ok(_) => Ok(ToolResult::text(format!("Note #{id} deleted."))),
                        Err(e) => Ok(ToolResult::error(format!("Failed to delete note: {e}"))),
                    }
                }
                _ => Ok(ToolResult::error(format!("Unknown action: {action}"))),
            }
        })
    }
}

// --- Blackboard ---

struct BlackboardExecutor;

impl ToolExecutor for BlackboardExecutor {
    fn execute<'a>(
        &'a self,
        input: &'a ToolInput,
        ctx: &'a ToolContext,
    ) -> Pin<Box<dyn Future<Output = Result<ToolResult>> + Send + 'a>> {
        Box::pin(async {
            let services = match require_services(ctx) {
                Ok(s) => s,
                Err(e) => return Ok(e),
            };
            let Some(bb_store) = services.blackboard_store.as_ref() else {
                return Ok(ToolResult::error("blackboard store not configured"));
            };

            let action = extract_str(&input.arguments, "action", &input.name)?;

            match action {
                "write" => {
                    let key = extract_str(&input.arguments, "key", &input.name)?;
                    let value = extract_str(&input.arguments, "value", &input.name)?;
                    let ttl = extract_opt_u64(&input.arguments, "ttl_seconds").unwrap_or(3600);

                    #[expect(
                        clippy::cast_possible_wrap,
                        reason = "TTL from u64 will not exceed i64::MAX in practice"
                    )]
                    match bb_store.write(key, value, ctx.nous_id.as_str(), ttl as i64) {
                        Ok(()) => Ok(ToolResult::text(format!(
                            "Blackboard [{key}] written (TTL: {ttl}s)"
                        ))),
                        Err(e) => Ok(ToolResult::error(format!(
                            "Failed to write blackboard: {e}"
                        ))),
                    }
                }
                "read" => {
                    let key = extract_str(&input.arguments, "key", &input.name)?;
                    match bb_store.read(key) {
                        Ok(Some(entry)) => Ok(ToolResult::text(format!(
                            "[{key}] = {} (by {}, expires: {})",
                            entry.value,
                            entry.author_nous_id,
                            entry.expires_at.as_deref().unwrap_or("never")
                        ))),
                        Ok(None) => Ok(ToolResult::text(format!(
                            "No entry for key: {key}"
                        ))),
                        Err(e) => Ok(ToolResult::error(format!(
                            "Failed to read blackboard: {e}"
                        ))),
                    }
                }
                "list" => match bb_store.list() {
                    Ok(entries) if entries.is_empty() => {
                        Ok(ToolResult::text("Blackboard is empty."))
                    }
                    Ok(entries) => {
                        let lines: Vec<String> = entries
                            .iter()
                            .map(|e| {
                                format!(
                                    "[{}] = {} (by {})",
                                    e.key, e.value, e.author_nous_id
                                )
                            })
                            .collect();
                        Ok(ToolResult::text(lines.join("\n")))
                    }
                    Err(e) => Ok(ToolResult::error(format!(
                        "Failed to list blackboard: {e}"
                    ))),
                },
                "delete" => {
                    let key = extract_str(&input.arguments, "key", &input.name)?;
                    match bb_store.delete(key, ctx.nous_id.as_str()) {
                        Ok(true) => Ok(ToolResult::text(format!(
                            "Blackboard [{key}] deleted."
                        ))),
                        Ok(false) => Ok(ToolResult::text(format!(
                            "No entry for key: {key} (or not your entry)"
                        ))),
                        Err(e) => Ok(ToolResult::error(format!(
                            "Failed to delete blackboard entry: {e}"
                        ))),
                    }
                }
                _ => Ok(ToolResult::error(format!("Unknown action: {action}"))),
            }
        })
    }
}

// --- Registration ---

/// Register memory tool executors.
pub fn register(registry: &mut ToolRegistry) -> Result<()> {
    registry.register(mem0_search_def(), Box::new(Mem0SearchExecutor))?;
    registry.register(note_def(), Box::new(NoteExecutor))?;
    registry.register(blackboard_def(), Box::new(BlackboardExecutor))?;
    Ok(())
}

// --- Tool Definitions (unchanged schemas) ---

fn mem0_search_def() -> ToolDef {
    ToolDef {
        name: ToolName::new("mem0_search").expect("valid tool name"),
        description: "Search long-term memory for facts, preferences, and relationships".to_owned(),
        extended_description: None,
        input_schema: InputSchema {
            properties: IndexMap::from([
                (
                    "query".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::String,
                        description: "Semantic search query".to_owned(),
                        enum_values: None,
                        default: None,
                    },
                ),
                (
                    "limit".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::Number,
                        description: "Max results (default 10)".to_owned(),
                        enum_values: None,
                        default: Some(serde_json::json!(10)),
                    },
                ),
            ]),
            required: vec!["query".to_owned()],
        },
        category: ToolCategory::Memory,
        auto_activate: false,
    }
}

fn note_def() -> ToolDef {
    ToolDef {
        name: ToolName::new("note").expect("valid tool name"),
        description: "Write a note to persistent session memory that survives distillation"
            .to_owned(),
        extended_description: None,
        input_schema: InputSchema {
            properties: IndexMap::from([
                (
                    "action".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::String,
                        description: "Action: 'add', 'list', 'delete'".to_owned(),
                        enum_values: None,
                        default: None,
                    },
                ),
                (
                    "content".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::String,
                        description: "Note content (required for 'add', max 500 chars)".to_owned(),
                        enum_values: None,
                        default: None,
                    },
                ),
                (
                    "category".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::String,
                        description:
                            "Note category: task, decision, preference, correction, context"
                                .to_owned(),
                        enum_values: None,
                        default: Some(serde_json::json!("context")),
                    },
                ),
                (
                    "id".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::Number,
                        description: "Note ID (required for 'delete')".to_owned(),
                        enum_values: None,
                        default: None,
                    },
                ),
            ]),
            required: vec!["action".to_owned()],
        },
        category: ToolCategory::Memory,
        auto_activate: false,
    }
}

fn blackboard_def() -> ToolDef {
    ToolDef {
        name: ToolName::new("blackboard").expect("valid tool name"),
        description: "Read and write shared state visible to all agents".to_owned(),
        extended_description: None,
        input_schema: InputSchema {
            properties: IndexMap::from([
                (
                    "action".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::String,
                        description: "Action: 'write', 'read', 'list', 'delete'".to_owned(),
                        enum_values: None,
                        default: None,
                    },
                ),
                (
                    "key".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::String,
                        description: "Blackboard key (required for write/read/delete)".to_owned(),
                        enum_values: None,
                        default: None,
                    },
                ),
                (
                    "value".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::String,
                        description: "Value to write (required for write action)".to_owned(),
                        enum_values: None,
                        default: None,
                    },
                ),
                (
                    "ttl_seconds".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::Number,
                        description: "Time-to-live in seconds (default 3600)".to_owned(),
                        enum_values: None,
                        default: Some(serde_json::json!(3600)),
                    },
                ),
            ]),
            required: vec!["action".to_owned()],
        },
        category: ToolCategory::Memory,
        auto_activate: false,
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::{Arc, Mutex};

    use aletheia_koina::id::{NousId, SessionId, ToolName};

    use crate::registry::ToolRegistry;
    use crate::types::{
        BlackboardEntry, BlackboardStore, NoteEntry, NoteStore, ToolContext, ToolInput,
        ToolServices,
    };

    type BoxError = Box<dyn std::error::Error + Send + Sync>;

    // --- In-memory mock stores ---

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
        ) -> Result<i64, BoxError> {
            let mut id = self.next_id.lock().unwrap();
            let note_id = *id;
            *id += 1;
            self.notes.lock().unwrap().push(NoteEntry {
                id: note_id,
                category: category.to_owned(),
                content: content.to_owned(),
                created_at: "2026-01-01T00:00:00Z".to_owned(),
            });
            Ok(note_id)
        }

        fn get_notes(&self, _session_id: &str) -> Result<Vec<NoteEntry>, BoxError> {
            Ok(self.notes.lock().unwrap().clone())
        }

        fn delete_note(&self, note_id: i64) -> Result<bool, BoxError> {
            let mut notes = self.notes.lock().unwrap();
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
        ) -> Result<(), BoxError> {
            let mut entries = self.entries.lock().unwrap();
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

        fn read(&self, key: &str) -> Result<Option<BlackboardEntry>, BoxError> {
            Ok(self
                .entries
                .lock()
                .unwrap()
                .iter()
                .find(|e| e.key == key)
                .cloned())
        }

        fn list(&self) -> Result<Vec<BlackboardEntry>, BoxError> {
            Ok(self.entries.lock().unwrap().clone())
        }

        fn delete(&self, key: &str, author: &str) -> Result<bool, BoxError> {
            let mut entries = self.entries.lock().unwrap();
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
        }
    }

    fn ctx_with_services(
        note_store: Arc<dyn NoteStore>,
        bb_store: Arc<dyn BlackboardStore>,
    ) -> ToolContext {
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
                http_client: reqwest::Client::new(),
            })),
        }
    }

    #[tokio::test]
    async fn register_memory_tools() {
        let mut reg = ToolRegistry::new();
        super::register(&mut reg).expect("register");
        assert_eq!(reg.definitions().len(), 3);
    }

    #[tokio::test]
    async fn mem0_search_def_requires_query() {
        let mut reg = ToolRegistry::new();
        super::register(&mut reg).expect("register");
        let name = ToolName::new("mem0_search").expect("valid");
        let def = reg.get_def(&name).expect("found");
        assert!(def.input_schema.required.contains(&"query".to_owned()));
    }

    #[tokio::test]
    async fn mem0_search_handles_sidecar_down() {
        let mut reg = ToolRegistry::new();
        super::register(&mut reg).expect("register");
        let note_store = Arc::new(MockNoteStore::new());
        let bb_store = Arc::new(MockBlackboardStore::new());
        let ctx = ctx_with_services(note_store, bb_store);

        let input = ToolInput {
            name: ToolName::new("mem0_search").expect("valid"),
            tool_use_id: "tu_1".to_owned(),
            arguments: serde_json::json!({"query": "test"}),
        };
        let result = reg.execute(&input, &ctx).await.expect("execute");
        assert!(result.is_error);
        assert!(
            result.content.text_summary().contains("unreachable"),
            "expected unreachable error: {}",
            result.content.text_summary()
        );
    }

    #[tokio::test]
    async fn mem0_search_no_services_returns_error() {
        let mut reg = ToolRegistry::new();
        super::register(&mut reg).expect("register");
        let input = ToolInput {
            name: ToolName::new("mem0_search").expect("valid"),
            tool_use_id: "tu_1".to_owned(),
            arguments: serde_json::json!({"query": "test"}),
        };
        let result = reg.execute(&input, &test_ctx()).await.expect("execute");
        assert!(result.is_error);
        assert!(result.content.text_summary().contains("not configured"));
    }

    // --- Note tests ---

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
        assert!(!r1.is_error);
        assert!(r1.content.text_summary().contains("#1"));

        let add2 = ToolInput {
            name: ToolName::new("note").expect("valid"),
            tool_use_id: "tu_2".to_owned(),
            arguments: serde_json::json!({"action": "add", "content": "second note"}),
        };
        let r2 = reg.execute(&add2, &ctx).await.expect("execute");
        assert!(!r2.is_error);
        assert!(r2.content.text_summary().contains("#2"));

        let list = ToolInput {
            name: ToolName::new("note").expect("valid"),
            tool_use_id: "tu_3".to_owned(),
            arguments: serde_json::json!({"action": "list"}),
        };
        let r3 = reg.execute(&list, &ctx).await.expect("execute");
        assert!(!r3.is_error);
        let text = r3.content.text_summary();
        assert!(text.contains("first note"));
        assert!(text.contains("second note"));
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
        assert!(!r.is_error);
        assert!(r.content.text_summary().contains("deleted"));

        let list = ToolInput {
            name: ToolName::new("note").expect("valid"),
            tool_use_id: "tu_3".to_owned(),
            arguments: serde_json::json!({"action": "list"}),
        };
        let r3 = reg.execute(&list, &ctx).await.expect("execute");
        assert!(r3.content.text_summary().contains("No session notes"));
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
        assert!(result.is_error);
        assert!(result.content.text_summary().contains("500"));
    }

    // --- Blackboard tests ---

    #[tokio::test]
    async fn blackboard_write_and_read() {
        let mut reg = ToolRegistry::new();
        super::register(&mut reg).expect("register");
        let note_store = Arc::new(MockNoteStore::new());
        let bb_store = Arc::new(MockBlackboardStore::new());
        let ctx = ctx_with_services(note_store, Arc::clone(&bb_store) as Arc<dyn BlackboardStore>);

        let write = ToolInput {
            name: ToolName::new("blackboard").expect("valid"),
            tool_use_id: "tu_1".to_owned(),
            arguments: serde_json::json!({"action": "write", "key": "goal", "value": "ship M0b"}),
        };
        let r1 = reg.execute(&write, &ctx).await.expect("execute");
        assert!(!r1.is_error);
        assert!(r1.content.text_summary().contains("[goal] written"));

        let read = ToolInput {
            name: ToolName::new("blackboard").expect("valid"),
            tool_use_id: "tu_2".to_owned(),
            arguments: serde_json::json!({"action": "read", "key": "goal"}),
        };
        let r2 = reg.execute(&read, &ctx).await.expect("execute");
        assert!(!r2.is_error);
        assert!(r2.content.text_summary().contains("ship M0b"));
    }

    #[tokio::test]
    async fn blackboard_list() {
        let mut reg = ToolRegistry::new();
        super::register(&mut reg).expect("register");
        let note_store = Arc::new(MockNoteStore::new());
        let bb_store = Arc::new(MockBlackboardStore::new());
        let ctx = ctx_with_services(note_store, Arc::clone(&bb_store) as Arc<dyn BlackboardStore>);

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
        assert!(!r.is_error);
        let text = r.content.text_summary();
        assert!(text.contains("[a] = 1"));
        assert!(text.contains("[b] = 2"));
    }

    #[tokio::test]
    async fn blackboard_delete_only_author() {
        let mut reg = ToolRegistry::new();
        super::register(&mut reg).expect("register");
        let note_store = Arc::new(MockNoteStore::new());
        let bb_store = Arc::new(MockBlackboardStore::new());
        let ctx = ctx_with_services(note_store, Arc::clone(&bb_store) as Arc<dyn BlackboardStore>);

        let write = ToolInput {
            name: ToolName::new("blackboard").expect("valid"),
            tool_use_id: "tu_1".to_owned(),
            arguments: serde_json::json!({"action": "write", "key": "secret", "value": "data"}),
        };
        reg.execute(&write, &ctx).await.expect("execute");

        // Try delete with different author
        let other_ctx = ToolContext {
            nous_id: NousId::new("other-agent").expect("valid"),
            session_id: SessionId::new(),
            workspace: PathBuf::from("/tmp/test"),
            allowed_roots: vec![PathBuf::from("/tmp")],
            services: ctx.services.clone(),
        };
        let del = ToolInput {
            name: ToolName::new("blackboard").expect("valid"),
            tool_use_id: "tu_2".to_owned(),
            arguments: serde_json::json!({"action": "delete", "key": "secret"}),
        };
        let r = reg.execute(&del, &other_ctx).await.expect("execute");
        assert!(r.content.text_summary().contains("not your entry"));

        // Original author can delete
        let del2 = ToolInput {
            name: ToolName::new("blackboard").expect("valid"),
            tool_use_id: "tu_3".to_owned(),
            arguments: serde_json::json!({"action": "delete", "key": "secret"}),
        };
        let r2 = reg.execute(&del2, &ctx).await.expect("execute");
        assert!(r2.content.text_summary().contains("deleted"));
    }
}

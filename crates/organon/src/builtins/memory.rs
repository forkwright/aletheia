//! Memory tool executors: `memory_search`, `note`, `blackboard`.

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

fn require_services(
    ctx: &ToolContext,
) -> std::result::Result<&crate::types::ToolServices, ToolResult> {
    ctx.services
        .as_deref()
        .ok_or_else(|| ToolResult::error("memory services not configured"))
}

// --- Memory Search ---

struct MemorySearchExecutor;

impl ToolExecutor for MemorySearchExecutor {
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
            #[expect(
                clippy::cast_possible_truncation,
                reason = "limit from user input is small"
            )]
            let limit = extract_opt_u64(&input.arguments, "limit").unwrap_or(10) as usize;

            let Some(knowledge) = services.knowledge.as_ref() else {
                return Ok(ToolResult::error("knowledge store not configured"));
            };

            match knowledge.search(query, ctx.nous_id.as_str(), limit).await {
                Ok(results) if results.is_empty() => Ok(ToolResult::text("No memories found.")),
                Ok(results) => Ok(ToolResult::text(format_results(&results))),
                Err(e) => Ok(ToolResult::error(format!("Memory search failed: {e}"))),
            }
        })
    }
}

fn format_results(results: &[crate::types::MemoryResult]) -> String {
    results
        .iter()
        .map(|r| format!("- {} (score: {:.2})", r.content, r.score))
        .collect::<Vec<_>>()
        .join("\n")
}

// --- Memory Correct ---

struct MemoryCorrectExecutor;

impl ToolExecutor for MemoryCorrectExecutor {
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
            let Some(knowledge) = services.knowledge.as_ref() else {
                return Ok(ToolResult::error("knowledge store not configured"));
            };

            let fact_id = extract_str(&input.arguments, "fact_id", &input.name)?;
            let new_content = extract_str(&input.arguments, "new_content", &input.name)?;

            match knowledge
                .correct_fact(fact_id, new_content, ctx.nous_id.as_str())
                .await
            {
                Ok(new_id) => Ok(ToolResult::text(format!(
                    "Fact {fact_id} corrected. New fact: {new_id}"
                ))),
                Err(e) => Ok(ToolResult::error(format!("Failed to correct fact: {e}"))),
            }
        })
    }
}

// --- Memory Retract ---

struct MemoryRetractExecutor;

impl ToolExecutor for MemoryRetractExecutor {
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
            let Some(knowledge) = services.knowledge.as_ref() else {
                return Ok(ToolResult::error("knowledge store not configured"));
            };

            let fact_id = extract_str(&input.arguments, "fact_id", &input.name)?;
            let reason = input.arguments.get("reason").and_then(|v| v.as_str());

            match knowledge.retract_fact(fact_id, reason).await {
                Ok(()) => Ok(ToolResult::text(format!("Fact {fact_id} retracted."))),
                Err(e) => Ok(ToolResult::error(format!("Failed to retract fact: {e}"))),
            }
        })
    }
}

// --- Memory Forget ---

struct MemoryForgetExecutor;

impl ToolExecutor for MemoryForgetExecutor {
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
            let Some(knowledge) = services.knowledge.as_ref() else {
                return Ok(ToolResult::error("knowledge store not configured"));
            };

            let fact_id = extract_str(&input.arguments, "fact_id", &input.name)?;
            let reason = extract_str(&input.arguments, "reason", &input.name)?;

            match knowledge.forget_fact(fact_id, reason).await {
                Ok(()) => Ok(ToolResult::text(format!(
                    "Fact {fact_id} forgotten (reason: {reason})."
                ))),
                Err(e) => Ok(ToolResult::error(format!("Failed to forget fact: {e}"))),
            }
        })
    }
}

// --- Memory Audit ---

struct MemoryAuditExecutor;

impl ToolExecutor for MemoryAuditExecutor {
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
            let Some(knowledge) = services.knowledge.as_ref() else {
                return Ok(ToolResult::error("knowledge store not configured"));
            };

            let nous_id = input
                .arguments
                .get("nous_id")
                .and_then(|v| v.as_str())
                .unwrap_or(ctx.nous_id.as_str());
            let since = input.arguments.get("since").and_then(|v| v.as_str());
            #[expect(
                clippy::cast_possible_truncation,
                reason = "audit limit from user input is small"
            )]
            let limit = extract_opt_u64(&input.arguments, "limit").unwrap_or(20) as usize;

            match knowledge.audit_facts(Some(nous_id), since, limit).await {
                Ok(facts) if facts.is_empty() => Ok(ToolResult::text("No facts found.")),
                Ok(facts) => {
                    let lines: Vec<String> = facts
                        .iter()
                        .map(|f| {
                            let forgotten_suffix = if f.is_forgotten {
                                let reason = f.forget_reason.as_deref().unwrap_or("unknown");
                                format!(" [FORGOTTEN: {reason}]")
                            } else {
                                String::new()
                            };
                            format!(
                                "- [{}] ({:.0}% {}) {} ({}){forgotten_suffix}",
                                f.id,
                                f.confidence * 100.0,
                                f.tier,
                                f.content,
                                f.recorded_at
                            )
                        })
                        .collect();
                    Ok(ToolResult::text(lines.join("\n")))
                }
                Err(e) => Ok(ToolResult::error(format!("Failed to audit facts: {e}"))),
            }
        })
    }
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
                "list" => match note_store.get_notes(&ctx.session_id.to_string()) {
                    Ok(notes) if notes.is_empty() => Ok(ToolResult::text("No session notes.")),
                    Ok(notes) => {
                        let lines: Vec<String> = notes
                            .iter()
                            .map(|n| format!("#{} [{}] {}", n.id, n.category, n.content))
                            .collect();
                        Ok(ToolResult::text(lines.join("\n")))
                    }
                    Err(e) => Ok(ToolResult::error(format!("Failed to list notes: {e}"))),
                },
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
                        Ok(None) => Ok(ToolResult::text(format!("No entry for key: {key}"))),
                        Err(e) => Ok(ToolResult::error(format!("Failed to read blackboard: {e}"))),
                    }
                }
                "list" => match bb_store.list() {
                    Ok(entries) if entries.is_empty() => {
                        Ok(ToolResult::text("Blackboard is empty."))
                    }
                    Ok(entries) => {
                        let lines: Vec<String> = entries
                            .iter()
                            .map(|e| format!("[{}] = {} (by {})", e.key, e.value, e.author_nous_id))
                            .collect();
                        Ok(ToolResult::text(lines.join("\n")))
                    }
                    Err(e) => Ok(ToolResult::error(format!("Failed to list blackboard: {e}"))),
                },
                "delete" => {
                    let key = extract_str(&input.arguments, "key", &input.name)?;
                    match bb_store.delete(key, ctx.nous_id.as_str()) {
                        Ok(true) => Ok(ToolResult::text(format!("Blackboard [{key}] deleted."))),
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

// --- Datalog Query ---

const MUTATION_KEYWORDS: &[&str] = &[":put", ":rm", ":replace", ":create", ":ensure"];
const DEFAULT_ROW_LIMIT: usize = 100;
const DEFAULT_TIMEOUT_SECS: f64 = 5.0;

struct DatalogQueryExecutor;

impl ToolExecutor for DatalogQueryExecutor {
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

            let params = input.arguments.get("params").cloned();
            let timeout = input
                .arguments
                .get("timeout_secs")
                .and_then(serde_json::Value::as_f64);
            let row_limit = input
                .arguments
                .get("row_limit")
                .and_then(serde_json::Value::as_u64)
                .map(|v| usize::try_from(v).unwrap_or(DEFAULT_ROW_LIMIT));

            // Defense in depth: reject mutation keywords before sending to engine
            let query_lower = query.to_lowercase();
            for kw in MUTATION_KEYWORDS {
                if query_lower.contains(kw) {
                    return Ok(ToolResult::error(format!(
                        "mutation keyword '{kw}' is not allowed in read-only queries"
                    )));
                }
            }

            let Some(knowledge) = services.knowledge.as_ref() else {
                return Ok(ToolResult::error("knowledge store not configured"));
            };

            match knowledge
                .datalog_query(
                    query,
                    params,
                    Some(timeout.unwrap_or(DEFAULT_TIMEOUT_SECS)),
                    Some(row_limit.unwrap_or(DEFAULT_ROW_LIMIT)),
                )
                .await
            {
                Ok(result) => {
                    let table = format_as_markdown_table(&result);
                    let mut output = table;
                    if result.truncated {
                        use std::fmt::Write;
                        let _ = write!(
                            output,
                            "\n\n_Results truncated to {} rows._",
                            row_limit.unwrap_or(DEFAULT_ROW_LIMIT)
                        );
                    }
                    Ok(ToolResult::text(output))
                }
                Err(e) => Ok(ToolResult::error(e)),
            }
        })
    }
}

fn format_as_markdown_table(result: &crate::types::DatalogResult) -> String {
    if result.columns.is_empty() || result.rows.is_empty() {
        return "No results.".to_owned();
    }

    let mut out = String::new();

    // Header
    out.push('|');
    for col in &result.columns {
        out.push(' ');
        out.push_str(col);
        out.push_str(" |");
    }
    out.push('\n');

    // Separator
    out.push('|');
    for _ in &result.columns {
        out.push_str(" --- |");
    }
    out.push('\n');

    // Rows
    for row in &result.rows {
        out.push('|');
        for cell in row {
            out.push(' ');
            match cell {
                serde_json::Value::String(s) => out.push_str(s),
                serde_json::Value::Null => out.push_str("null"),
                other => out.push_str(&other.to_string()),
            }
            out.push_str(" |");
        }
        out.push('\n');
    }

    out
}

fn datalog_query_def() -> ToolDef {
    ToolDef {
        name: ToolName::new("datalog_query").expect("valid tool name"),
        description: "Execute a read-only Datalog query against the knowledge graph. \
            Returns tabular results. Use for advanced knowledge exploration, debugging \
            recall quality, or querying graph structure. Cannot modify data."
            .to_owned(),
        extended_description: None,
        input_schema: InputSchema {
            properties: IndexMap::from([
                (
                    "query".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::String,
                        description:
                            "CozoScript/Datalog query. Must be read-only (no :put, :rm, :replace)."
                                .to_owned(),
                        enum_values: None,
                        default: None,
                    },
                ),
                (
                    "params".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::Object,
                        description:
                            "Optional named parameters for the query (e.g., {\"nous_id\": \"syn\"})"
                                .to_owned(),
                        enum_values: None,
                        default: None,
                    },
                ),
                (
                    "timeout_secs".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::Number,
                        description: "Query timeout in seconds (default: 5)".to_owned(),
                        enum_values: None,
                        default: Some(serde_json::json!(5)),
                    },
                ),
                (
                    "row_limit".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::Integer,
                        description: "Maximum number of result rows (default: 100)".to_owned(),
                        enum_values: None,
                        default: Some(serde_json::json!(100)),
                    },
                ),
            ]),
            required: vec!["query".to_owned()],
        },
        category: ToolCategory::Memory,
        auto_activate: false,
    }
}

pub fn register(registry: &mut ToolRegistry) -> Result<()> {
    registry.register(memory_search_def(), Box::new(MemorySearchExecutor))?;
    registry.register(memory_correct_def(), Box::new(MemoryCorrectExecutor))?;
    registry.register(memory_retract_def(), Box::new(MemoryRetractExecutor))?;
    registry.register(memory_forget_def(), Box::new(MemoryForgetExecutor))?;
    registry.register(memory_audit_def(), Box::new(MemoryAuditExecutor))?;
    registry.register(note_def(), Box::new(NoteExecutor))?;
    registry.register(blackboard_def(), Box::new(BlackboardExecutor))?;
    registry.register(datalog_query_def(), Box::new(DatalogQueryExecutor))?;
    Ok(())
}

// --- Tool Definitions (unchanged schemas) ---

fn memory_search_def() -> ToolDef {
    ToolDef {
        name: ToolName::new("memory_search").expect("valid tool name"),
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
        auto_activate: true,
    }
}

fn memory_correct_def() -> ToolDef {
    ToolDef {
        name: ToolName::new("memory_correct").expect("valid tool name"),
        description: "Correct a stored fact by superseding it with updated content".to_owned(),
        extended_description: None,
        input_schema: InputSchema {
            properties: IndexMap::from([
                (
                    "fact_id".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::String,
                        description: "ID of the fact to correct".to_owned(),
                        enum_values: None,
                        default: None,
                    },
                ),
                (
                    "new_content".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::String,
                        description: "Corrected fact content".to_owned(),
                        enum_values: None,
                        default: None,
                    },
                ),
            ]),
            required: vec!["fact_id".to_owned(), "new_content".to_owned()],
        },
        category: ToolCategory::Memory,
        auto_activate: false,
    }
}

fn memory_retract_def() -> ToolDef {
    ToolDef {
        name: ToolName::new("memory_retract").expect("valid tool name"),
        description: "Retract a stored fact (mark as no longer valid without deleting)".to_owned(),
        extended_description: None,
        input_schema: InputSchema {
            properties: IndexMap::from([
                (
                    "fact_id".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::String,
                        description: "ID of the fact to retract".to_owned(),
                        enum_values: None,
                        default: None,
                    },
                ),
                (
                    "reason".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::String,
                        description: "Optional reason for retraction".to_owned(),
                        enum_values: None,
                        default: None,
                    },
                ),
            ]),
            required: vec!["fact_id".to_owned()],
        },
        category: ToolCategory::Memory,
        auto_activate: false,
    }
}

fn memory_forget_def() -> ToolDef {
    ToolDef {
        name: ToolName::new("memory_forget").expect("valid tool name"),
        description: "Soft-delete a fact from memory (reversible, preserves audit trail)"
            .to_owned(),
        extended_description: None,
        input_schema: InputSchema {
            properties: IndexMap::from([
                (
                    "fact_id".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::String,
                        description: "ID of the fact to forget".to_owned(),
                        enum_values: None,
                        default: None,
                    },
                ),
                (
                    "reason".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::String,
                        description: "Why: user_requested, outdated, incorrect, privacy".to_owned(),
                        enum_values: Some(vec![
                            "user_requested".to_owned(),
                            "outdated".to_owned(),
                            "incorrect".to_owned(),
                            "privacy".to_owned(),
                        ]),
                        default: None,
                    },
                ),
            ]),
            required: vec!["fact_id".to_owned(), "reason".to_owned()],
        },
        category: ToolCategory::Memory,
        auto_activate: false,
    }
}

fn memory_audit_def() -> ToolDef {
    ToolDef {
        name: ToolName::new("memory_audit").expect("valid tool name"),
        description: "List recent fact extractions with confidence scores for review".to_owned(),
        extended_description: None,
        input_schema: InputSchema {
            properties: IndexMap::from([
                (
                    "nous_id".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::String,
                        description: "Filter by agent ID (defaults to current agent)".to_owned(),
                        enum_values: None,
                        default: None,
                    },
                ),
                (
                    "since".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::String,
                        description: "Filter facts recorded after this ISO datetime".to_owned(),
                        enum_values: None,
                        default: None,
                    },
                ),
                (
                    "limit".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::Number,
                        description: "Max results (default 20)".to_owned(),
                        enum_values: None,
                        default: Some(serde_json::json!(20)),
                    },
                ),
            ]),
            required: vec![],
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
        auto_activate: true,
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
        auto_activate: true,
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;
    use std::path::PathBuf;
    use std::sync::{Arc, Mutex, RwLock};

    use aletheia_koina::id::{NousId, SessionId, ToolName};

    use crate::registry::ToolRegistry;
    use crate::types::{
        BlackboardEntry, BlackboardStore, NoteEntry, NoteStore, ServerToolConfig, ToolContext,
        ToolInput, ToolServices,
    };

    type BoxError = Box<dyn std::error::Error + Send + Sync>;

    fn install_crypto_provider() {
        let _ = rustls::crypto::ring::default_provider().install_default();
    }

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
        assert_eq!(reg.definitions().len(), 8);
    }

    #[tokio::test]
    async fn memory_search_def_requires_query() {
        let mut reg = ToolRegistry::new();
        super::register(&mut reg).expect("register");
        let name = ToolName::new("memory_search").expect("valid");
        let def = reg.get_def(&name).expect("found");
        assert!(def.input_schema.required.contains(&"query".to_owned()));
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
        assert!(result.is_error);
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

        // Try delete with different author
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

    // --- Memory management tool tests ---

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
        assert!(result.is_error);
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
        assert!(result.is_error);
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
        assert!(result.is_error);
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
        assert!(!def.auto_activate);
    }

    #[tokio::test]
    async fn memory_retract_not_auto_activated() {
        let mut reg = ToolRegistry::new();
        super::register(&mut reg).expect("register");
        let name = ToolName::new("memory_retract").expect("valid");
        let def = reg.get_def(&name).expect("found");
        assert!(!def.auto_activate);
    }

    #[tokio::test]
    async fn memory_audit_not_auto_activated() {
        let mut reg = ToolRegistry::new();
        super::register(&mut reg).expect("register");
        let name = ToolName::new("memory_audit").expect("valid");
        let def = reg.get_def(&name).expect("found");
        assert!(!def.auto_activate);
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
        assert!(result.is_error);
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
        assert!(!def.auto_activate);
    }

    // --- Datalog query tool tests ---

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
        assert!(result.is_error);
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
        assert!(!def.auto_activate);
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
        let table = super::format_as_markdown_table(&result);
        assert_eq!(table, "No results.");
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
        let table = super::format_as_markdown_table(&result);
        assert!(table.contains("| id | name |"));
        assert!(table.contains("| --- | --- |"));
        assert!(table.contains("| 1 | alice |"));
        assert!(table.contains("| 2 | null |"));
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
}

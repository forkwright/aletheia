//! Knowledge graph memory operations: search, correct, retract, forget, audit.
#![expect(clippy::expect_used, reason = "ToolName::new() with static string literals is infallible — name validation would only fail on invalid chars which these names don't contain")]

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

use crate::builtins::workspace::{extract_opt_u64, extract_str};

use super::require_services;

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
                Ok(summary) => Ok(ToolResult::text(format!(
                    "Fact {} forgotten (reason: {reason}). Content: {}",
                    summary.id, summary.content
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

// --- Tool Definitions ---

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

// --- Registration ---

pub(super) fn register(registry: &mut ToolRegistry) -> Result<()> {
    registry.register(memory_search_def(), Box::new(MemorySearchExecutor))?;
    registry.register(memory_correct_def(), Box::new(MemoryCorrectExecutor))?;
    registry.register(memory_retract_def(), Box::new(MemoryRetractExecutor))?;
    registry.register(memory_forget_def(), Box::new(MemoryForgetExecutor))?;
    registry.register(memory_audit_def(), Box::new(MemoryAuditExecutor))?;
    Ok(())
}

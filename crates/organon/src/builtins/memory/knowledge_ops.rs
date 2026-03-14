//! Knowledge graph memory operations: search, correct, retract, forget, audit.
#![expect(
    clippy::expect_used,
    reason = "ToolName::new() with static string literals is infallible — name validation would only fail on invalid chars which these names don't contain"
)]

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
            let limit = extract_opt_u64(&input.arguments, "limit")
                .unwrap_or(10)
                .min(100) as usize;

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
        .fold(String::new(), |mut acc, s| {
            if !acc.is_empty() {
                acc.push('\n');
            }
            acc.push_str(&s);
            acc
        })
}

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
                        description: "Max results (default 10, max 100)".to_owned(),
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

pub(super) fn register(registry: &mut ToolRegistry) -> Result<()> {
    registry.register(memory_search_def(), Box::new(MemorySearchExecutor))?;
    registry.register(memory_correct_def(), Box::new(MemoryCorrectExecutor))?;
    registry.register(memory_retract_def(), Box::new(MemoryRetractExecutor))?;
    registry.register(memory_forget_def(), Box::new(MemoryForgetExecutor))?;
    registry.register(memory_audit_def(), Box::new(MemoryAuditExecutor))?;
    Ok(())
}

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test assertions")]
mod tests {
    use std::collections::HashSet;
    use std::path::PathBuf;
    use std::sync::{Arc, RwLock};

    use aletheia_koina::id::{NousId, SessionId, ToolName};

    use crate::registry::ToolExecutor;
    use crate::types::{ToolContext, ToolInput};

    use super::{MemoryAuditExecutor, MemorySearchExecutor};

    fn test_ctx_no_services() -> ToolContext {
        ToolContext {
            nous_id: NousId::new("test-agent").expect("valid"),
            session_id: SessionId::new(),
            workspace: PathBuf::from("/tmp/test"),
            allowed_roots: vec![PathBuf::from("/tmp")],
            services: None,
            active_tools: Arc::new(RwLock::new(HashSet::new())),
        }
    }

    fn tool_input(name: &str, args: serde_json::Value) -> ToolInput {
        ToolInput {
            name: ToolName::new(name).expect("valid"),
            tool_use_id: "toolu_test".to_owned(),
            arguments: args,
        }
    }

    #[tokio::test]
    async fn memory_search_returns_error_result_when_services_absent() {
        let ctx = test_ctx_no_services();
        let input = tool_input(
            "memory_search",
            serde_json::json!({ "query": "alice preferences" }),
        );
        let result = MemorySearchExecutor
            .execute(&input, &ctx)
            .await
            .expect("execute");
        assert!(
            result.is_error,
            "must be an error when no services: {}",
            result.content.text_summary()
        );
        assert!(
            result.content.text_summary().contains("not configured"),
            "error must mention services: {}",
            result.content.text_summary()
        );
    }

    #[tokio::test]
    async fn memory_audit_returns_error_result_when_services_absent() {
        let ctx = test_ctx_no_services();
        let input = tool_input("memory_audit", serde_json::json!({}));
        let result = MemoryAuditExecutor
            .execute(&input, &ctx)
            .await
            .expect("execute");
        assert!(result.is_error);
        assert!(result.content.text_summary().contains("not configured"));
    }

    #[test]
    fn memory_search_def_requires_query_field() {
        let mut reg = crate::registry::ToolRegistry::new();
        super::register(&mut reg).expect("register");
        let name = ToolName::new("memory_search").expect("valid");
        let def = reg.get_def(&name).expect("found");
        assert!(def.input_schema.required.contains(&"query".to_owned()));
    }

    #[test]
    fn memory_search_def_is_auto_activate() {
        let mut reg = crate::registry::ToolRegistry::new();
        super::register(&mut reg).expect("register");
        let name = ToolName::new("memory_search").expect("valid");
        let def = reg.get_def(&name).expect("found");
        assert!(def.auto_activate, "memory_search must be auto-activated");
    }

    #[test]
    fn knowledge_ops_registers_five_tools() {
        let mut reg = crate::registry::ToolRegistry::new();
        super::register(&mut reg).expect("register");
        assert_eq!(reg.definitions().len(), 5);
    }
}

//! Datalog query tool executor for read-only knowledge graph queries.
#![expect(
    clippy::expect_used,
    reason = "ToolName::new() with static string literals is infallible — name validation would only fail on invalid chars which these names don't contain"
)]

use std::future::Future;
use std::pin::Pin;

use indexmap::IndexMap;

use aletheia_koina::id::ToolName;

use crate::error::Result;
use crate::registry::{ToolExecutor, ToolRegistry};
use crate::types::{
    InputSchema, PropertyDef, PropertyType, ToolCategory, ToolContext, ToolDef, ToolInput,
    ToolResult,
};

use crate::builtins::workspace::extract_str;

use super::require_services;

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

            // WHY: Defense in depth: reject mutation keywords before sending to engine
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
                Err(e) => Ok(ToolResult::error(e.to_string())),
            }
        })
    }
}

pub(super) fn format_as_markdown_table(result: &crate::types::DatalogResult) -> String {
    if result.columns.is_empty() || result.rows.is_empty() {
        return "No results.".to_owned();
    }

    let mut out = String::new();

    out.push('|');
    for col in &result.columns {
        out.push(' ');
        out.push_str(col);
        out.push_str(" |");
    }
    out.push('\n');

    out.push('|');
    for _ in &result.columns {
        out.push_str(" --- |");
    }
    out.push('\n');

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
        name: ToolName::new("datalog_query").expect("valid tool name"), // kanon:ignore RUST/expect
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

pub(super) fn register(registry: &mut ToolRegistry) -> Result<()> {
    registry.register(datalog_query_def(), Box::new(DatalogQueryExecutor))?;
    Ok(())
}

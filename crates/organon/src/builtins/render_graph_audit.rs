//! `render_graph_audit` organon tool — render architecture fact audit to PDF.

use std::future::Future;
use std::pin::Pin;

use hermeneus::types::{DocumentSource, ToolResultBlock};
use indexmap::IndexMap;

use crate::error::Result;
use crate::registry::{ToolExecutor, ToolRegistry};
use crate::types::{
    InputSchema, PropertyDef, PropertyType, Reversibility, ToolCategory, ToolContext, ToolDef,
    ToolGroupId, ToolInput, ToolResult, ToolTag,
};

struct RenderGraphAuditExecutor;

/// Helper to emit a graph audit from the fact store at the default path.
async fn emit_graph_audit_from_default_store() -> std::result::Result<serde_json::Value, String> {
    let store = eidos::FactStore::default_path();
    let store = eidos::FactStore::new(store);

    // Load all facts from the store.
    let all_facts = store
        .list(None)
        .await
        .map_err(|e| format!("failed to list facts: {e}"))?;

    // Count facts by scope.
    let mut scope_counts: std::collections::HashMap<&str, usize> = std::collections::HashMap::new();
    for fact in &all_facts {
        *scope_counts
            .entry(fact.scope.to_string().leak())
            .or_insert(0) += 1;
    }

    let summary = serde_json::json!({
        "total": all_facts.len(),
        "by_scope": {
            "crate": scope_counts.get("crate").copied().unwrap_or(0),
            "module": scope_counts.get("module").copied().unwrap_or(0),
            "concept": scope_counts.get("concept").copied().unwrap_or(0),
            "boundary": scope_counts.get("boundary").copied().unwrap_or(0),
        }
    });

    // Transform facts into report-ready JSON.
    let facts_json: Vec<serde_json::Value> = all_facts
        .iter()
        .map(|f| {
            serde_json::json!({
                "id": f.id,
                "scope": f.scope.to_string(),
                "claim": f.claim,
                "evidence": f.evidence,
                "updated_at": f.updated_at,
                "updated_by": f.updated_by,
            })
        })
        .collect();

    Ok(serde_json::json!({
        "summary": summary,
        "facts": facts_json
    }))
}

impl ToolExecutor for RenderGraphAuditExecutor {
    fn execute<'a>(
        &'a self,
        input: &'a ToolInput,
        _ctx: &'a ToolContext,
    ) -> Pin<Box<dyn Future<Output = Result<ToolResult>> + Send + 'a>> {
        Box::pin(async move {
            let args = &input.arguments;

            // Extract or generate fact audit JSON data.
            let data: serde_json::Value =
                if let Some(raw) = args.get("data").and_then(serde_json::Value::as_str) {
                    // Inline JSON provided.
                    match serde_json::from_str(raw) {
                        Ok(v) => v,
                        Err(e) => {
                            return Ok(ToolResult::error(format!("data must be valid JSON: {e}")));
                        }
                    }
                } else if let Some(v) = args.get("data") {
                    // JSON value provided directly.
                    v.clone()
                } else if args.get("auto_load").and_then(serde_json::Value::as_bool) == Some(true) {
                    // Auto-load from default fact store path.
                    match emit_graph_audit_from_default_store().await {
                        Ok(d) => d,
                        Err(e) => {
                            return Ok(ToolResult::error(format!(
                                "failed to load from fact store: {e}"
                            )));
                        }
                    }
                } else {
                    return Ok(ToolResult::error(
                        "data field is required, or set auto_load=true to load from fact store"
                            .to_owned(),
                    ));
                };

            let pdf_bytes = match poiesis_typst::render_template("graph-audit", &data) {
                Ok(b) => b,
                Err(e) => {
                    return Ok(ToolResult::error(format!("graph audit render failed: {e}")));
                }
            };

            // Optional: write to a caller-provided path in addition to returning bytes.
            if let Some(out_path) = args.get("out_path").and_then(serde_json::Value::as_str)
                && let Err(e) = tokio::fs::write(out_path, &pdf_bytes).await
            {
                return Ok(ToolResult::error(format!(
                    "wrote 0 bytes to {out_path:?}: {e}"
                )));
            }

            let encoded = koina::base64::encode(&pdf_bytes);
            let summary = format!("Rendered graph audit: {} bytes PDF", pdf_bytes.len());

            Ok(ToolResult::blocks(vec![
                ToolResultBlock::Text { text: summary },
                ToolResultBlock::Document {
                    source: DocumentSource {
                        source_type: "base64".to_owned(),
                        media_type: "application/pdf".to_owned(),
                        data: encoded,
                    },
                },
            ]))
        })
    }
}

fn render_graph_audit_def() -> ToolDef {
    ToolDef {
        name: koina::id::ToolName::from_static("render_graph_audit"), // kanon:ignore RUST/expect
        description: "Render architecture fact audit to a PDF report via the graph-audit template."
            .to_owned(),
        extended_description: Some(
            "Pass a JSON object with `summary` (fact counts by scope) and `facts` array \
             (with id, scope, claim, evidence, updated_at, updated_by fields), or set \
             `auto_load=true` to automatically load facts from `~/aletheia/instance/facts/`. \
             The JSON blob is exposed to the Typst template as a virtual file read via \
             `json(\"data.json\")`. The result contains a text summary plus a base64-encoded \
             PDF document block; optionally also writes the PDF to `out_path` on the filesystem."
                .to_owned(),
        ),
        input_schema: InputSchema {
            properties: IndexMap::from([
                (
                    "data".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::String,
                        description: "JSON architecture fact audit data (summary + facts array). \
                             Mutually exclusive with auto_load=true."
                            .to_owned(),
                        enum_values: None,
                        default: None,
                    },
                ),
                (
                    "auto_load".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::String,
                        description:
                            "If true, automatically load facts from the default fact store path. \
                             Mutually exclusive with providing data."
                                .to_owned(),
                        enum_values: None,
                        default: None,
                    },
                ),
                (
                    "out_path".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::String,
                        description:
                            "Optional filesystem path to write the rendered PDF to, in addition \
                             to returning base64 bytes."
                                .to_owned(),
                        enum_values: None,
                        default: None,
                    },
                ),
            ]),
            required: vec![],
        },
        category: ToolCategory::Workspace,
        reversibility: Reversibility::FullyReversible,
        auto_activate: false,
        groups: vec![ToolGroupId::Read],
        tags: vec![ToolTag::Format],
    }
}

/// Register the `render_graph_audit` tool.
pub(crate) fn register(registry: &mut ToolRegistry) -> Result<()> {
    registry.register(render_graph_audit_def(), Box::new(RenderGraphAuditExecutor))?;
    Ok(())
}

//! `render_pptx_report` organon tool — JSON-first PPTX generation.
//!
//! Wraps [`poiesis_slides::render_pptx`] so agents can produce PowerPoint
//! presentations directly from a JSON slide descriptor.

use std::future::Future;
use std::pin::Pin;

use hermeneus::types::{DocumentSource, ToolResultBlock};
use indexmap::IndexMap;

use crate::error::Result;
use crate::registry::{ToolExecutor, ToolRegistry};
use crate::types::{
    InputSchema, PropertyDef, PropertyType, Reversibility, ToolCategory, ToolContext, ToolDef,
    ToolInput, ToolResult,
};

struct RenderPptxReportExecutor;

impl ToolExecutor for RenderPptxReportExecutor {
    fn execute<'a>(
        &'a self,
        input: &'a ToolInput,
        _ctx: &'a ToolContext,
    ) -> Pin<Box<dyn Future<Output = Result<ToolResult>> + Send + 'a>> {
        Box::pin(async move {
            let args = &input.arguments;

            let data: serde_json::Value =
                if let Some(raw) = args.get("data").and_then(serde_json::Value::as_str) {
                    match serde_json::from_str(raw) {
                        Ok(v) => v,
                        Err(e) => {
                            return Ok(ToolResult::error(format!("data must be valid JSON: {e}")));
                        }
                    }
                } else {
                    serde_json::json!({})
                };

            let pptx_bytes = match poiesis_slides::render_pptx(&data) {
                Ok(b) => b,
                Err(e) => {
                    return Ok(ToolResult::error(format!("pptx render failed: {e}")));
                }
            };

            // Optional: write to a caller-provided path in addition to returning bytes.
            if let Some(out_path) = args.get("out_path").and_then(serde_json::Value::as_str)
                && let Err(e) = tokio::fs::write(out_path, &pptx_bytes).await
            {
                return Ok(ToolResult::error(format!(
                    "wrote 0 bytes to {out_path:?}: {e}"
                )));
            }

            let encoded = koina::base64::encode(&pptx_bytes);
            let summary = format!("Rendered PPTX report: {} bytes", pptx_bytes.len());

            Ok(ToolResult::blocks(vec![
                ToolResultBlock::Text { text: summary },
                ToolResultBlock::Document {
                    source: DocumentSource {
                        source_type: "base64".to_owned(),
                        media_type: "application/vnd.openxmlformats-officedocument.presentationml.presentation".to_owned(),
                        data: encoded,
                    },
                },
            ]))
        })
    }
}

fn render_pptx_report_def() -> ToolDef {
    ToolDef {
        name: koina::id::ToolName::from_static("render_pptx_report"), // kanon:ignore RUST/expect
        description: "Render a JSON slide descriptor to a PPTX presentation.".to_owned(),
        extended_description: Some(
            "The JSON blob passed as `data` follows the schema: { slides: [{ title?, content: [{text?, bullets?: [..]}] }] }. \
             The result contains a text summary plus a base64-encoded PPTX document block; optionally \
             also writes the PPTX to `out_path` on the filesystem."
                .to_owned(),
        ),
        input_schema: InputSchema {
            properties: IndexMap::from([
                (
                    "data".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::String,
                        description: "Inline JSON slide descriptor (slides array with title and content).".to_owned(),
                        enum_values: None,
                        default: None,
                    },
                ),
                (
                    "out_path".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::String,
                        description: "Optional filesystem path to write the rendered PPTX to, in addition to returning base64 bytes.".to_owned(),
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
    }
}

/// Register the `render_pptx_report` tool.
pub(crate) fn register(registry: &mut ToolRegistry) -> Result<()> {
    registry.register(render_pptx_report_def(), Box::new(RenderPptxReportExecutor))
}

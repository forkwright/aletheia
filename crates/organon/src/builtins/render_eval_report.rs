//! `render_eval_report` organon tool — render eval results to PDF.

use std::future::Future;
use std::pin::Pin;

use hermeneus::types::{DocumentSource, ToolResultBlock};
use indexmap::IndexMap;

use crate::error::Result;
use crate::registry::{ToolExecutor, ToolRegistry};
use crate::types::{
    InputSchema, PropertyDef, PropertyType, Reversibility, ToolCategory, ToolContext, ToolDef,
    ToolGroupId, ToolInput, ToolResult,
};

struct RenderEvalReportExecutor;

impl ToolExecutor for RenderEvalReportExecutor {
    fn execute<'a>(
        &'a self,
        input: &'a ToolInput,
        _ctx: &'a ToolContext,
    ) -> Pin<Box<dyn Future<Output = Result<ToolResult>> + Send + 'a>> {
        Box::pin(async move {
            let args = &input.arguments;

            // Extract benchmark JSON data (required).
            let data: serde_json::Value =
                if let Some(raw) = args.get("data").and_then(serde_json::Value::as_str) {
                    match serde_json::from_str(raw) {
                        Ok(v) => v,
                        Err(e) => {
                            return Ok(ToolResult::error(format!("data must be valid JSON: {e}")));
                        }
                    }
                } else if let Some(v) = args.get("data") {
                    v.clone()
                } else {
                    return Ok(ToolResult::error(
                        "data field is required for render_eval_report".to_owned(),
                    ));
                };

            let pdf_bytes = match poiesis_typst::render_template("eval-report", &data) {
                Ok(b) => b,
                Err(e) => {
                    return Ok(ToolResult::error(format!("eval report render failed: {e}")));
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
            let summary = format!("Rendered eval report: {} bytes PDF", pdf_bytes.len());

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

fn render_eval_report_def() -> ToolDef {
    ToolDef {
        name: koina::id::ToolName::from_static("render_eval_report"), // kanon:ignore RUST/expect
        description: "Render evaluation results to a PDF report via the eval-report template."
            .to_owned(),
        extended_description: Some(
            "Pass a JSON object with `summary` (counts and timing) and `benchmarks` arrays. \
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
                        description: "JSON evaluation report data (summary + benchmarks array)."
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
            required: vec!["data".to_owned()],
        },
        category: ToolCategory::Workspace,
        reversibility: Reversibility::FullyReversible,
        auto_activate: false,
        groups: vec![ToolGroupId::Edit],
    }
}

/// Register the `render_eval_report` tool.
pub(crate) fn register(registry: &mut ToolRegistry) -> Result<()> {
    registry.register(render_eval_report_def(), Box::new(RenderEvalReportExecutor))?;
    Ok(())
}

//! `render_docx_report` organon tool — render a JSON document descriptor to DOCX.

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

pub(crate) struct RenderDocxReportExecutor;

impl ToolExecutor for RenderDocxReportExecutor {
    fn execute<'a>(
        &'a self,
        input: &'a ToolInput,
        _ctx: &'a ToolContext,
    ) -> Pin<Box<dyn Future<Output = Result<ToolResult>> + Send + 'a>> {
        Box::pin(async move {
            let args = &input.arguments;

            let data = match args.get("data") {
                Some(v) => {
                    if let Some(raw) = v.as_str() {
                        match serde_json::from_str(raw) {
                            Ok(parsed) => parsed,
                            Err(e) => {
                                return Ok(ToolResult::error(format!(
                                    "data must be valid JSON: {e}"
                                )));
                            }
                        }
                    } else {
                        v.clone()
                    }
                }
                None => {
                    return Ok(ToolResult::error(
                        "missing required argument: data".to_owned(),
                    ));
                }
            };

            let docx_bytes = match poiesis_doc::render_docx(&data) {
                Ok(b) => b,
                Err(e) => {
                    return Ok(ToolResult::error(format!("docx render failed: {e}")));
                }
            };

            // Optional: write to a caller-provided path in addition to returning bytes.
            if let Some(out_path) = args.get("out_path").and_then(serde_json::Value::as_str)
                && let Err(e) = tokio::fs::write(out_path, &docx_bytes).await
            {
                return Ok(ToolResult::error(format!(
                    "wrote 0 bytes to {out_path:?}: {e}"
                )));
            }

            let encoded = koina::base64::encode(&docx_bytes);
            let summary = format!("Rendered DOCX report: {} bytes", docx_bytes.len());

            Ok(ToolResult::blocks(vec![
                ToolResultBlock::Text { text: summary },
                ToolResultBlock::Document {
                    source: DocumentSource {
                        source_type: "base64".to_owned(),
                        media_type: "application/vnd.openxmlformats-officedocument.wordprocessingml.document".to_owned(),
                        data: encoded,
                    },
                },
            ]))
        })
    }
}

fn render_docx_report_def() -> ToolDef {
    ToolDef {
        name: koina::id::ToolName::from_static("render_docx_report"), // kanon:ignore RUST/expect
        description: "Render a JSON document descriptor to a DOCX file.".to_owned(),
        extended_description: Some(
            "The JSON blob passed as `data` follows the poiesis-doc schema: an optional \
             `title` string and a required `paragraphs` array of objects with `text` and \
             optional `style` fields. The result contains a text summary plus a base64-encoded \
             DOCX document block; optionally also writes the DOCX to `out_path` on the \
             filesystem."
                .to_owned(),
        ),
        input_schema: InputSchema {
            properties: IndexMap::from([
                (
                    "data".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::String,
                        description:
                            "Inline JSON data blob describing the document (title + paragraphs)."
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
                            "Optional filesystem path to write the rendered DOCX to, in addition \
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
        tags: vec![ToolTag::Format],
    }
}

/// Register the `render_docx_report` tool.
pub(crate) fn register(registry: &mut ToolRegistry) -> Result<()> {
    registry.register(render_docx_report_def(), Box::new(RenderDocxReportExecutor))?;
    Ok(())
}

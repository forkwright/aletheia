//! `render_xlsx_report` tool — JSON-first XLSX generation.

use std::future::Future;
use std::pin::Pin;

use hermeneus::types::{DocumentSource, ToolResultBlock};
use indexmap::IndexMap;

use crate::builtins::workspace::validate_path;
use crate::error::Result;
use crate::registry::{ToolExecutor, ToolRegistry};
use crate::types::{
    InputSchema, PropertyDef, PropertyType, Reversibility, ToolCategory, ToolContext, ToolDef,
    ToolGroupId, ToolInput, ToolResult, ToolTag,
};

fn extract_opt_str<'a>(args: &'a serde_json::Value, key: &str) -> Option<&'a str> {
    args.get(key).and_then(serde_json::Value::as_str)
}

// ── render_xlsx_report ────────────────────────────────────────────────────────

pub(crate) struct RenderXlsxReportExecutor;

impl ToolExecutor for RenderXlsxReportExecutor {
    fn execute<'a>(
        &'a self,
        input: &'a ToolInput,
        ctx: &'a ToolContext,
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

            let xlsx_result = poiesis_sheet::render_xlsx(&data);

            let xlsx_bytes = match xlsx_result {
                Ok(b) => b,
                Err(e) => {
                    return Ok(ToolResult::error(format!("xlsx render failed: {e}")));
                }
            };

            // Optional: write to a caller-provided path in addition to returning bytes.
            if let Some(out_path) = extract_opt_str(args, "out_path") {
                let validated = match validate_path(out_path, ctx, &input.name) {
                    Ok(path) => path,
                    Err(e) => {
                        return Ok(ToolResult::error(format!(
                            "invalid out_path {out_path:?}: {e}"
                        )));
                    }
                };
                if let Err(e) = tokio::fs::write(&validated, &xlsx_bytes).await {
                    return Ok(ToolResult::error(format!(
                        "wrote 0 bytes to {}: {e}",
                        validated.display()
                    )));
                }
            }

            let encoded = koina::base64::encode(&xlsx_bytes);
            let summary = format!("Rendered XLSX report: {} bytes", xlsx_bytes.len());

            Ok(ToolResult::blocks(vec![
                ToolResultBlock::Text { text: summary },
                ToolResultBlock::Document {
                    source: DocumentSource {
                        source_type: "base64".to_owned(),
                        media_type:
                            "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet"
                                .to_owned(),
                        data: encoded,
                    },
                },
            ]))
        })
    }
}

fn render_xlsx_report_def() -> ToolDef {
    ToolDef {
        name: koina::id::ToolName::from_static("render_xlsx_report"), // kanon:ignore RUST/expect
        description: "Render a JSON workbook descriptor to an XLSX spreadsheet.".to_owned(),
        extended_description: Some(
            "The JSON blob passed as `data` must conform to the workbook schema: \
             `{sheets: [{name, columns: [{header, width?}], rows: [[cell, ...]]}]}`. \
             The result contains a text summary plus a base64-encoded XLSX document block; \
             optionally also writes the XLSX to `out_path` on the filesystem."
                .to_owned(),
        ),
        input_schema: InputSchema {
            properties: IndexMap::from([
                (
                    "data".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::String,
                        description: "Inline JSON workbook descriptor.".to_owned(),
                        enum_values: None,
                        default: None,
                    },
                ),
                (
                    "out_path".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::String,
                        description:
                            "Optional filesystem path to write the rendered XLSX to, in addition \
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

/// Register the `render_xlsx_report` tool.
pub(crate) fn register(registry: &mut ToolRegistry) -> Result<()> {
    registry.register(render_xlsx_report_def(), Box::new(RenderXlsxReportExecutor))?;
    Ok(())
}

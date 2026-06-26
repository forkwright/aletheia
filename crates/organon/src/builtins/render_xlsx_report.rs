//! `render_xlsx_report` tool — JSON-first XLSX generation.

use std::future::Future;
use std::pin::Pin;

use hermeneus::types::{DocumentSource, ToolResultBlock};
use indexmap::IndexMap;

use crate::builtins::poiesis::json_data_property;
use crate::builtins::workspace::validate_path;
use crate::error::Result;
use crate::registry::{ToolExecutor, ToolRegistry};
use crate::types::{
    InputSchema, PropertyDef, PropertyType, Reversibility, ToolCallCapability,
    ToolCallCapabilityRule, ToolCategory, ToolContext, ToolDef, ToolGroupId, ToolInput, ToolResult,
    ToolTag,
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
                    json_data_property("JSON workbook descriptor object."),
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
                        ..Default::default()
                    },
                ),
            ]),
            required: vec!["data".to_owned()],
        },
        category: ToolCategory::Workspace,
        reversibility: Reversibility::PartiallyReversible,
        auto_activate: false,
        groups: vec![ToolGroupId::Edit],
        tags: vec![ToolTag::Format],
    }
}

fn render_xlsx_report_capability_rule() -> ToolCallCapabilityRule {
    ToolCallCapabilityRule::argument_presence(
        "out_path",
        ToolCallCapability::new(vec![ToolGroupId::Edit], Reversibility::PartiallyReversible),
        ToolCallCapability::new(vec![ToolGroupId::Read], Reversibility::FullyReversible),
    )
}

/// Register the `render_xlsx_report` tool.
pub(crate) fn register(registry: &mut ToolRegistry) -> Result<()> {
    registry.register_with_call_capability(
        render_xlsx_report_def(),
        render_xlsx_report_capability_rule(),
        Box::new(RenderXlsxReportExecutor),
    )?;
    Ok(())
}

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test assertions")]
#[expect(clippy::indexing_slicing, reason = "test schema assertions")]
mod tests {
    use super::*;
    use crate::types::ApprovalRequirement;
    use koina::id::ToolName;

    #[test]
    fn schema_declares_data_object_with_string_leniency() {
        let schema = render_xlsx_report_def().input_schema.to_json_schema();

        assert_eq!(schema["properties"]["data"]["type"], "object");
        assert!(
            schema["properties"]["data"]["description"]
                .as_str()
                .unwrap_or_default()
                .contains("JSON string"),
            "data schema must document stringified JSON leniency"
        );
    }

    #[test]
    fn render_xlsx_report_call_capability_requires_approval_when_out_path_present() {
        let mut registry = ToolRegistry::new();
        register(&mut registry).expect("register");

        assert_eq!(
            registry
                .approval_requirement_for_input(&ToolInput {
                    name: ToolName::from_static("render_xlsx_report"),
                    tool_use_id: "toolu_test".to_owned(),
                    arguments: serde_json::json!({
                        "data": {"sheets": [{"name": "Sheet1", "columns": [{"header": "A"}], "rows": [["x"]]}]},
                    }),
                })
                .expect("approval"),
            ApprovalRequirement::None,
            "no out_path means no disk write"
        );

        assert_eq!(
            registry
                .approval_requirement_for_input(&ToolInput {
                    name: ToolName::from_static("render_xlsx_report"),
                    tool_use_id: "toolu_test".to_owned(),
                    arguments: serde_json::json!({
                        "data": {"sheets": [{"name": "Sheet1", "columns": [{"header": "A"}], "rows": [["x"]]}]},
                        "out_path": "/tmp/report.xlsx",
                    }),
                })
                .expect("approval"),
            ApprovalRequirement::Required,
            "out_path present means disk write"
        );
    }
}

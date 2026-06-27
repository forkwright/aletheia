//! `render_docx_report` organon tool — render a JSON document descriptor to DOCX.

use std::future::Future;
use std::pin::Pin;

use hermeneus::types::{DocumentSource, ToolResultBlock};
use indexmap::IndexMap;
use poiesis_theme::sinks::emit_reference_docx;

use crate::builtins::poiesis::{
    extract_zip_entry, json_data_property, resolve_report_theme, rewrite_zip,
};
use crate::builtins::workspace::validate_path;
use crate::error::Result;
use crate::registry::{ToolExecutor, ToolRegistry};
use crate::types::{
    InputSchema, PropertyDef, PropertyType, Reversibility, ToolCallCapability,
    ToolCallCapabilityRule, ToolCategory, ToolContext, ToolDef, ToolGroupId, ToolInput, ToolResult,
    ToolTag,
};

pub(crate) struct RenderDocxReportExecutor;

impl ToolExecutor for RenderDocxReportExecutor {
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

            let docx_bytes = match poiesis_doc::render_docx(&data) {
                Ok(b) => b,
                Err(e) => {
                    return Ok(ToolResult::error(format!("docx render failed: {e}")));
                }
            };

            let theme = match resolve_report_theme(args, &data, ctx) {
                Ok(theme) => theme,
                Err(e) => return Ok(*e),
            };

            let reference_docx = match emit_reference_docx(&theme) {
                Ok(bytes) => bytes,
                Err(e) => {
                    return Ok(ToolResult::error(format!("theme docx sink failed: {e}")));
                }
            };
            let styles_xml = match extract_zip_entry(&reference_docx, "word/styles.xml") {
                Ok(bytes) => bytes,
                Err(e) => return Ok(ToolResult::error(e)),
            };
            let theme_xml = match extract_zip_entry(&reference_docx, "word/theme/theme1.xml") {
                Ok(bytes) => bytes,
                Err(e) => return Ok(ToolResult::error(e)),
            };
            let docx_bytes = match rewrite_zip(
                &docx_bytes,
                &[
                    ("word/styles.xml", styles_xml.as_slice()),
                    ("word/theme/theme1.xml", theme_xml.as_slice()),
                ],
            ) {
                Ok(bytes) => bytes,
                Err(e) => return Ok(ToolResult::error(e)),
            };

            // Optional: write to a caller-provided path in addition to returning bytes.
            if let Some(out_path) = args.get("out_path").and_then(serde_json::Value::as_str) {
                let validated = match validate_path(out_path, ctx, &input.name) {
                    Ok(path) => path,
                    Err(e) => {
                        return Ok(ToolResult::error(format!(
                            "invalid out_path {out_path:?}: {e}"
                        )));
                    }
                };
                if let Err(e) = tokio::fs::write(&validated, &docx_bytes).await {
                    return Ok(ToolResult::error(format!(
                        "wrote 0 bytes to {}: {e}",
                        validated.display()
                    )));
                }
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
                    json_data_property(
                        "JSON data object describing the document (title + paragraphs).",
                    ),
                ),
                (
                    "theme".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::String,
                        description: "Theme identifier (e.g. `summus`). Overrides any theme \
                                      declared inside `data`."
                            .to_owned(),
                        enum_values: None,
                        default: None,
                        ..Default::default()
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

fn render_docx_report_capability_rule() -> ToolCallCapabilityRule {
    ToolCallCapabilityRule::argument_presence(
        "out_path",
        ToolCallCapability::new(vec![ToolGroupId::Edit], Reversibility::PartiallyReversible),
        ToolCallCapability::new(vec![ToolGroupId::Read], Reversibility::FullyReversible),
    )
}

/// Register the `render_docx_report` tool.
pub(crate) fn register(registry: &mut ToolRegistry) -> Result<()> {
    registry.register_with_call_capability(
        render_docx_report_def(),
        render_docx_report_capability_rule(),
        Box::new(RenderDocxReportExecutor),
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
        let schema = render_docx_report_def().input_schema.to_json_schema();

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
    fn render_docx_report_call_capability_requires_approval_when_out_path_present() {
        let mut registry = ToolRegistry::new();
        register(&mut registry).expect("register");

        assert_eq!(
            registry
                .approval_requirement_for_input(&ToolInput {
                    name: ToolName::from_static("render_docx_report"),
                    tool_use_id: "toolu_test".to_owned(),
                    arguments: serde_json::json!({
                        "data": {"title": "Test", "paragraphs": [{"text": "Hello"}]},
                    }),
                })
                .expect("approval"),
            ApprovalRequirement::None,
            "no out_path means no disk write"
        );

        assert_eq!(
            registry
                .approval_requirement_for_input(&ToolInput {
                    name: ToolName::from_static("render_docx_report"),
                    tool_use_id: "toolu_test".to_owned(),
                    arguments: serde_json::json!({
                        "data": {"title": "Test", "paragraphs": [{"text": "Hello"}]},
                        "out_path": "/tmp/report.docx",
                    }),
                })
                .expect("approval"),
            ApprovalRequirement::Required,
            "out_path present means disk write"
        );
    }
}

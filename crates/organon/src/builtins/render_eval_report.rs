//! `render_eval_report` organon tool — render eval results to PDF.

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

struct RenderEvalReportExecutor;

impl ToolExecutor for RenderEvalReportExecutor {
    fn execute<'a>(
        &'a self,
        input: &'a ToolInput,
        ctx: &'a ToolContext,
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

            let validated_out_path =
                if let Some(out_path) = args.get("out_path").and_then(serde_json::Value::as_str) {
                    match validate_path(out_path, ctx, &input.name) {
                        Ok(path) => Some(path),
                        Err(e) => {
                            return Ok(ToolResult::error(format!(
                                "invalid out_path {out_path:?}: {e}"
                            )));
                        }
                    }
                } else {
                    None
                };

            let pdf_bytes = match poiesis_typst::render_template("eval-report", &data) {
                Ok(b) => b,
                Err(e) => {
                    return Ok(ToolResult::error(format!("eval report render failed: {e}")));
                }
            };

            // Optional: write to a caller-provided path in addition to returning bytes.
            if let Some(validated) = validated_out_path
                && let Err(e) = tokio::fs::write(&validated, &pdf_bytes).await
            {
                return Ok(ToolResult::error(format!(
                    "wrote 0 bytes to {}: {e}",
                    validated.display()
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
                    json_data_property(
                        "JSON evaluation report data object (summary + benchmarks array).",
                    ),
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
                        ..Default::default(),
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

fn render_eval_report_capability_rule() -> ToolCallCapabilityRule {
    ToolCallCapabilityRule::argument_presence(
        "out_path",
        ToolCallCapability::new(vec![ToolGroupId::Edit], Reversibility::PartiallyReversible),
        ToolCallCapability::new(vec![ToolGroupId::Read], Reversibility::FullyReversible),
    )
}

/// Register the `render_eval_report` tool.
pub(crate) fn register(registry: &mut ToolRegistry) -> Result<()> {
    registry.register_with_call_capability(
        render_eval_report_def(),
        render_eval_report_capability_rule(),
        Box::new(RenderEvalReportExecutor),
    )?;
    Ok(())
}

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test assertions")]
#[expect(clippy::indexing_slicing, reason = "test schema assertions")]
mod tests {
    use std::collections::HashSet;
    use std::sync::{Arc, RwLock};

    use koina::id::{NousId, SessionId, ToolName};

    use super::*;
    use crate::types::ApprovalRequirement;

    fn test_ctx(dir: &std::path::Path) -> ToolContext {
        ToolContext {
            nous_id: NousId::new("test-agent").expect("valid"),
            session_id: SessionId::new(),
            turn_number: 0,
            workspace: dir.to_path_buf(),
            allowed_roots: vec![dir.to_path_buf()],
            services: None,
            active_tools: Arc::new(RwLock::new(HashSet::new())),
            tool_config: Arc::new(taxis::config::ToolLimitsConfig::default()),
        }
    }

    fn input(out_path: &str) -> ToolInput {
        ToolInput {
            name: ToolName::from_static("render_eval_report"),
            tool_use_id: "toolu_test".to_owned(),
            arguments: serde_json::json!({
                "data": {"summary": {}, "benchmarks": []},
                "out_path": out_path,
            }),
        }
    }

    #[test]
    fn schema_declares_data_object_with_string_leniency() {
        let schema = render_eval_report_def().input_schema.to_json_schema();

        assert_eq!(schema["properties"]["data"]["type"], "object");
        assert!(
            schema["properties"]["data"]["description"]
                .as_str()
                .unwrap_or_default()
                .contains("JSON string"),
            "data schema must document stringified JSON leniency"
        );
    }

    #[tokio::test]
    async fn render_eval_report_rejects_out_path_escape() {
        let dir = tempfile::tempdir().expect("tmpdir");
        let ctx = test_ctx(dir.path());

        for out_path in ["/etc/eval-report.pdf", "../eval-report.pdf"] {
            let result = RenderEvalReportExecutor
                .execute(&input(out_path), &ctx)
                .await
                .expect("exec");
            assert!(result.is_error, "{out_path} must be rejected");
            assert!(result.content.text_summary().contains("invalid out_path"));
        }
    }

    #[test]
    fn render_eval_report_call_capability_requires_approval_when_out_path_present() {
        let mut registry = ToolRegistry::new();
        register(&mut registry).expect("register");

        assert_eq!(
            registry
                .approval_requirement_for_input(&ToolInput {
                    name: ToolName::from_static("render_eval_report"),
                    tool_use_id: "toolu_test".to_owned(),
                    arguments: serde_json::json!({
                        "data": {"summary": {}, "benchmarks": []},
                    }),
                })
                .expect("approval"),
            ApprovalRequirement::None,
            "no out_path means no disk write"
        );

        assert_eq!(
            registry
                .approval_requirement_for_input(&ToolInput {
                    name: ToolName::from_static("render_eval_report"),
                    tool_use_id: "toolu_test".to_owned(),
                    arguments: serde_json::json!({
                        "data": {"summary": {}, "benchmarks": []},
                        "out_path": "/tmp/eval.pdf",
                    }),
                })
                .expect("approval"),
            ApprovalRequirement::Required,
            "out_path present means disk write"
        );
    }
}

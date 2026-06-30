//! Diff report tool: compare two documents and report changes.

use std::fmt::Write;
use std::future::Future;
use std::pin::Pin;

use indexmap::IndexMap;
use poiesis_diff::{diff_presentations, diff_workbooks};

use crate::error::Result;
use crate::registry::{ToolExecutor, ToolRegistry};
use crate::types::{
    InputSchema, PropertyDef, PropertyType, Reversibility, ToolCategory, ToolContext, ToolGroupId,
    ToolInput, ToolResult, ToolTag,
};

struct DiffReportExecutor;

impl ToolExecutor for DiffReportExecutor {
    fn execute<'a>(
        &'a self,
        input: &'a ToolInput,
        _ctx: &'a ToolContext,
    ) -> Pin<Box<dyn Future<Output = Result<ToolResult>> + Send + 'a>> {
        Box::pin(async move {
            let args = &input.arguments;

            let format = args
                .get("format")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("xlsx");

            let Some(before_b64) = args.get("before").and_then(serde_json::Value::as_str) else {
                return Ok(ToolResult::error("missing required argument: before"));
            };

            let Some(after_b64) = args.get("after").and_then(serde_json::Value::as_str) else {
                return Ok(ToolResult::error("missing required argument: after"));
            };

            let before_bytes = match base64_decode(before_b64) {
                Ok(b) => b,
                Err(e) => return Ok(ToolResult::error(format!("failed to decode before: {e}"))),
            };

            let after_bytes = match base64_decode(after_b64) {
                Ok(b) => b,
                Err(e) => return Ok(ToolResult::error(format!("failed to decode after: {e}"))),
            };

            let diff_result = match format.to_lowercase().as_str() {
                "xlsx" => match diff_workbooks(&before_bytes, &after_bytes) {
                    Ok(diffs) => {
                        let mut summary = format!("Found {} cell changes:\n", diffs.len());
                        for diff in diffs.iter().take(20) {
                            let _ = writeln!(
                                summary,
                                "  {}.{}({},{}): {} -> {}",
                                diff.sheet,
                                col_index_to_letter(diff.col),
                                diff.row + 1,
                                col_index_to_letter(diff.col),
                                diff.before.as_deref().unwrap_or("(empty)"),
                                diff.after.as_deref().unwrap_or("(empty)")
                            );
                        }
                        if diffs.len() > 20 {
                            let _ = writeln!(summary, "... and {} more changes", diffs.len() - 20);
                        }
                        summary
                    }
                    Err(e) => return Ok(ToolResult::error(format!("XLSX diff failed: {e}"))),
                },
                "pptx" => match diff_presentations(&before_bytes, &after_bytes) {
                    Ok(diffs) => {
                        let mut summary = format!("Found {} slide changes:\n", diffs.len());
                        for diff in diffs.iter().take(10) {
                            let _ = writeln!(
                                summary,
                                "  Slide {}: {} -> {}",
                                diff.slide_index + 1,
                                diff.before.as_deref().unwrap_or("(empty)"),
                                diff.after.as_deref().unwrap_or("(empty)")
                            );
                        }
                        if diffs.len() > 10 {
                            let _ = writeln!(summary, "... and {} more changes", diffs.len() - 10);
                        }
                        summary
                    }
                    Err(e) => return Ok(ToolResult::error(format!("PPTX diff failed: {e}"))),
                },
                _ => return Ok(ToolResult::error(format!("unsupported format: {format}"))),
            };

            Ok(ToolResult::text(diff_result))
        })
    }
}

fn base64_decode(s: &str) -> std::result::Result<Vec<u8>, String> {
    use base64::{Engine as _, engine::general_purpose};
    general_purpose::STANDARD
        .decode(s)
        .map_err(|e| e.to_string())
}

fn col_index_to_letter(col: u32) -> String {
    let mut result = String::new();
    let mut n = col + 1;
    while n > 0 {
        n -= 1;
        result.insert(0, char::from(b'A' + u8::try_from(n % 26).unwrap_or(0)));
        n /= 26;
    }
    result
}

fn diff_report_def() -> crate::types::ToolDef {
    crate::types::ToolDef {
        name: koina::id::ToolName::from_static("diff_report"), // kanon:ignore RUST/expect
        description: "Compare two XLSX or PPTX documents and report cell/slide-level differences"
            .to_owned(),
        extended_description: Some(
            "Accepts base64-encoded binary documents (before and after), \
             detects format (XLSX or PPTX), and returns a structured diff report. \
             Useful for agents to track changes in their own generated outputs."
                .to_owned(),
        ),
        input_schema: InputSchema {
            properties: IndexMap::from([
                (
                    "format".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::String,
                        description: "Document format: 'xlsx' or 'pptx'".to_owned(),
                        enum_values: Some(vec!["xlsx".to_owned(), "pptx".to_owned()]),
                        default: Some(serde_json::json!("xlsx")),
                        ..Default::default()
                    },
                ),
                (
                    "before".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::String,
                        description: "Base64-encoded document (before state)".to_owned(),
                        enum_values: None,
                        default: None,
                        ..Default::default()
                    },
                ),
                (
                    "after".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::String,
                        description: "Base64-encoded document (after state)".to_owned(),
                        enum_values: None,
                        default: None,
                        ..Default::default()
                    },
                ),
            ]),
            required: vec!["before".to_owned(), "after".to_owned()],
        },
        category: ToolCategory::Workspace,
        reversibility: Reversibility::FullyReversible,
        auto_activate: false,
        groups: vec![ToolGroupId::Read, ToolGroupId::Verify],
        tags: vec![ToolTag::Verify, ToolTag::Recon],
    }
}

pub(crate) fn register(registry: &mut ToolRegistry) -> Result<()> {
    registry.register(diff_report_def(), Box::new(DiffReportExecutor))?;
    Ok(())
}

//! Inspect report tool: extract text content from PDF, XLSX, or PPTX documents.

use std::fmt::Write;
use std::future::Future;
use std::pin::Pin;

use indexmap::IndexMap;
use poiesis_inspect::{inspect_pdf, inspect_pptx, inspect_xlsx};

use crate::error::Result;
use crate::registry::{ToolExecutor, ToolRegistry};
use crate::types::{
    InputSchema, PropertyDef, PropertyType, Reversibility, ToolCategory, ToolContext, ToolGroupId,
    ToolInput, ToolResult, ToolTag,
};

struct InspectReportExecutor;

impl ToolExecutor for InspectReportExecutor {
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
                .unwrap_or("pdf");

            let Some(document_b64) = args.get("document").and_then(serde_json::Value::as_str)
            else {
                return Ok(ToolResult::error("missing required argument: document"));
            };

            let document_bytes = match base64_decode(document_b64) {
                Ok(b) => b,
                Err(e) => return Ok(ToolResult::error(format!("failed to decode document: {e}"))),
            };

            let inspect_result = match format.to_lowercase().as_str() {
                "pdf" => match inspect_pdf(&document_bytes) {
                    Ok(summary) => {
                        let mut text = "PDF Summary:\n".to_string();
                        let _ = writeln!(text, "  Pages: {}", summary.pages);
                        text.push_str("  Text snippets:\n");
                        for snippet in summary.text_snippets.iter().take(20) {
                            let _ = writeln!(text, "    {snippet}");
                        }
                        if summary.text_snippets.len() > 20 {
                            let _ = writeln!(
                                text,
                                "  ... and {} more snippets",
                                summary.text_snippets.len() - 20
                            );
                        }
                        text
                    }
                    Err(e) => return Ok(ToolResult::error(format!("PDF inspection failed: {e}"))),
                },
                "xlsx" => match inspect_xlsx(&document_bytes) {
                    Ok(summary) => {
                        let mut text = "Workbook Summary:\n".to_string();
                        for (sheet_name, content) in summary.sheets.iter().take(10) {
                            let _ = writeln!(text, "  Sheet: {sheet_name}");
                            let lines: Vec<&str> = content.lines().take(5).collect();
                            for line in lines {
                                let _ = writeln!(text, "    {line}");
                            }
                        }
                        if summary.sheets.len() > 10 {
                            let _ = writeln!(
                                text,
                                "  ... and {} more sheets",
                                summary.sheets.len() - 10
                            );
                        }
                        text
                    }
                    Err(e) => return Ok(ToolResult::error(format!("XLSX inspection failed: {e}"))),
                },
                "pptx" => match inspect_pptx(&document_bytes) {
                    Ok(summary) => {
                        let mut text = "Presentation Summary:\n".to_string();
                        for (idx, slide_text) in summary.slides.iter().enumerate().take(10) {
                            let _ = writeln!(text, "  Slide {}:", idx + 1);
                            let lines: Vec<&str> = slide_text.lines().take(3).collect();
                            for line in lines {
                                let _ = writeln!(text, "    {line}");
                            }
                        }
                        if summary.slides.len() > 10 {
                            let _ = writeln!(
                                text,
                                "  ... and {} more slides",
                                summary.slides.len() - 10
                            );
                        }
                        text
                    }
                    Err(e) => return Ok(ToolResult::error(format!("PPTX inspection failed: {e}"))),
                },
                _ => return Ok(ToolResult::error(format!("unsupported format: {format}"))),
            };

            Ok(ToolResult::text(inspect_result))
        })
    }
}

fn base64_decode(s: &str) -> std::result::Result<Vec<u8>, String> {
    use base64::{Engine as _, engine::general_purpose};
    general_purpose::STANDARD
        .decode(s)
        .map_err(|e| e.to_string())
}

fn inspect_report_def() -> crate::types::ToolDef {
    crate::types::ToolDef {
        name: koina::id::ToolName::from_static("inspect_report"), // kanon:ignore RUST/expect
        description: "Extract text content from PDF, XLSX, or PPTX documents".to_owned(),
        extended_description: Some(
            "Accepts a base64-encoded binary document (PDF, XLSX, or PPTX), \
             extracts readable text content, and returns a summary. \
             Useful for agents to read and inspect their own generated outputs."
                .to_owned(),
        ),
        input_schema: InputSchema {
            properties: IndexMap::from([
                (
                    "format".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::String,
                        description: "Document format: 'pdf', 'xlsx', or 'pptx'".to_owned(),
                        enum_values: Some(vec![
                            "pdf".to_owned(),
                            "xlsx".to_owned(),
                            "pptx".to_owned(),
                        ]),
                        default: Some(serde_json::json!("pdf")),
                    },
                ),
                (
                    "document".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::String,
                        description: "Base64-encoded document bytes".to_owned(),
                        enum_values: None,
                        default: None,
                    },
                ),
            ]),
            required: vec!["document".to_owned()],
        },
        category: ToolCategory::Workspace,
        reversibility: Reversibility::FullyReversible,
        auto_activate: false,
        groups: vec![ToolGroupId::Read],
        tags: vec![ToolTag::Recon],
    }
}

pub(crate) fn register(registry: &mut ToolRegistry) -> Result<()> {
    registry.register(inspect_report_def(), Box::new(InspectReportExecutor))?;
    Ok(())
}

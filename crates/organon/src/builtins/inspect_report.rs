//! Inspect report tool: extract text content from PDF, XLSX, PPTX, or DOCX documents.

use std::fmt::Write;
use std::future::Future;
use std::pin::Pin;

use indexmap::IndexMap;
use poiesis_doc::inspect_docx;
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
                "docx" => match inspect_docx(&document_bytes) {
                    Ok(summary) => {
                        let mut text = "DOCX Summary:\n".to_string();
                        for (idx, paragraph) in summary.paragraphs.iter().enumerate().take(20) {
                            let _ = writeln!(text, "  Paragraph {}: {paragraph}", idx + 1);
                        }
                        if summary.paragraphs.len() > 20 {
                            let _ = writeln!(
                                text,
                                "  ... and {} more paragraphs",
                                summary.paragraphs.len() - 20
                            );
                        }
                        text
                    }
                    Err(e) => return Ok(ToolResult::error(format!("DOCX inspection failed: {e}"))),
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
        description: "Extract text content from PDF, XLSX, PPTX, or DOCX documents".to_owned(),
        extended_description: Some(
            "Accepts a base64-encoded binary document (PDF, XLSX, PPTX, or DOCX), \
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
                        description: "Document format: 'pdf', 'xlsx', 'pptx', or 'docx'".to_owned(),
                        enum_values: Some(vec![
                            "pdf".to_owned(),
                            "xlsx".to_owned(),
                            "pptx".to_owned(),
                            "docx".to_owned(),
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

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test assertions")]
mod tests {
    use base64::{Engine as _, engine::general_purpose};

    use super::*;
    use crate::testing::make_test_context;

    #[tokio::test]
    async fn inspect_docx_round_trip() {
        let docx_bytes = poiesis_doc::render_docx(&serde_json::json!({
            "title": "Quarterly Report",
            "paragraphs": [
                { "text": "Revenue increased by 12%." },
                { "text": "Costs remained flat." }
            ]
        }))
        .expect("render must succeed");

        let input = ToolInput {
            name: koina::id::ToolName::from_static("inspect_report"),
            tool_use_id: "tu_docx_00001".to_owned(),
            arguments: serde_json::json!({
                "format": "docx",
                "document": general_purpose::STANDARD.encode(&docx_bytes)
            }),
        };

        let ctx = make_test_context();
        let result = InspectReportExecutor
            .execute(&input, &ctx)
            .await
            .expect("execute must succeed");

        assert!(!result.is_error, "docx inspection must succeed: {result:?}");

        let text = match &result.content {
            crate::types::ToolResultContent::Text(t) => t.as_str(),
            other => panic!("expected text content, got {other:?}"),
        };

        assert!(
            text.contains("DOCX Summary"),
            "summary header must be present"
        );
        assert!(
            text.contains("Quarterly Report"),
            "summary must include title paragraph"
        );
        assert!(
            text.contains("Revenue increased by 12%."),
            "summary must include first content paragraph"
        );
        assert!(
            text.contains("Costs remained flat."),
            "summary must include second content paragraph"
        );
    }
}

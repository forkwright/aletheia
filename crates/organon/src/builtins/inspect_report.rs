//! Inspect report tool: extract text content from PDF, XLSX, PPTX, or DOCX documents.

use std::fmt::Write;
use std::future::Future;
use std::pin::Pin;

use indexmap::IndexMap;
use poiesis_doc::inspect_docx;
use poiesis_inspect::{PdfInspectLimits, inspect_pdf_with_limits, inspect_pptx, inspect_xlsx};

use crate::builtins::workspace::base64_decode;
use crate::error::Result;
use crate::registry::{ToolExecutor, ToolRegistry};
use crate::types::{
    InputSchema, PropertyDef, PropertyType, Reversibility, RollbackSupport, ToolCapabilityMetadata,
    ToolCategory, ToolContext, ToolGroupId, ToolInput, ToolResult, ToolStability, ToolTag,
};

struct InspectReportExecutor;

impl ToolExecutor for InspectReportExecutor {
    fn execute<'a>(
        &'a self,
        input: &'a ToolInput,
        ctx: &'a ToolContext,
    ) -> Pin<Box<dyn Future<Output = Result<ToolResult>> + Send + 'a>> {
        Box::pin(async move {
            let turn_cancel = ctx.turn_cancel();
            if turn_cancel.is_cancelled() {
                return Ok(ToolResult::error("inspection cancelled before decoding"));
            }
            let args = &input.arguments;

            let format = args
                .get("format")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("pdf");

            let Some(document_b64) = args.get("document").and_then(serde_json::Value::as_str)
            else {
                return Ok(ToolResult::error("missing required argument: document"));
            };

            let (document_bytes, pdf_limits) =
                match decode_document_with_pdf_limit(document_b64, ctx.tool_config.max_pdf_bytes) {
                    Ok(decoded) => decoded,
                    Err(diagnostic) => return Ok(ToolResult::error(diagnostic)),
                };
            if turn_cancel.is_cancelled() {
                return Ok(ToolResult::error("inspection cancelled before parsing"));
            }

            let format = format.to_lowercase();
            let inspect_result = tokio::task::spawn_blocking(move || {
                inspect_document(&format, &document_bytes, &pdf_limits)
            })
            .await;
            if turn_cancel.is_cancelled() {
                return Ok(ToolResult::error("inspection cancelled after parsing"));
            }
            Ok(match inspect_result {
                Ok(result) => result,
                Err(_) => ToolResult::error("document inspection worker aborted"),
            })
        })
    }
}

#[expect(
    clippy::too_many_lines,
    reason = "one no-side-effect dispatcher keeps all supported report formats auditable"
)]
fn inspect_document(
    format: &str,
    document_bytes: &[u8],
    pdf_limits: &PdfInspectLimits,
) -> ToolResult {
    let inspect_result = match format {
        "pdf" => match inspect_pdf_with_limits(document_bytes, pdf_limits) {
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
            Err(e) => return ToolResult::error(format!("PDF inspection failed: {e}")),
        },
        "xlsx" => match inspect_xlsx(document_bytes) {
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
                    let _ = writeln!(text, "  ... and {} more sheets", summary.sheets.len() - 10);
                }
                text
            }
            Err(e) => return ToolResult::error(format!("XLSX inspection failed: {e}")),
        },
        "pptx" => match inspect_pptx(document_bytes) {
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
                    let _ = writeln!(text, "  ... and {} more slides", summary.slides.len() - 10);
                }
                text
            }
            Err(e) => return ToolResult::error(format!("PPTX inspection failed: {e}")),
        },
        "docx" => match inspect_docx(document_bytes) {
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
            Err(e) => return ToolResult::error(format!("DOCX inspection failed: {e}")),
        },
        _ => return ToolResult::error(format!("unsupported format: {format}")),
    };

    ToolResult::text(inspect_result)
}

/// Decode an inspect-report payload only after enforcing its PDF-sized boundary.
///
/// The encoded-length check happens before invoking the base64 decoder, which
/// would otherwise allocate its whole decoded output based on attacker input.
fn decode_document_with_pdf_limit(
    document_b64: &str,
    configured_limit: u64,
) -> std::result::Result<(Vec<u8>, PdfInspectLimits), String> {
    let Ok(max_input_bytes) = usize::try_from(configured_limit) else {
        return Err("configured PDF input limit is unsupported".to_owned());
    };
    if document_b64.len() > max_base64_length(max_input_bytes) {
        return Err("document exceeds the configured byte limit".to_owned());
    }
    let document_bytes = base64_decode(document_b64)
        .map_err(|error| format!("failed to decode document: {error}"))?;
    if document_bytes.len() > max_input_bytes {
        return Err("document exceeds the configured byte limit".to_owned());
    }
    Ok((
        document_bytes,
        PdfInspectLimits::for_input_bytes(max_input_bytes),
    ))
}

/// Largest padded base64 string that can decode within `max_decoded_bytes`.
///
/// Validate this before decoding, because a decoder normally allocates its
/// entire output from the encoded length.
fn max_base64_length(max_decoded_bytes: usize) -> usize {
    max_decoded_bytes
        .checked_add(2)
        .and_then(|bytes| bytes.checked_div(3))
        .and_then(|groups| groups.checked_mul(4))
        .unwrap_or(usize::MAX)
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
                        ..Default::default()
                    },
                ),
                (
                    "document".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::String,
                        description: "Base64-encoded document bytes".to_owned(),
                        enum_values: None,
                        default: None,
                        ..Default::default()
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
    registry.declare_capability(
        koina::id::ToolName::from_static("inspect_report"), // kanon:ignore RUST/expect
        ToolCapabilityMetadata {
            owner: "organon::builtins::inspect_report".to_owned(),
            // WHY Experimental: this module is behind `#[cfg(feature =
            // "poiesis")]` (see crates/organon/src/builtins/mod.rs) -- not
            // compiled by default.
            stability: ToolStability::Experimental,
            // WHY Supported: the executor extracts text from a base64-supplied
            // document in memory; nothing is written.
            rollback: RollbackSupport::Supported,
            ..ToolCapabilityMetadata::default()
        },
    )?;
    Ok(())
}

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test assertions")]
mod tests {
    use super::*;
    use crate::testing::make_test_context;

    fn overflowing_xref_stream_pdf() -> Vec<u8> {
        let mut bytes = b"%PDF-1.5\n".to_vec();
        let xref_offset = bytes.len();
        bytes.extend_from_slice(
            b"1 0 obj\n<< /Type /XRef /Size 1 /W [1 1 1] /Index [4294967295 2] /Length 6 >>\nstream\n",
        );
        bytes.extend_from_slice(&[1, 0, 0, 1, 0, 0]);
        bytes.extend_from_slice(
            format!("\nendstream\nendobj\nstartxref\n{xref_offset}\n%%EOF\n").as_bytes(),
        );
        bytes
    }

    #[test]
    fn base64_limit_is_checked_before_decode_allocation() {
        assert_eq!(max_base64_length(0), 0);
        assert_eq!(max_base64_length(1), 4);
        assert_eq!(max_base64_length(3), 4);
        assert_eq!(max_base64_length(4), 8);
    }

    #[tokio::test]
    async fn inspect_report_refuses_oversized_base64_before_decoding() {
        let input = ToolInput {
            name: koina::id::ToolName::from_static("inspect_report"),
            tool_use_id: "tu_pdf_limit_00001".to_owned(),
            // This is intentionally invalid base64. The configured-length
            // refusal must win, proving no decoder allocation/error path ran.
            arguments: serde_json::json!({ "format": "pdf", "document": "!!!!!" }),
        };
        let mut ctx = make_test_context();
        let mut limits = (*ctx.tool_config).clone();
        limits.max_pdf_bytes = 3;
        ctx.tool_config = std::sync::Arc::new(limits);

        let result = InspectReportExecutor
            .execute(&input, &ctx)
            .await
            .expect("tool execution must succeed");
        assert!(result.is_error);
        let text = match result.content {
            crate::types::ToolResultContent::Text(text) => text,
            other => panic!("expected error text, got {other:?}"),
        };
        assert!(text.contains("exceeds the configured byte limit"));
    }

    #[tokio::test]
    async fn inspect_report_refuses_an_already_cancelled_turn_before_decode() {
        let input = ToolInput {
            name: koina::id::ToolName::from_static("inspect_report"),
            tool_use_id: "tu_pdf_cancel_00001".to_owned(),
            arguments: serde_json::json!({ "format": "pdf", "document": "!!!!!" }),
        };
        let ctx = make_test_context();
        let cancel = tokio_util::sync::CancellationToken::new();
        cancel.cancel();

        let result =
            ToolContext::scope_turn_cancel(cancel, InspectReportExecutor.execute(&input, &ctx))
                .await
                .expect("tool execution must succeed");
        assert!(result.is_error);
        let text = match result.content {
            crate::types::ToolResultContent::Text(text) => text,
            other => panic!("expected error text, got {other:?}"),
        };
        assert!(text.contains("inspection cancelled before decoding"));
    }

    #[tokio::test]
    async fn inspect_report_contains_hostile_pdf_parser_failures_off_executor() {
        let input = ToolInput {
            name: koina::id::ToolName::from_static("inspect_report"),
            tool_use_id: "tu_pdf_overflow_00001".to_owned(),
            arguments: serde_json::json!({
                "format": "pdf",
                "document": koina::base64::encode(&overflowing_xref_stream_pdf())
            }),
        };
        let ctx = make_test_context();

        let result = InspectReportExecutor
            .execute(&input, &ctx)
            .await
            .expect("tool execution must stay contained");
        assert!(result.is_error);
        assert!(
            result
                .content
                .text_summary()
                .contains("PDF inspection failed"),
            "checked parser refusal should return through the tool: {result:?}"
        );
    }

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
                "document": koina::base64::encode(&docx_bytes)
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

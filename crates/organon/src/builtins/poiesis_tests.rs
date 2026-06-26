#![expect(clippy::expect_used, reason = "test assertions")]
#![expect(clippy::indexing_slicing, reason = "test object mutation")]
//! Tests for poiesis tool executors.
//!
//! Current coverage: `render_typst_report` and `generate_document` routing.
//! The other poiesis executors (lint, verify) are exercised by the underlying
//! crates' tests.

use std::collections::HashSet;
use std::io::{Cursor, Read, Write};
use std::sync::{Arc, RwLock};

use base64::Engine as _;
use koina::id::{NousId, SessionId, ToolName};
use poiesis_core::{Block, Document, Metadata, RichText};
use poiesis_theme::summus;
use zip::ZipArchive;

use super::*;
use crate::builtins::render_docx_report::RenderDocxReportExecutor;
use crate::builtins::render_pptx_report::RenderPptxReportExecutor;
use crate::builtins::render_xlsx_report::RenderXlsxReportExecutor;
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

fn tool_input(name: &str, args: serde_json::Value) -> ToolInput {
    ToolInput {
        name: ToolName::new(name).expect("valid"),
        tool_use_id: "toolu_test".to_owned(),
        arguments: args,
    }
}

fn document_bytes(result: &ToolResult, media_type: &str) -> Vec<u8> {
    match &result.content {
        crate::types::ToolResultContent::Blocks(blocks) => {
            let source = blocks
                .iter()
                .find_map(|block| match block {
                    ToolResultBlock::Document { source } if source.media_type == media_type => {
                        Some(&source.data)
                    }
                    _ => None,
                })
                .expect("document block must exist");
            base64::engine::general_purpose::STANDARD
                .decode(source)
                .expect("document block must decode")
        }
        other => panic!("expected Blocks content, got {other:?}"),
    }
}

fn pandoc_present() -> bool {
    poiesis_doc::render_md_from_doc(&simple_document()).is_ok()
}

fn simple_document() -> Document {
    Document {
        metadata: Metadata {
            title: "Test".to_owned(),
            author: None,
            created: None,
        },
        content: vec![
            Block::Heading {
                level: 1,
                text: RichText::from("Section"),
            },
            Block::Paragraph(RichText::from("Content.")),
        ],
    }
}

fn generate_document_args(format: &str) -> serde_json::Value {
    serde_json::json!({
        "format": format,
        "content": serde_json::json!([
            {"type": "heading", "level": 1, "text": "Title"},
            {"type": "paragraph", "text": "Body text."}
        ]).to_string()
    })
}

async fn assert_generate_document_ok(ctx: &ToolContext, format: &str) {
    let input = tool_input("generate_document", generate_document_args(format));
    let result = GenerateDocumentExecutor
        .execute(&input, ctx)
        .await
        .expect("exec");
    assert!(!result.is_error, "{format} render must succeed: {result:?}");
    let text = result.content.text_summary();
    assert!(
        text.contains(&format!("Generated {} document", format.to_uppercase())),
        "unexpected summary for {format}: {text}"
    );
    let expected_media_type = media_type_for_format(format);
    let bytes = document_bytes(&result, expected_media_type);
    assert!(
        !bytes.is_empty(),
        "{format} document block must contain bytes"
    );
}

#[tokio::test]
async fn render_typst_report_inline_source_returns_document_block() {
    let dir = tempfile::tempdir().expect("tmpdir");
    let ctx = test_ctx(dir.path());

    let input = tool_input(
        "render_typst_report",
        serde_json::json!({
            "source": "= Hello world\n\nA test report."
        }),
    );
    let result = RenderTypstReportExecutor
        .execute(&input, &ctx)
        .await
        .expect("exec");

    assert!(!result.is_error, "render must succeed: {result:?}");
    // Expect two content blocks: text summary + base64 PDF document.
    match &result.content {
        crate::types::ToolResultContent::Blocks(blocks) => {
            assert_eq!(blocks.len(), 2, "expected 2 blocks, got {}", blocks.len());
            let has_doc = blocks.iter().any(|b| {
                matches!(b, ToolResultBlock::Document { source } if source.media_type == "application/pdf")
            });
            assert!(has_doc, "result must include a PDF document block");
        }
        other => panic!("expected Blocks content, got {other:?}"),
    }
}

#[test]
fn render_typst_report_schema_matches_executor_inputs() {
    let def = render_typst_report_def();
    let schema = def.input_schema.to_json_schema();

    assert_eq!(schema["properties"]["data"]["type"], "object");
    assert!(
        schema["properties"]["data"]["description"]
            .as_str()
            .unwrap_or_default()
            .contains("JSON string"),
        "data schema must document stringified JSON leniency"
    );

    let enum_values = schema["properties"]["template"]["enum"]
        .as_array()
        .expect("template enum values");
    for slug in poiesis_typst::templates::SLUGS {
        assert!(
            enum_values.iter().any(|value| value.as_str() == Some(slug)),
            "template schema must include {slug}"
        );
    }
}

#[tokio::test]
async fn render_typst_report_default_template_with_data() {
    let dir = tempfile::tempdir().expect("tmpdir");
    let ctx = test_ctx(dir.path());

    let input = tool_input(
        "render_typst_report",
        serde_json::json!({
            "template": "default",
            "data": serde_json::json!({
                "title": "Research Summary",
                "author": "alice",
                "body": ["One paragraph.", "Two paragraphs."]
            }).to_string()
        }),
    );
    let result = RenderTypstReportExecutor
        .execute(&input, &ctx)
        .await
        .expect("exec");

    assert!(!result.is_error, "template render must succeed: {result:?}");
}

#[tokio::test]
async fn render_typst_report_writes_out_path() {
    let dir = tempfile::tempdir().expect("tmpdir");
    let ctx = test_ctx(dir.path());
    let out_path = dir.path().join("report.pdf");

    let input = tool_input(
        "render_typst_report",
        serde_json::json!({
            "source": "Hello from Typst.",
            "out_path": out_path.display().to_string(),
        }),
    );
    let result = RenderTypstReportExecutor
        .execute(&input, &ctx)
        .await
        .expect("exec");
    assert!(!result.is_error, "render must succeed: {result:?}");

    #[expect(
        clippy::disallowed_methods,
        reason = "test inspects the sandbox path the executor wrote to"
    )]
    let bytes = std::fs::read(&out_path).expect("PDF must exist at out_path");
    assert!(bytes.starts_with(b"%PDF-"), "file must be a PDF");
}

#[tokio::test]
async fn render_typst_report_rejects_out_path_escape() {
    let dir = tempfile::tempdir().expect("tmpdir");
    let ctx = test_ctx(dir.path());

    for out_path in ["/etc/aletheia-report.pdf", "../escape.pdf"] {
        let input = tool_input(
            "render_typst_report",
            serde_json::json!({
                "source": "Hello from Typst.",
                "out_path": out_path,
            }),
        );
        let result = RenderTypstReportExecutor
            .execute(&input, &ctx)
            .await
            .expect("exec");
        assert!(result.is_error, "{out_path} must be rejected");
        assert!(
            result.content.text_summary().contains("invalid out_path"),
            "unexpected error: {:?}",
            result.content
        );
    }
}

#[tokio::test]
async fn render_typst_report_requires_source_or_template() {
    let dir = tempfile::tempdir().expect("tmpdir");
    let ctx = test_ctx(dir.path());

    let input = tool_input("render_typst_report", serde_json::json!({}));
    let result = RenderTypstReportExecutor
        .execute(&input, &ctx)
        .await
        .expect("exec");
    assert!(
        result.is_error,
        "must error when neither source nor template is provided"
    );
}

#[tokio::test]
async fn render_typst_report_unknown_template_is_error() {
    let dir = tempfile::tempdir().expect("tmpdir");
    let ctx = test_ctx(dir.path());

    let input = tool_input(
        "render_typst_report",
        serde_json::json!({ "template": "no-such-template" }),
    );
    let result = RenderTypstReportExecutor
        .execute(&input, &ctx)
        .await
        .expect("exec");
    assert!(result.is_error, "unknown template must error");
}

#[tokio::test]
async fn render_typst_report_malformed_typst_is_error() {
    let dir = tempfile::tempdir().expect("tmpdir");
    let ctx = test_ctx(dir.path());

    let input = tool_input(
        "render_typst_report",
        serde_json::json!({ "source": "#this-function-does-not-exist()" }),
    );
    let result = RenderTypstReportExecutor
        .execute(&input, &ctx)
        .await
        .expect("exec");
    assert!(result.is_error, "malformed typst must error");
}

#[tokio::test]
async fn render_typst_report_renders_chart_payload() {
    let dir = tempfile::tempdir().expect("tmpdir");
    let ctx = test_ctx(dir.path());

    let input = tool_input(
        "render_typst_report",
        serde_json::json!({
            "template": "default",
            "data": {
                "title": "Chart Report",
                "body": ["This report carries a chart."],
                "chart": {
                    "kind": "bar",
                    "series": [
                        {
                            "name": "Revenue",
                            "tone": 0,
                            "points": [
                                {
                                    "label": "Q1",
                                    "y": { "id": "f1", "value": 12.0, "unit": "number" }
                                }
                            ]
                        }
                    ]
                }
            }
        }),
    );
    let result = RenderTypstReportExecutor
        .execute(&input, &ctx)
        .await
        .expect("exec");
    assert!(!result.is_error, "chart render must succeed: {result:?}");

    let bytes = document_bytes(&result, "application/pdf");
    assert!(bytes.starts_with(b"%PDF-"), "rendered document must be PDF");
    assert!(bytes.len() > 200, "rendered PDF must not be empty");
}

#[tokio::test]
async fn render_typst_report_bad_chart_is_error() {
    let dir = tempfile::tempdir().expect("tmpdir");
    let ctx = test_ctx(dir.path());

    let input = tool_input(
        "render_typst_report",
        serde_json::json!({
            "template": "default",
            "data": {
                "chart": "not a chart"
            }
        }),
    );
    let result = RenderTypstReportExecutor
        .execute(&input, &ctx)
        .await
        .expect("exec");
    assert!(result.is_error, "bad chart must fail");
    assert!(
        result
            .content
            .text_summary()
            .contains("chart must be valid JSON"),
        "unexpected error text: {:?}",
        result.content
    );
}

#[tokio::test]
async fn generate_document_unsupported_block_is_error() {
    let dir = tempfile::tempdir().expect("tmpdir");
    let ctx = test_ctx(dir.path());

    let input = tool_input(
        "generate_document",
        serde_json::json!({
            "content": serde_json::json!([
                {"type": "heading", "level": 1, "text": "Title"},
                {"type": "unsupported_foo", "text": "drop me"}
            ]).to_string()
        }),
    );
    let result = GenerateDocumentExecutor
        .execute(&input, &ctx)
        .await
        .expect("exec");
    assert!(
        result.is_error,
        "unsupported block must error, got: {result:?}"
    );
    let text = result.content.text_summary();
    assert!(
        text.contains("unsupported"),
        "error must mention unsupported type: {text}"
    );
}

#[tokio::test]
async fn generate_document_odt_uses_clean_room_renderer() {
    let bytes = poiesis_doc::render_odt_from_doc(&simple_document()).expect("must render");
    assert!(bytes.starts_with(b"PK"), "must be an ODT ZIP archive");
}

#[tokio::test]
async fn generate_document_odt_routes_without_pandoc() {
    let dir = tempfile::tempdir().expect("tmpdir");
    let ctx = test_ctx(dir.path());

    assert_generate_document_ok(&ctx, "odt").await;
}

#[tokio::test]
async fn generate_document_pdf_arm_is_unchanged() {
    let dir = tempfile::tempdir().expect("tmpdir");
    let ctx = test_ctx(dir.path());

    assert_generate_document_ok(&ctx, "pdf").await;
}

#[tokio::test]
async fn generate_document_xlsx_arm_is_unchanged() {
    let dir = tempfile::tempdir().expect("tmpdir");
    let ctx = test_ctx(dir.path());

    assert_generate_document_ok(&ctx, "xlsx").await;
}

#[tokio::test]
async fn generate_document_docx_routes_via_pandoc() {
    if !pandoc_present() {
        return;
    }

    let dir = tempfile::tempdir().expect("tmpdir");
    let ctx = test_ctx(dir.path());

    assert_generate_document_ok(&ctx, "docx").await;
}

#[tokio::test]
async fn generate_document_html_routes_via_pandoc() {
    if !pandoc_present() {
        return;
    }

    let dir = tempfile::tempdir().expect("tmpdir");
    let ctx = test_ctx(dir.path());

    assert_generate_document_ok(&ctx, "html").await;
}

#[tokio::test]
async fn generate_document_md_routes_via_pandoc() {
    if !pandoc_present() {
        return;
    }

    let dir = tempfile::tempdir().expect("tmpdir");
    let ctx = test_ctx(dir.path());

    assert_generate_document_ok(&ctx, "md").await;
}

#[tokio::test]
async fn generate_document_latex_routes_via_pandoc() {
    if !pandoc_present() {
        return;
    }

    let dir = tempfile::tempdir().expect("tmpdir");
    let ctx = test_ctx(dir.path());

    assert_generate_document_ok(&ctx, "latex").await;
}

#[tokio::test]
async fn generate_document_epub_routes_via_pandoc() {
    if !pandoc_present() {
        return;
    }

    let dir = tempfile::tempdir().expect("tmpdir");
    let ctx = test_ctx(dir.path());

    assert_generate_document_ok(&ctx, "epub").await;
}

async fn assert_generate_document_out_path(ctx: &ToolContext, format: &str) {
    let out_path = ctx.workspace.join(format!("output.{format}"));
    let mut args = generate_document_args(format);
    args["out_path"] = serde_json::json!(out_path.display().to_string());
    let input = tool_input("generate_document", args);
    let result = GenerateDocumentExecutor
        .execute(&input, ctx)
        .await
        .expect("exec");
    assert!(
        !result.is_error,
        "{format} out_path render must succeed: {result:?}"
    );
    let expected_media_type = media_type_for_format(format);
    let block_bytes = document_bytes(&result, expected_media_type);
    assert!(
        !block_bytes.is_empty(),
        "{format} returned document block must contain bytes"
    );

    #[expect(
        clippy::disallowed_methods,
        reason = "test inspects the sandbox path the executor wrote to"
    )]
    let file_bytes = std::fs::read(&out_path).expect("file must exist at out_path");
    assert_eq!(
        block_bytes, file_bytes,
        "{format} out_path bytes must match returned document block"
    );
}

#[tokio::test]
async fn generate_document_odt_writes_out_path() {
    let dir = tempfile::tempdir().expect("tmpdir");
    let ctx = test_ctx(dir.path());
    assert_generate_document_out_path(&ctx, "odt").await;
}

#[tokio::test]
async fn generate_document_pdf_writes_out_path() {
    let dir = tempfile::tempdir().expect("tmpdir");
    let ctx = test_ctx(dir.path());
    assert_generate_document_out_path(&ctx, "pdf").await;
}

#[tokio::test]
async fn generate_document_xlsx_writes_out_path() {
    let dir = tempfile::tempdir().expect("tmpdir");
    let ctx = test_ctx(dir.path());
    assert_generate_document_out_path(&ctx, "xlsx").await;
}

#[tokio::test]
async fn generate_document_docx_writes_out_path() {
    if !pandoc_present() {
        return;
    }
    let dir = tempfile::tempdir().expect("tmpdir");
    let ctx = test_ctx(dir.path());
    assert_generate_document_out_path(&ctx, "docx").await;
}

#[tokio::test]
async fn generate_document_html_writes_out_path() {
    if !pandoc_present() {
        return;
    }
    let dir = tempfile::tempdir().expect("tmpdir");
    let ctx = test_ctx(dir.path());
    assert_generate_document_out_path(&ctx, "html").await;
}

#[tokio::test]
async fn generate_document_md_writes_out_path() {
    if !pandoc_present() {
        return;
    }
    let dir = tempfile::tempdir().expect("tmpdir");
    let ctx = test_ctx(dir.path());
    assert_generate_document_out_path(&ctx, "md").await;
}

#[tokio::test]
async fn generate_document_latex_writes_out_path() {
    if !pandoc_present() {
        return;
    }
    let dir = tempfile::tempdir().expect("tmpdir");
    let ctx = test_ctx(dir.path());
    assert_generate_document_out_path(&ctx, "latex").await;
}

#[tokio::test]
async fn generate_document_epub_writes_out_path() {
    if !pandoc_present() {
        return;
    }
    let dir = tempfile::tempdir().expect("tmpdir");
    let ctx = test_ctx(dir.path());
    assert_generate_document_out_path(&ctx, "epub").await;
}

#[tokio::test]
async fn generate_document_rejects_out_path_escape() {
    let dir = tempfile::tempdir().expect("tmpdir");
    let ctx = test_ctx(dir.path());

    for out_path in ["/etc/aletheia-doc.odt", "../escape.odt"] {
        let mut args = generate_document_args("odt");
        args["out_path"] = serde_json::json!(out_path);
        let input = tool_input("generate_document", args);
        let result = GenerateDocumentExecutor
            .execute(&input, &ctx)
            .await
            .expect("exec");
        assert!(result.is_error, "{out_path} must be rejected");
        assert!(
            result.content.text_summary().contains("invalid out_path"),
            "unexpected error: {:?}",
            result.content
        );
    }
}

#[tokio::test]
async fn generate_document_unsupported_format_includes_details() {
    let dir = tempfile::tempdir().expect("tmpdir");
    let ctx = test_ctx(dir.path());

    let content = serde_json::json!([
        {"type": "heading", "level": 1, "text": "Title"},
        {"type": "paragraph", "text": "Body text."}
    ])
    .to_string();
    let input = tool_input(
        "generate_document",
        serde_json::json!({
            "format": "txt",
            "content": content
        }),
    );
    let result = GenerateDocumentExecutor
        .execute(&input, &ctx)
        .await
        .expect("exec");
    assert!(result.is_error, "unsupported format must error");
    let text = result.content.text_summary();
    assert!(
        text.contains("unsupported format"),
        "error must mention unsupported format: {text}"
    );
    assert!(
        text.contains("txt"),
        "error must mention the unsupported format name: {text}"
    );
}

#[tokio::test]
async fn generate_document_missing_pandoc_includes_dependency_details() {
    if pandoc_present() {
        return;
    }
    let dir = tempfile::tempdir().expect("tmpdir");
    let ctx = test_ctx(dir.path());

    let input = tool_input("generate_document", generate_document_args("docx"));
    let result = GenerateDocumentExecutor
        .execute(&input, &ctx)
        .await
        .expect("exec");
    assert!(result.is_error, "missing pandoc must error");
    let text = result.content.text_summary();
    assert!(
        text.to_lowercase().contains("pandoc"),
        "error must mention missing Pandoc dependency: {text}"
    );
}

#[test]
fn generate_document_call_capability_requires_approval_when_out_path_present() {
    let mut registry = ToolRegistry::new();
    register(&mut registry).expect("register");

    assert_eq!(
        registry
            .approval_requirement_for_input(&tool_input(
                "generate_document",
                serde_json::json!({
                    "content": serde_json::json!([{"type": "paragraph", "text": "x"}]).to_string(),
                }),
            ))
            .expect("approval"),
        ApprovalRequirement::None,
        "no out_path means no disk write"
    );

    assert_eq!(
        registry
            .approval_requirement_for_input(&tool_input(
                "generate_document",
                serde_json::json!({
                    "content": serde_json::json!([{"type": "paragraph", "text": "x"}]).to_string(),
                    "out_path": "/tmp/report.odt",
                }),
            ))
            .expect("approval"),
        ApprovalRequirement::Required,
        "out_path present means disk write"
    );
}

#[tokio::test]
async fn render_docx_report_missing_data_is_error() {
    let dir = tempfile::tempdir().expect("tmpdir");
    let ctx = test_ctx(dir.path());

    let input = tool_input("render_docx_report", serde_json::json!({}));
    let result = RenderDocxReportExecutor
        .execute(&input, &ctx)
        .await
        .expect("exec");
    assert!(result.is_error, "missing data must error");
}

#[tokio::test]
async fn render_pptx_report_missing_data_is_error() {
    let dir = tempfile::tempdir().expect("tmpdir");
    let ctx = test_ctx(dir.path());

    let input = tool_input("render_pptx_report", serde_json::json!({}));
    let result = RenderPptxReportExecutor
        .execute(&input, &ctx)
        .await
        .expect("exec");
    assert!(result.is_error, "missing data must error");
}

#[tokio::test]
async fn render_xlsx_report_missing_data_is_error() {
    let dir = tempfile::tempdir().expect("tmpdir");
    let ctx = test_ctx(dir.path());

    let input = tool_input("render_xlsx_report", serde_json::json!({}));
    let result = RenderXlsxReportExecutor
        .execute(&input, &ctx)
        .await
        .expect("exec");
    assert!(result.is_error, "missing data must error");
}

#[tokio::test]
async fn render_pptx_report_applies_summus_theme() {
    let dir = tempfile::tempdir().expect("tmpdir");
    let ctx = test_ctx(dir.path());

    let input = tool_input(
        "render_pptx_report",
        serde_json::json!({
            "data": {
                "slides": [
                    {
                        "title": "Theme Check",
                        "content": [
                            { "text": "Hello, alice." }
                        ]
                    }
                ]
            }
        }),
    );
    let result = RenderPptxReportExecutor
        .execute(&input, &ctx)
        .await
        .expect("exec");
    assert!(!result.is_error, "pptx render must succeed: {result:?}");

    let bytes = document_bytes(
        &result,
        "application/vnd.openxmlformats-officedocument.presentationml.presentation",
    );
    let mut archive = ZipArchive::new(Cursor::new(bytes)).expect("valid zip");
    let mut theme_xml = String::new();
    archive
        .by_name("ppt/theme/theme1.xml")
        .expect("theme1.xml must exist")
        .read_to_string(&mut theme_xml)
        .expect("read theme1.xml");
    assert!(
        theme_xml.contains("232E54"),
        "theme1.xml must carry the summus navy color: {theme_xml}"
    );
}

#[tokio::test]
async fn render_docx_report_applies_summus_reference() {
    let dir = tempfile::tempdir().expect("tmpdir");
    let ctx = test_ctx(dir.path());

    let input = tool_input(
        "render_docx_report",
        serde_json::json!({
            "data": {
                "title": "Docx Check",
                "paragraphs": [
                    { "text": "Hello, bob." }
                ]
            }
        }),
    );
    let result = RenderDocxReportExecutor
        .execute(&input, &ctx)
        .await
        .expect("exec");
    assert!(!result.is_error, "docx render must succeed: {result:?}");

    let bytes = document_bytes(
        &result,
        "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
    );
    let mut archive = ZipArchive::new(Cursor::new(bytes)).expect("valid zip");
    let mut styles_xml = String::new();
    archive
        .by_name("word/styles.xml")
        .expect("styles.xml must exist")
        .read_to_string(&mut styles_xml)
        .expect("read styles.xml");
    assert!(
        styles_xml.contains("Geist"),
        "styles.xml must carry the summus sans family: {styles_xml}"
    );
}

#[test]
fn chart_theme_adapter_maps_summus() {
    let theme = summus();
    let chart_theme = poiesis_charts::ResolvedTheme::from_poiesis_theme(&theme);
    assert_eq!(chart_theme.theme_name, "summus");
    assert_eq!(
        chart_theme.series[0].hex,
        theme
            .lookup_color(&theme.chart.series[0])
            .expect("series[0]")
            .as_str()
    );
    assert_eq!(
        chart_theme.series[1].hex,
        theme
            .lookup_color(&theme.chart.series[1])
            .expect("series[1]")
            .as_str()
    );
    assert!(
        chart_theme.font_sans.contains("Geist"),
        "sans family must come from the theme"
    );
    assert!(
        chart_theme.font_mono.contains("Geist Mono"),
        "mono family must come from the theme"
    );
}

#[tokio::test]
async fn qa_gate_clean_prose_passes() {
    let dir = tempfile::tempdir().expect("tmpdir");
    let ctx = test_ctx(dir.path());

    let input = tool_input(
        "qa_gate",
        serde_json::json!({
            "prose": "## Summary\nThe analysis shows 47 cases across three employers.\n## Appendix\nSources are internal.\n"
        }),
    );
    let result = QaGateExecutor.execute(&input, &ctx).await.expect("exec");
    assert!(!result.is_error, "qa_gate must succeed: {result:?}");
    let text = result.content.text_summary();
    let parsed: serde_json::Value = serde_json::from_str(&text).expect("valid json");
    assert_eq!(
        parsed["has_issues"], false,
        "clean prose must report no issues: {parsed:?}"
    );
}

#[tokio::test]
async fn qa_gate_banned_word_fails() {
    let dir = tempfile::tempdir().expect("tmpdir");
    let ctx = test_ctx(dir.path());

    let input = tool_input(
        "qa_gate",
        serde_json::json!({
            "prose": "## Summary\nThe approach is robust and comprehensive.\n## Appendix\n"
        }),
    );
    let result = QaGateExecutor.execute(&input, &ctx).await.expect("exec");
    assert!(
        !result.is_error,
        "qa_gate must succeed even when findings exist: {result:?}"
    );
    let text = result.content.text_summary();
    let parsed: serde_json::Value = serde_json::from_str(&text).expect("valid json");
    assert_eq!(
        parsed["has_issues"], true,
        "banned words must report issues: {parsed:?}"
    );
    assert!(
        parsed["issue_count"].as_u64().unwrap_or(0) > 0,
        "issue_count must be > 0: {parsed:?}"
    );
    assert!(
        parsed["issues"].is_array(),
        "issues must be an array: {parsed:?}"
    );
}

#[tokio::test]
async fn qa_gate_raw_color_literal_fails() {
    let dir = tempfile::tempdir().expect("tmpdir");
    let ctx = test_ctx(dir.path());

    let input = tool_input(
        "qa_gate",
        serde_json::json!({
            "prose": serde_json::json!({
                "slides": [{ "title_color": "#FF00AA" }]
            }).to_string()
        }),
    );
    let result = QaGateExecutor.execute(&input, &ctx).await.expect("exec");
    assert!(!result.is_error, "qa_gate must succeed: {result:?}");
    let text = result.content.text_summary();
    let parsed: serde_json::Value = serde_json::from_str(&text).expect("valid json");
    assert_eq!(
        parsed["has_issues"], true,
        "raw color must report issues: {parsed:?}"
    );
    let issues = parsed["issues"].as_array().expect("issues array");
    assert!(
        issues.iter().any(|i| {
            i["kind"].as_str() == Some("theme_violation")
                && i["message"].as_str().unwrap_or("").contains("raw color")
        }),
        "expected a theme color violation: {issues:?}"
    );
}

#[tokio::test]
async fn qa_gate_raw_font_literal_fails() {
    let dir = tempfile::tempdir().expect("tmpdir");
    let ctx = test_ctx(dir.path());

    let input = tool_input(
        "qa_gate",
        serde_json::json!({
            "prose": serde_json::json!({
                "style": "font-family: Arial"
            }).to_string()
        }),
    );
    let result = QaGateExecutor.execute(&input, &ctx).await.expect("exec");
    assert!(!result.is_error, "qa_gate must succeed: {result:?}");
    let text = result.content.text_summary();
    let parsed: serde_json::Value = serde_json::from_str(&text).expect("valid json");
    assert_eq!(
        parsed["has_issues"], true,
        "raw font must report issues: {parsed:?}"
    );
    let issues = parsed["issues"].as_array().expect("issues array");
    assert!(
        issues.iter().any(|i| {
            i["kind"].as_str() == Some("theme_violation")
                && i["message"].as_str().unwrap_or("").contains("raw font")
        }),
        "expected a theme font violation: {issues:?}"
    );
}

#[tokio::test]
async fn qa_gate_unknown_token_fails() {
    let dir = tempfile::tempdir().expect("tmpdir");
    let ctx = test_ctx(dir.path());

    let input = tool_input(
        "qa_gate",
        serde_json::json!({
            "prose": serde_json::json!({
                "fill": "color.role.fuchsia"
            }).to_string()
        }),
    );
    let result = QaGateExecutor.execute(&input, &ctx).await.expect("exec");
    assert!(!result.is_error, "qa_gate must succeed: {result:?}");
    let text = result.content.text_summary();
    let parsed: serde_json::Value = serde_json::from_str(&text).expect("valid json");
    assert_eq!(
        parsed["has_issues"], true,
        "unknown token must report issues: {parsed:?}"
    );
    let issues = parsed["issues"].as_array().expect("issues array");
    assert!(
        issues.iter().any(|i| {
            i["kind"].as_str() == Some("theme_violation")
                && i["message"]
                    .as_str()
                    .unwrap_or("")
                    .contains("unknown-token")
        }),
        "expected an unknown token violation: {issues:?}"
    );
}

#[tokio::test]
async fn report_renderers_reject_out_path_escape() {
    let dir = tempfile::tempdir().expect("tmpdir");
    let ctx = test_ctx(dir.path());

    let cases = [
        (
            "render_docx_report",
            serde_json::json!({
                "data": {"title": "Test", "paragraphs": [{"text": "Hello"}]}
            }),
        ),
        (
            "render_pptx_report",
            serde_json::json!({
                "data": {"slides": [{"title": "Test", "content": [{"text": "Hello"}]}]}
            }),
        ),
        (
            "render_xlsx_report",
            serde_json::json!({
                "data": {"sheets": [{"name": "Sheet1", "columns": [{"header": "A"}], "rows": [["x"]]}]}
            }),
        ),
    ];

    for (name, base_args) in cases {
        for out_path in ["/etc/aletheia-report.bin", "../escape.bin"] {
            let mut args = base_args.clone();
            args["out_path"] = serde_json::json!(out_path);
            let input = tool_input(name, args);
            let result = match name {
                "render_docx_report" => RenderDocxReportExecutor.execute(&input, &ctx).await,
                "render_pptx_report" => RenderPptxReportExecutor.execute(&input, &ctx).await,
                "render_xlsx_report" => RenderXlsxReportExecutor.execute(&input, &ctx).await,
                other => panic!("unexpected renderer {other}"),
            }
            .expect("exec");
            assert!(result.is_error, "{name} {out_path} must be rejected");
            assert!(
                result.content.text_summary().contains("invalid out_path"),
                "unexpected error for {name}: {:?}",
                result.content
            );
        }
    }
}

#[test]
fn render_typst_report_call_capability_requires_approval_when_out_path_present() {
    let mut registry = ToolRegistry::new();
    register(&mut registry).expect("register");

    assert_eq!(
        registry
            .approval_requirement_for_input(&tool_input(
                "render_typst_report",
                serde_json::json!({
                    "source": "Hello from Typst.",
                }),
            ))
            .expect("approval"),
        ApprovalRequirement::None,
        "no out_path means no disk write"
    );

    assert_eq!(
        registry
            .approval_requirement_for_input(&tool_input(
                "render_typst_report",
                serde_json::json!({
                    "source": "Hello from Typst.",
                    "out_path": "/tmp/report.pdf",
                }),
            ))
            .expect("approval"),
        ApprovalRequirement::Required,
        "out_path present means disk write"
    );
}

const CUSTOM_THEME_TOML: &str = r##"
[meta]
id = "custom"
title = "Custom"

[color.role]
navy = "#232E54"
teal = "#318891"

[color.tone]
positive = "teal"
neutral = "navy"

[color.surface]
page = "navy"

[type.family]
sans = ["Geist", "system-ui"]
"##;

fn write_theme_toml(themes_dir: &std::path::Path, name: &str, body: &str) -> std::path::PathBuf {
    std::fs::create_dir_all(themes_dir).expect("create themes dir");
    let path = themes_dir.join(format!("{name}.toml"));
    let mut file = std::fs::File::create(&path).expect("create theme toml");
    file.write_all(body.as_bytes()).expect("write theme toml");
    path
}

#[test]
fn resolve_report_theme_defaults_to_summus() {
    let dir = tempfile::tempdir().expect("tmpdir");
    let ctx = test_ctx(dir.path());

    let theme = resolve_report_theme(&serde_json::json!({}), &serde_json::json!({}), &ctx)
        .expect("default theme resolves");
    assert_eq!(theme.id.as_str(), "summus");
}

#[test]
fn resolve_report_theme_top_level_arg_wins() {
    let dir = tempfile::tempdir().expect("tmpdir");
    write_theme_toml(&dir.path().join("themes"), "custom", CUSTOM_THEME_TOML);
    let ctx = test_ctx(dir.path());

    let theme = resolve_report_theme(
        &serde_json::json!({ "theme": "custom" }),
        &serde_json::json!({ "theme": "summus" }),
        &ctx,
    )
    .expect("top-level theme wins");
    assert_eq!(theme.id.as_str(), "custom");
}

#[test]
fn resolve_report_theme_data_theme_id() {
    let dir = tempfile::tempdir().expect("tmpdir");
    write_theme_toml(&dir.path().join("themes"), "custom", CUSTOM_THEME_TOML);
    let ctx = test_ctx(dir.path());

    let theme = resolve_report_theme(
        &serde_json::json!({}),
        &serde_json::json!({ "theme_id": "custom" }),
        &ctx,
    )
    .expect("data theme_id resolves");
    assert_eq!(theme.id.as_str(), "custom");
}

#[test]
fn resolve_report_theme_data_spec_theme() {
    let dir = tempfile::tempdir().expect("tmpdir");
    write_theme_toml(&dir.path().join("themes"), "custom", CUSTOM_THEME_TOML);
    let ctx = test_ctx(dir.path());

    let theme = resolve_report_theme(
        &serde_json::json!({}),
        &serde_json::json!({ "spec": { "theme": "custom" } }),
        &ctx,
    )
    .expect("spec.theme resolves");
    assert_eq!(theme.id.as_str(), "custom");
}

#[test]
fn resolve_report_theme_invalid_id_is_error() {
    let dir = tempfile::tempdir().expect("tmpdir");
    let ctx = test_ctx(dir.path());

    let err = resolve_report_theme(
        &serde_json::json!({ "theme": "Bad Name" }),
        &serde_json::json!({}),
        &ctx,
    )
    .expect_err("invalid id must error");
    let text = err.content.text_summary();
    assert!(
        text.contains("invalid theme id"),
        "unexpected error: {text}"
    );
}

#[test]
fn resolve_report_theme_missing_registry_is_error() {
    let dir = tempfile::tempdir().expect("tmpdir");
    let ctx = test_ctx(dir.path());

    let err = resolve_report_theme(
        &serde_json::json!({ "theme": "custom" }),
        &serde_json::json!({}),
        &ctx,
    )
    .expect_err("missing registry must error");
    let text = err.content.text_summary();
    assert!(
        text.contains("themes registry directory not found"),
        "unexpected error: {text}"
    );
}

#[test]
fn resolve_report_theme_unknown_theme_is_error() {
    let dir = tempfile::tempdir().expect("tmpdir");
    write_theme_toml(&dir.path().join("themes"), "custom", CUSTOM_THEME_TOML);
    let ctx = test_ctx(dir.path());

    let err = resolve_report_theme(
        &serde_json::json!({ "theme": "unknown" }),
        &serde_json::json!({}),
        &ctx,
    )
    .expect_err("unknown theme must error");
    let text = err.content.text_summary();
    assert!(
        text.contains("failed to resolve theme"),
        "unexpected error: {text}"
    );
}

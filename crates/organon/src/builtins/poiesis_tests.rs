#![expect(clippy::expect_used, reason = "test assertions")]
#![expect(clippy::indexing_slicing, reason = "test object mutation")]
//! Tests for poiesis tool executors.
//!
//! Current coverage: `render_typst_report`. The other poiesis executors (lint,
//! verify, `generate_document`) are exercised by the underlying crates' tests.

use std::collections::HashSet;
use std::sync::{Arc, RwLock};

use koina::id::{NousId, SessionId, ToolName};

use super::*;
use crate::builtins::render_docx_report::RenderDocxReportExecutor;
use crate::builtins::render_pptx_report::RenderPptxReportExecutor;
use crate::builtins::render_xlsx_report::RenderXlsxReportExecutor;

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

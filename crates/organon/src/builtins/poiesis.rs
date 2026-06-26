//! Poiesis report tools: `generate_document`, `lint_report`, `verify_report`,
//! `render_typst_report`, `qa_gate`.
//!
//! - `generate_document`    — render a JSON document descriptor to ODT/XLSX/PDF/DOCX/HTML/MD/LaTeX/EPUB bytes
//! - `lint_report`          — check report prose quality (banned words, citations, structure)
//! - `verify_report`        — validate numeric claims in a verify manifest
//! - `render_typst_report`  — render a Typst source (or built-in template slug) to PDF
//! - `qa_gate`              — run prose lint and optional factbase validation, returning a structured QA report

use std::future::Future;
use std::io::{Cursor, Read as _, Write as _};
use std::pin::Pin;

use hermeneus::types::{DocumentSource, ToolResultBlock};
use indexmap::IndexMap;
use poiesis_charts::render::{Canvas, DocCanvas};
use poiesis_charts::{
    Chart, ColorMode as ChartColorMode, ResolvedTheme as ChartResolvedTheme, render_chart,
};
use poiesis_core::{
    Block, Document, Factbase, Metadata, QaIssue, QaIssueKind, QaReport, Renderer, RichText, Span,
};
use poiesis_lint::{LintConfig, Linter};
use poiesis_theme::{
    Registry, ResolvedTheme, ThemeId,
    lint::{RawColorLiteralRule, RawFontLiteralRule, UnknownTokenRule},
    sinks::emit_typst_template,
    summus,
};
use poiesis_verify::Verifier;
use zip::write::SimpleFileOptions;
use zip::{ZipArchive, ZipWriter};

use crate::builtins::workspace::validate_path;
use crate::error::Result;
use crate::registry::{ToolExecutor, ToolRegistry};
use crate::types::{
    InputSchema, PropertyDef, PropertyType, Reversibility, ToolCallCapability,
    ToolCallCapabilityRule, ToolCategory, ToolContext, ToolDef, ToolGroupId, ToolInput, ToolResult,
    ToolTag,
};

// ── Helpers ───────────────────────────────────────────────────────────────────

fn extract_opt_str<'a>(args: &'a serde_json::Value, key: &str) -> Option<&'a str> {
    args.get(key).and_then(serde_json::Value::as_str)
}

#[expect(
    clippy::result_large_err,
    reason = "ToolResult grew by receipt field; boxing would change public API"
)]
fn extract_str<'a>(
    args: &'a serde_json::Value,
    key: &str,
) -> std::result::Result<&'a str, ToolResult> {
    args.get(key)
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| ToolResult::error(format!("missing required argument: {key}")))
}

const DEFAULT_TYPST_TEMPLATE: &str = include_str!("../../../poiesis/typst/templates/default.typ");
const TYPST_CHART_APPENDIX: &str = r#"
#if "chart_svg" in data [
  #v(16pt)
  #align(center)[
    #image(bytes(data.chart_svg), format: "svg", width: 100%)
  ]
]
"#;

pub(crate) fn json_data_property(description: &str) -> PropertyDef {
    PropertyDef {
        property_type: PropertyType::Object,
        description: format!("{description} Also accepts a JSON string for legacy callers."),
        enum_values: None,
        default: None,
        ..Default::default(),
    }
}

fn typst_template_enum_values() -> Vec<String> {
    poiesis_typst::templates::SLUGS
        .iter()
        .map(|slug| (*slug).to_owned())
        .collect()
}

fn render_chart_svg(
    data: &serde_json::Value,
    theme: &ResolvedTheme,
) -> std::result::Result<Option<String>, String> {
    let Some(chart_value) = data.get("chart") else {
        return Ok(None);
    };

    let chart: Chart = serde_json::from_value(chart_value.clone())
        .map_err(|e| format!("chart must be valid JSON: {e}"))?;
    let chart_theme = ChartResolvedTheme::from_poiesis_theme(theme);
    let svg = render_chart(
        &chart,
        &chart_theme,
        &Canvas::Doc(DocCanvas::default()),
        ChartColorMode::Resolved,
    )
    .map_err(|e| format!("chart render failed: {e}"))?;
    Ok(Some(svg))
}

/// Extract an optional theme identifier from the parsed `data` object.
///
/// Looks for `theme`, `theme_id`, or `spec.theme` so callers can embed the
/// theme choice in their report descriptor without adding a top-level argument.
fn theme_id_from_data(data: &serde_json::Value) -> Option<String> {
    data.as_object().and_then(|obj| {
        obj.get("theme")
            .or_else(|| obj.get("theme_id"))
            .or_else(|| obj.get("spec").and_then(|spec| spec.get("theme")))
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned)
    })
}

/// Resolve the report theme from tool inputs.
///
/// Resolution order:
/// 1. Top-level `theme` argument.
/// 2. `theme` / `theme_id` / `spec.theme` inside the parsed `data` object.
/// 3. Embedded `summus()` fallback for backward compatibility.
///
/// Non-`summus` identifiers are parsed and resolved against a theme registry
/// loaded from `<workspace>/themes`. Errors are returned as `ToolResult::error`
/// so executors can return them directly.
pub(crate) fn resolve_report_theme(
    args: &serde_json::Value,
    data: &serde_json::Value,
    ctx: &ToolContext,
) -> std::result::Result<ResolvedTheme, Box<ToolResult>> {
    let candidate = extract_opt_str(args, "theme")
        .map(str::to_owned)
        .or_else(|| theme_id_from_data(data))
        .filter(|s| !s.is_empty());

    let Some(candidate) = candidate else {
        return Ok(summus());
    };

    if candidate == "summus" {
        return Ok(summus());
    }

    let id = ThemeId::parse(&candidate).map_err(|e| {
        Box::new(ToolResult::error(format!(
            "invalid theme id '{candidate}': {e}"
        )))
    })?;

    let themes_dir = ctx.workspace.join("themes");
    if !themes_dir.exists() {
        return Err(Box::new(ToolResult::error(format!(
            "themes registry directory not found at {}",
            themes_dir.display()
        ))));
    }

    let registry = Registry::load_dir(&themes_dir).map_err(|e| {
        Box::new(ToolResult::error(format!(
            "failed to load themes registry from {}: {e}",
            themes_dir.display()
        )))
    })?;

    registry.resolve(&id).map_err(|e| {
        Box::new(ToolResult::error(format!(
            "failed to resolve theme {id}: {e}"
        )))
    })
}

pub(crate) fn extract_zip_entry(
    zip_bytes: &[u8],
    name: &str,
) -> std::result::Result<Vec<u8>, String> {
    let cursor = Cursor::new(zip_bytes);
    let mut archive = ZipArchive::new(cursor).map_err(|e| format!("failed to open zip: {e}"))?;
    let mut file = archive
        .by_name(name)
        .map_err(|e| format!("missing zip entry {name}: {e}"))?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)
        .map_err(|e| format!("failed to read zip entry {name}: {e}"))?;
    Ok(bytes)
}

pub(crate) fn rewrite_zip(
    original: &[u8],
    replacements: &[(&str, &[u8])],
) -> std::result::Result<Vec<u8>, String> {
    let cursor = Cursor::new(original);
    let mut archive = ZipArchive::new(cursor).map_err(|e| format!("failed to open zip: {e}"))?;
    let mut output = ZipWriter::new(Cursor::new(Vec::new()));
    let mut remaining = std::collections::BTreeMap::new();
    for (name, bytes) in replacements {
        remaining.insert(*name, *bytes);
    }

    for idx in 0..archive.len() {
        let mut file = archive
            .by_index(idx)
            .map_err(|e| format!("failed to read zip entry #{idx}: {e}"))?;
        let name = file.name().to_owned();
        if name.ends_with('/') {
            continue;
        }
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes)
            .map_err(|e| format!("failed to read zip entry {name}: {e}"))?;
        let payload = remaining.remove(name.as_str()).unwrap_or(bytes.as_slice());
        output
            .start_file(&name, SimpleFileOptions::default())
            .map_err(|e| format!("failed to write zip entry {name}: {e}"))?;
        output
            .write_all(payload)
            .map_err(|e| format!("failed to write zip entry {name}: {e}"))?;
    }

    for (name, bytes) in remaining {
        output
            .start_file(name, SimpleFileOptions::default())
            .map_err(|e| format!("failed to write zip entry {name}: {e}"))?;
        output
            .write_all(bytes)
            .map_err(|e| format!("failed to write zip entry {name}: {e}"))?;
    }

    let cursor = output
        .finish()
        .map_err(|e| format!("failed to finish zip: {e}"))?;
    Ok(cursor.into_inner())
}

async fn write_document_out_path(
    out_path: &str,
    ctx: &ToolContext,
    tool_name: &koina::id::ToolName,
    bytes: &[u8],
) -> Option<ToolResult> {
    let validated = match validate_path(out_path, ctx, tool_name) {
        Ok(path) => path,
        Err(e) => {
            return Some(ToolResult::error(format!(
                "invalid out_path {out_path:?}: {e}"
            )));
        }
    };
    if let Err(e) = tokio::fs::write(&validated, bytes).await {
        return Some(ToolResult::error(format!(
            "wrote 0 bytes to {}: {e}",
            validated.display()
        )));
    }
    None
}

fn build_document_result(format: &str, bytes: &[u8]) -> ToolResult {
    let encoded = koina::base64::encode(bytes);
    let summary = format!(
        "Generated {} document: {} bytes",
        format.to_uppercase(),
        bytes.len()
    );
    let media_type = media_type_for_format(format).to_owned();
    ToolResult::blocks(vec![
        ToolResultBlock::Text { text: summary },
        ToolResultBlock::Document {
            source: DocumentSource {
                source_type: "base64".to_owned(),
                media_type,
                data: encoded,
            },
        },
    ])
}

// ── generate_document ─────────────────────────────────────────────────────────

struct GenerateDocumentExecutor;

impl ToolExecutor for GenerateDocumentExecutor {
    fn execute<'a>(
        &'a self,
        input: &'a ToolInput,
        ctx: &'a ToolContext,
    ) -> Pin<Box<dyn Future<Output = Result<ToolResult>> + Send + 'a>> {
        Box::pin(async move {
            let args = &input.arguments;

            let title = extract_opt_str(args, "title").unwrap_or("Untitled Document");
            let author = extract_opt_str(args, "author");
            let format = extract_opt_str(args, "format").unwrap_or("odt");
            let out_path = extract_opt_str(args, "out_path");
            let content_str = match extract_str(args, "content") {
                Ok(s) => s,
                Err(e) => return Ok(e),
            };

            let raw_blocks: Vec<serde_json::Value> = match serde_json::from_str(content_str) {
                Ok(v) => v,
                Err(e) => {
                    return Ok(ToolResult::error(format!(
                        "content must be a JSON array of block objects: {e}"
                    )));
                }
            };

            let blocks: Vec<Block> = match raw_blocks
                .iter()
                .enumerate()
                .map(|(i, v)| parse_block(v, i))
                .collect::<std::result::Result<Vec<_>, _>>()
            {
                Ok(b) => b,
                Err(e) => return Ok(ToolResult::error(e)),
            };

            let doc = Document {
                metadata: Metadata {
                    title: title.to_owned(),
                    author: author.map(str::to_owned),
                    created: None,
                },
                content: blocks,
            };

            let format = format.to_lowercase();
            // WHY: every renderer below blocks (pandoc/typst subprocesses,
            // file IO); run on the blocking pool so a slow render occupies a
            // blocking thread instead of pinning a tokio runtime worker.
            let render_format = format.clone();
            let render_result = tokio::task::spawn_blocking(move || match render_format.as_str() {
                "xlsx" => poiesis_sheet::XlsxRenderer::new()
                    .render(&doc)
                    .map_err(|e| format!("XLSX render failed: {e}")),
                "pdf" => poiesis_doc::render_pdf_from_doc(&doc)
                    .map_err(|e| format!("PDF render failed: {e}")),
                "odt" => poiesis_doc::render_odt_from_doc(&doc)
                    .map_err(|e| format!("ODT render failed: {e}")),
                "docx" => poiesis_doc::render_docx_from_doc(&doc)
                    .map_err(|e| format!("DOCX render failed: {e}")),
                "html" => poiesis_doc::render_html_from_doc(&doc)
                    .map_err(|e| format!("HTML render failed: {e}")),
                "md" => poiesis_doc::render_md_from_doc(&doc)
                    .map_err(|e| format!("MD render failed: {e}")),
                "latex" => poiesis_doc::render_latex_from_doc(&doc)
                    .map_err(|e| format!("LATEX render failed: {e}")),
                "epub" => poiesis_doc::render_epub_from_doc(&doc)
                    .map_err(|e| format!("EPUB render failed: {e}")),
                other => Err(format!(
                    "unsupported format '{other}'; supported formats are: odt, docx, html, md, \
                     latex, epub, pdf, xlsx"
                )),
            })
            .await;
            let bytes = match render_result {
                Ok(Ok(b)) => b,
                Ok(Err(msg)) => return Ok(ToolResult::error(msg)),
                Err(e) => {
                    return Ok(ToolResult::error(format!("render task failed: {e}")));
                }
            };

            // NOTE: Write to a caller-provided path in addition to returning bytes.
            if let Some(out_path) = out_path
                && let Some(err) = write_document_out_path(out_path, ctx, &input.name, &bytes).await
            {
                return Ok(err);
            }

            Ok(build_document_result(&format, &bytes))
        })
    }
}

/// Parse a JSON block object into a `poiesis_core::Block`.
fn parse_block(v: &serde_json::Value, index: usize) -> std::result::Result<Block, String> {
    let kind = v
        .get("type")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| format!("block {index}: missing or non-string 'type' field"))?;
    match kind {
        "heading" => {
            let level = v
                .get("level")
                .and_then(serde_json::Value::as_u64)
                .and_then(|n| u8::try_from(n.min(6)).ok())
                .unwrap_or(1);
            let text = plain(
                v.get("text")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or(""),
            );
            Ok(Block::Heading { level, text })
        }
        "paragraph" => {
            let text = plain(
                v.get("text")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or(""),
            );
            Ok(Block::Paragraph(text))
        }
        "page_break" => Ok(Block::PageBreak),
        other => Err(format!("block {index}: unsupported block type '{other}'")),
    }
}

fn plain(s: &str) -> RichText {
    RichText {
        spans: vec![Span::Plain(s.to_owned())],
    }
}

/// Map a `generate_document` format name to its MIME/media type.
pub(crate) fn media_type_for_format(format: &str) -> &'static str {
    match format {
        "xlsx" => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        "pdf" => "application/pdf",
        "odt" => "application/vnd.oasis.opendocument.text",
        "docx" => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        "html" => "text/html",
        "md" => "text/markdown",
        "latex" => "application/x-latex",
        "epub" => "application/epub+zip",
        _ => "application/octet-stream",
    }
}

fn generate_document_def() -> ToolDef {
    ToolDef {
        name: koina::id::ToolName::from_static("generate_document"), // kanon:ignore RUST/expect
        description: "Render a document descriptor to ODT, XLSX, PDF, DOCX, HTML, MD, LaTeX, or EPUB bytes.".to_owned(),
        extended_description: None,
        input_schema: InputSchema {
            properties: IndexMap::from([
                (
                    "content".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::String,
                        description:
                            "JSON array of block objects (each with type, text, level fields)"
                                .to_owned(),
                        enum_values: None,
                        default: None,
                        ..Default::default(),
                    },
                ),
                (
                    "format".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::String,
                        description:
                            "Output format: odt (default), docx, html, md, latex, epub, pdf, or xlsx"
                                .to_owned(),
                        enum_values: Some(vec![
                            "odt".to_owned(),
                            "docx".to_owned(),
                            "html".to_owned(),
                            "md".to_owned(),
                            "latex".to_owned(),
                            "epub".to_owned(),
                            "pdf".to_owned(),
                            "xlsx".to_owned(),
                        ]),
                        default: Some(serde_json::json!("odt")),
                        ..Default::default(),
                    },
                ),
                (
                    "title".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::String,
                        description: "Document title (default: Untitled Document)".to_owned(),
                        enum_values: None,
                        default: None,
                        ..Default::default(),
                    },
                ),
                (
                    "author".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::String,
                        description: "Document author (optional)".to_owned(),
                        enum_values: None,
                        default: None,
                        ..Default::default(),
                    },
                ),
                (
                    "out_path".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::String,
                        description:
                            "Optional filesystem path to write the rendered document to, in addition \
                             to returning base64 bytes."
                                .to_owned(),
                        enum_values: None,
                        default: None,
                    },
                ),
            ]),
            required: vec!["content".to_owned()],
        },
        category: ToolCategory::Workspace,
        reversibility: Reversibility::FullyReversible,
        auto_activate: false,
        groups: vec![ToolGroupId::Edit],
        tags: vec![ToolTag::Format],
    }
}

fn generate_document_capability_rule() -> ToolCallCapabilityRule {
    ToolCallCapabilityRule::argument_presence(
        "out_path",
        ToolCallCapability::new(vec![ToolGroupId::Edit], Reversibility::PartiallyReversible),
        ToolCallCapability::new(vec![ToolGroupId::Read], Reversibility::FullyReversible),
    )
}

fn render_typst_report_capability_rule() -> ToolCallCapabilityRule {
    ToolCallCapabilityRule::argument_presence(
        "out_path",
        ToolCallCapability::new(vec![ToolGroupId::Edit], Reversibility::PartiallyReversible),
        ToolCallCapability::new(vec![ToolGroupId::Read], Reversibility::FullyReversible),
    )
}

// ── lint_report ───────────────────────────────────────────────────────────────

struct LintReportExecutor;

impl ToolExecutor for LintReportExecutor {
    fn execute<'a>(
        &'a self,
        input: &'a ToolInput,
        _ctx: &'a ToolContext,
    ) -> Pin<Box<dyn Future<Output = Result<ToolResult>> + Send + 'a>> {
        Box::pin(async move {
            let args = &input.arguments;
            let json_output = args
                .get("json")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false);

            // Accept either inline text or a file path.
            let text_owned: String;
            let text: &str = if let Some(t) = extract_opt_str(args, "text") {
                t
            } else if let Some(path_str) = extract_opt_str(args, "path") {
                match std::fs::read_to_string(path_str) {
                    Ok(s) => {
                        text_owned = s;
                        &text_owned
                    }
                    Err(e) => {
                        return Ok(ToolResult::error(format!(
                            "failed to read {path_str:?}: {e}"
                        )));
                    }
                }
            } else {
                return Ok(ToolResult::error(
                    "lint_report requires 'text' or 'path'".to_owned(),
                ));
            };

            let linter = Linter::new(LintConfig::default());
            let findings = linter.check(text);

            if json_output {
                match Linter::to_json(&findings) {
                    Ok(json) => return Ok(ToolResult::text(json)),
                    Err(e) => {
                        return Ok(ToolResult::error(format!("serialize failed: {e}")));
                    }
                }
            }

            if findings.is_empty() {
                return Ok(ToolResult::text("LINT: no findings".to_owned()));
            }

            let mut lines: Vec<String> = Vec::with_capacity(findings.len());
            for f in &findings {
                if f.line_start == f.line_end {
                    lines.push(format!("LINT: line {}: {}", f.line_start, f.message));
                } else {
                    lines.push(format!(
                        "LINT: lines {}-{}: {}",
                        f.line_start, f.line_end, f.message
                    ));
                }
            }
            Ok(ToolResult::text(lines.join("\n")))
        })
    }
}

fn lint_report_def() -> ToolDef {
    ToolDef {
        name: koina::id::ToolName::from_static("lint_report"), // kanon:ignore RUST/expect
        description: "Check report prose quality: banned words, citation coverage, structure."
            .to_owned(),
        extended_description: None,
        input_schema: InputSchema {
            properties: IndexMap::from([
                (
                    "text".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::String,
                        description: "Report text to lint (inline)".to_owned(),
                        enum_values: None,
                        default: None,
                        ..Default::default(),
                    },
                ),
                (
                    "path".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::String,
                        description: "Path to report file to lint".to_owned(),
                        enum_values: None,
                        default: None,
                        ..Default::default(),
                    },
                ),
                (
                    "json".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::Boolean,
                        description:
                            "Return findings as JSON array instead of human-readable lines"
                                .to_owned(),
                        enum_values: None,
                        default: Some(serde_json::json!(false)),
                        ..Default::default(),
                    },
                ),
            ]),
            required: vec![],
        },
        category: ToolCategory::Workspace,
        reversibility: Reversibility::FullyReversible,
        auto_activate: false,
        groups: vec![ToolGroupId::Verify],
        tags: vec![ToolTag::Verify, ToolTag::Format],
    }
}

// ── verify_report ─────────────────────────────────────────────────────────────

struct VerifyReportExecutor;

impl ToolExecutor for VerifyReportExecutor {
    fn execute<'a>(
        &'a self,
        input: &'a ToolInput,
        _ctx: &'a ToolContext,
    ) -> Pin<Box<dyn Future<Output = Result<ToolResult>> + Send + 'a>> {
        Box::pin(async move {
            let args = &input.arguments;

            let verifier = Verifier::new();

            let results = if let Some(manifest_str) = extract_opt_str(args, "manifest") {
                let manifest: poiesis_verify::VerifyManifest =
                    match serde_json::from_str(manifest_str) {
                        Ok(m) => m,
                        Err(e) => {
                            return Ok(ToolResult::error(format!(
                                "failed to parse manifest JSON: {e}"
                            )));
                        }
                    };
                verifier.verify(&manifest)
            } else if let Some(path_str) = extract_opt_str(args, "path") {
                let path = std::path::Path::new(path_str);
                match verifier.verify_file(path) {
                    Ok(r) => r,
                    Err(e) => {
                        return Ok(ToolResult::error(format!(
                            "failed to verify manifest at {path_str:?}: {e}"
                        )));
                    }
                }
            } else {
                return Ok(ToolResult::error(
                    "verify_report requires 'manifest' (inline JSON) or 'path'".to_owned(),
                ));
            };

            let summary = poiesis_verify::VerifyResult::from_claims(results);
            let any_failed = summary.any_failed();

            match serde_json::to_string_pretty(&summary) {
                Ok(json) => {
                    if any_failed {
                        Ok(ToolResult::error(format!(
                            "VERIFY FAILED: {}/{} claims passed\n{json}",
                            summary.passed, summary.total
                        )))
                    } else {
                        Ok(ToolResult::text(format!(
                            "VERIFY PASSED: {}/{} claims passed\n{json}",
                            summary.passed, summary.total
                        )))
                    }
                }
                Err(e) => Ok(ToolResult::error(format!("serialize failed: {e}"))),
            }
        })
    }
}

fn verify_report_def() -> ToolDef {
    ToolDef {
        name: koina::id::ToolName::from_static("verify_report"), // kanon:ignore RUST/expect
        description:
            "Validate numeric claims in a verify manifest against derived and reference sources."
                .to_owned(),
        extended_description: None,
        input_schema: InputSchema {
            properties: IndexMap::from([
                (
                    "manifest".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::String,
                        description: "Inline JSON verify manifest string".to_owned(),
                        enum_values: None,
                        default: None,
                        ..Default::default(),
                    },
                ),
                (
                    "path".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::String,
                        description: "Path to a verify manifest JSON file".to_owned(),
                        enum_values: None,
                        default: None,
                        ..Default::default(),
                    },
                ),
            ]),
            required: vec![],
        },
        category: ToolCategory::Workspace,
        reversibility: Reversibility::FullyReversible,
        auto_activate: false,
        groups: vec![ToolGroupId::Verify],
        tags: vec![ToolTag::Verify, ToolTag::Format],
    }
}

// ── render_typst_report ───────────────────────────────────────────────────────

struct RenderTypstReportExecutor;

impl ToolExecutor for RenderTypstReportExecutor {
    #[expect(
        clippy::too_many_lines,
        reason = "tool executor wires the full report render path"
    )]
    fn execute<'a>(
        &'a self,
        input: &'a ToolInput,
        ctx: &'a ToolContext,
    ) -> Pin<Box<dyn Future<Output = Result<ToolResult>> + Send + 'a>> {
        Box::pin(async move {
            let args = &input.arguments;

            // NOTE: Optional inline JSON data; accepts either a structured object or a JSON string.
            let mut data: serde_json::Value = match args.get("data") {
                None => serde_json::json!({}),
                Some(serde_json::Value::Object(_)) => match args.get("data").cloned() {
                    Some(value) => value,
                    None => {
                        return Ok(ToolResult::error(
                            "data object must be present after lookup".to_owned(),
                        ));
                    }
                },
                Some(serde_json::Value::String(raw)) => match serde_json::from_str(raw) {
                    Ok(v) => v,
                    Err(e) => {
                        return Ok(ToolResult::error(format!("data must be valid JSON: {e}")));
                    }
                },
                Some(_) => {
                    return Ok(ToolResult::error(
                        "data must be a JSON object or JSON string".to_owned(),
                    ));
                }
            };

            let theme = match resolve_report_theme(args, &data, ctx) {
                Ok(theme) => theme,
                Err(e) => return Ok(*e),
            };

            if let Some(chart_svg) = match render_chart_svg(&data, &theme) {
                Ok(svg) => svg,
                Err(e) => return Ok(ToolResult::error(e)),
            } && let Some(obj) = data.as_object_mut()
            {
                obj.insert("chart_svg".to_owned(), serde_json::Value::String(chart_svg));
            }

            let theme_source = match emit_typst_template(&theme) {
                Ok(source) => source,
                Err(e) => {
                    return Ok(ToolResult::error(format!("theme typst sink failed: {e}")));
                }
            };

            // NOTE: Inline `source` wins over the `template` slug.
            let pdf_result = if let Some(source) = extract_opt_str(args, "source") {
                let mut source_text = String::with_capacity(theme_source.len() + source.len() + 2);
                source_text.push_str(&theme_source);
                source_text.push_str("\n\n");
                source_text.push_str(source);
                poiesis_typst::render_typst(&source_text, &data)
            } else if let Some(slug) = extract_opt_str(args, "template") {
                match slug {
                    "default" => {
                        let mut source_text = String::with_capacity(
                            theme_source.len() + DEFAULT_TYPST_TEMPLATE.len() + 64,
                        );
                        source_text.push_str(&theme_source);
                        source_text.push_str("\n\n");
                        source_text.push_str(DEFAULT_TYPST_TEMPLATE);
                        if data.get("chart_svg").is_some() {
                            source_text.push_str(TYPST_CHART_APPENDIX);
                        }
                        poiesis_typst::render_typst(&source_text, &data)
                    }
                    other => poiesis_typst::render_template(other, &data),
                }
            } else {
                return Ok(ToolResult::error(
                    "render_typst_report requires 'source' (inline Typst) or 'template' (slug)"
                        .to_owned(),
                ));
            };

            let pdf_bytes = match pdf_result {
                Ok(b) => b,
                Err(e) => {
                    return Ok(ToolResult::error(format!("typst render failed: {e}")));
                }
            };

            // NOTE: Write to a caller-provided path in addition to returning bytes.
            if let Some(out_path) = extract_opt_str(args, "out_path") {
                let validated = match validate_path(out_path, ctx, &input.name) {
                    Ok(path) => path,
                    Err(e) => {
                        return Ok(ToolResult::error(format!(
                            "invalid out_path {out_path:?}: {e}"
                        )));
                    }
                };
                if let Err(e) = tokio::fs::write(&validated, &pdf_bytes).await {
                    return Ok(ToolResult::error(format!(
                        "wrote 0 bytes to {}: {e}",
                        validated.display()
                    )));
                }
            }

            let encoded = koina::base64::encode(&pdf_bytes);
            let summary = format!("Rendered Typst report: {} bytes PDF", pdf_bytes.len());

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

fn render_typst_report_def() -> ToolDef {
    ToolDef {
        name: koina::id::ToolName::from_static("render_typst_report"), // kanon:ignore RUST/expect
        description: "Render a Typst source string (or built-in template slug) to a PDF, \
                      with optional JSON data injected at the virtual path `data.json` and \
                      an embedded chart payload when present."
            .to_owned(),
        extended_description: Some(
            "Pass either `source` (inline Typst markup) or `template` (one of the built-in \
             slugs, currently: `default`). The JSON blob passed as `data` is exposed to the \
             Typst document as a virtual file read via `json(\"data.json\")`. When `data.chart` \
             is present, the executor renders it through poiesis-charts and appends the SVG to \
             the default template. The result contains a text summary plus a base64-encoded PDF \
             document block; optionally also writes the PDF to `out_path` on the filesystem."
                .to_owned(),
        ),
        input_schema: InputSchema {
            properties: IndexMap::from([
                (
                    "source".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::String,
                        description: "Inline Typst source. Mutually exclusive with `template`."
                            .to_owned(),
                        enum_values: None,
                        default: None,
                        ..Default::default(),
                    },
                ),
                (
                    "template".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::String,
                        description: "Built-in template slug (e.g. `default`).".to_owned(),
                        enum_values: Some(typst_template_enum_values()),
                        default: None,
                        ..Default::default(),
                    },
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
                        ..Default::default(),
                    },
                ),
                (
                    "data".to_owned(),
                    json_data_property("JSON object exposed to the template as `data.json`."),
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
            required: vec![],
        },
        category: ToolCategory::Workspace,
        reversibility: Reversibility::PartiallyReversible,
        auto_activate: false,
        groups: vec![ToolGroupId::Edit],
        tags: vec![ToolTag::Format],
    }
}

fn walk_json_strings(
    value: &serde_json::Value,
    pointer: &str,
    visitor: &mut impl FnMut(&str, &str),
) {
    match value {
        serde_json::Value::String(s) => visitor(pointer, s),
        serde_json::Value::Array(arr) => {
            for (i, item) in arr.iter().enumerate() {
                let child = format!("{pointer}/{i}");
                walk_json_strings(item, &child, visitor);
            }
        }
        serde_json::Value::Object(obj) => {
            for (key, item) in obj {
                let child = if pointer.is_empty() {
                    format!("/{key}")
                } else {
                    format!("{pointer}/{key}")
                };
                walk_json_strings(item, &child, visitor);
            }
        }
        _ => {}
    }
}

fn looks_like_token_ref(value: &str) -> bool {
    let parts: Vec<&str> = value.split('.').collect();
    matches!(
        parts.as_slice(),
        ["color" | "type", "role", _]
            | ["color", "tone" | "surface", _]
            | ["type", "family" | "scale", _]
            | ["space", _]
            | ["chart", "series", _]
    )
}

fn theme_violation_to_qa_issue(v: &poiesis_theme::lint::Violation) -> QaIssue {
    QaIssue {
        kind: QaIssueKind::ThemeViolation,
        location: Some(v.pointer.clone()),
        message: format!("[{}] {}", v.rule_id, v.message),
    }
}

// ── qa_gate ───────────────────────────────────────────────────────────────────

struct QaGateExecutor;

impl ToolExecutor for QaGateExecutor {
    fn execute<'a>(
        &'a self,
        input: &'a ToolInput,
        _ctx: &'a ToolContext,
    ) -> Pin<Box<dyn Future<Output = Result<ToolResult>> + Send + 'a>> {
        Box::pin(async move {
            let args = &input.arguments;

            let prose = match extract_str(args, "prose") {
                Ok(s) => s,
                Err(e) => return Ok(e),
            };

            let mut issues: Vec<QaIssue> = Vec::new();

            // 1. Optional factbase validation
            if let Some(fb_json) = extract_opt_str(args, "factbase_json") {
                let fb: Factbase = match serde_json::from_str(fb_json) {
                    Ok(fb) => fb,
                    Err(e) => {
                        return Ok(ToolResult::error(format!(
                            "failed to parse factbase_json: {e}"
                        )));
                    }
                };
                if let Err(e) = fb.validate() {
                    issues.push(QaIssue {
                        kind: QaIssueKind::CitationUnresolvable,
                        location: None,
                        message: e.to_string(),
                    });
                }
            }

            // 2. Prose lint
            let linter = Linter::default();
            let findings = linter.check(prose);
            for finding in &findings {
                issues.push(QaIssue {
                    kind: QaIssueKind::ProseViolation,
                    location: Some(format!("line {}–{}", finding.line_start, finding.line_end)),
                    message: finding.message.clone(),
                });
            }

            let theme = summus();
            let color_rule = RawColorLiteralRule;
            let font_rule = RawFontLiteralRule;
            let unknown_rule = UnknownTokenRule::new(&theme);

            let mut visit_string = |pointer: &str, value: &str| {
                for v in color_rule.scan(pointer, value) {
                    issues.push(theme_violation_to_qa_issue(&v));
                }
                for v in font_rule.scan(pointer, value) {
                    issues.push(theme_violation_to_qa_issue(&v));
                }
                if looks_like_token_ref(value)
                    && let Some(v) = unknown_rule.check(pointer, value)
                {
                    issues.push(theme_violation_to_qa_issue(&v));
                }
            };

            if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(prose) {
                walk_json_strings(&parsed, "", &mut visit_string);
            } else {
                visit_string("", prose);
            }

            let report = QaReport::new(issues);

            match QaReport::to_json(&report) {
                Ok(json) => Ok(ToolResult::text(json)),
                Err(e) => Ok(ToolResult::error(format!("serialize failed: {e}"))),
            }
        })
    }
}

fn qa_gate_def() -> ToolDef {
    ToolDef {
        name: koina::id::ToolName::from_static("qa_gate"), // kanon:ignore RUST/expect
        description:
            "Run prose lint, theme token lint, and optional factbase validation, returning a structured QA report."
                .to_owned(),
        extended_description: None,
        input_schema: InputSchema {
            properties: IndexMap::from([
                (
                    "prose".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::String,
                        description: "Document text to lint".to_owned(),
                        enum_values: None,
                        default: None,
                        ..Default::default(),
                    },
                ),
                (
                    "factbase_json".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::String,
                        description: "Optional JSON-serialized Factbase to validate".to_owned(),
                        enum_values: None,
                        default: None,
                        ..Default::default(),
                    },
                ),
            ]),
            required: vec!["prose".to_owned()],
        },
        category: ToolCategory::Workspace,
        reversibility: Reversibility::FullyReversible,
        auto_activate: false,
        groups: vec![ToolGroupId::Verify],
        tags: vec![ToolTag::Verify, ToolTag::Format],
    }
}

// ── Registration ──────────────────────────────────────────────────────────────

/// Register the poiesis report tools: `generate_document`, `lint_report`, `verify_report`,
/// `render_typst_report`, `qa_gate`.
pub(crate) fn register(registry: &mut ToolRegistry) -> Result<()> {
    registry.register_with_call_capability(
        generate_document_def(),
        generate_document_capability_rule(),
        Box::new(GenerateDocumentExecutor),
    )?;
    registry.register(lint_report_def(), Box::new(LintReportExecutor))?;
    registry.register(verify_report_def(), Box::new(VerifyReportExecutor))?;
    registry.register_with_call_capability(
        render_typst_report_def(),
        render_typst_report_capability_rule(),
        Box::new(RenderTypstReportExecutor),
    )?;
    registry.register(qa_gate_def(), Box::new(QaGateExecutor))?;
    Ok(())
}

#[cfg(test)]
#[path = "poiesis_tests.rs"]
mod tests;

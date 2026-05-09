//! Poiesis report tools: `generate_document`, `lint_report`, `verify_report`,
//! `render_typst_report`.
//!
//! - `generate_document`    — render a JSON document descriptor to ODT/XLSX/PDF bytes
//! - `lint_report`          — check report prose quality (banned words, citations, structure)
//! - `verify_report`        — validate numeric claims in a verify manifest
//! - `render_typst_report`  — render a Typst source (or built-in template slug) to PDF

use std::future::Future;
use std::pin::Pin;

use hermeneus::types::{DocumentSource, ToolResultBlock};
use indexmap::IndexMap;
use poiesis_core::{Block, Document, Metadata, Renderer, RichText, Span};
use poiesis_lint::{LintConfig, Linter};
use poiesis_verify::Verifier;

use crate::builtins::workspace::validate_path;
use crate::error::Result;
use crate::registry::{ToolExecutor, ToolRegistry};
use crate::types::{
    InputSchema, PropertyDef, PropertyType, Reversibility, ToolCategory, ToolContext, ToolDef,
    ToolGroupId, ToolInput, ToolResult, ToolTag,
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

// ── generate_document ─────────────────────────────────────────────────────────

struct GenerateDocumentExecutor;

impl ToolExecutor for GenerateDocumentExecutor {
    fn execute<'a>(
        &'a self,
        input: &'a ToolInput,
        _ctx: &'a ToolContext,
    ) -> Pin<Box<dyn Future<Output = Result<ToolResult>> + Send + 'a>> {
        Box::pin(async move {
            let args = &input.arguments;

            let title = extract_opt_str(args, "title").unwrap_or("Untitled Document");
            let author = extract_opt_str(args, "author");
            let format = extract_opt_str(args, "format").unwrap_or("odt");
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

            let bytes = match format.to_lowercase().as_str() {
                "xlsx" => {
                    let renderer = poiesis_sheet::XlsxRenderer::new();
                    match renderer.render(&doc) {
                        Ok(b) => b,
                        Err(e) => {
                            return Ok(ToolResult::error(format!("XLSX render failed: {e}")));
                        }
                    }
                }
                "pdf" => match poiesis_text::PdfRenderer::new() {
                    Ok(renderer) => match renderer.render(&doc) {
                        Ok(b) => b,
                        Err(e) => {
                            return Ok(ToolResult::error(format!(
                                "PDF render failed (falling back): {e}"
                            )));
                        }
                    },
                    Err(e) => {
                        return Ok(ToolResult::error(format!(
                            "PDF renderer unavailable (no system font?): {e}"
                        )));
                    }
                },
                // Default: ODT
                _ => {
                    let renderer = poiesis_text::OdtRenderer::new();
                    match renderer.render(&doc) {
                        Ok(b) => b,
                        Err(e) => {
                            return Ok(ToolResult::error(format!("ODT render failed: {e}")));
                        }
                    }
                }
            };

            Ok(ToolResult::text(format!(
                "Generated {} document: {} bytes",
                format.to_uppercase(),
                bytes.len()
            )))
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

fn generate_document_def() -> ToolDef {
    ToolDef {
        name: koina::id::ToolName::from_static("generate_document"), // kanon:ignore RUST/expect
        description: "Render a document descriptor to ODT, XLSX, or PDF bytes.".to_owned(),
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
                    },
                ),
                (
                    "format".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::String,
                        description: "Output format: odt (default), xlsx, or pdf".to_owned(),
                        enum_values: Some(vec![
                            "odt".to_owned(),
                            "xlsx".to_owned(),
                            "pdf".to_owned(),
                        ]),
                        default: Some(serde_json::json!("odt")),
                    },
                ),
                (
                    "title".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::String,
                        description: "Document title (default: Untitled Document)".to_owned(),
                        enum_values: None,
                        default: None,
                    },
                ),
                (
                    "author".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::String,
                        description: "Document author (optional)".to_owned(),
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
                    },
                ),
                (
                    "path".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::String,
                        description: "Path to report file to lint".to_owned(),
                        enum_values: None,
                        default: None,
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
                    },
                ),
                (
                    "path".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::String,
                        description: "Path to a verify manifest JSON file".to_owned(),
                        enum_values: None,
                        default: None,
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
    fn execute<'a>(
        &'a self,
        input: &'a ToolInput,
        ctx: &'a ToolContext,
    ) -> Pin<Box<dyn Future<Output = Result<ToolResult>> + Send + 'a>> {
        Box::pin(async move {
            let args = &input.arguments;

            // Optional inline JSON data; defaults to empty object.
            let data: serde_json::Value = if let Some(raw) = extract_opt_str(args, "data") {
                match serde_json::from_str(raw) {
                    Ok(v) => v,
                    Err(e) => {
                        return Ok(ToolResult::error(format!("data must be valid JSON: {e}")));
                    }
                }
            } else {
                serde_json::json!({})
            };

            // Choose source: inline `source` wins over `template` slug.
            let pdf_result = if let Some(source) = extract_opt_str(args, "source") {
                poiesis_typst::render_typst(source, &data)
            } else if let Some(slug) = extract_opt_str(args, "template") {
                poiesis_typst::render_template(slug, &data)
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
                      with optional JSON data injected at the virtual path `data.json`."
            .to_owned(),
        extended_description: Some(
            "Pass either `source` (inline Typst markup) or `template` (one of the built-in \
             slugs, currently: `default`). The JSON blob passed as `data` is exposed to the \
             Typst document as a virtual file read via `json(\"data.json\")`. The result \
             contains a text summary plus a base64-encoded PDF document block; optionally \
             also writes the PDF to `out_path` on the filesystem."
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
                    },
                ),
                (
                    "template".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::String,
                        description: "Built-in template slug (e.g. `default`).".to_owned(),
                        enum_values: Some(vec!["default".to_owned()]),
                        default: None,
                    },
                ),
                (
                    "data".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::String,
                        description:
                            "Inline JSON data blob exposed to the template as `data.json`."
                                .to_owned(),
                        enum_values: None,
                        default: None,
                    },
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
                    },
                ),
            ]),
            required: vec![],
        },
        category: ToolCategory::Workspace,
        reversibility: Reversibility::FullyReversible,
        auto_activate: false,
        groups: vec![ToolGroupId::Edit],
        tags: vec![ToolTag::Format],
    }
}

// ── Registration ──────────────────────────────────────────────────────────────

/// Register the poiesis report tools: `generate_document`, `lint_report`, `verify_report`,
/// `render_typst_report`.
pub(crate) fn register(registry: &mut ToolRegistry) -> Result<()> {
    registry.register(generate_document_def(), Box::new(GenerateDocumentExecutor))?;
    registry.register(lint_report_def(), Box::new(LintReportExecutor))?;
    registry.register(verify_report_def(), Box::new(VerifyReportExecutor))?;
    registry.register(
        render_typst_report_def(),
        Box::new(RenderTypstReportExecutor),
    )?;
    Ok(())
}

#[cfg(test)]
#[path = "poiesis_tests.rs"]
mod tests;

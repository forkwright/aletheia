//! `render_deck_report` organon tool — deck-spec to HTML, optionally PDF.
//!
//! Wraps [`poiesis_deck::DeckRenderer`] (zone-layout solving is internal to
//! `DeckRenderer::render`) and, for `format: "pdf"`,
//! [`poiesis_printer_chromium::print_to_pdf`]. Distinct from
//! [`crate::builtins::render_pptx_report`]: that tool renders a raw
//! slide-JSON descriptor straight to PPTX via `poiesis_slides` with no
//! zone-layout solving; this one renders a [`poiesis_core::bodies::Deck`]
//! spec through the component-registry/template pipeline. The two compose
//! (distinct format paths) rather than one superseding the other.

use std::future::Future;
use std::pin::Pin;

use hermeneus::types::{DocumentSource, ToolResultBlock};
use indexmap::IndexMap;
use poiesis_core::bodies::Deck;
use poiesis_core::envelope::Meta;
use poiesis_deck::DeckRenderer;

use crate::builtins::poiesis::{json_data_property, media_type_for_format};
use crate::builtins::workspace::validate_path;
use crate::error::Result;
use crate::registry::{ToolExecutor, ToolRegistry};
use crate::types::{
    InputSchema, PropertyDef, PropertyType, Reversibility, RollbackSupport, ToolCallCapability,
    ToolCallCapabilityRule, ToolCapabilityMetadata, ToolCategory, ToolContext, ToolDef,
    ToolGroupId, ToolInput, ToolResult, ToolStability, ToolTag,
};

const SUPPORTED_FORMATS: &[&str] = &["html", "pdf"];

pub(crate) struct RenderDeckReportExecutor;

impl ToolExecutor for RenderDeckReportExecutor {
    fn execute<'a>(
        &'a self,
        input: &'a ToolInput,
        ctx: &'a ToolContext,
    ) -> Pin<Box<dyn Future<Output = Result<ToolResult>> + Send + 'a>> {
        Box::pin(async move {
            let args = &input.arguments;

            let format = match args.get("format").and_then(serde_json::Value::as_str) {
                Some(f) if SUPPORTED_FORMATS.contains(&f) => f,
                Some(other) => {
                    return Ok(ToolResult::error(format!(
                        "unsupported format {other:?}; supported formats are: html, pdf"
                    )));
                }
                None => {
                    return Ok(ToolResult::error(
                        "missing required argument: format (html or pdf)".to_owned(),
                    ));
                }
            };

            let deck: Deck = match args.get("data") {
                Some(v) => {
                    let parsed: std::result::Result<Deck, serde_json::Error> =
                        if let Some(raw) = v.as_str() {
                            serde_json::from_str(raw)
                        } else {
                            serde_json::from_value(v.clone())
                        };
                    match parsed {
                        Ok(deck) => deck,
                        Err(e) => {
                            return Ok(ToolResult::error(format!(
                                "data does not match the Deck spec (aspect + slides): {e}"
                            )));
                        }
                    }
                }
                None => {
                    return Ok(ToolResult::error(
                        "missing required argument: data".to_owned(),
                    ));
                }
            };

            let title = args
                .get("title")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("Untitled Deck");
            let meta = match Meta::new(title) {
                Ok(meta) => meta,
                Err(e) => return Ok(ToolResult::error(format!("invalid title: {e}"))),
            };

            let disable_sandbox = args
                .get("disable_sandbox")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false);
            let (bytes, effective_format) =
                match render_deck_to_bytes(&deck, &meta, format, disable_sandbox).await {
                    Ok(v) => v,
                    Err(err) => return Ok(err),
                };

            if let Some(out_path) = args.get("out_path").and_then(serde_json::Value::as_str) {
                let validated = match validate_path(out_path, ctx, &input.name) {
                    Ok(path) => path,
                    Err(e) => {
                        return Ok(ToolResult::error(format!(
                            "invalid out_path {out_path:?}: {e}"
                        )));
                    }
                };
                if let Err(e) = tokio::fs::write(&validated, &bytes).await {
                    return Ok(ToolResult::error(format!(
                        "wrote 0 bytes to {}: {e}",
                        validated.display()
                    )));
                }
            }

            let encoded = koina::base64::encode(&bytes);
            let summary = format!(
                "Rendered deck report ({} slide(s)) as {}: {} bytes",
                deck.slides.len(),
                effective_format.to_uppercase(),
                bytes.len()
            );

            Ok(ToolResult::blocks(vec![
                ToolResultBlock::Text { text: summary },
                ToolResultBlock::Document {
                    source: DocumentSource {
                        source_type: "base64".to_owned(),
                        media_type: media_type_for_format(effective_format).to_owned(),
                        data: encoded,
                    },
                },
            ]))
        })
    }
}

/// Materializes the embedded component packs into a scratch directory,
/// renders `deck` through them via [`DeckRenderer`], and converts the
/// result to the requested output format's bytes -- an HTML pass-through,
/// or a PDF via a headless Chromium subprocess.
async fn render_deck_to_bytes(
    deck: &Deck,
    meta: &Meta,
    format: &str,
    disable_sandbox: bool,
) -> std::result::Result<(Vec<u8>, &'static str), ToolResult> {
    // WHY a fresh temp directory per call, not a cached registry: `Deck`
    // rendering reads each component's template FILE by path again at
    // render time (poiesis_deck::render reads def.html via
    // std::fs::read_to_string), not just at discovery time, so the
    // extracted directory must outlive the render() call below —
    // simplest correct lifetime is "lives exactly as long as this render".
    let tempdir = tempfile::tempdir().map_err(|e| {
        ToolResult::error(format!(
            "failed to create a temp directory for component packs: {e}"
        ))
    })?;
    let registry = poiesis_core::embedded::extract_to(tempdir.path())
        .map_err(|e| ToolResult::error(format!("failed to materialize component packs: {e}")))?;

    let renderer = DeckRenderer::new(registry, &deck.aspect);
    let html = renderer
        .render(deck, meta)
        .map_err(|e| ToolResult::error(format!("deck render failed: {e}")))?;

    let result = if format == "pdf" {
        let mut opts = poiesis_printer_chromium::PrintOptions::from_aspect(&deck.aspect);
        opts.disable_sandbox = disable_sandbox;
        // WHY awaited directly, not `spawn_blocking`: unlike poiesis_doc's
        // pandoc/typst renderers (genuinely blocking subprocess calls),
        // `print_to_pdf` is already `async fn` — it drives chromiumoxide's
        // CDP connection on the tokio runtime itself, and `opts.timeout`
        // (from `PrintOptions`) already bounds the whole operation.
        // Wrapping an async fn in `spawn_blocking` would need a nested
        // `block_on`, which is the anti-pattern this avoids.
        match poiesis_printer_chromium::print_to_pdf(&html, &opts).await {
            Ok(pdf_bytes) => (pdf_bytes, "pdf"),
            Err(e) => return Err(ToolResult::error(format!("PDF render failed: {e}"))),
        }
    } else {
        (html.into_bytes(), "html")
    };
    // WHY `tempdir` still in scope here: it must outlive both the HTML
    // render above AND, for the pdf path, chromium's own navigation to a
    // `data:` URL built from that HTML — dropping it early would risk the
    // renderer re-reading a template file out from under a still-in-flight
    // operation. It is dropped (and cleaned up) at the end of this
    // function, after every use.
    drop(tempdir);
    Ok(result)
}

fn render_deck_report_def() -> ToolDef {
    ToolDef {
        name: koina::id::ToolName::from_static("render_deck_report"), // kanon:ignore RUST/expect
        description: "Render a Deck spec (aspect + slides, each a component id and field payload) to HTML or PDF.".to_owned(),
        extended_description: Some(
            "`data` follows poiesis_core::bodies::Deck's shape: \
             { aspect: {width, height}, slides: [{ component, fields, notes? }] }. \
             `component` must name one of the shipped component packs (e.g. \
             `title`, `bullet`, `stat`, `chart`, `table`, `two-col`, `image-text`, \
             `image-full`, `comparison`, `quote`, `timeline`, `section`, `blank`); \
             `fields` is validated against that component's own schema at render time. \
             `format: \"pdf\"` renders through a headless Chromium subprocess (bounded by \
             a one-minute default deadline) with the sandbox ENABLED unless \
             `disable_sandbox: true` is explicitly passed."
                .to_owned(),
        ),
        input_schema: InputSchema {
            properties: IndexMap::from([
                (
                    "data".to_owned(),
                    json_data_property(
                        "Deck spec: { aspect: {width, height}, slides: [{component, fields, notes?}] }.",
                    ),
                ),
                (
                    "format".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::String,
                        description: "Output format.".to_owned(),
                        enum_values: Some(
                            SUPPORTED_FORMATS.iter().map(|s| (*s).to_owned()).collect(),
                        ),
                        default: None,
                        ..Default::default()
                    },
                ),
                (
                    "title".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::String,
                        description: "Deck title (deliverable metadata).".to_owned(),
                        enum_values: None,
                        default: None,
                        ..Default::default()
                    },
                ),
                (
                    "disable_sandbox".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::Boolean,
                        description: "PDF only: explicitly disable the Chromium sandbox. \
                                      Sandboxed by default; only disable in a trusted, \
                                      already-isolated deployment (#4501)."
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
                        description: "Optional filesystem path to write the rendered bytes to, in addition to returning base64 bytes.".to_owned(),
                        enum_values: None,
                        default: None,
                        ..Default::default()
                    },
                ),
            ]),
            required: vec!["data".to_owned(), "format".to_owned()],
        },
        category: ToolCategory::Workspace,
        reversibility: Reversibility::PartiallyReversible,
        auto_activate: false,
        groups: vec![ToolGroupId::Edit],
        tags: vec![ToolTag::Format],
    }
}

// WHY keyed on `out_path`, not `format`: this mirrors every sibling report
// tool's capability rule (`render_pptx_report_capability_rule`,
// `generate_document`'s out_path handling) — presence of a filesystem
// write is the axis the capability-rule system is built to classify on.
// Known simplification: a `format: "pdf"` call with no `out_path` still
// spawns a real Chromium subprocess (a lesser but non-zero risk vs a
// disk write) at the lower capability level; `ToolCallCapabilityRule`
// only supports single-argument classification today, and adding a
// compound rule kind is a larger, separate change.
fn render_deck_report_capability_rule() -> ToolCallCapabilityRule {
    ToolCallCapabilityRule::argument_presence(
        "out_path",
        ToolCallCapability::new(vec![ToolGroupId::Edit], Reversibility::PartiallyReversible),
        ToolCallCapability::new(vec![ToolGroupId::Read], Reversibility::FullyReversible),
    )
}

/// Register the `render_deck_report` tool.
pub(crate) fn register(registry: &mut ToolRegistry) -> Result<()> {
    registry.register_with_call_capability(
        render_deck_report_def(),
        render_deck_report_capability_rule(),
        Box::new(RenderDeckReportExecutor),
    )?;
    registry.declare_capability(
        koina::id::ToolName::from_static("render_deck_report"), // kanon:ignore RUST/expect
        ToolCapabilityMetadata {
            owner: "organon::builtins::render_deck_report".to_owned(),
            // WHY Experimental: this module is behind `#[cfg(feature =
            // "poiesis")]` (see crates/organon/src/builtins/mod.rs) -- not
            // compiled by default.
            stability: ToolStability::Experimental,
            rollback: RollbackSupport::PartialSupport {
                reason: "rendering runs in memory; a caller-provided out_path writes the \
                         rendered HTML/PDF to disk, overwriting any existing file without \
                         retaining its prior contents"
                    .to_owned(),
            },
            ..ToolCapabilityMetadata::default()
        },
    );
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
    fn schema_declares_format_enum_and_required_fields() {
        let schema = render_deck_report_def().input_schema.to_json_schema();

        assert_eq!(
            schema["properties"]["format"]["enum"],
            serde_json::json!(["html", "pdf"])
        );
        let required = schema["required"]
            .as_array()
            .expect("required array present");
        assert!(required.contains(&serde_json::json!("data")));
        assert!(required.contains(&serde_json::json!("format")));
    }

    #[test]
    fn render_deck_report_call_capability_requires_approval_when_out_path_present() {
        let mut registry = ToolRegistry::new();
        register(&mut registry).expect("register");

        assert_eq!(
            registry
                .approval_requirement_for_input(&ToolInput {
                    name: ToolName::from_static("render_deck_report"),
                    tool_use_id: "toolu_test".to_owned(),
                    arguments: serde_json::json!({
                        "data": {"aspect": {"width": 16, "height": 9}, "slides": []},
                        "format": "html",
                    }),
                })
                .expect("approval"),
            ApprovalRequirement::None,
            "no out_path means no disk write"
        );

        assert_eq!(
            registry
                .approval_requirement_for_input(&ToolInput {
                    name: ToolName::from_static("render_deck_report"),
                    tool_use_id: "toolu_test".to_owned(),
                    arguments: serde_json::json!({
                        "data": {"aspect": {"width": 16, "height": 9}, "slides": []},
                        "format": "html",
                        "out_path": "/tmp/deck.html",
                    }),
                })
                .expect("approval"),
            ApprovalRequirement::Required,
            "out_path present means disk write"
        );
    }

    // WHY this exercises DeckRenderer directly rather than going through
    // ToolExecutor::execute: building a real ToolContext needs the full
    // service-locator machinery organon's other builtin tests construct by
    // hand per-module (see e.g. communication.rs's mock_ctx) — orthogonal
    // to what this test needs to prove, which is that the NEW integration
    // (embedded component packs -> ComponentRegistry -> DeckRenderer)
    // actually renders a real shipped component's content, exactly the
    // path `RenderDeckReportExecutor::execute` drives above.
    #[test]
    fn embedded_components_render_a_real_deck_slide() {
        let tempdir = tempfile::tempdir().expect("create temp dir");
        let registry =
            poiesis_core::embedded::extract_to(tempdir.path()).expect("extract components");

        let aspect = poiesis_core::scalar::AspectRatio::WIDESCREEN_16_9;
        let renderer = DeckRenderer::new(registry, &aspect);
        let deck = Deck {
            aspect,
            slides: vec![poiesis_core::bodies::Slide {
                component: poiesis_core::ids::ComponentId::new("title")
                    .expect("valid component id"),
                fields: serde_json::json!({"title": "Hello Deck", "subtitle": "A test slide"}),
                notes: None,
            }],
        };
        let meta = Meta::new("Test Deck").expect("valid meta");

        let html = renderer.render(&deck, &meta).expect("render deck");
        assert!(
            html.contains("Hello Deck"),
            "rendered HTML should contain the slide title, got: {html}"
        );
        assert!(
            html.contains("A test slide"),
            "rendered HTML should contain the slide subtitle, got: {html}"
        );
    }
}

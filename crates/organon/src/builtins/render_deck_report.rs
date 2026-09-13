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
use poiesis_printer_chromium::{PrintOptions, PrinterError};

use crate::builtins::poiesis::{json_data_property, media_type_for_format};
use crate::builtins::report_capability::{ReportOutputEffect, ReportToolEffect, SubprocessEffect};
use crate::builtins::workspace::validate_prepared_path;
use crate::error::Result;
use crate::registry::{ToolExecutor, ToolRegistry};
use crate::types::{
    InputSchema, PropertyDef, PropertyType, Reversibility, ToolCategory, ToolContext, ToolDef,
    ToolGroupId, ToolInput, ToolResult, ToolTag,
};

const SUPPORTED_FORMATS: &[&str] = &["html", "pdf"];

pub(crate) struct RenderDeckReportExecutor;

impl ToolExecutor for RenderDeckReportExecutor {
    fn path_arguments(&self) -> &'static [&'static str] {
        &["out_path"]
    }

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
                let validated = match validate_prepared_path(out_path, ctx, &input.name) {
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

/// Seam over the Chromium-backed PDF backend
/// ([`poiesis_printer_chromium::print_to_pdf`]) so tests can substitute a
/// fake instead of depending on a real Chromium launch (#7346). Production
/// code always routes through [`RealPdfPrinter`]; `tests::FakePdfPrinter`
/// is the only other implementation and is reachable only from
/// `#[cfg(test)]` code.
trait PdfPrinter: Send + Sync {
    /// Convert `html` to PDF bytes per `opts`. Mirrors
    /// [`poiesis_printer_chromium::print_to_pdf`]'s signature and error
    /// type exactly, so [`RealPdfPrinter`] is a pure pass-through.
    fn print_to_pdf<'a>(
        &'a self,
        html: &'a str,
        opts: &'a PrintOptions,
    ) -> Pin<Box<dyn Future<Output = std::result::Result<Vec<u8>, PrinterError>> + Send + 'a>>;
}

/// The real backend: routes straight to
/// [`poiesis_printer_chromium::print_to_pdf`].
struct RealPdfPrinter;

impl PdfPrinter for RealPdfPrinter {
    fn print_to_pdf<'a>(
        &'a self,
        html: &'a str,
        opts: &'a PrintOptions,
    ) -> Pin<Box<dyn Future<Output = std::result::Result<Vec<u8>, PrinterError>> + Send + 'a>> {
        Box::pin(poiesis_printer_chromium::print_to_pdf(html, opts))
    }
}

/// Classification of a PDF-render failure (#7346), kept as a typed value
/// instead of only a formatted string, so callers can distinguish
/// [`PrinterError::ChromiumNotFound`] from [`PrinterError::BrowserLaunch`]
/// from every other renderer-side failure without parsing prose.
#[derive(Debug)]
enum PdfRenderError {
    /// No Chromium binary on `PATH` / `CHROMIUM_PATH`
    /// ([`PrinterError::ChromiumNotFound`]).
    ChromiumNotFound,
    /// A Chromium binary was found but the CDP browser process itself
    /// failed to launch ([`PrinterError::BrowserLaunch`]) -- e.g. a
    /// missing D-Bus session bus, or a sandbox rejection on a
    /// locked-down CI runner. `reason` is the launcher's own error text;
    /// for chromiumoxide's `CdpError::LaunchExit`/`LaunchTimeout` that
    /// text already includes the child process's exit status and
    /// captured stderr, so nothing is lost by classifying instead of
    /// just interpolating.
    ChromiumLaunchFailed {
        /// The launcher's own error text (exit status / stderr, when the
        /// underlying `CdpError` carries them).
        reason: String,
    },
    /// Any other renderer-side failure (page navigation, PDF generation,
    /// timeout, or cleanup) -- a real defect or resource exhaustion, not a
    /// recognized environment precondition.
    Other(String),
}

impl PdfRenderError {
    /// Classify a [`PrinterError`] without discarding its message.
    fn classify(err: PrinterError) -> Self {
        match err {
            PrinterError::ChromiumNotFound => Self::ChromiumNotFound,
            PrinterError::BrowserLaunch { source } => Self::ChromiumLaunchFailed {
                reason: source.to_string(),
            },
            other => Self::Other(other.to_string()),
        }
    }
}

impl std::fmt::Display for PdfRenderError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            // WHY delegate instead of re-typing the message: the wording
            // is defined once, in PrinterError::ChromiumNotFound's own
            // #[snafu(display(..))] (poiesis-printer-chromium error.rs).
            Self::ChromiumNotFound => std::fmt::Display::fmt(&PrinterError::ChromiumNotFound, f),
            Self::ChromiumLaunchFailed { reason } => write!(f, "Chromium launch failed: {reason}"),
            Self::Other(reason) => write!(f, "{reason}"),
        }
    }
}

/// Everything [`render_deck_to_bytes_inner`] can fail with: a setup
/// failure unrelated to Chromium (tempdir, component extraction, or deck
/// rendering itself -- reported as free-form text, as before #7346), or a
/// PDF-render failure carrying a typed [`PdfRenderError`] classification.
#[derive(Debug)]
enum RenderDeckError {
    /// Failed before the PDF/Chromium step was ever reached.
    Setup(String),
    /// Failed inside the PDF/Chromium step; see [`PdfRenderError`].
    Pdf(PdfRenderError),
}

impl RenderDeckError {
    /// Render into the [`ToolResult`] error the agent sees -- the same
    /// error path this tool always reported through, just built from a
    /// classified value rather than an ad hoc format string.
    fn into_tool_result(self) -> ToolResult {
        match self {
            Self::Setup(message) => ToolResult::error(message),
            Self::Pdf(pdf_err) => ToolResult::error(format!("PDF render failed: {pdf_err}")),
        }
    }
}

/// Materializes the embedded component packs into a scratch directory,
/// renders `deck` through them via [`DeckRenderer`], and converts the
/// result to the requested output format's bytes -- an HTML pass-through,
/// or a PDF via a headless Chromium subprocess reached through `printer`.
///
/// Takes the [`PdfPrinter`] seam explicitly so tests can exercise this
/// function's PDF branch -- and each [`PdfRenderError`] classification --
/// hermetically. [`render_deck_to_bytes`] is the production entry point,
/// always calling this with [`RealPdfPrinter`].
async fn render_deck_to_bytes_inner(
    deck: &Deck,
    meta: &Meta,
    format: &str,
    disable_sandbox: bool,
    printer: &dyn PdfPrinter,
) -> std::result::Result<(Vec<u8>, &'static str), RenderDeckError> {
    // WHY a fresh temp directory per call, not a cached registry: `Deck`
    // rendering reads each component's template FILE by path again at
    // render time (poiesis_deck::render reads def.html via
    // std::fs::read_to_string), not just at discovery time, so the
    // extracted directory must outlive the render() call below —
    // simplest correct lifetime is "lives exactly as long as this render".
    let tempdir = tempfile::tempdir().map_err(|e| {
        RenderDeckError::Setup(format!(
            "failed to create a temp directory for component packs: {e}"
        ))
    })?;
    let registry = poiesis_core::embedded::extract_to(tempdir.path()).map_err(|e| {
        RenderDeckError::Setup(format!("failed to materialize component packs: {e}"))
    })?;

    let renderer = DeckRenderer::new(registry, &deck.aspect);
    let html = renderer
        .render(deck, meta)
        .map_err(|e| RenderDeckError::Setup(format!("deck render failed: {e}")))?;

    let result = if format == "pdf" {
        let mut opts = PrintOptions::from_aspect(&deck.aspect);
        opts.disable_sandbox = disable_sandbox;
        // WHY awaited directly, not `spawn_blocking`: unlike poiesis_doc's
        // pandoc/typst renderers (genuinely blocking subprocess calls),
        // `print_to_pdf` is already `async fn` — it drives chromiumoxide's
        // CDP connection on the tokio runtime itself, and `opts.timeout`
        // (from `PrintOptions`) already bounds the whole operation.
        // Wrapping an async fn in `spawn_blocking` would need a nested
        // `block_on`, which is the anti-pattern this avoids.
        match printer.print_to_pdf(&html, &opts).await {
            Ok(pdf_bytes) => (pdf_bytes, "pdf"),
            Err(e) => return Err(RenderDeckError::Pdf(PdfRenderError::classify(e))),
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

/// Production entry point: [`render_deck_to_bytes_inner`] against the real
/// Chromium backend, with its [`RenderDeckError`] collapsed to the
/// [`ToolResult`] error `RenderDeckReportExecutor::execute` already knows
/// how to propagate.
async fn render_deck_to_bytes(
    deck: &Deck,
    meta: &Meta,
    format: &str,
    disable_sandbox: bool,
) -> std::result::Result<(Vec<u8>, &'static str), ToolResult> {
    render_deck_to_bytes_inner(deck, meta, format, disable_sandbox, &RealPdfPrinter)
        .await
        .map_err(RenderDeckError::into_tool_result)
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

// ARCHITECTURE(#7030): unlike every sibling report tool, this one declares a
// `subprocess` effect alongside its `out_path` write effect. A `format:
// "pdf"` call with no `out_path` still spawns a real Chromium subprocess --
// a lesser but non-zero risk vs a disk write, and a real one vs a call that
// touches nothing at all -- so `ReportToolEffect::capability_rule` composes
// the two axes with a `ToolCallCapabilityRule::Decision` instead of
// collapsing the PDF/no-write case into the same `Read` + `FullyReversible`
// classification as a no-op call.
fn render_deck_report_effect() -> ReportToolEffect {
    ReportToolEffect {
        owner: "organon::builtins::render_deck_report",
        output: ReportOutputEffect::CallerFile {
            argument: "out_path",
            artifact: "the rendered HTML/PDF",
        },
        subprocess: Some(SubprocessEffect {
            argument: "format",
            values: &["pdf"],
        }),
    }
}

/// Register the `render_deck_report` tool.
pub(crate) fn register(registry: &mut ToolRegistry) -> Result<()> {
    let effect = render_deck_report_effect();
    registry.register_with_call_capability(
        render_deck_report_def(),
        effect.capability_rule(),
        Box::new(RenderDeckReportExecutor),
    )?;
    registry.declare_capability(
        koina::id::ToolName::from_static("render_deck_report"), // kanon:ignore RUST/expect
        effect.capability_metadata(),
    )?;
    Ok(())
}

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test assertions")]
#[expect(clippy::indexing_slicing, reason = "test schema assertions")]
mod tests {
    use koina::id::ToolName;

    use super::*;
    use crate::types::ApprovalRequirement;

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

    #[test]
    fn render_deck_report_pdf_with_no_out_path_is_not_a_plain_read() {
        let mut registry = ToolRegistry::new();
        register(&mut registry).expect("register");

        let no_write_html = ToolInput {
            name: ToolName::from_static("render_deck_report"),
            tool_use_id: "toolu_test".to_owned(),
            arguments: serde_json::json!({
                "data": {"aspect": {"width": 16, "height": 9}, "slides": []},
                "format": "html",
            }),
        };
        let pdf_no_out_path = ToolInput {
            name: ToolName::from_static("render_deck_report"),
            tool_use_id: "toolu_test".to_owned(),
            arguments: serde_json::json!({
                "data": {"aspect": {"width": 16, "height": 9}, "slides": []},
                "format": "pdf",
            }),
        };

        let html_capability = registry
            .call_capability(&no_write_html)
            .expect("html capability");
        let pdf_capability = registry
            .call_capability(&pdf_no_out_path)
            .expect("pdf capability");

        assert_ne!(
            html_capability, pdf_capability,
            "a subprocess-backed PDF render must not read identically to a call that spawns nothing"
        );
        assert!(
            pdf_capability.groups.contains(&ToolGroupId::Command),
            "PDF-without-out_path spawns Chromium, so it must carry the Command group"
        );
        assert_eq!(
            registry
                .approval_requirement_for_input(&pdf_no_out_path)
                .expect("approval"),
            ApprovalRequirement::Advisory,
            "a subprocess spawn with no persisted effect warrants advisory approval, not none"
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

        let deck = minimal_deck();
        let renderer = DeckRenderer::new(registry, &deck.aspect);
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

    /// A single-slide `Deck` shared by the render and PDF-path tests above
    /// and below.
    fn minimal_deck() -> Deck {
        Deck {
            aspect: poiesis_core::scalar::AspectRatio::WIDESCREEN_16_9,
            slides: vec![poiesis_core::bodies::Slide {
                component: poiesis_core::ids::ComponentId::new("title")
                    .expect("valid component id"),
                fields: serde_json::json!({"title": "Hello Deck", "subtitle": "A test slide"}),
                notes: None,
            }],
        }
    }

    // WHY hermetic tests (a) plus an environment probe (b), not one test:
    // #7346's original single test ran real Chromium and matched only
    // `Ok(pdf) | Err(ChromiumNotFound)`, so `PrinterError::BrowserLaunch`
    // fell outside that match and panicked. Hermetic tests here prove the
    // PDF branch and every `PdfRenderError` classification deterministically
    // via `FakePdfPrinter`; the probe below proves only that a real
    // Chromium launch on this host lands in one of those same outcomes.

    /// Fakes [`PdfPrinter`] so the PDF branch -- and each
    /// [`PdfRenderError`] classification -- can be exercised without a
    /// real Chromium launch. `outcome` is a non-capturing fn pointer
    /// (rather than a boxed closure) because every fixture below is a
    /// fixed, `Copy`-able value; a fn pointer keeps the fake trivially
    /// constructible per test.
    struct FakePdfPrinter {
        outcome: fn() -> std::result::Result<Vec<u8>, PrinterError>,
    }

    impl PdfPrinter for FakePdfPrinter {
        fn print_to_pdf<'a>(
            &'a self,
            _html: &'a str,
            _opts: &'a PrintOptions,
        ) -> Pin<Box<dyn Future<Output = std::result::Result<Vec<u8>, PrinterError>> + Send + 'a>>
        {
            let outcome = (self.outcome)();
            Box::pin(async move { outcome })
        }
    }

    #[tokio::test]
    async fn render_deck_to_bytes_pdf_branch_succeeds_via_fake_renderer() {
        let deck = minimal_deck();
        let meta = Meta::new("Test Deck").expect("valid meta");
        let fake = FakePdfPrinter {
            outcome: || Ok(b"%PDF-fake".to_vec()),
        };

        let (bytes, format) = render_deck_to_bytes_inner(&deck, &meta, "pdf", true, &fake)
            .await
            .expect("fake printer reports success");

        assert_eq!(format, "pdf");
        assert_eq!(bytes, b"%PDF-fake");
    }

    #[tokio::test]
    async fn render_deck_to_bytes_classifies_chromium_not_found() {
        let deck = minimal_deck();
        let meta = Meta::new("Test Deck").expect("valid meta");
        let fake = FakePdfPrinter {
            outcome: || Err(PrinterError::ChromiumNotFound),
        };

        let err = render_deck_to_bytes_inner(&deck, &meta, "pdf", true, &fake)
            .await
            .expect_err("fake printer reports missing chromium");

        assert!(
            matches!(err, RenderDeckError::Pdf(PdfRenderError::ChromiumNotFound)),
            "got: {err:?}"
        );
    }

    #[tokio::test]
    async fn render_deck_to_bytes_classifies_browser_launch_failure() {
        let deck = minimal_deck();
        let meta = Meta::new("Test Deck").expect("valid meta");
        let fake = FakePdfPrinter {
            outcome: || {
                Err(PrinterError::BrowserLaunch {
                    // WHY std::io::Error, not the real chromiumoxide
                    // CdpError: it is Send + Sync + std::error::Error
                    // (all `BrowserLaunch { source }` requires) and its
                    // Display carries whatever text it is given, standing
                    // in for the real launcher's own exit-status/stderr
                    // text without depending on chromiumoxide internals.
                    source: Box::new(std::io::Error::other(
                        "Browser process exited with status exit status: 1, \
                         stderr: \"Failed to connect to the bus\"",
                    )),
                })
            },
        };

        let err = render_deck_to_bytes_inner(&deck, &meta, "pdf", true, &fake)
            .await
            .expect_err("fake printer reports a launch failure");

        match err {
            RenderDeckError::Pdf(PdfRenderError::ChromiumLaunchFailed { reason }) => {
                assert!(
                    reason.contains("Failed to connect to the bus"),
                    "a launch failure must carry the launcher's own stderr/exit \
                     context, not just a generic label, got: {reason}"
                );
            }
            other => panic!("expected ChromiumLaunchFailed, got: {other:?}"),
        }
    }

    #[tokio::test]
    async fn render_deck_to_bytes_classifies_other_renderer_failures_distinctly() {
        let deck = minimal_deck();
        let meta = Meta::new("Test Deck").expect("valid meta");
        let fake = FakePdfPrinter {
            outcome: || {
                Err(PrinterError::Timeout {
                    operation: "browser launch",
                    timeout_secs: 60,
                })
            },
        };

        let err = render_deck_to_bytes_inner(&deck, &meta, "pdf", true, &fake)
            .await
            .expect_err("fake printer reports a timeout");

        assert!(
            matches!(err, RenderDeckError::Pdf(PdfRenderError::Other(_))),
            "a timeout is a real defect/resource-exhaustion signal, not an \
             environment precondition like ChromiumNotFound or \
             ChromiumLaunchFailed, and must classify distinctly from both: {err:?}"
        );
    }

    // WHY a probe against the real binary, classification only, rather
    // than `if !chromium_available { return; }`:
    // mirrors the established crate idiom in
    // `poiesis_doc::pandoc::tests::docx_format_without_pandoc_returns_not_installed_or_fails`
    // -- run the real call on every host and match on whichever outcome the
    // environment actually produces, instead of silently skipping. Unlike
    // the hermetic tests above, this drives the real
    // [`RealPdfPrinter`]/Chromium launch on whatever host runs the suite,
    // so it asserts only that the outcome is one of the three recognized
    // classifications (real PDF bytes, `ChromiumNotFound`, or
    // `ChromiumLaunchFailed`) -- never that rendering itself succeeds. A
    // `ChromiumNotFound` or `ChromiumLaunchFailed` outcome is a loudly
    // logged, named environment precondition, not a suppressed failure:
    // the test still passes, but nothing about the classification is
    // silent. Anything else (a `Setup` failure, or `PdfRenderError::Other`
    // from a page/PDF/cleanup error) is a real defect this test must still
    // fail loud on, exactly like the pre-#7346 test did for any outcome
    // beyond its original two.
    //
    // WHY `disable_sandbox: true`, not `false`: that third argument only
    // selects Chromium's OS-level process sandbox, a host kernel/container
    // capability this test has no stake in (this crate's own
    // `chromium_impl::print_to_pdf_inner` already carries the WHY for
    // sandbox disablement, and `PrintOptions::disable_sandbox` /
    // `POIESIS_CHROMIUM_DISABLE_SANDBOX` are the shipped, intentional way to
    // opt out of it). With it left enabled, GitHub's `ubuntu-24.04` runners
    // (unprivileged user namespaces disabled since Ubuntu 23.10+) crash
    // Chromium's zygote with "No usable sandbox!", which without this flag
    // would surface as a `ChromiumLaunchFailed` precondition rather than
    // exercising the real-PDF branch; it is safe to force off here because
    // the deck HTML rendered is fixed, locally generated fixture content,
    // not untrusted input.
    #[tokio::test]
    async fn render_deck_to_bytes_pdf_probe_classifies_environment_outcome() {
        let deck = minimal_deck();
        let meta = Meta::new("Test Deck").expect("valid meta");

        match render_deck_to_bytes_inner(&deck, &meta, "pdf", true, &RealPdfPrinter).await {
            Ok((bytes, format)) => {
                assert_eq!(format, "pdf");
                assert!(
                    bytes.starts_with(b"%PDF"),
                    "chromium is present and its launch succeeded, so the PDF \
                     branch must produce a real PDF; got {} byte(s) starting \
                     with {:?}",
                    bytes.len(),
                    &bytes[..bytes.len().min(16)]
                );
            }
            Err(RenderDeckError::Pdf(PdfRenderError::ChromiumNotFound)) => {
                eprintln!(
                    "precondition: no chromium/chromium-browser/google-chrome(-stable) \
                     on PATH and no CHROMIUM_PATH override"
                );
            }
            Err(RenderDeckError::Pdf(PdfRenderError::ChromiumLaunchFailed { reason })) => {
                eprintln!("precondition: chromium found but its launch failed: {reason}");
            }
            Err(other) => panic!(
                "render_deck_to_bytes produced an unclassified outcome -- a real \
                 defect (or a new environment precondition this test does not yet \
                 know how to classify), not one of the three recognized outcomes \
                 (Ok(pdf) | ChromiumNotFound | ChromiumLaunchFailed): {other:?}"
            ),
        }
    }
}

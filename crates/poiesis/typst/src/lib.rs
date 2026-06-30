#![deny(missing_docs)]
//! poiesis-typst: Typst-based PDF rendering for poiesis.
//!
//! Typst is the primary rendering backend for the poiesis report tooling arc.
//! It is more expressive than direct PDF generation: templates, math,
//! citations, breakable blocks, cross-references, and page-aware layout all
//! come for free, and diagnostics carry source locations.
//!
//! This crate embeds the Typst compiler as a library (not a subprocess), so
//! rendering is reproducible, offline, and returns structured errors.
//!
//! ## Public API
//!
//! - [`render_typst`] — compile an arbitrary Typst source string with an
//!   injected JSON data blob and return PDF bytes.
//! - [`render_template`] — compile a built-in template (looked up by slug)
//!   against JSON data and return PDF bytes.
//! - [`templates`] — listing of built-in template slugs.
//!
//! ## Data injection
//!
//! The JSON value supplied to [`render_typst`] is exposed to the Typst source
//! as a virtual file named `data.json`. Templates load it with:
//!
//! ```typst
//! #let data = json("data.json")
//! ```
//!
//! This mirrors Typst's own `json()` function and avoids a bespoke macro
//! layer. See [`templates::DEFAULT`] for an example.
//!
//! ## Attribution
//!
//! The compiler `World` implementation is adapted from a prior private project,
//! used with permission per issue #3450.

mod error;
mod world;

/// Built-in Typst templates.
pub mod templates;

pub use error::PoiesisError;

use tracing::instrument;
use typst::layout::PagedDocument;

use crate::world::{TypstWorld, format_diagnostics};

/// Result alias for poiesis-typst operations.
pub type Result<T, E = PoiesisError> = std::result::Result<T, E>;

/// Render a Typst source string to PDF bytes, with optional JSON data injected
/// at the virtual path `data.json`.
///
/// Templates can read the injected data via `json("data.json")`. Pass
/// `serde_json::Value::Null` (or any empty object) if the template does not
/// consume data.
///
/// # Errors
///
/// Returns [`PoiesisError::SerializeData`] if `data` cannot be serialized,
/// [`PoiesisError::Compile`] if Typst rejects the source, or
/// [`PoiesisError::PdfExport`] if PDF export fails.
#[instrument(skip_all, fields(source_bytes = source.len()))]
pub fn render_typst(source: &str, data: &serde_json::Value) -> Result<Vec<u8>> {
    let data_bytes = serde_json::to_vec(data).map_err(|e| PoiesisError::SerializeData {
        detail: e.to_string(),
    })?;

    let world = TypstWorld::new(source, Some(data_bytes));

    let result = typst::compile::<PagedDocument>(&world);

    for warning in &result.warnings {
        tracing::warn!(message = %warning.message, "typst warning");
    }

    let document = result.output.map_err(|diagnostics| PoiesisError::Compile {
        diagnostics: format_diagnostics(&world, &diagnostics),
    })?;

    let pdf_bytes =
        typst_pdf::pdf(&document, &typst_pdf::PdfOptions::default()).map_err(|diagnostics| {
            PoiesisError::PdfExport {
                diagnostics: format_diagnostics(&world, &diagnostics),
            }
        })?;

    Ok(pdf_bytes)
}

/// Render a built-in template slug against JSON data.
///
/// See [`templates`] for the list of slugs.
///
/// # Errors
///
/// Returns [`PoiesisError::UnknownTemplate`] if the slug is not recognized,
/// plus any error from [`render_typst`].
#[instrument(skip_all, fields(slug))]
pub fn render_template(slug: &str, data: &serde_json::Value) -> Result<Vec<u8>> {
    let source = templates::lookup(slug).ok_or_else(|| PoiesisError::UnknownTemplate {
        slug: slug.to_owned(),
        known: templates::SLUGS.join(", "),
    })?;
    render_typst(source, data)
}

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test assertions")]
mod tests {
    use super::*;

    #[test]
    fn render_minimal_document_produces_pdf() {
        let source = "= Hello\n\nThis is a test.";
        let pdf = render_typst(source, &serde_json::Value::Null).expect("minimal must render");
        assert!(pdf.starts_with(b"%PDF-"), "output must be PDF-magic");
        assert!(pdf.len() > 200, "PDF should be more than a few bytes");
        assert!(pdf.len() < 5_000_000, "one-pager PDF should be <5MB");
    }

    #[test]
    fn render_default_template_round_trip() {
        let data = serde_json::json!({
            "title": "Quarterly Review",
            "author": "alice",
            "subtitle": "Draft — internal circulation only",
            "body": [
                "First body paragraph describing the headline finding.",
                "Second body paragraph with supporting detail."
            ],
            "table": {
                "columns": 3,
                "header": ["Metric", "Q1", "Q2"],
                "rows": [
                    ["Revenue", "100", "120"],
                    ["Costs",   "60",  "70"]
                ]
            },
            "footer": "Prepared for internal review."
        });
        let pdf = render_template(templates::DEFAULT, &data).expect("default must render");
        assert!(pdf.starts_with(b"%PDF-"), "output must be PDF-magic");
        // WHY: exact byte equality is not meaningful — PDFs embed fonts and
        // creation dates. Assert on magic prefix and reasonable size bounds.
        assert!(
            pdf.len() > 500,
            "template PDF should be more than 500 bytes"
        );
        assert!(pdf.len() < 5_000_000, "template PDF should be <5MB");
    }

    #[test]
    fn render_default_template_with_only_title() {
        let data = serde_json::json!({ "title": "Minimal" });
        let pdf =
            render_template(templates::DEFAULT, &data).expect("minimal template data must render");
        assert!(pdf.starts_with(b"%PDF-"));
    }

    #[test]
    fn render_default_template_no_data_uses_default_title() {
        let data = serde_json::json!({});
        let pdf = render_template(templates::DEFAULT, &data).expect("empty data must still render");
        assert!(pdf.starts_with(b"%PDF-"));
    }

    #[test]
    fn unknown_template_returns_error() {
        let data = serde_json::Value::Null;
        let err = render_template("no-such-template", &data).expect_err("must error");
        assert!(
            matches!(err, PoiesisError::UnknownTemplate { .. }),
            "expected UnknownTemplate, got: {err:?}"
        );
    }

    #[test]
    fn malformed_typst_returns_compile_error() {
        let source = "#this-function-does-not-exist()";
        let err = render_typst(source, &serde_json::Value::Null).expect_err("must error");
        assert!(
            matches!(err, PoiesisError::Compile { .. }),
            "expected Compile, got: {err:?}"
        );
    }

    #[test]
    fn compile_diagnostics_include_location() {
        let source = "#unknown-function()";
        let err = render_typst(source, &serde_json::Value::Null).expect_err("must error");
        let PoiesisError::Compile { diagnostics } = err else {
            panic!("expected Compile error");
        };
        assert!(
            diagnostics.contains("main.typ"),
            "diagnostics must reference main.typ: {diagnostics}"
        );
    }

    #[test]
    fn data_injection_is_visible_to_template() {
        // Template that prints a field from data; failure to load means
        // json() returns an error and compile fails.
        let source = r#"
#let d = json("data.json")
The marker is #d.marker.
"#;
        let data = serde_json::json!({ "marker": "sentinel-value-xyz" });
        let pdf = render_typst(source, &data).expect("must render");
        assert!(pdf.starts_with(b"%PDF-"));
    }

    #[test]
    fn eval_report_template_renders_sample_fixture() {
        let data = serde_json::json!({
            "summary": {
                "passed": 42,
                "failed": 3,
                "skipped": 2,
                "total_duration_ms": 5000
            },
            "benchmarks": [
                {
                    "id": "test_coherence_basic",
                    "category": "coherence",
                    "outcome": "passed",
                    "duration_ms": 1200,
                    "error": null,
                    "skip_reason": null
                },
                {
                    "id": "test_memory_overflow",
                    "category": "memory",
                    "outcome": "failed",
                    "duration_ms": 800,
                    "error": "assertion failed: count < max",
                    "skip_reason": null
                },
                {
                    "id": "test_deprecated_api",
                    "category": "compatibility",
                    "outcome": "skipped",
                    "duration_ms": null,
                    "error": null,
                    "skip_reason": "API deprecated in v2"
                }
            ]
        });
        let pdf = render_template(templates::EVAL_REPORT, &data)
            .expect("eval-report template must render");
        assert!(pdf.starts_with(b"%PDF-"), "output must be PDF");
        assert!(pdf.len() > 500, "template PDF should be >500 bytes");
        assert!(pdf.len() < 5_000_000, "template PDF should be <5MB");
    }

    #[test]
    fn eval_report_template_renders_memory_benchmark_statistics() {
        let data = serde_json::json!({
            "benchmark": "LongMemEval",
            "total": 3,
            "scored": 3,
            "errors": 0,
            "timeouts": 0,
            "no_answers": 0,
            "statistics": {
                "f1_ci_low": 0.5,
                "f1_ci_high": 0.9,
                "em_ci_low": 0.4,
                "em_ci_high": 0.8,
                "n_resamples": 2000,
                "method": "percentile bootstrap"
            },
            "publishability": {
                "publishable": true,
                "minimum_scored_questions": 2,
                "reasons": []
            },
            "comparisons": [
                {
                    "metric": "f1",
                    "label": "baseline_vs_candidate f1",
                    "status": "complete",
                    "matched_questions": 3,
                    "statistics": {
                        "label": "baseline_vs_candidate f1",
                        "n_a": 3,
                        "n_b": 3,
                        "mean_a": 0.4,
                        "mean_b": 0.7,
                        "ci_a": {
                            "point": 0.4,
                            "ci_low": 0.2,
                            "ci_high": 0.6,
                            "confidence": 0.95,
                            "n_resamples": 2000
                        },
                        "ci_b": {
                            "point": 0.7,
                            "ci_low": 0.5,
                            "ci_high": 0.9,
                            "confidence": 0.95,
                            "n_resamples": 2000
                        },
                        "effect": {
                            "d": -0.5,
                            "ci_low": -0.8,
                            "ci_high": -0.2,
                            "interpretation": "medium"
                        },
                        "p_raw": 0.04,
                        "p_adjusted": 0.04,
                        "significant_raw": true,
                        "significant_adjusted": true
                    }
                }
            ]
        });
        let pdf = render_template(templates::EVAL_REPORT, &data)
            .expect("benchmark eval-report template must render");
        assert!(pdf.starts_with(b"%PDF-"), "output must be PDF");
        assert!(pdf.len() > 500, "template PDF should be >500 bytes");
        assert!(pdf.len() < 5_000_000, "template PDF should be <5MB");
    }

    #[test]
    fn graph_audit_template_renders_sample_fixture() {
        let data = serde_json::json!({
            "summary": {
                "total": 3,
                "by_scope": {
                    "crate": 1,
                    "module": 1,
                    "concept": 1,
                    "boundary": 0
                }
            },
            "facts": [
                {
                    "id": "aletheia.spawn.model",
                    "scope": "crate",
                    "claim": "Spawn model is configured via environment variable ALETHEIA_MODEL.",
                    "evidence": ["crates/aletheia/src/spawn.rs", "PR-3789"],
                    "updated_at": "2026-04-21T14:30:00Z",
                    "updated_by": "PR-3789"
                },
                {
                    "id": "aletheia.providers.isolation",
                    "scope": "module",
                    "claim": "Each provider is isolated by a channel-based request/response boundary.",
                    "evidence": ["crates/aletheia/src/providers/mod.rs"],
                    "updated_at": "2026-04-20T10:00:00Z",
                    "updated_by": "PR-3750"
                },
                {
                    "id": "aletheia.memory.bi_temporal",
                    "scope": "concept",
                    "claim": "Memory facts use bi-temporal versioning with valid_from and valid_to timestamps.",
                    "evidence": ["crates/eidos/src/knowledge/fact.rs", "docs/temporal.md"],
                    "updated_at": "2026-03-15T09:00:00Z",
                    "updated_by": "session_abc123"
                }
            ]
        });
        let pdf = render_template(templates::GRAPH_AUDIT, &data)
            .expect("graph-audit template must render");
        assert!(pdf.starts_with(b"%PDF-"), "output must be PDF");
        assert!(pdf.len() > 500, "template PDF should be >500 bytes");
        assert!(pdf.len() < 5_000_000, "template PDF should be <5MB");
    }
}

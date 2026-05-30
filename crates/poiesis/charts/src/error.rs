//! Chart model and render errors.
//!
//! Errors here surface at parse time (`Chart::validate`) or at render time
//! (`render_chart`). Every variant has actionable context — callers should
//! be able to report the failing field path or the failing render path
//! without re-walking the input.

use snafu::Snafu;

/// Errors produced while parsing a [`Chart`](crate::model::Chart) or while
/// rendering it to SVG.
#[derive(Debug, Snafu)]
#[snafu(visibility(pub(crate)))]
#[non_exhaustive]
pub enum Error {
    /// The supplied JSON does not match the [`Chart`](crate::model::Chart) schema.
    #[snafu(display("invalid chart JSON at {path}: {message}"))]
    InvalidJson {
        /// JSON pointer / dotted field path of the offending value.
        path: String,
        /// Human-readable description of the schema violation.
        message: String,
    },

    /// A required series count or shape was not satisfied for the given kind.
    ///
    /// Example: `kind = combo` requires exactly two series, one `column`
    /// styled and one `line` styled.
    #[snafu(display("kind `{kind}` requires {expected} but got {actual} (at series {path})"))]
    BadSeriesShape {
        /// The chart kind whose contract was violated.
        kind: String,
        /// Human-readable shape requirement (e.g. "exactly 2 series").
        expected: String,
        /// What was actually present.
        actual: String,
        /// Field path to the offending series array.
        path: String,
    },

    /// A data point used a literal number instead of a `FactCite`.
    ///
    /// The model types `y` (and `x` for scatter/linear axes) as
    /// `FactCite` exactly so that this never happens at runtime — but the
    /// JSON ingest path may still encounter raw numbers, and they must be
    /// rejected loudly, per `B-008` `naked-number`.
    #[snafu(display("naked number at {path}: chart data must reference Fact ids"))]
    NakedNumber {
        /// JSON pointer to the offending raw number.
        path: String,
    },

    /// The spec requires the Vega-Lite fallback but `charts-vega` is disabled.
    ///
    /// Hard refusal, never a silent degrade. The caller is expected to enable
    /// the feature, switch to a kind the Rust emitter owns, or change the
    /// axis scale.
    #[snafu(display(
        "kind `{kind}` (scale `{scale}`) needs the `charts-vega` feature; \
         enable it or use a Rust-owned kind"
    ))]
    VegaRequired {
        /// The chart kind that triggered the routing.
        kind: String,
        /// The axis scale that triggered the routing (or `linear` if kind-driven).
        scale: String,
    },

    /// A theme tone reference did not resolve.
    ///
    /// Series fills and strokes must resolve from `theme.chart.series[i]`
    /// or a named tone in the active theme — never literal hex.
    #[snafu(display("unresolved tone reference `{tone}` in series {series_index}"))]
    UnresolvedTone {
        /// The tone reference that failed to resolve.
        tone: String,
        /// Index of the offending series.
        series_index: usize,
    },

    /// A `FactCite` did not resolve in the supplied factbase.
    #[snafu(display("unresolved fact `{fact_id}` at {path}"))]
    UnresolvedFact {
        /// The fact id that failed to resolve.
        fact_id: String,
        /// JSON pointer to the offending cite.
        path: String,
    },

    /// A pure-Rust emitter arm is not yet implemented for this kind.
    ///
    /// Distinct from [`Error::VegaRequired`]: the kind belongs to the Rust
    /// path, but the per-kind code is still stubbed. The PR body documents
    /// which arms are stubbed and the completion plan; this error is the
    /// gate-traceable handle on each stub.
    #[snafu(display("kind `{kind}` is scaffolded but the emitter arm is not yet implemented"))]
    EmitterStub {
        /// The chart kind whose emitter arm is still a stub.
        kind: String,
    },

    /// Vega-Lite shell-out failed (npx not found, non-zero exit, etc.).
    #[snafu(display("vega-lite shell-out failed: {message}"))]
    VegaShellout {
        /// Error detail.
        message: String,
    },
}

#![deny(missing_docs)]
//! poiesis-charts: typed chart model + deterministic SVG emitter.
//!
//! # What this crate is
//!
//! A chart in poiesis is the validated payload of the `chart` slide component
//! and of `Figure { chart }` in a document. The model is parse-don't-validate:
//! every field is a newtype or a closed enum. Numeric data points reference
//! [`FactId`](model::FactId) entries in the deliverable's factbase — they are
//! never naked floats. The renderer is a hand-rolled SVG emitter; geometry
//! is fixed-source-order and float formatting is fixed-precision, so the
//! emitted bytes are deterministic and golden-snapshottable.
//!
//! # The render-path decision rule
//!
//! ```text
//! render_path(kind) =
//!     Vega   if kind ∈ {heatmap, boxplot, sankey, candlestick, …}
//!                  OR axis.scale ∈ {log, time}
//!     Rust   otherwise
//! ```
//!
//! The pure-Rust path covers `bar`, `column`, `line`, `area`, `combo`,
//! `scatter`, `pie`, `doughnut`, and `stat`. Anything else routes to the
//! Vega-Lite fallback, gated by the [`charts-vega`](crate#feature-flags)
//! feature. If a spec requires Vega while the feature is off, [`Chart::validate`]
//! returns [`Error::VegaRequired`] — a hard parse-time refusal, never a silent
//! degrade.
//!
//! # Module map
//!
//! - [`model`] — the `Chart`, `Series`, `Axes` parse-don't-validate types and
//!   the [`FactCite`](model::FactCite) reference used by every datum.
//! - [`theme`] — the [`ResolvedTheme`](theme::ResolvedTheme) seam: tone tokens,
//!   resolved colors, the `themed` vs `resolved` color modes that keep HTML
//!   and bake outputs geometrically identical.
//! - [`scale`] — linear [`Scale`](scale::Scale) + the `nice()` extension that
//!   produces auto-domain end-points and the 1-2-5 tick generator.
//! - [`format`] — fixed-precision number formatting (the only path that turns
//!   an `f64` into chart `<text>`).
//! - [`render`] — the SVG emitter: per-kind functions in fixed source order,
//!   plus the `Vega-Lite` shell-out wrapper behind `charts-vega`.
//!
//! # Determinism contract
//!
//! Every emitted SVG is reproducible byte-for-byte from the same `Chart` + theme:
//!
//! - All `f64` → text passes through [`format::format_number`].
//! - All coordinates round to two decimal places before string interpolation.
//! - Group order is fixed (`gridlines → axes → bars → line → labels → x-labels`).
//! - IDs (markers, gradients) are content-derived or index-based, never random.
//!
//! See `B-005` (`poiesis-evolution/B-005-poiesis-charts.md`) for the
//! acceptance contract and the offsite slide-3 golden the determinism rules
//! are calibrated against.
//!
//! # Feature flags
//!
//! - `charts-vega` (off by default): enables the Vega-Lite fallback for
//!   kinds the pure-Rust emitter does not own. The fallback shells to
//!   `npx -y vega-lite@<pin>` and is intentionally not on the deck-render
//!   critical path.

/// Error type for chart model parsing and render-path validation.
pub mod error;
/// Fixed-precision number formatting for chart text.
pub mod format;
/// Parse-don't-validate model: `Chart`, `Series`, `Axes`, fact citations.
pub mod model;
/// SVG emitter (pure-Rust path) + Vega-Lite shell-out (feature-gated).
pub mod render;
/// Linear scale + `nice()` domain extension + 1-2-5 tick generator.
pub mod scale;
/// Theme-binding seam: tone tokens, color modes (`themed` / `resolved`).
pub mod theme;

pub use error::Error;
pub use model::{
    Axes, AxisSide, AxisSpec, Chart, ChartKind, CiteOrScalar, CiteOrText, Domain, FactCite, FactId,
    Inlines, LegendSpec, NumFormat, Point, Scale as ScaleKind, Series, SeriesStyle, Ticks, ToneRef,
    Unit,
};
pub use render::{ColorMode, render_chart};
pub use theme::ResolvedTheme;

/// Result alias for poiesis-charts operations.
pub type Result<T, E = Error> = std::result::Result<T, E>;

//! SVG emitter (pure-Rust path) + Vega-Lite shell-out (feature-gated).
//!
//! # Entry point
//!
//! [`render_chart`] is the single function callers use. It:
//!
//! 1. Calls [`Chart::validate`](crate::model::Chart::validate) — returns
//!    [`crate::Error::VegaRequired`] if the spec needs the fallback.
//! 2. Dispatches on `chart.kind` to the matching emitter arm.
//!
//! # Per-kind emitter status
//!
//! | Kind     | Status                          | Owner       |
//! |----------|---------------------------------|-------------|
//! | bar      | stub (`Error::EmitterStub`)     | follow-up   |
//! | column   | stub (`Error::EmitterStub`)     | follow-up   |
//! | line     | stub (`Error::EmitterStub`)     | follow-up   |
//! | area     | stub (`Error::EmitterStub`)     | follow-up   |
//! | combo    | scaffolded (`emit_combo`)       | this PR     |
//! | scatter  | stub (`Error::EmitterStub`)     | follow-up   |
//! | pie      | stub (`Error::EmitterStub`)     | follow-up   |
//! | doughnut | stub (`Error::EmitterStub`)     | follow-up   |
//! | stat     | stub (`Error::EmitterStub`)     | follow-up   |
//!
//! The `combo` arm is scaffolded first because the B-005 acceptance gate
//! reproduces the offsite slide-3 chart, which is a combo. The other arms
//! follow the same module shape (one file each under `kinds/`); the design
//! fan-out is tracked in the PR body.

mod canvas;
mod kinds;
#[cfg(feature = "charts-vega")]
mod vega;

pub use canvas::{Canvas, DeckCanvas, DocCanvas};

use crate::Result;
use crate::model::{Chart, ChartKind};
use crate::theme::ResolvedTheme;

pub use crate::theme::ColorMode;

/// Render a [`Chart`] to an SVG byte string.
///
/// Geometry follows the canvas the caller picks:
/// [`DeckCanvas`] for the deck stage (`1600×540`), [`DocCanvas`] for a
/// document figure (intrinsic box per writer). The same chart spec renders
/// identically across canvases up to inner-box scaling.
///
/// # Errors
///
/// - [`crate::Error::VegaRequired`] — kind / scale needs the Vega-Lite
///   fallback and `charts-vega` is disabled.
/// - [`crate::Error::BadSeriesShape`] — series count violates the per-kind
///   contract.
/// - [`crate::Error::EmitterStub`] — pure-Rust arm for this kind is not yet
///   implemented (stub list above).
/// - [`crate::Error::UnresolvedTone`] — a series references a tone the
///   theme does not provide.
pub fn render_chart(
    chart: &Chart,
    theme: &ResolvedTheme,
    canvas: &Canvas,
    mode: ColorMode,
) -> Result<String> {
    chart.validate()?;

    match chart.kind {
        ChartKind::Combo => kinds::combo::emit(chart, theme, canvas, mode),
        ChartKind::Bar
        | ChartKind::Column
        | ChartKind::Line
        | ChartKind::Area
        | ChartKind::Scatter
        | ChartKind::Pie
        | ChartKind::Doughnut
        | ChartKind::Stat => Err(crate::Error::EmitterStub {
            kind: chart.kind.name().to_owned(),
        }),
        ChartKind::Heatmap | ChartKind::Boxplot | ChartKind::Sankey | ChartKind::Candlestick => {
            #[cfg(feature = "charts-vega")]
            {
                vega::emit(chart, theme, canvas, mode)
            }
            #[cfg(not(feature = "charts-vega"))]
            {
                let _ = (theme, canvas, mode);
                Err(crate::Error::VegaRequired {
                    kind: chart.kind.name().to_owned(),
                    scale: "linear".to_owned(),
                })
            }
        }
    }
}

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test assertions")]
mod tests {
    use super::*;
    use crate::model::{
        Axes, AxisSide, Chart, ChartKind, CiteOrText, FactCite, FactId, LegendSpec, Point, Series,
        SeriesStyle, ToneRef, Unit,
    };

    fn combo_spec() -> Chart {
        let cite = |id: &str, v: f64| FactCite {
            id: FactId(id.to_owned()),
            value: v,
            unit: Unit::Number,
        };
        let pt = |label: &str, c: FactCite| Point {
            label: Some(CiteOrText::Text(label.to_owned())),
            x: None,
            y: c,
        };
        Chart {
            kind: ChartKind::Combo,
            title: None,
            series: vec![
                Series {
                    name: CiteOrText::Text("Revenue".to_owned()),
                    points: vec![
                        pt("MAR", cite("f1", 18.0)),
                        pt("APR", cite("f2", 22.0)),
                        pt("MAY", cite("f3", 25.0)),
                    ],
                    tone: ToneRef::Indexed(0),
                    axis: AxisSide::Left,
                    style: SeriesStyle::Column,
                },
                Series {
                    name: CiteOrText::Text("Headcount".to_owned()),
                    points: vec![
                        pt("MAR", cite("f4", 120.0)),
                        pt("APR", cite("f5", 150.0)),
                        pt("MAY", cite("f6", 175.0)),
                    ],
                    tone: ToneRef::Indexed(1),
                    axis: AxisSide::Right,
                    style: SeriesStyle::Line,
                },
            ],
            axes: Axes::default(),
            legend: LegendSpec::Auto,
            data_labels: true,
            caption: None,
        }
    }

    #[test]
    fn combo_renders_to_nonempty_svg() {
        let spec = combo_spec();
        let theme = ResolvedTheme::summus_stub();
        let svg = render_chart(
            &spec,
            &theme,
            &Canvas::Deck(DeckCanvas::default()),
            ColorMode::Resolved,
        )
        .expect("combo emitter returns Ok");
        assert!(svg.starts_with("<svg"));
        assert!(svg.contains("viewBox=\"0 0 1600 540\""));
    }

    #[test]
    fn stub_kinds_return_emitter_stub_error() {
        let mut spec = combo_spec();
        spec.kind = ChartKind::Line;
        spec.series.truncate(1);
        let theme = ResolvedTheme::summus_stub();
        let r = render_chart(
            &spec,
            &theme,
            &Canvas::Deck(DeckCanvas::default()),
            ColorMode::Resolved,
        );
        assert!(matches!(r, Err(crate::Error::EmitterStub { kind }) if kind == "line"));
    }
}

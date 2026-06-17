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
//! The pure-Rust emitter covers `bar`, `column`, `line`, `area`, `combo`,
//! `scatter`, `pie`, `doughnut`, and `stat`; the remaining kinds (`heatmap`,
//! `boxplot`, `sankey`, `candlestick`) route to Vega-Lite behind the
//! `charts-vega` feature.

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
        ChartKind::Bar => kinds::bar::emit(chart, theme, canvas, mode),
        ChartKind::Column => kinds::column::emit(chart, theme, canvas, mode),
        ChartKind::Line => kinds::line::emit(chart, theme, canvas, mode),
        ChartKind::Area => kinds::area::emit(chart, theme, canvas, mode),
        ChartKind::Combo => kinds::combo::emit(chart, theme, canvas, mode),
        ChartKind::Scatter => kinds::scatter::emit(chart, theme, canvas, mode),
        ChartKind::Pie => kinds::pie::emit(chart, theme, canvas, mode),
        ChartKind::Doughnut => kinds::doughnut::emit(chart, theme, canvas, mode),
        ChartKind::Stat => kinds::stat::emit(chart, theme, canvas, mode),
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
        Axes, AxisSide, Chart, ChartKind, CiteOrScalar, CiteOrText, Domain, FactCite, FactId,
        Inlines, LegendSpec, Point, Series, SeriesStyle, Ticks, ToneRef, Unit,
    };
    use crate::render::canvas::DeckCanvas;

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
    fn bar_renders() {
        let spec = chart_spec(
            ChartKind::Bar,
            vec![series(
                "Revenue",
                0,
                SeriesStyle::Default,
                AxisSide::Left,
                vec![point("MAR", 18.0), point("APR", 22.0)],
            )],
            true,
        );
        assert_kind_svg(&spec, &["<svg", "<g class=\"bars\">", "<rect"]);
    }

    #[test]
    fn column_renders() {
        let spec = chart_spec(
            ChartKind::Column,
            vec![series(
                "Revenue",
                0,
                SeriesStyle::Default,
                AxisSide::Left,
                vec![point("MAR", 18.0), point("APR", 22.0)],
            )],
            true,
        );
        assert_kind_svg(&spec, &["<svg", "<g class=\"bars\">", "<rect"]);
    }

    #[test]
    fn line_renders() {
        let spec = chart_spec(
            ChartKind::Line,
            vec![series(
                "Revenue",
                0,
                SeriesStyle::Default,
                AxisSide::Left,
                vec![point("MAR", 18.0), point("APR", 22.0)],
            )],
            true,
        );
        assert_kind_svg(
            &spec,
            &["<svg", "<g class=\"lines\">", "<polyline", "<circle"],
        );
    }

    #[test]
    fn area_renders() {
        let spec = chart_spec(
            ChartKind::Area,
            vec![series(
                "Revenue",
                0,
                SeriesStyle::Default,
                AxisSide::Left,
                vec![point("MAR", 18.0), point("APR", 22.0)],
            )],
            true,
        );
        assert_kind_svg(
            &spec,
            &["<svg", "<g class=\"areas\">", "<polygon", "<polyline"],
        );
    }

    #[test]
    fn scatter_renders() {
        let spec = chart_spec(
            ChartKind::Scatter,
            vec![series(
                "Revenue",
                0,
                SeriesStyle::Default,
                AxisSide::Left,
                vec![scatter_point(1.0, 18.0), scatter_point(2.0, 22.0)],
            )],
            true,
        );
        assert_kind_svg(&spec, &["<svg", "<g class=\"dots\">", "<circle"]);
    }

    #[test]
    fn pie_renders() {
        let spec = chart_spec(
            ChartKind::Pie,
            vec![series(
                "Revenue",
                0,
                SeriesStyle::Default,
                AxisSide::Left,
                vec![point("MAR", 30.0), point("APR", 70.0)],
            )],
            true,
        );
        assert_kind_svg(&spec, &["<svg", "<g class=\"sectors\">", "<path"]);
    }

    #[test]
    fn doughnut_renders() {
        let spec = chart_spec(
            ChartKind::Doughnut,
            vec![series(
                "Revenue",
                0,
                SeriesStyle::Default,
                AxisSide::Left,
                vec![point("MAR", 30.0), point("APR", 70.0)],
            )],
            true,
        );
        assert_kind_svg(&spec, &["<svg", "<g class=\"sectors\">", "<path"]);
    }

    #[test]
    fn stat_renders() {
        let spec = chart_spec(
            ChartKind::Stat,
            vec![series(
                "Users",
                0,
                SeriesStyle::Default,
                AxisSide::Left,
                vec![point("MAR", 42.0)],
            )],
            false,
        );
        assert_kind_svg(&spec, &["<svg", "<g class=\"stat\">", "42"]);
    }

    fn chart_spec(kind: ChartKind, series: Vec<Series>, data_labels: bool) -> Chart {
        Chart {
            kind,
            title: None,
            series,
            axes: Axes::default(),
            legend: LegendSpec::Auto,
            data_labels,
            caption: None,
        }
    }

    fn series(
        name: &str,
        tone: usize,
        style: SeriesStyle,
        axis: AxisSide,
        points: Vec<Point>,
    ) -> Series {
        Series {
            name: CiteOrText::Text(name.to_owned()),
            points,
            tone: ToneRef::Indexed(tone),
            axis,
            style,
        }
    }

    fn point(label: &str, value: f64) -> Point {
        Point {
            label: Some(CiteOrText::Text(label.to_owned())),
            x: None,
            y: cite(value),
        }
    }

    fn scatter_point(x: f64, y: f64) -> Point {
        Point {
            label: None,
            x: Some(CiteOrScalar::Scalar(x)),
            y: cite(y),
        }
    }

    fn cite(value: f64) -> FactCite {
        FactCite {
            id: FactId(format!("fact-{value}")),
            value,
            unit: Unit::Number,
        }
    }

    #[test]
    fn fixed_domain_and_explicit_ticks_appear_in_bar_svg() {
        let mut spec = chart_spec(
            ChartKind::Bar,
            vec![series(
                "A",
                0,
                SeriesStyle::Default,
                AxisSide::Left,
                vec![point("Q1", 10.0), point("Q2", 90.0)],
            )],
            false,
        );
        spec.axes.x.domain = Domain::Fixed { min: 0.0, max: 100.0 };
        spec.axes.x.ticks = Ticks::Explicit(vec![25.0, 50.0, 75.0]);
        let theme = ResolvedTheme::summus_stub();
        let svg = render_chart(
            &spec,
            &theme,
            &Canvas::Deck(DeckCanvas::default()),
            ColorMode::Resolved,
        )
        .expect("renders");
        assert!(svg.contains(">25<"), "explicit tick label should appear: {svg}");
        assert!(svg.contains(">50<"));
        assert!(svg.contains(">75<"));
    }

    #[test]
    fn caption_appears_in_line_svg() {
        let mut spec = chart_spec(
            ChartKind::Line,
            vec![series(
                "A",
                0,
                SeriesStyle::Default,
                AxisSide::Left,
                vec![point("Q1", 10.0), point("Q2", 20.0)],
            )],
            false,
        );
        spec.caption = Some(Inlines(vec!["Fig 1:".to_owned(), "caption text".to_owned()]));
        let theme = ResolvedTheme::summus_stub();
        let svg = render_chart(
            &spec,
            &theme,
            &Canvas::Deck(DeckCanvas::default()),
            ColorMode::Resolved,
        )
        .expect("renders");
        assert!(svg.contains("<g class=\"caption\">"));
        assert!(svg.contains("Fig 1: caption text"));
    }

    #[test]
    fn legend_appears_for_multi_series_when_forced() {
        let spec = Chart {
            kind: ChartKind::Bar,
            title: None,
            series: vec![
                series("A", 0, SeriesStyle::Default, AxisSide::Left, vec![point("Q1", 10.0)]),
                series("B", 1, SeriesStyle::Default, AxisSide::Left, vec![point("Q1", 20.0)]),
            ],
            axes: Axes::default(),
            legend: LegendSpec::TopRight,
            data_labels: false,
            caption: None,
        };
        let theme = ResolvedTheme::summus_stub();
        let svg = render_chart(
            &spec,
            &theme,
            &Canvas::Deck(DeckCanvas::default()),
            ColorMode::Resolved,
        )
        .expect("renders");
        assert!(svg.contains("<g class=\"legend\">"));
        assert!(svg.contains(">A<"));
        assert!(svg.contains(">B<"));
    }

    fn assert_kind_svg(spec: &Chart, markers: &[&str]) {
        let theme = ResolvedTheme::summus_stub();
        let svg = render_chart(
            spec,
            &theme,
            &Canvas::Deck(DeckCanvas::default()),
            ColorMode::Resolved,
        )
        .expect("kind emits");
        assert!(!svg.is_empty());
        assert!(svg.starts_with("<svg"));
        for marker in markers {
            assert!(
                svg.contains(marker),
                "expected {} SVG to contain {marker}, got {svg}",
                spec.kind.name()
            );
        }
    }
}

//! Column chart emitter: vertical grouped bars, 1+ series.
//!
//! # Fixed source order
//!
//! 1. `<svg>` open
//! 2. `<g class="gridlines">` — horizontal gridlines at y-tick positions
//! 3. `<g class="axes">` — y-tick labels + x-category labels
//! 4. `<g class="bars">` — one `<rect>` per (series, category)
//! 5. `<g class="labels">` — value text above each bar (when enabled)
//! 6. `</svg>` close

use std::fmt::Write as _;

use crate::Result;
use crate::format::{coord, format_number};
use crate::model::{Chart, CiteOrText, Unit};
use crate::render::canvas::{Canvas, PlotBox};
use crate::scale::{self, Scale};
use crate::theme::{ColorMode, ResolvedTheme};

// WHY: the value below is the W3C SVG 1.1 namespace identifier — a fixed URI
// literal mandated by the SVG spec. Renderers match it as an opaque string;
// substituting `https://` produces SVG that browsers refuse to render.
const SVG_NAMESPACE: &str = "http://www.w3.org/2000/svg";

/// Emit the column chart SVG.
///
/// Caller invariant (enforced by [`Chart::validate`](crate::model::Chart::validate)):
/// - `chart.kind == ChartKind::Column`
/// - `chart.series` is non-empty
pub fn emit(
    chart: &Chart,
    theme: &ResolvedTheme,
    canvas: &Canvas,
    mode: ColorMode,
) -> Result<String> {
    let plot = canvas.plot_box();
    let n_cats = chart
        .series
        .iter()
        .map(|s| s.points.len())
        .max()
        .unwrap_or(0);
    if n_cats == 0 {
        return Err(crate::Error::BadSeriesShape {
            kind: "column".to_owned(),
            expected: "1+ data points".to_owned(),
            actual: "0 points".to_owned(),
            path: "/series/0/points".to_owned(),
        });
    }

    let all_values: Vec<f64> = chart
        .series
        .iter()
        .flat_map(|s| s.points.iter().map(|p| p.y.value))
        .collect();
    let (lo, hi) = nice_domain(&all_values);
    let y_scale = Scale::new((lo, hi), (plot.y1, plot.y0));

    let n_cats_f = idx_to_f64(n_cats);
    let n_series_f = idx_to_f64(chart.series.len());
    let band_w = plot.width() / n_cats_f;
    let sub_w = band_w / n_series_f;
    let bar_w = sub_w * 0.8;

    let fills: Vec<String> = chart
        .series
        .iter()
        .enumerate()
        .map(|(i, s)| theme.fill_for(&s.tone, mode, i))
        .collect::<Result<Vec<_>>>()?;

    let mut out = String::new();
    emit_svg_open(&mut out, chart, canvas);
    emit_gridlines(&mut out, lo, hi, &y_scale, &plot);
    emit_axes(&mut out, chart, lo, hi, &y_scale, &plot, band_w, theme);
    emit_bars(
        &mut out, chart, &y_scale, &plot, band_w, sub_w, bar_w, &fills,
    );
    if chart.data_labels {
        emit_labels(
            &mut out, chart, &y_scale, &plot, band_w, sub_w, theme, &fills,
        );
    }
    out.push_str("</svg>");
    Ok(out)
}

fn emit_svg_open(out: &mut String, chart: &Chart, canvas: &Canvas) {
    let _ = write!(
        out,
        "<svg xmlns=\"{ns}\" \
         viewBox=\"0 0 {w} {h}\" \
         preserveAspectRatio=\"{aspect}\" \
         role=\"img\" aria-label=\"{aria}\">",
        ns = SVG_NAMESPACE,
        w = canvas.width(),
        h = canvas.height(),
        aspect = canvas.preserve_aspect_ratio(),
        aria = aria_label(chart),
    );
}

fn emit_gridlines(out: &mut String, lo: f64, hi: f64, y_scale: &Scale, plot: &PlotBox) {
    out.push_str("<g class=\"gridlines\">");
    let ticks = scale::ticks(lo, hi, 5);
    for tick in &ticks {
        let tick_y = y_scale.map(*tick);
        let _ = write!(
            out,
            "<line x1=\"{x1}\" y1=\"{y}\" x2=\"{x2}\" y2=\"{y}\" stroke=\"#e5e7eb\" stroke-width=\"1\"/>",
            x1 = coord(plot.x0),
            y = coord(tick_y),
            x2 = coord(plot.x1),
        );
    }
    out.push_str("</g>");
}

fn emit_axes(
    out: &mut String,
    chart: &Chart,
    lo: f64,
    hi: f64,
    y_scale: &Scale,
    plot: &PlotBox,
    band_w: f64,
    theme: &ResolvedTheme,
) {
    out.push_str("<g class=\"axes\">");

    // y-tick labels
    let ticks = scale::ticks(lo, hi, 5);
    for tick in &ticks {
        let tick_y = y_scale.map(*tick);
        let tick_label = format_number(*tick, chart.axes.y_left.format, Unit::Number);
        let _ = write!(
            out,
            "<text x=\"{x}\" y=\"{y}\" text-anchor=\"end\" dominant-baseline=\"middle\" font-family=\"{font}\">{label}</text>",
            x = coord(plot.x0 - 8.0),
            y = coord(tick_y),
            font = theme.font_sans,
            label = escape_xml(&tick_label),
        );
    }

    // x-category labels
    if let Some(first_series) = chart.series.first() {
        for (j, point) in first_series.points.iter().enumerate() {
            let cx = plot.x0 + band_w * idx_to_f64(j) + band_w * 0.5;
            let label = match &point.label {
                Some(CiteOrText::Text(t)) => t.clone(),
                Some(CiteOrText::Cite(id)) => id.0.clone(),
                None => String::new(),
            };
            let _ = write!(
                out,
                "<text x=\"{x}\" y=\"{y}\" text-anchor=\"middle\" font-family=\"{font}\">{label}</text>",
                x = coord(cx),
                y = coord(plot.y1 + 24.0),
                font = theme.font_sans,
                label = escape_xml(&label),
            );
        }
    }

    out.push_str("</g>");
}

fn emit_bars(
    out: &mut String,
    chart: &Chart,
    y_scale: &Scale,
    plot: &PlotBox,
    band_w: f64,
    sub_w: f64,
    bar_w: f64,
    fills: &[String],
) {
    out.push_str("<g class=\"bars\">");
    for (i, (series, fill)) in chart.series.iter().zip(fills.iter()).enumerate() {
        for (j, point) in series.points.iter().enumerate() {
            let x = plot.x0 + band_w * idx_to_f64(j) + sub_w * idx_to_f64(i) + sub_w * 0.1;
            let y = y_scale.map(point.y.value);
            let h = plot.y1 - y;
            let _ = write!(
                out,
                "<rect x=\"{x}\" y=\"{y}\" width=\"{w}\" height=\"{h}\" rx=\"2\" fill=\"{fill}\"/>",
                x = coord(x),
                y = coord(y),
                w = coord(bar_w),
                h = coord(h),
                fill = fill,
            );
        }
    }
    out.push_str("</g>");
}

fn emit_labels(
    out: &mut String,
    chart: &Chart,
    y_scale: &Scale,
    plot: &PlotBox,
    band_w: f64,
    sub_w: f64,
    theme: &ResolvedTheme,
    fills: &[String],
) {
    out.push_str("<g class=\"labels\">");
    for (i, (series, fill)) in chart.series.iter().zip(fills.iter()).enumerate() {
        for (j, point) in series.points.iter().enumerate() {
            let cx = plot.x0 + band_w * idx_to_f64(j) + sub_w * idx_to_f64(i) + sub_w * 0.5;
            let label_y = y_scale.map(point.y.value) - 6.0;
            let text = format_number(point.y.value, chart.axes.y_left.format, point.y.unit);
            let _ = write!(
                out,
                "<text x=\"{x}\" y=\"{y}\" text-anchor=\"middle\" dominant-baseline=\"auto\" font-family=\"{font}\" fill=\"{fill}\">{label}</text>",
                x = coord(cx),
                y = coord(label_y),
                font = theme.font_sans,
                fill = fill,
                label = escape_xml(&text),
            );
        }
    }
    out.push_str("</g>");
}

#[expect(
    clippy::cast_precision_loss,
    clippy::as_conversions,
    reason = "category index never approaches f64 mantissa limit"
)]
const fn idx_to_f64(i: usize) -> f64 {
    i as f64
}

fn nice_domain(values: &[f64]) -> (f64, f64) {
    let (mut lo, mut hi) = (f64::INFINITY, f64::NEG_INFINITY);
    for v in values {
        if *v < lo {
            lo = *v;
        }
        if *v > hi {
            hi = *v;
        }
    }
    if lo > 0.0 {
        lo = 0.0;
    }
    if !lo.is_finite() || !hi.is_finite() {
        return (0.0, 1.0);
    }
    scale::nice(lo, hi)
}

fn aria_label(chart: &Chart) -> String {
    match &chart.title {
        Some(CiteOrText::Text(t)) => escape_xml(t),
        Some(CiteOrText::Cite(id)) => escape_xml(&id.0),
        None => format!("{} chart", chart.kind.name()),
    }
}

fn escape_xml(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test assertions")]
mod tests {
    use super::*;
    use crate::model::{
        Axes, AxisSide, Chart, ChartKind, CiteOrText, FactCite, FactId, LegendSpec, Point, Series,
        SeriesStyle, ToneRef, Unit,
    };
    use crate::render::canvas::DeckCanvas;

    fn cite(id: &str, v: f64) -> FactCite {
        FactCite {
            id: FactId(id.to_owned()),
            value: v,
            unit: Unit::Number,
        }
    }

    fn pt(label: &str, c: FactCite) -> Point {
        Point {
            label: Some(CiteOrText::Text(label.to_owned())),
            x: None,
            y: c,
        }
    }

    fn single_chart() -> Chart {
        Chart {
            kind: ChartKind::Column,
            title: None,
            series: vec![Series {
                name: CiteOrText::Text("A".to_owned()),
                points: vec![
                    pt("Q1", cite("f1", 10.0)),
                    pt("Q2", cite("f2", 20.0)),
                    pt("Q3", cite("f3", 30.0)),
                ],
                tone: ToneRef::Indexed(0),
                axis: AxisSide::Left,
                style: SeriesStyle::Default,
            }],
            axes: Axes::default(),
            legend: LegendSpec::Auto,
            data_labels: true,
            caption: None,
        }
    }

    #[test]
    fn single_series_emits_nonempty_svg() {
        let theme = ResolvedTheme::summus_stub();
        let svg = emit(
            &single_chart(),
            &theme,
            &Canvas::Deck(DeckCanvas::default()),
            ColorMode::Resolved,
        )
        .expect("emit");
        assert!(svg.starts_with("<svg"));
        assert!(svg.contains("rx=\"2\""));
    }

    #[test]
    fn themed_mode_emits_css_var() {
        let theme = ResolvedTheme::summus_stub();
        let svg = emit(
            &single_chart(),
            &theme,
            &Canvas::Deck(DeckCanvas::default()),
            ColorMode::Themed,
        )
        .expect("emit");
        assert!(svg.contains("var(--tone-series-0)"));
        assert!(!svg.contains("#232E54"));
    }

    #[test]
    fn output_is_deterministic() {
        let theme = ResolvedTheme::summus_stub();
        let a = emit(
            &single_chart(),
            &theme,
            &Canvas::Deck(DeckCanvas::default()),
            ColorMode::Resolved,
        )
        .expect("emit a");
        let b = emit(
            &single_chart(),
            &theme,
            &Canvas::Deck(DeckCanvas::default()),
            ColorMode::Resolved,
        )
        .expect("emit b");
        assert_eq!(a, b);
    }

    #[test]
    fn multi_series_emits_grouped_rects() {
        let chart = Chart {
            kind: ChartKind::Column,
            title: None,
            series: vec![
                Series {
                    name: CiteOrText::Text("A".to_owned()),
                    points: vec![
                        pt("Q1", cite("f1", 10.0)),
                        pt("Q2", cite("f2", 20.0)),
                        pt("Q3", cite("f3", 30.0)),
                    ],
                    tone: ToneRef::Indexed(0),
                    axis: AxisSide::Left,
                    style: SeriesStyle::Default,
                },
                Series {
                    name: CiteOrText::Text("B".to_owned()),
                    points: vec![
                        pt("Q1", cite("f4", 15.0)),
                        pt("Q2", cite("f5", 25.0)),
                        pt("Q3", cite("f6", 35.0)),
                    ],
                    tone: ToneRef::Indexed(1),
                    axis: AxisSide::Left,
                    style: SeriesStyle::Default,
                },
            ],
            axes: Axes::default(),
            legend: LegendSpec::Auto,
            data_labels: false,
            caption: None,
        };
        let theme = ResolvedTheme::summus_stub();
        let svg = emit(
            &chart,
            &theme,
            &Canvas::Deck(DeckCanvas::default()),
            ColorMode::Resolved,
        )
        .expect("emit");
        let rect_count = svg.matches("<rect").count();
        assert!(
            rect_count >= 6,
            "expected at least 6 rects, got {rect_count}"
        );
    }
}

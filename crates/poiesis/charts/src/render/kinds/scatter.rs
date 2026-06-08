//! Scatter plot emitter: 1+ series, both axes linear.
//!
//! # Fixed source order
//!
//! Every emitted SVG follows this order, byte-for-byte:
//!
//! 1. `<svg>` open + viewBox + `preserveAspectRatio` + `role` + `aria-label`
//! 2. `<g class="gridlines">` — horizontal lines at each y-tick position
//! 3. `<g class="axes">` — y-tick labels + x-tick labels
//! 4. `<g class="dots">` — one `<circle>` per point with valid x
//! 5. `<g class="labels">` — data labels above each circle (if enabled)
//! 6. `</svg>` close
//!
//! Coordinates round to 2 dp via [`crate::format::coord`]; numeric text
//! routes through [`crate::format::format_number`]. No `format!("{}", f64)`
//! anywhere; no map iteration into output; no random IDs.

use std::fmt::Write as _;

use crate::Result;
use crate::format::{coord, format_number};
use crate::model::{Chart, CiteOrScalar, CiteOrText, Unit};
use crate::render::canvas::{Canvas, PlotBox};
use crate::scale::{self, Scale};
use crate::theme::{ColorMode, ResolvedTheme};

// WHY: the value below is the W3C SVG 1.1 namespace identifier — a fixed URI
// literal mandated by the SVG spec. Renderers match it as an opaque string;
// substituting `https://` produces SVG that browsers refuse to render.
const SVG_NAMESPACE: &str = "http://www.w3.org/2000/svg";

/// Emit the scatter chart SVG.
///
/// Caller invariants (enforced by [`Chart::validate`](crate::model::Chart::validate)):
/// - `chart.kind == ChartKind::Scatter`
/// - `1+ series`
pub fn emit(
    chart: &Chart,
    theme: &ResolvedTheme,
    canvas: &Canvas,
    mode: ColorMode,
) -> Result<String> {
    let plot = canvas.plot_box();

    let x_values: Vec<f64> = chart
        .series
        .iter()
        .flat_map(|s| s.points.iter().filter_map(point_x))
        .collect();
    let (x_lo, x_hi) = if x_values.is_empty() {
        (0.0, 1.0)
    } else {
        nice_domain(&x_values)
    };

    let y_values: Vec<f64> = chart
        .series
        .iter()
        .flat_map(|s| s.points.iter().map(|p| p.y.value))
        .collect();
    let (y_lo, y_hi) = nice_domain(&y_values);

    let x_scale = Scale::new((x_lo, x_hi), (plot.x0, plot.x1));
    let y_scale = Scale::new((y_lo, y_hi), (plot.y1, plot.y0));

    let x_ticks = scale::ticks(x_lo, x_hi, 5);
    let y_ticks = scale::ticks(y_lo, y_hi, 5);

    let mut out = String::new();
    emit_svg_open(&mut out, chart, canvas);
    emit_gridlines(&mut out, &plot, &y_scale, &y_ticks);
    emit_axes(
        &mut out, &plot, &x_scale, &y_scale, &x_ticks, &y_ticks, theme, chart,
    );
    emit_dots(&mut out, chart, &x_scale, &y_scale, theme, mode)?;
    if chart.data_labels {
        emit_labels(&mut out, chart, &x_scale, &y_scale, theme, mode)?;
    }
    out.push_str("</svg>");
    Ok(out)
}

fn point_x(p: &crate::model::Point) -> Option<f64> {
    match &p.x {
        Some(CiteOrScalar::Cite(fc)) => Some(fc.value),
        Some(CiteOrScalar::Scalar(v)) => Some(*v),
        None => None,
    }
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

fn emit_gridlines(out: &mut String, plot: &PlotBox, y_scale: &Scale, y_ticks: &[f64]) {
    out.push_str("<g class=\"gridlines\">");
    for tick in y_ticks {
        let y = y_scale.map(*tick);
        let _ = write!(
            out,
            "<line x1=\"{x1}\" y1=\"{y}\" x2=\"{x2}\" y2=\"{y}\" stroke=\"#e5e7eb\" stroke-width=\"1\"/>",
            x1 = coord(plot.x0),
            y = coord(y),
            x2 = coord(plot.x1),
        );
    }
    out.push_str("</g>");
}

fn emit_axes(
    out: &mut String,
    plot: &PlotBox,
    x_scale: &Scale,
    y_scale: &Scale,
    x_ticks: &[f64],
    y_ticks: &[f64],
    theme: &ResolvedTheme,
    chart: &Chart,
) {
    out.push_str("<g class=\"axes\">");
    for tick in y_ticks {
        let y = y_scale.map(*tick);
        let label = format_number(*tick, chart.axes.y_left.format, Unit::Number);
        let _ = write!(
            out,
            "<text x=\"{x}\" y=\"{y}\" text-anchor=\"end\" dominant-baseline=\"middle\" font-family=\"{font}\">{label}</text>",
            x = coord(plot.x0 - 8.0),
            y = coord(y),
            font = theme.font_sans,
            label = escape_xml(&label),
        );
    }
    for tick in x_ticks {
        let x = x_scale.map(*tick);
        let label = format_number(*tick, chart.axes.x.format, Unit::Number);
        let _ = write!(
            out,
            "<text x=\"{x}\" y=\"{y}\" text-anchor=\"middle\" font-family=\"{font}\">{label}</text>",
            x = coord(x),
            y = coord(plot.y1 + 24.0),
            font = theme.font_sans,
            label = escape_xml(&label),
        );
    }
    out.push_str("</g>");
}

fn emit_dots(
    out: &mut String,
    chart: &Chart,
    x_scale: &Scale,
    y_scale: &Scale,
    theme: &ResolvedTheme,
    mode: ColorMode,
) -> Result<()> {
    out.push_str("<g class=\"dots\">");
    for (i, series) in chart.series.iter().enumerate() {
        let fill = theme.fill_for(&series.tone, mode, i)?;
        for point in &series.points {
            if let Some(x_val) = point_x(point) {
                let cx = x_scale.map(x_val);
                let cy = y_scale.map(point.y.value);
                let _ = write!(
                    out,
                    "<circle cx=\"{cx}\" cy=\"{cy}\" r=\"9\" fill=\"{fill}\"/>",
                    cx = coord(cx),
                    cy = coord(cy),
                    fill = fill,
                );
            }
        }
    }
    out.push_str("</g>");
    Ok(())
}

fn emit_labels(
    out: &mut String,
    chart: &Chart,
    x_scale: &Scale,
    y_scale: &Scale,
    theme: &ResolvedTheme,
    mode: ColorMode,
) -> Result<()> {
    out.push_str("<g class=\"labels\">");
    for (i, series) in chart.series.iter().enumerate() {
        let fill = theme.fill_for(&series.tone, mode, i)?;
        for point in &series.points {
            if let Some(x_val) = point_x(point) {
                let cx = x_scale.map(x_val);
                let cy = y_scale.map(point.y.value);
                let text = format_number(point.y.value, chart.axes.y_left.format, point.y.unit);
                let _ = write!(
                    out,
                    "<text x=\"{x}\" y=\"{y}\" text-anchor=\"middle\" dominant-baseline=\"auto\" font-family=\"{font}\" fill=\"{fill}\">{text}</text>",
                    x = coord(cx),
                    y = coord(cy - 14.0),
                    font = theme.font_sans,
                    fill = fill,
                    text = escape_xml(&text),
                );
            }
        }
    }
    out.push_str("</g>");
    Ok(())
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
        None => "scatter chart".to_owned(),
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
        Axes, AxisSide, Chart, ChartKind, CiteOrScalar, CiteOrText, FactCite, FactId, LegendSpec,
        Point, Series, SeriesStyle, ToneRef, Unit,
    };
    use crate::render::canvas::DeckCanvas;

    fn cite(id: &str, v: f64) -> FactCite {
        FactCite {
            id: FactId(id.to_owned()),
            value: v,
            unit: Unit::Number,
        }
    }

    fn pt_xy(x: f64, y_cite: FactCite) -> Point {
        Point {
            label: None,
            x: Some(CiteOrScalar::Scalar(x)),
            y: y_cite,
        }
    }

    fn scatter_chart() -> Chart {
        Chart {
            kind: ChartKind::Scatter,
            title: None,
            series: vec![Series {
                name: CiteOrText::Text("Series A".to_owned()),
                points: vec![
                    pt_xy(1.0, cite("y1", 10.0)),
                    pt_xy(2.0, cite("y2", 20.0)),
                    pt_xy(3.0, cite("y3", 15.0)),
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
    fn single_series_emits_circles() {
        let theme = ResolvedTheme::summus_stub();
        let svg = emit(
            &scatter_chart(),
            &theme,
            &Canvas::Deck(DeckCanvas::default()),
            ColorMode::Resolved,
        )
        .expect("scatter emits");
        assert!(svg.starts_with("<svg"));
        assert!(svg.contains("<circle"));
    }

    #[test]
    fn themed_mode_emits_css_var() {
        let theme = ResolvedTheme::summus_stub();
        let svg = emit(
            &scatter_chart(),
            &theme,
            &Canvas::Deck(DeckCanvas::default()),
            ColorMode::Themed,
        )
        .expect("themed mode emits");
        assert!(svg.contains("var(--tone-series-0)"));
        assert!(!svg.contains("#232E54"));
    }

    #[test]
    fn output_is_deterministic() {
        let theme = ResolvedTheme::summus_stub();
        let a = emit(
            &scatter_chart(),
            &theme,
            &Canvas::Deck(DeckCanvas::default()),
            ColorMode::Resolved,
        )
        .expect("first emit");
        let b = emit(
            &scatter_chart(),
            &theme,
            &Canvas::Deck(DeckCanvas::default()),
            ColorMode::Resolved,
        )
        .expect("second emit");
        assert_eq!(a, b);
    }

    #[test]
    fn multi_series_different_fills() {
        let mut chart = scatter_chart();
        chart.series.push(Series {
            name: CiteOrText::Text("Series B".to_owned()),
            points: vec![pt_xy(1.5, cite("y4", 12.0)), pt_xy(2.5, cite("y5", 18.0))],
            tone: ToneRef::Indexed(1),
            axis: AxisSide::Left,
            style: SeriesStyle::Default,
        });
        let theme = ResolvedTheme::summus_stub();
        let svg = emit(
            &chart,
            &theme,
            &Canvas::Deck(DeckCanvas::default()),
            ColorMode::Themed,
        )
        .expect("multi-series emits");
        assert!(svg.contains("var(--tone-series-0)"));
        assert!(svg.contains("var(--tone-series-1)"));
    }
}

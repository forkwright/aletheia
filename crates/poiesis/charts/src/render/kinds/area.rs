//! Area chart emitter: filled polygon + stroked top edge + circle markers, 1+ series.
//!
//! # Fixed source order
//!
//! 1. `<svg>` open + viewBox + `preserveAspectRatio` + `role` + `aria-label`
//! 2. `<g class="gridlines">` — horizontal gridlines at y-tick positions
//! 3. `<g class="axes">` — y-tick labels + x-category labels
//! 4. `<g class="areas">` — one `<polygon>` + top-edge `<polyline>` per series
//! 5. `<g class="markers">` — `<circle>` markers per series per point
//! 6. `<g class="labels">` — on-point value labels (conditional)
//! 7. `</svg>` close
//!
//! Coordinates round to 2 dp via [`crate::format::coord`]; numeric text
//! routes through [`crate::format::format_number`].

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

/// Emit the area chart SVG.
///
/// Caller invariants (enforced by [`Chart::validate`]):
/// - `chart.kind == ChartKind::Area`
/// - one or more series, each with one or more points
pub fn emit(
    chart: &Chart,
    theme: &ResolvedTheme,
    canvas: &Canvas,
    mode: ColorMode,
) -> Result<String> {
    let plot = canvas.plot_box();

    let n_pts = chart
        .series
        .iter()
        .map(|s| s.points.len())
        .max()
        .unwrap_or(0);
    if n_pts == 0 {
        return Err(crate::Error::BadSeriesShape {
            kind: "area".to_owned(),
            expected: "1+ data points".to_owned(),
            actual: "0 points".to_owned(),
            path: "/series/0/points".to_owned(),
        });
    }

    let n_pts_f = idx_to_f64(n_pts);
    let band_w = plot.width() / n_pts_f;

    let all_values: Vec<f64> = chart
        .series
        .iter()
        .flat_map(|s| s.points.iter().map(|p| p.y.value))
        .collect();
    let (lo, hi) = nice_domain(&all_values);
    let y_scale = Scale::new((lo, hi), (plot.y1, plot.y0));

    let mut out = String::new();
    emit_svg_open(&mut out, chart, canvas);
    emit_gridlines(&mut out, &y_scale, &plot, lo, hi);
    emit_axes(&mut out, chart, &y_scale, &plot, band_w, theme, lo, hi);
    emit_areas(&mut out, chart, &y_scale, &plot, band_w, theme, mode)?;
    emit_markers(&mut out, chart, &y_scale, &plot, band_w, theme, mode)?;
    if chart.data_labels {
        emit_data_labels(&mut out, chart, &y_scale, &plot, band_w, theme, mode)?;
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

fn emit_gridlines(out: &mut String, y_scale: &Scale, plot: &PlotBox, lo: f64, hi: f64) {
    out.push_str("<g class=\"gridlines\">");
    for tick in scale::ticks(lo, hi, 5) {
        let y = y_scale.map(tick);
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
    chart: &Chart,
    y_scale: &Scale,
    plot: &PlotBox,
    band_w: f64,
    theme: &ResolvedTheme,
    lo: f64,
    hi: f64,
) {
    out.push_str("<g class=\"axes\">");

    for tick in scale::ticks(lo, hi, 5) {
        let y = y_scale.map(tick);
        let label = escape_xml(&format_number(tick, chart.axes.y_left.format, Unit::Number));
        let _ = write!(
            out,
            "<text x=\"{x}\" y=\"{y}\" text-anchor=\"end\" dominant-baseline=\"middle\" font-family=\"{font}\">{label}</text>",
            x = coord(plot.x0 - 8.0),
            y = coord(y),
            font = theme.font_sans,
        );
    }

    if let Some(series) = chart.series.first() {
        for (j, point) in series.points.iter().enumerate() {
            let cx = plot.x0 + band_w * idx_to_f64(j) + band_w * 0.5;
            let label = match &point.label {
                Some(CiteOrText::Text(t)) => escape_xml(t),
                Some(CiteOrText::Cite(id)) => escape_xml(&id.0),
                None => String::new(),
            };
            let _ = write!(
                out,
                "<text x=\"{x}\" y=\"{y}\" text-anchor=\"middle\" font-family=\"{font}\">{label}</text>",
                x = coord(cx),
                y = coord(plot.y1 + 24.0),
                font = theme.font_sans,
            );
        }
    }

    out.push_str("</g>");
}

fn emit_areas(
    out: &mut String,
    chart: &Chart,
    y_scale: &Scale,
    plot: &PlotBox,
    band_w: f64,
    theme: &ResolvedTheme,
    mode: ColorMode,
) -> Result<()> {
    out.push_str("<g class=\"areas\">");
    for (i, series) in chart.series.iter().enumerate() {
        let fill = theme.fill_for(&series.tone, mode, i)?;

        // Polygon: baseline up through data points and back down to baseline.
        let mut poly_pts = String::new();
        for (j, point) in series.points.iter().enumerate() {
            let cx = plot.x0 + band_w * idx_to_f64(j) + band_w * 0.5;
            let cy = y_scale.map(point.y.value);
            if j == 0 {
                let _ = write!(poly_pts, "{}, {}", coord(cx), coord(plot.y1));
            }
            let _ = write!(poly_pts, " {}, {}", coord(cx), coord(cy));
            if j == series.points.len().saturating_sub(1) {
                let _ = write!(poly_pts, " {}, {}", coord(cx), coord(plot.y1));
            }
        }
        let _ = write!(
            out,
            "<polygon points=\"{poly_pts}\" fill=\"{fill}\" fill-opacity=\"0.25\"/>",
        );

        // Stroked top edge: data points only.
        let mut edge_pts = String::new();
        for (j, point) in series.points.iter().enumerate() {
            let cx = plot.x0 + band_w * idx_to_f64(j) + band_w * 0.5;
            let cy = y_scale.map(point.y.value);
            if j > 0 {
                edge_pts.push(' ');
            }
            let _ = write!(edge_pts, "{}, {}", coord(cx), coord(cy));
        }
        let _ = write!(
            out,
            "<polyline fill=\"none\" stroke=\"{fill}\" stroke-width=\"2\" points=\"{edge_pts}\"/>",
        );
    }
    out.push_str("</g>");
    Ok(())
}

fn emit_markers(
    out: &mut String,
    chart: &Chart,
    y_scale: &Scale,
    plot: &PlotBox,
    band_w: f64,
    theme: &ResolvedTheme,
    mode: ColorMode,
) -> Result<()> {
    out.push_str("<g class=\"markers\">");
    for (i, series) in chart.series.iter().enumerate() {
        let fill = theme.fill_for(&series.tone, mode, i)?;
        for (j, point) in series.points.iter().enumerate() {
            let cx = plot.x0 + band_w * idx_to_f64(j) + band_w * 0.5;
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
    out.push_str("</g>");
    Ok(())
}

fn emit_data_labels(
    out: &mut String,
    chart: &Chart,
    y_scale: &Scale,
    plot: &PlotBox,
    band_w: f64,
    theme: &ResolvedTheme,
    mode: ColorMode,
) -> Result<()> {
    out.push_str("<g class=\"labels\">");
    for (i, series) in chart.series.iter().enumerate() {
        let fill = theme.fill_for(&series.tone, mode, i)?;
        for (j, point) in series.points.iter().enumerate() {
            let cx = plot.x0 + band_w * idx_to_f64(j) + band_w * 0.5;
            let cy = y_scale.map(point.y.value);
            let label = escape_xml(&format_number(
                point.y.value,
                chart.axes.y_left.format,
                point.y.unit,
            ));
            let _ = write!(
                out,
                "<text x=\"{x}\" y=\"{y}\" text-anchor=\"middle\" dominant-baseline=\"auto\" font-family=\"{font}\" fill=\"{fill}\">{label}</text>",
                x = coord(cx),
                y = coord(cy - 14.0),
                font = theme.font_sans,
                fill = fill,
            );
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
        None => "area chart".to_owned(),
    }
}

fn escape_xml(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

#[expect(
    clippy::cast_precision_loss,
    clippy::as_conversions,
    reason = "category index never approaches f64 mantissa limit"
)]
const fn idx_to_f64(i: usize) -> f64 {
    i as f64
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

    fn area_spec() -> Chart {
        let cite = |id: &str, v: f64, u: Unit| FactCite {
            id: FactId(id.to_owned()),
            value: v,
            unit: u,
        };
        let pt = |label: &str, c: FactCite| Point {
            label: Some(CiteOrText::Text(label.to_owned())),
            x: None,
            y: c,
        };
        Chart {
            kind: ChartKind::Area,
            title: Some(CiteOrText::Text("Test Area".to_owned())),
            series: vec![Series {
                name: CiteOrText::Text("Series 1".to_owned()),
                points: vec![
                    pt("A", cite("f1", 10.0, Unit::Number)),
                    pt("B", cite("f2", 20.0, Unit::Number)),
                    pt("C", cite("f3", 15.0, Unit::Number)),
                ],
                tone: ToneRef::Indexed(0),
                axis: AxisSide::Left,
                style: SeriesStyle::Default,
            }],
            axes: Axes::default(),
            legend: LegendSpec::Auto,
            data_labels: false,
            caption: None,
        }
    }

    #[test]
    fn single_series_emits_polygon_and_polyline() {
        let theme = ResolvedTheme::summus_stub();
        let svg = emit(
            &area_spec(),
            &theme,
            &Canvas::Deck(DeckCanvas::default()),
            ColorMode::Resolved,
        )
        .expect("area emits");
        assert!(svg.starts_with("<svg"));
        assert!(svg.contains("<polygon"));
        assert!(svg.contains("<polyline"));
    }

    #[test]
    fn themed_mode_emits_css_var() {
        let theme = ResolvedTheme::summus_stub();
        let svg = emit(
            &area_spec(),
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
            &area_spec(),
            &theme,
            &Canvas::Deck(DeckCanvas::default()),
            ColorMode::Resolved,
        )
        .expect("first emit");
        let b = emit(
            &area_spec(),
            &theme,
            &Canvas::Deck(DeckCanvas::default()),
            ColorMode::Resolved,
        )
        .expect("second emit");
        assert_eq!(a, b);
    }

    #[test]
    fn multi_series_emits_multiple_polygons() {
        let mut spec = area_spec();
        spec.series.push(Series {
            name: CiteOrText::Text("Series 2".to_owned()),
            points: vec![
                Point {
                    label: Some(CiteOrText::Text("A".to_owned())),
                    x: None,
                    y: FactCite {
                        id: FactId("f4".to_owned()),
                        value: 5.0,
                        unit: Unit::Number,
                    },
                },
                Point {
                    label: Some(CiteOrText::Text("B".to_owned())),
                    x: None,
                    y: FactCite {
                        id: FactId("f5".to_owned()),
                        value: 25.0,
                        unit: Unit::Number,
                    },
                },
                Point {
                    label: Some(CiteOrText::Text("C".to_owned())),
                    x: None,
                    y: FactCite {
                        id: FactId("f6".to_owned()),
                        value: 10.0,
                        unit: Unit::Number,
                    },
                },
            ],
            tone: ToneRef::Indexed(1),
            axis: AxisSide::Left,
            style: SeriesStyle::Default,
        });
        let theme = ResolvedTheme::summus_stub();
        let svg = emit(
            &spec,
            &theme,
            &Canvas::Deck(DeckCanvas::default()),
            ColorMode::Resolved,
        )
        .expect("multi-series emits");
        let count = svg.matches("<polygon").count();
        assert!(count >= 2, "expected at least 2 polygons, got {count}");
    }
}

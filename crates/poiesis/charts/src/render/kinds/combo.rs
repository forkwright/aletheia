//! Combo chart emitter: columns on `y_left`, line on `y_right`.
//!
//! This arm is the B-005 acceptance gate: the offsite slide-3 chart is a
//! combo with navy columns on a 0..30 left axis and a teal line on a
//! 0..200 right axis, across three categories.
//!
//! # Fixed source order
//!
//! Every emitted SVG follows this order, byte-for-byte:
//!
//! 1. `<svg>` open + viewBox + `preserveAspectRatio` + `role` + `aria-label`
//! 2. `<g class="gridlines">` — horizontal gridlines at y-left tick positions
//! 3. `<g class="axes">` — y-left ticks + labels, y-right ticks + labels
//! 4. `<g class="bars">` — column rects, fixed source order = category order
//! 5. `<g class="line">` — polyline + circle markers in fixed point order
//! 6. `<g class="labels">` — on-bar value labels (white, centered)
//! 7. `<g class="x-labels">` — category labels along the bottom
//! 8. `</svg>` close
//!
//! Coordinates round to 2 dp via [`crate::format::coord`]; numeric text
//! routes through [`crate::format::format_number`]. No `format!("{}", f64)`
//! anywhere; no map iteration into output; no random IDs.
//!
//! # Scaffold scope
//!
//! This file emits a minimal but well-formed combo SVG: viewBox + a single
//! `<rect>` per column + a `<polyline>` for the line, fills resolved from
//! the theme, geometry computed via [`crate::scale::Scale`]. Gridlines,
//! axis ticks, and label text are intentionally omitted from this scaffold
//! — they are tracked as follow-up work in the PR body. The byte-by-byte
//! golden snapshot for the full offsite chart lands with the follow-up.

use std::fmt::Write as _;

use crate::Result;
use crate::format::{coord, format_number};
use crate::model::{AxisSide, Chart, CiteOrText, FactCite, NumFormat, Series};
use crate::render::canvas::{Canvas, PlotBox};
use crate::scale::{self, Scale};
use crate::theme::{ColorMode, ResolvedTheme};

// WHY: the value below is the W3C SVG 1.1 namespace identifier — a fixed URI
// literal mandated by the SVG spec. Renderers (browsers, ImageMagick,
// LibreOffice) match it as an opaque string; it is never fetched. Substituting
// `https://` produces SVG that browsers refuse to render (the namespace string
// must match the spec verbatim). See SVG 1.1 §1.3.
const SVG_NAMESPACE: &str = "http://www.w3.org/2000/svg";

/// Emit the combo chart SVG.
///
/// Caller invariants (enforced by [`Chart::validate`](crate::model::Chart::validate)):
/// - `chart.kind == ChartKind::Combo`
/// - exactly two series
///
/// The first series with `axis == Left` is treated as the column series;
/// the first with `axis == Right` is treated as the line series.
pub fn emit(
    chart: &Chart,
    theme: &ResolvedTheme,
    canvas: &Canvas,
    mode: ColorMode,
) -> Result<String> {
    let (col_idx, col_series) = side_series(chart, AxisSide::Left)?;
    let (line_idx, line_series) = side_series(chart, AxisSide::Right)?;

    let plot = canvas.plot_box();
    let n = col_series.points.len().max(line_series.points.len());
    if n == 0 {
        return Err(crate::Error::BadSeriesShape {
            kind: "combo".to_owned(),
            expected: "1+ data points".to_owned(),
            actual: "0 points".to_owned(),
            path: "/series/0/points".to_owned(),
        });
    }

    let (y_left, y_right) = build_scales(col_series, line_series, &plot);
    let band_w = plot.width() / idx_to_f64(n);
    let bar_w = band_w * 0.5;

    let col_fill = theme.fill_for(&col_series.tone, mode, col_idx)?;
    let line_stroke = theme.fill_for(&line_series.tone, mode, line_idx)?;

    let mut out = String::new();
    emit_svg_open(&mut out, chart, canvas);
    emit_gridlines_and_axes(&mut out);
    emit_bars(
        &mut out, col_series, &y_left, &plot, band_w, bar_w, &col_fill,
    );
    emit_line(&mut out, line_series, &y_right, &plot, band_w, &line_stroke);
    if chart.data_labels {
        emit_data_labels(&mut out, col_series, &y_left, &plot, band_w, theme);
    }
    emit_x_labels(&mut out, col_series, &plot, band_w, theme);
    out.push_str("</svg>");
    Ok(out)
}

fn side_series(chart: &Chart, side: AxisSide) -> Result<(usize, &Series)> {
    chart
        .series
        .iter()
        .enumerate()
        .find(|(_, s)| s.axis == side)
        .ok_or_else(|| crate::Error::BadSeriesShape {
            kind: "combo".to_owned(),
            expected: format!("one series with axis: {}", axis_name(side)),
            actual: format!("no {}-axis series", axis_name(side)),
            path: "/series".to_owned(),
        })
}

const fn axis_name(side: AxisSide) -> &'static str {
    match side {
        AxisSide::Left => "left",
        AxisSide::Right => "right",
    }
}

fn build_scales(col_series: &Series, line_series: &Series, plot: &PlotBox) -> (Scale, Scale) {
    let col_values: Vec<f64> = col_series.points.iter().map(|p| p.y.value).collect();
    let line_values: Vec<f64> = line_series.points.iter().map(|p| p.y.value).collect();
    let (l_lo, l_hi) = nice_domain(&col_values);
    let (r_lo, r_hi) = nice_domain(&line_values);
    (
        Scale::new((l_lo, l_hi), (plot.y1, plot.y0)),
        Scale::new((r_lo, r_hi), (plot.y1, plot.y0)),
    )
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

fn emit_gridlines_and_axes(out: &mut String) {
    // Scaffold: structural placeholders. The follow-up arm fills these in with
    // tick lines + tick labels at the y_left tick positions (and y_right for
    // combo), using the `scale::ticks` generator.
    out.push_str("<g class=\"gridlines\"></g>");
    out.push_str("<g class=\"axes\"></g>");
}

fn emit_bars(
    out: &mut String,
    series: &Series,
    y_scale: &Scale,
    plot: &PlotBox,
    band_w: f64,
    bar_w: f64,
    fill: &str,
) {
    out.push_str("<g class=\"bars\">");
    for (i, point) in series.points.iter().enumerate() {
        let cx = plot.x0 + band_w * idx_to_f64(i) + band_w * 0.5;
        let x = cx - bar_w * 0.5;
        let y = y_scale.map(point.y.value);
        let h = plot.y1 - y;
        let _ = write!(
            out,
            "<rect x=\"{x}\" y=\"{y}\" width=\"{w}\" height=\"{h}\" rx=\"3\" fill=\"{fill}\"/>",
            x = coord(x),
            y = coord(y),
            w = coord(bar_w),
            h = coord(h),
            fill = fill,
        );
    }
    out.push_str("</g>");
}

fn emit_line(
    out: &mut String,
    series: &Series,
    y_scale: &Scale,
    plot: &PlotBox,
    band_w: f64,
    stroke: &str,
) {
    out.push_str("<g class=\"line\">");
    let mut points_attr = String::new();
    for (i, point) in series.points.iter().enumerate() {
        let cx = plot.x0 + band_w * idx_to_f64(i) + band_w * 0.5;
        let cy = y_scale.map(point.y.value);
        if i > 0 {
            points_attr.push(' ');
        }
        let _ = write!(points_attr, "{},{}", coord(cx), coord(cy));
    }
    let pts = points_attr;
    let _ = write!(
        out,
        "<polyline fill=\"none\" stroke=\"{stroke}\" stroke-width=\"2\" points=\"{pts}\"/>",
    );
    for (i, point) in series.points.iter().enumerate() {
        let cx = plot.x0 + band_w * idx_to_f64(i) + band_w * 0.5;
        let cy = y_scale.map(point.y.value);
        let _ = write!(
            out,
            "<circle cx=\"{cx}\" cy=\"{cy}\" r=\"9\" fill=\"{fill}\"/>",
            cx = coord(cx),
            cy = coord(cy),
            fill = stroke,
        );
    }
    out.push_str("</g>");
}

fn emit_data_labels(
    out: &mut String,
    series: &Series,
    y_scale: &Scale,
    plot: &PlotBox,
    band_w: f64,
    theme: &ResolvedTheme,
) {
    out.push_str("<g class=\"labels\">");
    for (i, point) in series.points.iter().enumerate() {
        let cx = plot.x0 + band_w * idx_to_f64(i) + band_w * 0.5;
        let y = y_scale.map(point.y.value);
        let label = label_text(&point.y);
        let _ = write!(
            out,
            "<text x=\"{x}\" y=\"{y}\" \
             text-anchor=\"middle\" \
             dominant-baseline=\"hanging\" \
             fill=\"#ffffff\" \
             font-family=\"{font}\">{label}</text>",
            x = coord(cx),
            y = coord(y + 8.0),
            font = theme.font_sans,
        );
    }
    out.push_str("</g>");
}

fn emit_x_labels(
    out: &mut String,
    series: &Series,
    plot: &PlotBox,
    band_w: f64,
    theme: &ResolvedTheme,
) {
    out.push_str("<g class=\"x-labels\">");
    for (i, point) in series.points.iter().enumerate() {
        let cx = plot.x0 + band_w * idx_to_f64(i) + band_w * 0.5;
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
        );
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

fn label_text(cite: &FactCite) -> String {
    escape_xml(&format_number(cite.value, NumFormat::FromUnit, cite.unit))
}

fn escape_xml(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    clippy::indexing_slicing,
    reason = "test assertions"
)]
mod tests {
    use super::*;
    use crate::model::{
        Axes, AxisSide, Chart, ChartKind, CiteOrText, FactCite, FactId, LegendSpec, Point,
        SeriesStyle, ToneRef, Unit,
    };
    use crate::render::canvas::DeckCanvas;

    fn offsite_spec() -> Chart {
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
            kind: ChartKind::Combo,
            title: Some(CiteOrText::Text("Offsite slide-3".to_owned())),
            series: vec![
                Series {
                    name: CiteOrText::Text("Revenue".to_owned()),
                    points: vec![
                        pt("MAR", cite("rev-mar", 18.0, Unit::Money)),
                        pt("APR", cite("rev-apr", 22.0, Unit::Money)),
                        pt("MAY", cite("rev-may", 25.0, Unit::Money)),
                    ],
                    tone: ToneRef::Indexed(0),
                    axis: AxisSide::Left,
                    style: SeriesStyle::Column,
                },
                Series {
                    name: CiteOrText::Text("Headcount".to_owned()),
                    points: vec![
                        pt("MAR", cite("hc-mar", 120.0, Unit::Number)),
                        pt("APR", cite("hc-apr", 150.0, Unit::Number)),
                        pt("MAY", cite("hc-may", 175.0, Unit::Number)),
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
    fn offsite_combo_emits_navy_columns_and_teal_line_in_resolved_mode() {
        let theme = ResolvedTheme::summus_stub();
        let svg = emit(
            &offsite_spec(),
            &theme,
            &Canvas::Deck(DeckCanvas::default()),
            ColorMode::Resolved,
        )
        .expect("offsite combo emits");
        assert!(svg.contains("fill=\"#232E54\""), "navy not present");
        assert!(
            svg.contains("stroke=\"#318891\""),
            "teal stroke not present"
        );
        assert!(
            svg.contains("fill=\"#318891\""),
            "teal marker fill not present"
        );
        assert!(svg.contains("rx=\"3\""));
        assert!(svg.contains("r=\"9\""));
    }

    #[test]
    fn themed_mode_emits_css_var_fills() {
        let theme = ResolvedTheme::summus_stub();
        let svg = emit(
            &offsite_spec(),
            &theme,
            &Canvas::Deck(DeckCanvas::default()),
            ColorMode::Themed,
        )
        .expect("themed mode emits");
        assert!(svg.contains("var(--tone-series-0)"));
        assert!(svg.contains("var(--tone-series-1)"));
        assert!(!svg.contains("#232E54"));
        assert!(!svg.contains("#318891"));
    }

    #[test]
    fn output_is_deterministic_across_two_renders() {
        let theme = ResolvedTheme::summus_stub();
        let a = emit(
            &offsite_spec(),
            &theme,
            &Canvas::Deck(DeckCanvas::default()),
            ColorMode::Resolved,
        )
        .expect("first emit");
        let b = emit(
            &offsite_spec(),
            &theme,
            &Canvas::Deck(DeckCanvas::default()),
            ColorMode::Resolved,
        )
        .expect("second emit");
        assert_eq!(a, b);
    }

    #[test]
    fn missing_axis_side_errors() {
        let mut spec = offsite_spec();
        spec.series[1].axis = AxisSide::Left;
        let theme = ResolvedTheme::summus_stub();
        let r = emit(
            &spec,
            &theme,
            &Canvas::Deck(DeckCanvas::default()),
            ColorMode::Resolved,
        );
        assert!(matches!(r, Err(crate::Error::BadSeriesShape { .. })));
    }
}

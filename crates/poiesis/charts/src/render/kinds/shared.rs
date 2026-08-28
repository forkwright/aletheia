//! Shared helpers for the pure-Rust emitter arms.
//!
//! These primitives are reused by every per-kind module so that fixes to
//! escaping, domain calculation, or the SVG open tag happen in one place.

use std::fmt::Write as _;

pub(crate) use poiesis_core::escape_xml;

use crate::Result;
use crate::format::{coord, format_number};
use crate::model::{AxisSpec, Chart, CiteOrText, Domain, LegendSpec, Ticks, Unit};
use crate::render::canvas::{Canvas, PlotBox};
use crate::scale::Scale;
use crate::theme::{ColorMode, ResolvedTheme};

// WHY: the value below is the W3C SVG 1.1 namespace identifier — a fixed URI
// literal mandated by the SVG spec. Renderers match it as an opaque string;
// substituting `https://` produces SVG that browsers refuse to render.
/// W3C SVG namespace URI.
pub(crate) const SVG_NAMESPACE: &str = "http://www.w3.org/2000/svg";

/// Emit the opening `<svg>` tag shared by every Rust-path kind.
pub(crate) fn emit_svg_open(out: &mut String, chart: &Chart, canvas: &Canvas) {
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

/// Build an accessible label from the chart title or kind name.
pub(crate) fn aria_label(chart: &Chart) -> String {
    match &chart.title {
        Some(CiteOrText::Text(t)) => escape_xml(t),
        Some(CiteOrText::Cite(id)) => escape_xml(id.as_str()),
        None => format!("{} chart", chart.kind.name()),
    }
}

/// Compute a nice-rounded domain from data values.
///
/// `lo` is clamped to zero so bar/column/area charts keep a data baseline at
/// the axis origin. Non-finite extents fall back to `(0, 1)` so downstream
/// scale math does not produce `NaN` coordinates.
pub(crate) fn nice_domain(values: &[f64]) -> (f64, f64) {
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
    crate::scale::nice(lo, hi)
}

/// Determine the value domain for an axis.
///
/// Honors [`Domain::Fixed`] when set; otherwise computes a nice-rounded
/// domain from the supplied data values.
pub(crate) fn domain_bounds(values: &[f64], spec: &AxisSpec) -> (f64, f64) {
    match spec.domain {
        Domain::Fixed { min, max } => (min, max),
        Domain::Auto => nice_domain(values),
    }
}

/// Generate tick positions for an axis.
///
/// Honors [`Ticks::Count`] and [`Ticks::Explicit`]; falls back to the
/// default ~5 tick 1-2-5 generator for [`Ticks::Auto`].
pub(crate) fn ticks_for_axis(spec: &AxisSpec, lo: f64, hi: f64) -> Vec<f64> {
    match &spec.ticks {
        Ticks::Auto => crate::scale::ticks(lo, hi, 5),
        Ticks::Count(n) => crate::scale::ticks(lo, hi, *n),
        Ticks::Explicit(values) => values.clone(),
    }
}

/// Emit `<g class="gridlines">`: horizontal gridlines at y-tick positions.
///
/// Shared by every kind laid out on a linear y-axis with a banded category
/// x-axis (line, area) — the central series geometry is the only thing that
/// differs between them. Bar/column/scatter axis layouts differ enough to
/// keep their own arms.
pub(crate) fn emit_gridlines(
    out: &mut String,
    y_scale: &Scale,
    plot: &PlotBox,
    lo: f64,
    hi: f64,
    axis: &AxisSpec,
) {
    out.push_str("<g class=\"gridlines\">");
    for tick in ticks_for_axis(axis, lo, hi) {
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

/// Emit `<g class="axes">`: y-tick labels plus x-category labels taken from
/// the chart's first series.
///
/// Shared by line and area — see [`emit_gridlines`].
pub(crate) fn emit_axes(
    out: &mut String,
    chart: &Chart,
    y_scale: &Scale,
    plot: &PlotBox,
    band_w: f64,
    theme: &ResolvedTheme,
    lo: f64,
    hi: f64,
    axis: &AxisSpec,
) {
    out.push_str("<g class=\"axes\">");

    for tick in ticks_for_axis(axis, lo, hi) {
        let y = y_scale.map(tick);
        let label = escape_xml(&format_number(tick, axis.format, Unit::Number));
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
                Some(CiteOrText::Cite(id)) => escape_xml(id.as_str()),
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

/// Emit `<g class="labels">`: on-point value labels for every series.
///
/// Shared by line and area — see [`emit_gridlines`].
pub(crate) fn emit_data_labels(
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

/// Convert a category index to `f64` for geometric calculations.
#[expect(
    clippy::cast_precision_loss,
    clippy::as_conversions,
    reason = "category index never approaches f64 mantissa limit"
)]
pub(crate) const fn idx_to_f64(i: usize) -> f64 {
    i as f64
}

/// Map a polar coordinate (center, radius, angle from 12 o'clock, clockwise)
/// to an SVG-space `(x, y)` pair.
///
/// Shared by pie and doughnut — both sweep sectors the same way; only the
/// inner-radius cut distinguishes doughnut's geometry.
pub(crate) fn polar_to_xy(cx: f64, cy: f64, r: f64, angle_rad: f64) -> (f64, f64) {
    let svg_angle = angle_rad - std::f64::consts::FRAC_PI_2;
    (cx + r * svg_angle.cos(), cy + r * svg_angle.sin())
}

/// Decide whether a legend should be emitted for this chart.
pub(crate) fn legend_needed(spec: LegendSpec, n_series: usize) -> bool {
    match spec {
        LegendSpec::None => false,
        LegendSpec::Auto => n_series > 1,
        LegendSpec::TopRight | LegendSpec::Bottom => true,
    }
}

/// Emit a `<g class="legend">` with colored swatches and series names.
pub(crate) fn emit_legend(
    out: &mut String,
    chart: &Chart,
    theme: &ResolvedTheme,
    mode: ColorMode,
    plot: &PlotBox,
) -> Result<()> {
    let n = chart.series.len();
    if n == 0 {
        return Ok(());
    }

    let item_w = 120.0;
    let total_w = item_w * idx_to_f64(n);
    let start_x = (plot.x1 - total_w).max(plot.x0);
    let y = if matches!(chart.legend, LegendSpec::Bottom) {
        plot.y1 + 50.0
    } else {
        plot.y0 - 20.0
    };

    out.push_str("<g class=\"legend\">");
    for (i, series) in chart.series.iter().enumerate() {
        let fill = theme.fill_for(&series.tone, mode, i)?;
        let x = start_x + item_w * idx_to_f64(i);
        let name = match &series.name {
            CiteOrText::Text(t) => escape_xml(t),
            CiteOrText::Cite(id) => escape_xml(id.as_str()),
        };
        let _ = write!(
            out,
            "<rect x=\"{x}\" y=\"{y}\" width=\"12\" height=\"12\" fill=\"{fill}\"/>\
             <text x=\"{tx}\" y=\"{ty}\" font-family=\"{font}\" font-size=\"12\">{name}</text>",
            x = crate::format::coord(x),
            y = crate::format::coord(y),
            fill = fill,
            tx = crate::format::coord(x + 16.0),
            ty = crate::format::coord(y + 10.0),
            font = theme.font_sans,
            name = name,
        );
    }
    out.push_str("</g>");
    Ok(())
}

/// Emit an optional `<g class="caption">` below the plot box.
///
/// The caption carries the doc stack's [`poiesis_core::RichText`] span
/// model. This arm flattens spans to plain text ([`RichText::plain_text`]) —
/// per-span styling (bold/italic/code `<tspan>`s) is not yet wired into the
/// SVG emitter, matching every other text group in this emitter (legend,
/// axis, data labels), which are also flat `<text>` elements.
pub(crate) fn emit_caption(out: &mut String, chart: &Chart, theme: &ResolvedTheme, plot: &PlotBox) {
    let Some(rich) = chart.caption.as_ref() else {
        return;
    };
    if rich.spans.is_empty() {
        return;
    }
    let text = escape_xml(&rich.plain_text());
    let _ = write!(
        out,
        "<g class=\"caption\"><text x=\"{x}\" y=\"{y}\" font-family=\"{font}\" font-size=\"14\">{text}</text></g>",
        x = crate::format::coord(plot.x0),
        y = crate::format::coord(plot.y1 + 50.0),
        font = theme.font_sans,
        text = text,
    );
}

/// Test-only [`FactCite`] builders shared by the per-kind arm test modules.
///
/// WHY: every Rust-path arm needs a `FactCite` with a plain [`Unit::Number`]
/// to build its fixture points; kept here so a change to `FactCite`'s shape
/// is one edit rather than one per arm.
#[cfg(test)]
pub(crate) mod test_support {
    use crate::model::{FactCite, FactId, Unit};

    #[expect(clippy::expect_used, reason = "test fixture; id is a fixed literal")]
    pub(crate) fn cite(id: &str, v: f64) -> FactCite {
        FactCite {
            id: FactId::new(id.to_owned()).expect("valid fact id"),
            value: v,
            unit: Unit::Number,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Axes, Chart, ChartKind, LegendSpec, Series, SeriesStyle, ToneRef};
    use crate::render::canvas::DeckCanvas;

    #[test]
    fn nice_domain_clamps_to_zero_baseline() {
        let (lo, hi) = nice_domain(&[10.0, 28.0]);
        assert!((lo - 0.0).abs() < 1e-9);
        assert!(hi >= 28.0);
    }

    #[test]
    fn nice_domain_handles_non_finite() {
        let (lo, hi) = nice_domain(&[f64::NAN, f64::NAN]);
        assert!((lo - 0.0).abs() < 1e-9);
        assert!((hi - 1.0).abs() < 1e-9);
    }

    #[test]
    fn idx_to_f64_round_trips() {
        assert!((idx_to_f64(5) - 5.0).abs() < 1e-9);
    }

    #[test]
    fn domain_bounds_honors_fixed_domain() {
        let spec = AxisSpec {
            domain: Domain::Fixed {
                min: 5.0,
                max: 50.0,
            },
            ..AxisSpec::default()
        };
        let (lo, hi) = domain_bounds(&[1.0, 100.0], &spec);
        assert!((lo - 5.0).abs() < 1e-9);
        assert!((hi - 50.0).abs() < 1e-9);
    }

    #[test]
    fn ticks_for_axis_honors_explicit_ticks() {
        let spec = AxisSpec {
            ticks: Ticks::Explicit(vec![0.0, 25.0, 75.0]),
            ..AxisSpec::default()
        };
        assert_eq!(ticks_for_axis(&spec, 0.0, 100.0), vec![0.0, 25.0, 75.0]);
    }

    #[test]
    fn legend_auto_shows_for_multi_series() {
        assert!(legend_needed(LegendSpec::Auto, 2));
        assert!(!legend_needed(LegendSpec::Auto, 1));
    }

    #[test]
    fn emit_svg_open_includes_namespace_and_viewbox() {
        let chart = Chart {
            kind: ChartKind::Bar,
            title: None,
            series: vec![Series {
                name: CiteOrText::Text("A".to_owned()),
                points: vec![],
                tone: ToneRef::Indexed(0),
                axis: crate::model::AxisSide::Left,
                style: SeriesStyle::Default,
            }],
            axes: Axes::default(),
            legend: LegendSpec::Auto,
            data_labels: false,
            caption: None,
        };
        let mut out = String::new();
        emit_svg_open(&mut out, &chart, &Canvas::Deck(DeckCanvas::default()));
        assert!(out.starts_with("<svg"));
        assert!(out.contains("xmlns=\"http://www.w3.org/2000/svg\""));
        assert!(out.contains("viewBox=\"0 0 1600 540\""));
    }
}

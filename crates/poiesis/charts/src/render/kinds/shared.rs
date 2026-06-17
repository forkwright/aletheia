//! Shared helpers for the pure-Rust emitter arms.
//!
//! These primitives are reused by every per-kind module so that fixes to
//! escaping, domain calculation, or the SVG open tag happen in one place.

use std::fmt::Write as _;

use crate::Result;
use crate::model::{AxisSpec, Chart, CiteOrText, Domain, Inlines, LegendSpec, Ticks};
use crate::render::canvas::{Canvas, PlotBox};
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
        Some(CiteOrText::Cite(id)) => escape_xml(&id.0),
        None => format!("{} chart", chart.kind.name()),
    }
}

/// Escape `&`, `<`, `>`, and `"` so text is safe inside SVG attributes and
/// text nodes.
pub(crate) fn escape_xml(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
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

/// Convert a category index to `f64` for geometric calculations.
#[expect(
    clippy::cast_precision_loss,
    clippy::as_conversions,
    reason = "category index never approaches f64 mantissa limit"
)]
pub(crate) const fn idx_to_f64(i: usize) -> f64 {
    i as f64
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
            CiteOrText::Cite(id) => escape_xml(&id.0),
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
pub(crate) fn emit_caption(out: &mut String, chart: &Chart, theme: &ResolvedTheme, plot: &PlotBox) {
    let Some(Inlines(parts)) = chart.caption.as_ref() else {
        return;
    };
    if parts.is_empty() {
        return;
    }
    let text = escape_xml(&parts.join(" "));
    let _ = write!(
        out,
        "<g class=\"caption\"><text x=\"{x}\" y=\"{y}\" font-family=\"{font}\" font-size=\"14\">{text}</text></g>",
        x = crate::format::coord(plot.x0),
        y = crate::format::coord(plot.y1 + 50.0),
        font = theme.font_sans,
        text = text,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Axes, Chart, ChartKind, LegendSpec, Series, SeriesStyle, ToneRef};
    use crate::render::canvas::DeckCanvas;

    #[test]
    fn escape_xml_escapes_reserved_characters() {
        assert_eq!(escape_xml("R&D"), "R&amp;D");
        assert_eq!(escape_xml("<2024"), "&lt;2024");
        assert_eq!(escape_xml(">2024"), "&gt;2024");
        assert_eq!(escape_xml("\"quote\""), "&quot;quote&quot;");
    }

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

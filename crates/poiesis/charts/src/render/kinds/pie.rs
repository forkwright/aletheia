//! Pie chart emitter: SVG arc sectors.
//!
//! # Fixed source order
//!
//! Every emitted SVG follows this order, byte-for-byte:
//!
//! 1. `<svg>` open + viewBox + `preserveAspectRatio` + `role` + `aria-label`
//! 2. `<g class="sectors">` — one `<path>` per slice
//! 3. `<g class="labels">` — percentage text at slice midpoint (if enabled)
//! 4. `</svg>` close
//!
//! Coordinates round to 2 dp via [`crate::format::coord`]; numeric text
//! routes through [`crate::format::format_number`]. No `format!("{}", f64)`
//! anywhere.

use std::fmt::Write as _;

use crate::Result;
use crate::format::{coord, format_number};
use crate::model::{Chart, CiteOrText, NumFormat, ToneRef, Unit};
use crate::render::canvas::Canvas;
use crate::theme::{ColorMode, ResolvedTheme};

// WHY: the value below is the W3C SVG 1.1 namespace identifier — a fixed URI
// literal mandated by the SVG spec. Renderers match it as an opaque string;
// it is never fetched. Substituting `https://` produces SVG that browsers
// refuse to render. See SVG 1.1 §1.3.
const SVG_NAMESPACE: &str = "http://www.w3.org/2000/svg";

/// Emit the pie chart SVG.
///
/// Caller invariants (enforced by [`Chart::validate`](crate::model::Chart::validate)):
/// - `chart.kind == ChartKind::Pie`
/// - `series.len() == 1`
/// - `points.len() >= 1`
pub fn emit(
    chart: &Chart,
    theme: &ResolvedTheme,
    canvas: &Canvas,
    mode: ColorMode,
) -> Result<String> {
    #[expect(
        clippy::indexing_slicing,
        reason = "caller invariant: exactly 1 series"
    )]
    let series = &chart.series[0];
    let plot = canvas.plot_box();

    let cx = (plot.x0 + plot.x1) * 0.5;
    let cy = (plot.y0 + plot.y1) * 0.5;
    let r = (plot.width().min(plot.height()) * 0.5 - 20.0).max(10.0);

    let total: f64 = series.points.iter().map(|p| p.y.value.abs()).sum();

    let mut out = String::new();
    emit_svg_open(&mut out, chart, canvas);

    if total <= 0.0 {
        // Empty pie: emit a background circle and close.
        let _ = write!(
            out,
            "<circle cx=\"{cx}\" cy=\"{cy}\" r=\"{r}\" fill=\"#e5e5e5\"/>",
            cx = coord(cx),
            cy = coord(cy),
            r = coord(r),
        );
        out.push_str("</svg>");
        return Ok(out);
    }

    out.push_str("<g class=\"sectors\">");

    let mut cumulative_angle: f64 = 0.0;
    for (j, point) in series.points.iter().enumerate() {
        let fraction = point.y.value.abs() / total;
        let sweep = fraction * 2.0 * std::f64::consts::PI;
        let start_angle = cumulative_angle;
        let end_angle = start_angle + sweep;

        let fill = theme.fill_for(&ToneRef::Indexed(j), mode, j)?;

        let path_d = if (sweep - 2.0 * std::f64::consts::PI).abs() < 1e-12 {
            // Degenerate single-slice full circle: split into two 180° arcs.
            let (x0, y0) = polar_to_xy(cx, cy, r, 0.0);
            let (xm, ym) = polar_to_xy(cx, cy, r, std::f64::consts::PI);
            let mut d = String::new();
            let _ = write!(
                d,
                "M {x0},{y0} A {r} {r} 0 0 1 {xm},{ym} A {r} {r} 0 0 1 {x0},{y0} Z",
                x0 = coord(x0),
                y0 = coord(y0),
                r = coord(r),
                xm = coord(xm),
                ym = coord(ym),
            );
            d
        } else {
            let (x0, y0) = polar_to_xy(cx, cy, r, start_angle);
            let (x1, y1) = polar_to_xy(cx, cy, r, end_angle);
            let large_arc = if sweep >= std::f64::consts::PI {
                "1"
            } else {
                "0"
            };
            let mut d = String::new();
            let _ = write!(
                d,
                "M {cx},{cy} L {x0},{y0} A {r} {r} 0 {large_arc} 1 {x1},{y1} Z",
                cx = coord(cx),
                cy = coord(cy),
                x0 = coord(x0),
                y0 = coord(y0),
                r = coord(r),
                x1 = coord(x1),
                y1 = coord(y1),
            );
            d
        };

        let _ = write!(out, "<path d=\"{path_d}\" fill=\"{fill}\"/>");
        cumulative_angle = end_angle;
    }

    out.push_str("</g>");

    if chart.data_labels {
        out.push_str("<g class=\"labels\">");
        let mut cumulative_angle: f64 = 0.0;
        for point in &series.points {
            let fraction = point.y.value.abs() / total;
            let sweep = fraction * 2.0 * std::f64::consts::PI;
            let mid_angle = cumulative_angle + sweep * 0.5;
            let (lx, ly) = polar_to_xy(cx, cy, r * 0.65, mid_angle);
            let pct_text = format_number(fraction * 100.0, NumFormat::Percent, Unit::Percent);
            let _ = write!(
                out,
                "<text x=\"{x}\" y=\"{y}\" \
                 text-anchor=\"middle\" \
                 dominant-baseline=\"middle\" \
                 font-family=\"{font}\" \
                 fill=\"#ffffff\">{label}</text>",
                x = coord(lx),
                y = coord(ly),
                font = theme.font_sans,
                label = escape_xml(&pct_text),
            );
            cumulative_angle += sweep;
        }
        out.push_str("</g>");
    }

    out.push_str("</svg>");
    Ok(out)
}

fn polar_to_xy(cx: f64, cy: f64, r: f64, angle_rad: f64) -> (f64, f64) {
    let svg_angle = angle_rad - std::f64::consts::FRAC_PI_2;
    (cx + r * svg_angle.cos(), cy + r * svg_angle.sin())
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

fn aria_label(chart: &Chart) -> String {
    match &chart.title {
        Some(CiteOrText::Text(t)) => escape_xml(t),
        Some(CiteOrText::Cite(id)) => escape_xml(&id.0),
        None => "pie chart".to_owned(),
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
        Axes, Chart, ChartKind, CiteOrText, FactCite, FactId, LegendSpec, Point, Series,
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

    fn pie_chart(values: &[(f64, &str)]) -> Chart {
        let points: Vec<Point> = values
            .iter()
            .map(|(v, suffix)| Point {
                label: None,
                x: None,
                y: cite(&format!("pie-{suffix}"), *v),
            })
            .collect();

        Chart {
            kind: ChartKind::Pie,
            title: None,
            series: vec![Series {
                name: CiteOrText::Text("Pie".to_owned()),
                points,
                tone: ToneRef::Indexed(0),
                axis: crate::model::AxisSide::Left,
                style: SeriesStyle::Default,
            }],
            axes: Axes::default(),
            legend: LegendSpec::Auto,
            data_labels: true,
            caption: None,
        }
    }

    #[test]
    fn two_slice_pie_emits_two_paths() {
        let theme = ResolvedTheme::summus_stub();
        let svg = emit(
            &pie_chart(&[(30.0, "a"), (70.0, "b")]),
            &theme,
            &Canvas::Deck(DeckCanvas::default()),
            ColorMode::Resolved,
        )
        .expect("pie emits");
        let path_count = svg.matches("<path").count();
        assert_eq!(path_count, 2, "expected 2 path elements, got {path_count}");
    }

    #[test]
    fn themed_mode_emits_css_vars() {
        let theme = ResolvedTheme::summus_stub();
        let svg = emit(
            &pie_chart(&[(30.0, "a"), (70.0, "b")]),
            &theme,
            &Canvas::Deck(DeckCanvas::default()),
            ColorMode::Themed,
        )
        .expect("themed pie emits");
        assert!(svg.contains("var(--tone-series-0)"));
        assert!(svg.contains("var(--tone-series-1)"));
    }

    #[test]
    fn output_is_deterministic() {
        let theme = ResolvedTheme::summus_stub();
        let a = emit(
            &pie_chart(&[(30.0, "a"), (70.0, "b")]),
            &theme,
            &Canvas::Deck(DeckCanvas::default()),
            ColorMode::Resolved,
        )
        .expect("first emit");
        let b = emit(
            &pie_chart(&[(30.0, "a"), (70.0, "b")]),
            &theme,
            &Canvas::Deck(DeckCanvas::default()),
            ColorMode::Resolved,
        )
        .expect("second emit");
        assert_eq!(a, b);
    }

    #[test]
    fn single_slice_full_circle() {
        let theme = ResolvedTheme::summus_stub();
        let svg = emit(
            &pie_chart(&[(100.0, "only")]),
            &theme,
            &Canvas::Deck(DeckCanvas::default()),
            ColorMode::Resolved,
        )
        .expect("single slice emits");
        assert!(svg.contains("<path"));
    }
}

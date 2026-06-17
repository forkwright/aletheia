//! Doughnut chart emitter: arc sectors with inner radius.
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

use super::shared::{
    emit_caption, emit_legend, emit_svg_open, escape_xml, legend_needed,
};
use crate::Result;
use crate::format::{coord, format_number};
use crate::model::{Chart, NumFormat, Unit};
use crate::render::canvas::Canvas;
use crate::theme::{ColorMode, ResolvedTheme};

/// Emit the doughnut chart SVG.
///
/// Caller invariants (enforced by [`Chart::validate`](crate::model::Chart::validate)):
/// - `chart.kind == ChartKind::Doughnut`
/// - `series.len() == 1`
/// - `points.len() >= 1`
#[expect(
    clippy::too_many_lines,
    reason = "single emit function per kind pattern"
)]
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
    let r_inner = r * 0.5;

    let total: f64 = series.points.iter().map(|p| p.y.value.abs()).sum();

    let mut out = String::new();
    emit_svg_open(&mut out, chart, canvas);

    if total <= 0.0 {
        let _ = write!(
            out,
            "<circle cx=\"{cx}\" cy=\"{cy}\" r=\"{r}\" fill=\"#e5e5e5\"/><circle cx=\"{cx}\" cy=\"{cy}\" r=\"{ri}\" fill=\"#ffffff\"/>",
            cx = coord(cx),
            cy = coord(cy),
            r = coord(r),
            ri = coord(r_inner),
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

        let fill = theme.fill_for_slice(&series.tone, mode, 0, j)?;

        let path_d = if (sweep - 2.0 * std::f64::consts::PI).abs() < 1e-12 {
            // WHY: a single slice spans the full circle, and an SVG arc with
            // coincident endpoints renders nothing — emit two 180° ring arcs.
            let (x0o, y0o) = polar_to_xy(cx, cy, r, 0.0);
            let (xmo, ymo) = polar_to_xy(cx, cy, r, std::f64::consts::PI);
            let (x0i, y0i) = polar_to_xy(cx, cy, r_inner, 0.0);
            let (xmi, ymi) = polar_to_xy(cx, cy, r_inner, std::f64::consts::PI);
            let mut d = String::new();
            let _ = write!(
                d,
                "M {x0o},{y0o} A {r} {r} 0 0 1 {xmo},{ymo} A {r} {r} 0 0 1 {x0o},{y0o} \
                 L {x0i},{y0i} A {ri} {ri} 0 0 0 {xmi},{ymi} A {ri} {ri} 0 0 0 {x0i},{y0i} Z",
                x0o = coord(x0o),
                y0o = coord(y0o),
                r = coord(r),
                xmo = coord(xmo),
                ymo = coord(ymo),
                x0i = coord(x0i),
                y0i = coord(y0i),
                ri = coord(r_inner),
                xmi = coord(xmi),
                ymi = coord(ymi),
            );
            d
        } else {
            let (x0o, y0o) = polar_to_xy(cx, cy, r, start_angle);
            let (x1o, y1o) = polar_to_xy(cx, cy, r, end_angle);
            let (x0i, y0i) = polar_to_xy(cx, cy, r_inner, start_angle);
            let (x1i, y1i) = polar_to_xy(cx, cy, r_inner, end_angle);
            let large_arc = if sweep >= std::f64::consts::PI {
                "1"
            } else {
                "0"
            };
            let mut d = String::new();
            let _ = write!(
                d,
                "M {x0o},{y0o} A {r} {r} 0 {la} 1 {x1o},{y1o} \
                 L {x1i},{y1i} A {ri} {ri} 0 {la} 0 {x0i},{y0i} Z",
                x0o = coord(x0o),
                y0o = coord(y0o),
                r = coord(r),
                la = large_arc,
                x1o = coord(x1o),
                y1o = coord(y1o),
                x1i = coord(x1i),
                y1i = coord(y1i),
                ri = coord(r_inner),
                x0i = coord(x0i),
                y0i = coord(y0i),
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
            let (lx, ly) = polar_to_xy(cx, cy, r * 0.75, mid_angle);
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

    if legend_needed(chart.legend, chart.series.len()) {
        emit_legend(&mut out, chart, theme, mode, &plot)?;
    }
    emit_caption(&mut out, chart, theme, &plot);
    out.push_str("</svg>");
    Ok(out)
}

fn polar_to_xy(cx: f64, cy: f64, r: f64, angle_rad: f64) -> (f64, f64) {
    let svg_angle = angle_rad - std::f64::consts::FRAC_PI_2;
    (cx + r * svg_angle.cos(), cy + r * svg_angle.sin())
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

    fn doughnut_chart(values: &[(f64, &str)]) -> Chart {
        let points: Vec<Point> = values
            .iter()
            .map(|(v, suffix)| Point {
                label: None,
                x: None,
                y: cite(&format!("doughnut-{suffix}"), *v),
            })
            .collect();

        Chart {
            kind: ChartKind::Doughnut,
            title: None,
            series: vec![Series {
                name: CiteOrText::Text("Doughnut".to_owned()),
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
    fn two_slice_doughnut_emits_two_paths() {
        let theme = ResolvedTheme::summus_stub();
        let svg = emit(
            &doughnut_chart(&[(30.0, "a"), (70.0, "b")]),
            &theme,
            &Canvas::Deck(DeckCanvas::default()),
            ColorMode::Resolved,
        )
        .expect("doughnut emits");
        let path_count = svg.matches("<path").count();
        assert_eq!(path_count, 2, "expected 2 path elements, got {path_count}");
    }

    #[test]
    fn themed_mode_emits_css_vars() {
        let theme = ResolvedTheme::summus_stub();
        let svg = emit(
            &doughnut_chart(&[(30.0, "a"), (70.0, "b")]),
            &theme,
            &Canvas::Deck(DeckCanvas::default()),
            ColorMode::Themed,
        )
        .expect("themed doughnut emits");
        assert!(svg.contains("var(--tone-series-0)"));
        assert!(svg.contains("var(--tone-series-1)"));
    }

    #[test]
    fn output_is_deterministic() {
        let theme = ResolvedTheme::summus_stub();
        let a = emit(
            &doughnut_chart(&[(30.0, "a"), (70.0, "b")]),
            &theme,
            &Canvas::Deck(DeckCanvas::default()),
            ColorMode::Resolved,
        )
        .expect("first emit");
        let b = emit(
            &doughnut_chart(&[(30.0, "a"), (70.0, "b")]),
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
            &doughnut_chart(&[(100.0, "only")]),
            &theme,
            &Canvas::Deck(DeckCanvas::default()),
            ColorMode::Resolved,
        )
        .expect("single slice emits");
        assert!(svg.contains("<path"));
    }

    #[test]
    fn more_slices_than_palette_renders_without_error() {
        let theme = ResolvedTheme::summus_stub();
        let svg = emit(
            &doughnut_chart(&[
                (10.0, "a"),
                (15.0, "b"),
                (20.0, "c"),
                (25.0, "d"),
                (30.0, "e"),
            ]),
            &theme,
            &Canvas::Deck(DeckCanvas::default()),
            ColorMode::Resolved,
        )
        .expect("palette-cycled doughnut emits");
        assert_eq!(svg.matches("<path").count(), 5);
    }
}

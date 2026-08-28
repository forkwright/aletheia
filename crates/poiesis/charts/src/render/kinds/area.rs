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

use super::shared::{
    domain_bounds, emit_axes, emit_caption, emit_data_labels, emit_gridlines, emit_legend,
    emit_svg_open, idx_to_f64, legend_needed,
};
use crate::Result;
use crate::format::coord;
use crate::model::Chart;
use crate::render::canvas::{Canvas, PlotBox};
use crate::scale::Scale;
use crate::theme::{ColorMode, ResolvedTheme};

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
    let (lo, hi) = domain_bounds(&all_values, &chart.axes.y_left);
    let y_scale = Scale::new((lo, hi), (plot.y1, plot.y0));

    let mut out = String::new();
    emit_svg_open(&mut out, chart, canvas);
    emit_gridlines(&mut out, &y_scale, &plot, lo, hi, &chart.axes.y_left);
    emit_axes(
        &mut out,
        chart,
        &y_scale,
        &plot,
        band_w,
        theme,
        lo,
        hi,
        &chart.axes.y_left,
    );
    emit_areas(&mut out, chart, &y_scale, &plot, band_w, theme, mode)?;
    emit_markers(&mut out, chart, &y_scale, &plot, band_w, theme, mode)?;
    if chart.data_labels {
        emit_data_labels(&mut out, chart, &y_scale, &plot, band_w, theme, mode)?;
    }
    if legend_needed(chart.legend, chart.series.len()) {
        emit_legend(&mut out, chart, theme, mode, &plot)?;
    }
    emit_caption(&mut out, chart, theme, &plot);
    out.push_str("</svg>");
    Ok(out)
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
            id: FactId::new(id.to_owned()).expect("valid fact id"),
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
        let theme = ResolvedTheme::protos_stub();
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
        let theme = ResolvedTheme::protos_stub();
        let svg = emit(
            &area_spec(),
            &theme,
            &Canvas::Deck(DeckCanvas::default()),
            ColorMode::Themed,
        )
        .expect("themed mode emits");
        assert!(svg.contains("var(--tone-series-0)"));
        assert!(!svg.contains("#1E293B"));
    }

    #[test]
    fn output_is_deterministic() {
        let theme = ResolvedTheme::protos_stub();
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
                        id: FactId::new("f4".to_owned()).expect("valid fact id"),
                        value: 5.0,
                        unit: Unit::Number,
                    },
                },
                Point {
                    label: Some(CiteOrText::Text("B".to_owned())),
                    x: None,
                    y: FactCite {
                        id: FactId::new("f5".to_owned()).expect("valid fact id"),
                        value: 25.0,
                        unit: Unit::Number,
                    },
                },
                Point {
                    label: Some(CiteOrText::Text("C".to_owned())),
                    x: None,
                    y: FactCite {
                        id: FactId::new("f6".to_owned()).expect("valid fact id"),
                        value: 10.0,
                        unit: Unit::Number,
                    },
                },
            ],
            tone: ToneRef::Indexed(1),
            axis: AxisSide::Left,
            style: SeriesStyle::Default,
        });
        let theme = ResolvedTheme::protos_stub();
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

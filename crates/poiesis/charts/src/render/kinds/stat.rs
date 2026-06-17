//! Stat chart emitter: single "big number" KPI display.
//!
//! # Fixed source order
//!
//! Every emitted SVG follows this order, byte-for-byte:
//!
//! 1. `<svg>` open + viewBox + `preserveAspectRatio` + `role` + `aria-label`
//! 2. `<g class="stat">` — series name (optional), big number, chart title (optional)
//! 3. `</svg>` close
//!
//! Coordinates round to 2 dp via [`crate::format::coord`]; numeric text
//! routes through [`crate::format::format_number`]. No `format!("{}", f64)`
//! anywhere.

use std::fmt::Write as _;

use super::shared::{
    SVG_NAMESPACE, emit_caption, emit_legend, escape_xml, legend_needed,
};
use crate::Result;
use crate::format::{coord, format_number};
use crate::model::{Chart, CiteOrText};
use crate::render::canvas::{Canvas, PlotBox};
use crate::theme::{ColorMode, ResolvedTheme};

/// Emit the stat chart SVG.
///
/// Caller invariants (enforced by [`Chart::validate`](crate::model::Chart::validate)):
/// - `chart.kind == ChartKind::Stat`
/// - exactly one series
///
/// The emitter also checks that the series has at least one data point and
/// returns [`crate::Error::BadSeriesShape`] otherwise.
#[expect(
    clippy::indexing_slicing,
    reason = "validated by Chart::validate (exactly 1 series) and empty-point guard above"
)]
#[expect(clippy::too_many_lines, reason = "single emit function per kind pattern")]
pub fn emit(
    chart: &Chart,
    theme: &ResolvedTheme,
    canvas: &Canvas,
    mode: ColorMode,
) -> Result<String> {
    let series = &chart.series[0];
    if series.points.is_empty() {
        return Err(crate::Error::BadSeriesShape {
            kind: "stat".to_owned(),
            expected: "1+ data points".to_owned(),
            actual: "0 points".to_owned(),
            path: "/series/0/points".to_owned(),
        });
    }

    let point = &series.points[0];
    let number_text = format_number(point.y.value, chart.axes.y_left.format, point.y.unit);

    let width = if canvas.width() == 0 {
        1600
    } else {
        canvas.width()
    };
    let height = if canvas.height() == 0 {
        540
    } else {
        canvas.height()
    };
    let cx = f64::from(width) * 0.5;
    let cy = f64::from(height) * 0.5;

    let fill = theme.fill_for(&series.tone, mode, 0)?;

    let series_name = match &series.name {
        CiteOrText::Text(t) => t.clone(),
        CiteOrText::Cite(id) => id.0.clone(),
    };

    let aria = if series_name.is_empty() {
        escape_xml(&number_text)
    } else {
        format!(
            "{} — {}",
            escape_xml(&number_text),
            escape_xml(&series_name)
        )
    };

    let mut out = String::new();
    let _ = write!(
        out,
        "<svg xmlns=\"{ns}\" \
         viewBox=\"0 0 {w} {h}\" \
         preserveAspectRatio=\"{aspect}\" \
         role=\"img\" aria-label=\"{aria}\">",
        ns = SVG_NAMESPACE,
        w = width,
        h = height,
        aspect = canvas.preserve_aspect_ratio(),
        aria = aria,
    );

    out.push_str("<g class=\"stat\">");

    if !series_name.is_empty() {
        let _ = write!(
            out,
            "<text x=\"{x}\" y=\"{y}\" text-anchor=\"middle\" class=\"code\" font-size=\"24\" font-family=\"{font}\" fill=\"{fill}\">{label}</text>",
            x = coord(cx),
            y = coord(cy - 50.0),
            font = theme.font_mono,
            fill = fill,
            label = escape_xml(&series_name),
        );
    }

    let _ = write!(
        out,
        "<text x=\"{x}\" y=\"{y}\" text-anchor=\"middle\" font-size=\"96\" font-weight=\"bold\" font-family=\"{font}\" fill=\"{fill}\">{label}</text>",
        x = coord(cx),
        y = coord(cy + 32.0),
        font = theme.font_sans,
        fill = fill,
        label = escape_xml(&number_text),
    );

    if let Some(title) = &chart.title {
        let title_text = match title {
            CiteOrText::Text(t) => t.clone(),
            CiteOrText::Cite(id) => id.0.clone(),
        };
        let _ = write!(
            out,
            "<text x=\"{x}\" y=\"{y}\" text-anchor=\"middle\" font-size=\"18\" font-family=\"{font}\" fill=\"{fill}\">{label}</text>",
            x = coord(cx),
            y = coord(cy + 80.0),
            font = theme.font_sans,
            fill = fill,
            label = escape_xml(&title_text),
        );
    }

    if legend_needed(chart.legend, chart.series.len()) {
        emit_legend(&mut out, chart, theme, mode, &PlotBox {
            x0: 0.0,
            y0: 0.0,
            x1: f64::from(width),
            y1: f64::from(height),
        })?;
    }
    emit_caption(&mut out, chart, theme, &PlotBox {
        x0: 0.0,
        y0: 0.0,
        x1: f64::from(width),
        y1: f64::from(height),
    });
    out.push_str("</g></svg>");
    Ok(out)
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

    fn cite(id: &str, v: f64, unit: Unit) -> FactCite {
        FactCite {
            id: FactId(id.to_owned()),
            value: v,
            unit,
        }
    }

    fn stat_chart(v: f64, unit: Unit, name: &str) -> Chart {
        Chart {
            kind: ChartKind::Stat,
            title: None,
            series: vec![Series {
                name: CiteOrText::Text(name.to_owned()),
                points: vec![Point {
                    label: None,
                    x: None,
                    y: cite("stat-1", v, unit),
                }],
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
    fn emits_big_number_text() {
        let theme = ResolvedTheme::summus_stub();
        let svg = emit(
            &stat_chart(42.0, Unit::Number, "Users"),
            &theme,
            &Canvas::Deck(DeckCanvas::default()),
            ColorMode::Resolved,
        )
        .expect("stat emits");
        assert!(svg.starts_with("<svg"));
        assert!(svg.contains("42"));
    }

    #[test]
    fn themed_mode_emits_css_var() {
        let theme = ResolvedTheme::summus_stub();
        let svg = emit(
            &stat_chart(99.0, Unit::Number, "Score"),
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
            &stat_chart(123.0, Unit::Number, "Total"),
            &theme,
            &Canvas::Deck(DeckCanvas::default()),
            ColorMode::Resolved,
        )
        .expect("first emit");
        let b = emit(
            &stat_chart(123.0, Unit::Number, "Total"),
            &theme,
            &Canvas::Deck(DeckCanvas::default()),
            ColorMode::Resolved,
        )
        .expect("second emit");
        assert_eq!(a, b);
    }

    #[test]
    fn series_name_uses_mono_font() {
        let theme = ResolvedTheme::summus_stub();
        let svg = emit(
            &stat_chart(42.0, Unit::Number, "Users"),
            &theme,
            &Canvas::Deck(DeckCanvas::default()),
            ColorMode::Resolved,
        )
        .expect("stat emits");
        assert!(svg.contains("class=\"code\""));
        assert!(svg.contains(&theme.font_mono));
    }

    #[test]
    fn money_unit_formats_as_dollars() {
        let theme = ResolvedTheme::summus_stub();
        let svg = emit(
            &stat_chart(1500.0, Unit::Money, "Revenue"),
            &theme,
            &Canvas::Deck(DeckCanvas::default()),
            ColorMode::Resolved,
        )
        .expect("money stat emits");
        assert!(svg.contains("$1500"));
    }
}

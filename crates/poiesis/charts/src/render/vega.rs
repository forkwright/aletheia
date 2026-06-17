//! Vega-Lite shell-out emitter (feature `charts-vega`).
//!
//! Handles kinds the pure-Rust emitter does not own: `heatmap`, `boxplot`,
//! `sankey`, `candlestick`, plus any axis with `Scale::Log` or `Scale::Time`.
//!
//! # Strategy
//!
//! Subprocess shell-out to `npx -y vega-lite@<pin> vl2svg in.vl.json > out.svg`.
//! Embedding a JS engine (`deno_core`/`quickjs`) was rejected because it
//! bloats the binary and couples the deck-render critical path to a JS
//! runtime; the CLI shell-out is the isolation.
//!
//! # Determinism
//!
//! - `vega-lite` + `vega` versions are pinned; the pin is part of the
//!   golden-snapshot contract.
//! - The Vega config block is theme-derived (palette + fonts) so theme
//!   swaps recolor here the same way they do on the Rust path.

use std::process::Command;

use serde_json::Value;

use crate::model::{Chart, ChartKind, CiteOrText, Point, Series};
use crate::render::canvas::Canvas;
use crate::theme::{ColorMode, ResolvedTheme};

/// Emit a Vega-Lite-rendered chart.
///
/// Builds a Vega-Lite 5.x JSON spec from the [`Chart`] model, then shells out
/// to `npx --yes vega-lite@5.20.1 --vl2svg` to produce SVG.
///
/// # Errors
///
/// Returns [`crate::Error::VegaShellout`] if the npx subprocess fails or the
/// spec cannot be serialised.
pub fn emit(
    chart: &Chart,
    theme: &ResolvedTheme,
    canvas: &Canvas,
    _mode: ColorMode,
) -> crate::Result<String> {
    let spec = build_spec(chart, theme, canvas);
    let spec_json = serde_json::to_string(&spec).map_err(|e| crate::Error::VegaShellout {
        message: format!("JSON serialization failed: {e}"),
    })?;
    vl_to_svg(&spec_json)
}

fn build_spec(chart: &Chart, theme: &ResolvedTheme, canvas: &Canvas) -> Value {
    let plot = canvas.plot_box();
    let width = plot.width();
    let height = plot.height();

    let colors: Vec<String> = theme.series.iter().map(|t| t.hex.clone()).collect();

    let data_rows = build_data_rows(chart);
    let mark = mark_for_kind(chart.kind);
    let encoding = encoding_for_kind(chart.kind);

    let mut spec = serde_json::Map::new();
    spec.insert(
        "$schema".to_owned(),
        "https://vega.github.io/schema/vega-lite/v5.json".into(),
    );
    spec.insert("width".to_owned(), width.into());
    spec.insert("height".to_owned(), height.into());
    spec.insert(
        "config".to_owned(),
        serde_json::json!({
            "font": theme.font_sans,
            "range": {
                "category": colors
            }
        }),
    );
    spec.insert(
        "data".to_owned(),
        serde_json::json!({ "values": data_rows }),
    );
    spec.insert("mark".to_owned(), mark);
    spec.insert("encoding".to_owned(), encoding);

    if let Some(title) = chart_title(chart) {
        spec.insert(
            "title".to_owned(),
            serde_json::json!({ "text": title, "anchor": "start" }),
        );
    }

    Value::Object(spec)
}

fn chart_title(chart: &Chart) -> Option<String> {
    match &chart.title {
        Some(CiteOrText::Text(t)) => Some(t.clone()),
        Some(CiteOrText::Cite(id)) => Some(id.0.clone()),
        None => None,
    }
}

fn build_data_rows(chart: &Chart) -> Vec<Value> {
    match chart.kind {
        ChartKind::Heatmap => build_heatmap_rows(chart),
        _ => build_generic_rows(chart),
    }
}

fn build_heatmap_rows(chart: &Chart) -> Vec<Value> {
    let mut rows = Vec::new();
    for series in &chart.series {
        let y_name = series_name(series);
        for point in &series.points {
            let x_name = point_label(point);
            rows.push(serde_json::json!({
                "x": x_name,
                "y": y_name,
                "value": point.y.value,
                "fact_id": &point.y.id.0,
            }));
        }
    }
    rows
}

fn build_generic_rows(chart: &Chart) -> Vec<Value> {
    let mut rows = Vec::new();
    for series in &chart.series {
        let s_name = series_name(series);
        for point in &series.points {
            rows.push(serde_json::json!({
                "series": s_name,
                "category": point_label(point),
                "value": point.y.value,
                "fact_id": &point.y.id.0,
            }));
        }
    }
    rows
}

fn series_name(series: &Series) -> String {
    match &series.name {
        CiteOrText::Text(t) => t.clone(),
        CiteOrText::Cite(id) => id.0.clone(),
    }
}

fn point_label(point: &Point) -> String {
    match &point.label {
        Some(CiteOrText::Text(t)) => t.clone(),
        Some(CiteOrText::Cite(id)) => id.0.clone(),
        None => String::new(),
    }
}

fn mark_for_kind(kind: ChartKind) -> Value {
    let mark_type = match kind {
        ChartKind::Heatmap => "rect",
        ChartKind::Boxplot => "boxplot",
        ChartKind::Sankey => "point",
        _ => "bar",
    };
    serde_json::json!({ "type": mark_type })
}

fn encoding_for_kind(kind: ChartKind) -> Value {
    match kind {
        ChartKind::Heatmap => serde_json::json!({
            "x": { "field": "x", "type": "ordinal" },
            "y": { "field": "y", "type": "ordinal" },
            "color": { "field": "value", "type": "quantitative" }
        }),
        _ => serde_json::json!({
            "x": { "field": "category", "type": "nominal" },
            "y": { "field": "value", "type": "quantitative" },
            "color": { "field": "series", "type": "nominal" }
        }),
    }
}

fn vl_to_svg(spec_json: &str) -> crate::Result<String> {
    let tmp = tempfile_path();
    std::fs::write(&tmp, spec_json).map_err(|e| crate::Error::VegaShellout {
        message: format!("temp file write failed: {e}"),
    })?;

    let output = Command::new("npx")
        .args([
            "--yes",
            "vega-lite@5.20.1",
            "--vl2svg",
            tmp.to_str().unwrap_or(""),
        ])
        .output()
        .map_err(|e| crate::Error::VegaShellout {
            message: format!("npx spawn failed: {e}"),
        })?;

    let _ = std::fs::remove_file(&tmp);

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(crate::Error::VegaShellout {
            message: format!("vega-lite exited {}: {stderr}", output.status),
        });
    }

    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

fn tempfile_path() -> std::path::PathBuf {
    let mut p = std::env::temp_dir();
    p.push(format!("poiesis-charts-vl-{}.json", std::process::id()));
    p
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
        Axes, AxisSide, Chart, ChartKind, CiteOrText, FactCite, FactId, LegendSpec, Point, Series,
        SeriesStyle, ToneRef, Unit,
    };
    use crate::render::canvas::{Canvas, DeckCanvas};
    use crate::theme::ResolvedTheme;

    fn heatmap_chart() -> Chart {
        Chart {
            kind: ChartKind::Heatmap,
            title: Some(CiteOrText::Text("Heat Test".to_owned())),
            series: vec![Series {
                name: CiteOrText::Text("Row1".to_owned()),
                points: vec![Point {
                    label: Some(CiteOrText::Text("ColA".to_owned())),
                    x: None,
                    y: FactCite {
                        id: FactId("f1".to_owned()),
                        value: 42.0,
                        unit: Unit::Number,
                    },
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
    fn heatmap_spec_has_rect_mark() {
        let theme = ResolvedTheme::summus_stub();
        let spec = build_spec(
            &heatmap_chart(),
            &theme,
            &Canvas::Deck(DeckCanvas::default()),
        );
        let mark = spec.get("mark").expect("mark present");
        assert_eq!(mark.get("type").and_then(|v| v.as_str()), Some("rect"));
    }

    #[test]
    fn spec_includes_theme_colors() {
        let theme = ResolvedTheme::summus_stub();
        let spec = build_spec(
            &heatmap_chart(),
            &theme,
            &Canvas::Deck(DeckCanvas::default()),
        );
        let colors = spec
            .pointer("/config/range/category")
            .and_then(|v| v.as_array())
            .expect("category range");
        assert_eq!(colors.len(), 3);
        assert_eq!(colors[0].as_str(), Some("#232E54"));
    }

    #[test]
    fn heatmap_data_has_x_y_value_fields() {
        let theme = ResolvedTheme::summus_stub();
        let spec = build_spec(
            &heatmap_chart(),
            &theme,
            &Canvas::Deck(DeckCanvas::default()),
        );
        let data = spec
            .pointer("/data/values")
            .and_then(|v| v.as_array())
            .expect("data values");
        assert_eq!(data.len(), 1);
        let row = &data[0];
        assert_eq!(row.get("x").and_then(|v| v.as_str()), Some("ColA"));
        assert_eq!(row.get("y").and_then(|v| v.as_str()), Some("Row1"));
        assert_eq!(
            row.get("value").and_then(serde_json::Value::as_f64),
            Some(42.0)
        );
    }

    #[test]
    fn generic_spec_has_bar_mark_for_candlestick() {
        let chart = Chart {
            kind: ChartKind::Candlestick,
            title: None,
            series: vec![Series {
                name: CiteOrText::Text("Prices".to_owned()),
                points: vec![Point {
                    label: Some(CiteOrText::Text("A".to_owned())),
                    x: None,
                    y: FactCite {
                        id: FactId("f2".to_owned()),
                        value: 10.0,
                        unit: Unit::Number,
                    },
                }],
                tone: ToneRef::Indexed(0),
                axis: AxisSide::Left,
                style: SeriesStyle::Default,
            }],
            axes: Axes::default(),
            legend: LegendSpec::Auto,
            data_labels: false,
            caption: None,
        };
        let theme = ResolvedTheme::summus_stub();
        let spec = build_spec(&chart, &theme, &Canvas::Deck(DeckCanvas::default()));
        let mark = spec.get("mark").expect("mark present");
        assert_eq!(mark.get("type").and_then(|v| v.as_str()), Some("bar"));
    }

    #[test]
    fn spec_uses_plot_box_dimensions() {
        let theme = ResolvedTheme::summus_stub();
        let spec = build_spec(
            &heatmap_chart(),
            &theme,
            &Canvas::Deck(DeckCanvas::default()),
        );
        assert_eq!(
            spec.get("width").and_then(serde_json::Value::as_f64),
            Some(1440.0)
        );
        assert_eq!(
            spec.get("height").and_then(serde_json::Value::as_f64),
            Some(400.0)
        );
    }

    #[test]
    fn spec_includes_chart_title() {
        let theme = ResolvedTheme::summus_stub();
        let spec = build_spec(
            &heatmap_chart(),
            &theme,
            &Canvas::Deck(DeckCanvas::default()),
        );
        let title = spec.get("title").expect("title present");
        assert_eq!(
            title.get("text").and_then(|v| v.as_str()),
            Some("Heat Test")
        );
    }

    #[test]
    fn boxplot_spec_has_boxplot_mark() {
        let chart = Chart {
            kind: ChartKind::Boxplot,
            title: None,
            series: vec![Series {
                name: CiteOrText::Text("Group".to_owned()),
                points: vec![
                    Point {
                        label: Some(CiteOrText::Text("A".to_owned())),
                        x: None,
                        y: FactCite {
                            id: FactId("b1".to_owned()),
                            value: 1.0,
                            unit: Unit::Number,
                        },
                    },
                    Point {
                        label: Some(CiteOrText::Text("A".to_owned())),
                        x: None,
                        y: FactCite {
                            id: FactId("b2".to_owned()),
                            value: 2.0,
                            unit: Unit::Number,
                        },
                    },
                ],
                tone: ToneRef::Indexed(0),
                axis: AxisSide::Left,
                style: SeriesStyle::Default,
            }],
            axes: Axes::default(),
            legend: LegendSpec::Auto,
            data_labels: false,
            caption: None,
        };
        let theme = ResolvedTheme::summus_stub();
        let spec = build_spec(&chart, &theme, &Canvas::Deck(DeckCanvas::default()));
        let mark = spec.get("mark").expect("mark present");
        assert_eq!(mark.get("type").and_then(|v| v.as_str()), Some("boxplot"));
    }

    #[test]
    fn sankey_spec_has_point_mark() {
        let chart = Chart {
            kind: ChartKind::Sankey,
            title: None,
            series: vec![Series {
                name: CiteOrText::Text("Flow".to_owned()),
                points: vec![Point {
                    label: Some(CiteOrText::Text("Src".to_owned())),
                    x: None,
                    y: FactCite {
                        id: FactId("s1".to_owned()),
                        value: 5.0,
                        unit: Unit::Number,
                    },
                }],
                tone: ToneRef::Indexed(0),
                axis: AxisSide::Left,
                style: SeriesStyle::Default,
            }],
            axes: Axes::default(),
            legend: LegendSpec::Auto,
            data_labels: false,
            caption: None,
        };
        let theme = ResolvedTheme::summus_stub();
        let spec = build_spec(&chart, &theme, &Canvas::Deck(DeckCanvas::default()));
        let mark = spec.get("mark").expect("mark present");
        assert_eq!(mark.get("type").and_then(|v| v.as_str()), Some("point"));
    }
}

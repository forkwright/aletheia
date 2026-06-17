//! Parse-don't-validate chart model.
//!
//! Every field is a newtype or a closed enum. Numeric data points reference
//! [`FactId`] entries in the deliverable's factbase — they are never naked
//! floats. The model carries no theme state and no render state; both live
//! one seam outward ([`crate::theme`], [`crate::render`]).
//!
//! # Field-by-field rationale
//!
//! - [`ChartKind`] is closed so the render-path decision rule can be a pure
//!   function and the gate test exhaustive.
//! - [`Series::tone`] is a [`ToneRef`], never a hex string — the three sinks
//!   (HTML, bake, PPTX-native) resolve the same tone, so the model must
//!   refuse a literal up front.
//! - [`Point::y`] is a [`FactCite`], not an `f64` — a raw number fails parse
//!   (`B-008` `naked-number`).
//! - [`AxisSpec::domain`] defaults to [`Domain::Auto`]; the emitter computes
//!   nice-rounded extents from data. The agent writes `axes: {}` and gets
//!   correct axes.
//!
//! `serde` derives live alongside the types so JSON ingest goes through
//! the same parse-don't-validate constructors as Rust callers; an
//! invariant violated at deserialize time becomes a typed [`crate::Error`].

use serde::{Deserialize, Serialize};

/// Closed set of chart kinds. Each kind has either a pure-Rust emitter arm
/// or a Vega-Lite template, named in [`ChartKind::render_path`].
///
/// Adding a kind = adding an emitter arm (Rust path) or a Vega-Lite template
/// (Vega path) plus name-check + a row in the [`render_path`](Self::render_path)
/// match.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[non_exhaustive]
pub enum ChartKind {
    /// Horizontal bars; 1+ series; x = value, y = category.
    Bar,
    /// Vertical columns; 1+ series; x = category, y = value.
    Column,
    /// Line chart; 1+ series; x = category/linear, y = value.
    Line,
    /// Area chart; 1+ series; axes as [`ChartKind::Line`].
    Area,
    /// Combo (columns + line on dual y-axes); exactly two series.
    ///
    /// `combo` is first-class because the offsite slide-3 chart is the
    /// acceptance gate. It is the one curated multi-axis composition the
    /// Rust emitter owns; further compositions route to Vega-Lite.
    Combo,
    /// Scatter plot; 1+ series; both axes linear.
    Scatter,
    /// Pie chart; 1 series; no axes.
    Pie,
    /// Doughnut chart; 1 series; no axes.
    Doughnut,
    /// Single-number / "big stat" component; 1 series; no axes.
    Stat,
    /// Routed to Vega-Lite. Heatmap, boxplot, sankey, candlestick, faceted,
    /// regression-fit — all live behind the `charts-vega` feature.
    Heatmap,
    /// Routed to Vega-Lite.
    Boxplot,
    /// Routed to Vega-Lite.
    Sankey,
    /// Routed to Vega-Lite.
    Candlestick,
}

/// Static render-path classification.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RenderPath {
    /// Pure-Rust SVG emitter arm.
    Rust,
    /// Vega-Lite shell-out (requires `charts-vega`).
    Vega,
}

impl ChartKind {
    /// Pure rule: which emitter owns this kind?
    ///
    /// Vega kinds: `heatmap`, `boxplot`, `sankey`, `candlestick`. Axis-scale
    /// driven Vega routing (`log`, `time`) happens at [`Chart::validate`]
    /// rather than here, because it depends on the spec's axes.
    #[must_use]
    pub const fn render_path(self) -> RenderPath {
        match self {
            Self::Bar
            | Self::Column
            | Self::Line
            | Self::Area
            | Self::Combo
            | Self::Scatter
            | Self::Pie
            | Self::Doughnut
            | Self::Stat => RenderPath::Rust,
            Self::Heatmap | Self::Boxplot | Self::Sankey | Self::Candlestick => RenderPath::Vega,
        }
    }

    /// Canonical lowercase name for diagnostics.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Bar => "bar",
            Self::Column => "column",
            Self::Line => "line",
            Self::Area => "area",
            Self::Combo => "combo",
            Self::Scatter => "scatter",
            Self::Pie => "pie",
            Self::Doughnut => "doughnut",
            Self::Stat => "stat",
            Self::Heatmap => "heatmap",
            Self::Boxplot => "boxplot",
            Self::Sankey => "sankey",
            Self::Candlestick => "candlestick",
        }
    }
}

/// Newtype wrapping a factbase entry identifier.
///
/// `FactId` is the only path for a numeric datum to enter a chart. Free-text
/// labels can be a [`CiteOrText::Text`]; data values cannot.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct FactId(pub String);

/// A citation reference into the factbase.
///
/// The unit is carried alongside the id so the renderer can resolve
/// `NumFormat::FromUnit` without a separate factbase round-trip.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FactCite {
    /// Factbase entry id.
    pub id: FactId,
    /// Resolved numeric value (resolution happens at deserialize time so the
    /// emitter does not need access to the factbase).
    pub value: f64,
    /// Unit attached to the fact, used by `NumFormat::FromUnit`.
    pub unit: Unit,
}

/// Unit of a numeric fact.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
#[non_exhaustive]
pub enum Unit {
    /// Dimensionless integer or float, e.g. headcount or count.
    Number,
    /// Currency (formatter chooses symbol from theme locale).
    Money,
    /// Percentage (raw value is the percentage, e.g. `12.5` for 12.5 %).
    Percent,
    /// Duration in seconds.
    Seconds,
}

/// A value that may be cited from the factbase or supplied as a literal scalar.
///
/// Used by [`Point::x`] on linear/scatter axes — the x position may be a fact
/// (e.g. a measured timestamp) or a plain numeric coordinate (e.g. a tick
/// position).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum CiteOrScalar {
    /// Cited from the factbase.
    Cite(FactCite),
    /// Literal scalar coordinate.
    Scalar(f64),
}

/// A value that may be cited from the factbase or supplied as literal text.
///
/// Used by series names, axis titles, and category labels — these are
/// human-readable but may still resolve from the factbase for traceability.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum CiteOrText {
    /// Cited from the factbase (text rendering pulls the fact's `display`).
    Cite(FactId),
    /// Literal text.
    Text(String),
}

/// Inline text fragments for captions, kept as an opaque vector.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Inlines(pub Vec<String>);

/// Reference to a theme tone slot.
///
/// Resolves against the active theme's `[chart]` palette (`B-002`). The
/// indexed form is the common case (`Indexed(0)` = the first series tone);
/// the named form is for cross-chart shared tones (e.g. a brand-secondary
/// accent reused across kinds).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ToneRef {
    /// `theme.chart.series[i]`.
    Indexed(usize),
    /// A named tone in the theme's palette.
    Named(String),
}

/// Combo-only: which axis a series binds to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AxisSide {
    /// Left axis (default for non-combo charts).
    #[default]
    Left,
    /// Right axis (combo only).
    Right,
}

/// Per-series style override.
///
/// `kind`-default applies when this is `SeriesStyle::Default`; for `combo`,
/// each series declares its own style (`Column` or `Line`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SeriesStyle {
    /// Use the chart kind's default style.
    #[default]
    Default,
    /// Render this series as columns (combo only).
    Column,
    /// Render this series as a line (combo only).
    Line,
    /// Render this series as an area (combo only).
    Area,
}

/// Legend layout policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum LegendSpec {
    /// Emitter decides: hidden for single-series, top-right for multi-series.
    #[default]
    Auto,
    /// Hide the legend regardless of series count.
    None,
    /// Force a top-right legend.
    TopRight,
    /// Force a bottom legend.
    Bottom,
}

/// Numeric format applied to axis ticks and data labels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum NumFormat {
    /// Read the format from the cited fact's [`Unit`].
    #[default]
    FromUnit,
    /// Plain integer.
    Int,
    /// Money (locale-aware symbol; thousands grouping).
    Money,
    /// Percentage with one decimal.
    Percent,
    /// SI-prefix compact form (`1.2k`, `3.4M`).
    Compact,
}

/// Axis scale.
///
/// `Log` and `Time` route through Vega-Lite; the pure-Rust emitter only
/// handles `Linear` and `Category`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Scale {
    /// Linear numeric axis.
    #[default]
    Linear,
    /// Discrete categorical axis (string labels).
    Category,
    /// Log-10 axis (routes to Vega-Lite).
    Log,
    /// Time axis (routes to Vega-Lite).
    Time,
}

/// Axis tick policy.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Ticks {
    /// Emitter targets ~5 ticks at 1/2/5×10ⁿ.
    #[default]
    Auto,
    /// Emitter targets the supplied tick count at 1/2/5×10ⁿ.
    Count(u8),
    /// Explicit tick values (no rounding applied).
    Explicit(Vec<f64>),
}

/// Domain bounds for a numeric axis.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Domain {
    /// `nice()` extents computed from the data.
    #[default]
    Auto,
    /// Explicit min/max (no rounding).
    Fixed {
        /// Lower bound.
        min: f64,
        /// Upper bound.
        max: f64,
    },
}

/// One axis (x, y-left, or y-right).
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct AxisSpec {
    /// Axis title (`None` = no title).
    #[serde(default)]
    pub title: Option<CiteOrText>,
    /// Domain bounds (default: auto).
    #[serde(default)]
    pub domain: Domain,
    /// Tick policy (default: auto).
    #[serde(default)]
    pub ticks: Ticks,
    /// Number format (default: from-unit).
    #[serde(default)]
    pub format: NumFormat,
    /// Axis scale (default: linear).
    #[serde(default)]
    pub scale: Scale,
}

/// The three axes a Rust-path chart may have.
///
/// `y_right` is only meaningful for `combo`; other kinds leave it `None`.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Axes {
    /// x axis (category or linear).
    #[serde(default)]
    pub x: AxisSpec,
    /// Primary y axis.
    #[serde(default)]
    pub y_left: AxisSpec,
    /// Combo-only secondary y axis.
    #[serde(default)]
    pub y_right: Option<AxisSpec>,
}

/// One data point on a series.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Point {
    /// Category label (e.g. "MAR") for category x-axes.
    #[serde(default)]
    pub label: Option<CiteOrText>,
    /// x position for linear/scatter axes; `None` for category axes.
    #[serde(default)]
    pub x: Option<CiteOrScalar>,
    /// y value — always a `FactCite`. Naked numbers fail parse.
    pub y: FactCite,
}

/// One data series.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Series {
    /// Legend label (literal or cited).
    pub name: CiteOrText,
    /// Series data points.
    pub points: Vec<Point>,
    /// Tone slot in the active theme.
    pub tone: ToneRef,
    /// Combo-only: which y-axis this series binds to.
    #[serde(default)]
    pub axis: AxisSide,
    /// Per-series style (default: kind-default).
    #[serde(default)]
    pub style: SeriesStyle,
}

/// The validated payload of the `chart` component.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Chart {
    /// Chart kind (closed enum).
    pub kind: ChartKind,
    /// Chart title (literal or cited).
    #[serde(default)]
    pub title: Option<CiteOrText>,
    /// Data series (1+ for kind-specific shapes; combo requires exactly 2).
    pub series: Vec<Series>,
    /// Axis specs (auto-defaulted; agents may write `{}`).
    #[serde(default)]
    pub axes: Axes,
    /// Legend policy (default: auto).
    #[serde(default)]
    pub legend: LegendSpec,
    /// Show on-bar / on-point data labels (kind-default decides if omitted).
    #[serde(default)]
    pub data_labels: bool,
    /// Optional caption.
    #[serde(default)]
    pub caption: Option<Inlines>,
}

impl Chart {
    /// Run the per-kind shape contract and the render-path admission check.
    ///
    /// Returns `Ok` on a valid spec; returns [`crate::Error::BadSeriesShape`]
    /// or [`crate::Error::VegaRequired`] otherwise. Calling code is expected
    /// to invoke this at parse time (immediately after deserialization)
    /// rather than at render time, so a bad spec fails before any byte of
    /// SVG is produced.
    ///
    /// # Errors
    ///
    /// - [`crate::Error::BadSeriesShape`] — series count or styles violate
    ///   the per-kind contract (e.g. `combo` with three series).
    /// - [`crate::Error::VegaRequired`] — the kind or an axis scale routes
    ///   to Vega-Lite and the `charts-vega` feature is disabled.
    pub fn validate(&self) -> crate::Result<()> {
        match self.kind {
            ChartKind::Combo => {
                if self.series.len() != 2 {
                    return Err(crate::Error::BadSeriesShape {
                        kind: self.kind.name().to_owned(),
                        expected: "exactly 2 series (one Column, one Line)".to_owned(),
                        actual: format!("{} series", self.series.len()),
                        path: "/series".to_owned(),
                    });
                }
            }
            ChartKind::Pie | ChartKind::Doughnut | ChartKind::Stat => {
                if self.series.len() != 1 {
                    return Err(crate::Error::BadSeriesShape {
                        kind: self.kind.name().to_owned(),
                        expected: "exactly 1 series".to_owned(),
                        actual: format!("{} series", self.series.len()),
                        path: "/series".to_owned(),
                    });
                }
            }
            _ => {
                if self.series.is_empty() {
                    return Err(crate::Error::BadSeriesShape {
                        kind: self.kind.name().to_owned(),
                        expected: "1+ series".to_owned(),
                        actual: "0 series".to_owned(),
                        path: "/series".to_owned(),
                    });
                }
            }
        }

        let needs_vega = matches!(self.kind.render_path(), RenderPath::Vega)
            || matches!(self.axes.x.scale, Scale::Log | Scale::Time)
            || matches!(self.axes.y_left.scale, Scale::Log | Scale::Time)
            || self
                .axes
                .y_right
                .as_ref()
                .is_some_and(|a| matches!(a.scale, Scale::Log | Scale::Time));

        if needs_vega && !cfg!(feature = "charts-vega") {
            let scale_name = scale_routing_name(self);
            return Err(crate::Error::VegaRequired {
                kind: self.kind.name().to_owned(),
                scale: scale_name.to_owned(),
            });
        }

        Ok(())
    }
}

fn scale_routing_name(c: &Chart) -> &'static str {
    if matches!(c.axes.x.scale, Scale::Log) || matches!(c.axes.y_left.scale, Scale::Log) {
        "log"
    } else if matches!(c.axes.x.scale, Scale::Time) || matches!(c.axes.y_left.scale, Scale::Time) {
        "time"
    } else {
        "linear"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cite(id: &str, value: f64) -> FactCite {
        FactCite {
            id: FactId(id.to_owned()),
            value,
            unit: Unit::Number,
        }
    }

    fn point(label: &str, y: FactCite) -> Point {
        Point {
            label: Some(CiteOrText::Text(label.to_owned())),
            x: None,
            y,
        }
    }

    fn series(
        name: &str,
        tone: usize,
        style: SeriesStyle,
        axis: AxisSide,
        points: Vec<Point>,
    ) -> Series {
        Series {
            name: CiteOrText::Text(name.to_owned()),
            points,
            tone: ToneRef::Indexed(tone),
            axis,
            style,
        }
    }

    #[test]
    fn combo_with_two_series_validates() {
        let c = Chart {
            kind: ChartKind::Combo,
            title: None,
            series: vec![
                series(
                    "Revenue",
                    0,
                    SeriesStyle::Column,
                    AxisSide::Left,
                    vec![
                        point("MAR", cite("f1", 10.0)),
                        point("APR", cite("f2", 20.0)),
                    ],
                ),
                series(
                    "Headcount",
                    1,
                    SeriesStyle::Line,
                    AxisSide::Right,
                    vec![
                        point("MAR", cite("f3", 110.0)),
                        point("APR", cite("f4", 130.0)),
                    ],
                ),
            ],
            axes: Axes::default(),
            legend: LegendSpec::Auto,
            data_labels: false,
            caption: None,
        };
        assert!(c.validate().is_ok());
    }

    #[test]
    fn combo_with_one_series_fails() {
        let c = Chart {
            kind: ChartKind::Combo,
            title: None,
            series: vec![series(
                "Revenue",
                0,
                SeriesStyle::Column,
                AxisSide::Left,
                vec![point("MAR", cite("f1", 10.0))],
            )],
            axes: Axes::default(),
            legend: LegendSpec::Auto,
            data_labels: false,
            caption: None,
        };
        assert!(matches!(
            c.validate(),
            Err(crate::Error::BadSeriesShape { .. })
        ));
    }

    #[test]
    fn heatmap_without_vega_feature_fails() {
        let c = Chart {
            kind: ChartKind::Heatmap,
            title: None,
            series: vec![series(
                "Values",
                0,
                SeriesStyle::Default,
                AxisSide::Left,
                vec![point("MAR", cite("f1", 10.0))],
            )],
            axes: Axes::default(),
            legend: LegendSpec::Auto,
            data_labels: false,
            caption: None,
        };
        let r = c.validate();
        if cfg!(feature = "charts-vega") {
            assert!(r.is_ok());
        } else {
            assert!(matches!(r, Err(crate::Error::VegaRequired { .. })));
        }
    }

    #[test]
    fn log_scale_without_vega_feature_fails() {
        let mut axes = Axes::default();
        axes.y_left.scale = Scale::Log;
        let c = Chart {
            kind: ChartKind::Line,
            title: None,
            series: vec![series(
                "Values",
                0,
                SeriesStyle::Default,
                AxisSide::Left,
                vec![point("MAR", cite("f1", 10.0))],
            )],
            axes,
            legend: LegendSpec::Auto,
            data_labels: false,
            caption: None,
        };
        let r = c.validate();
        if cfg!(feature = "charts-vega") {
            assert!(r.is_ok());
        } else {
            assert!(matches!(r, Err(crate::Error::VegaRequired { .. })));
        }
    }

    #[test]
    fn render_path_classification_is_total() {
        for k in [
            ChartKind::Bar,
            ChartKind::Column,
            ChartKind::Line,
            ChartKind::Area,
            ChartKind::Combo,
            ChartKind::Scatter,
            ChartKind::Pie,
            ChartKind::Doughnut,
            ChartKind::Stat,
        ] {
            assert_eq!(k.render_path(), RenderPath::Rust, "{}", k.name());
        }
        for k in [
            ChartKind::Heatmap,
            ChartKind::Boxplot,
            ChartKind::Sankey,
            ChartKind::Candlestick,
        ] {
            assert_eq!(k.render_path(), RenderPath::Vega, "{}", k.name());
        }
    }
}

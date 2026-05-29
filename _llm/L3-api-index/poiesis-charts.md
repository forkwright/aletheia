# L3 API Index: poiesis-charts

Crate path: `crates/poiesis/charts`

Public API signatures extracted from source. Each signature is preceded by its doc comment.
For implementation context, read the source directly (`L4`).

## `src/error.rs`

```rust
pub enum Error {
    /// The supplied JSON does not match the [`Chart`](crate::model::Chart) schema.
    #[snafu(display("invalid chart JSON at {path}: {message}"))]
    InvalidJson {
        /// JSON pointer / dotted field path of the offending value.
        path: String,
        /// Human-readable description of the schema violation.
        message: String,
    },

    /// A required series count or shape was not satisfied for the given kind.
    ///
    /// Example: `kind = combo` requires exactly two series, one `column`
    /// styled and one `line` styled.
    #[snafu(display("kind `{kind}` requires {expected} but got {actual} (at series {path})"))]
    BadSeriesShape {
        /// The chart kind whose contract was violated.
        kind: String,
        /// Human-readable shape requirement (e.g. "exactly 2 series").
        expected: String,
        /// What was actually present.
        actual: String,
        /// Field path to the offending series array.
        path: String,
    },

    /// A data point used a literal number instead of a `FactCite`.
    ///
    /// The model types `y` (and `x` for scatter/linear axes) as
    /// `FactCite` exactly so that this never happens at runtime — but the
    /// JSON ingest path may still encounter raw numbers, and they must be
    /// rejected loudly, per `B-008` `naked-number`.
    #[snafu(display("naked number at {path}: chart data must reference Fact ids"))]
    NakedNumber {
        /// JSON pointer to the offending raw number.
        path: String,
    },

    /// The spec requires the Vega-Lite fallback but `charts-vega` is disabled.
    ///
    /// Hard refusal, never a silent degrade. The caller is expected to enable
    /// the feature, switch to a kind the Rust emitter owns, or change the
    /// axis scale.
    #[snafu(display(
        "kind `{kind}` (scale `{scale}`) needs the `charts-vega` feature; \
         enable it or use a Rust-owned kind"
    ))]
    VegaRequired {
        /// The chart kind that triggered the routing.
        kind: String,
        /// The axis scale that triggered the routing (or `linear` if kind-driven).
        scale: String,
    },

    /// A theme tone reference did not resolve.
    ///
    /// Series fills and strokes must resolve from `theme.chart.series[i]`
    /// or a named tone in the active theme — never literal hex.
    #[snafu(display("unresolved tone reference `{tone}` in series {series_index}"))]
    UnresolvedTone {
        /// The tone reference that failed to resolve.
        tone: String,
        /// Index of the offending series.
        series_index: usize,
    },

    /// A `FactCite` did not resolve in the supplied factbase.
    #[snafu(display("unresolved fact `{fact_id}` at {path}"))]
    UnresolvedFact {
        /// The fact id that failed to resolve.
        fact_id: String,
        /// JSON pointer to the offending cite.
        path: String,
    },

    /// A pure-Rust emitter arm is not yet implemented for this kind.
    ///
    /// Distinct from [`Error::VegaRequired`]: the kind belongs to the Rust
    /// path, but the per-kind code is still stubbed. The PR body documents
    /// which arms are stubbed and the completion plan; this error is the
    /// gate-traceable handle on each stub.
    #[snafu(display("kind `{kind}` is scaffolded but the emitter arm is not yet implemented"))]
    EmitterStub {
        /// The chart kind whose emitter arm is still a stub.
        kind: String,
    },
}
```

## `src/format.rs`

```rust
pub fn format_number (value: f64, format: NumFormat, unit: Unit) -> String
```

```rust
pub fn coord (v: f64) -> String
```

## `src/lib.rs`

> Result alias for poiesis-charts operations.
```rust
pub type Result<T, E = Error> = std::result::Result<T, E>;
```

## `src/model.rs`

```rust
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
```

```rust
pub enum RenderPath {
    /// Pure-Rust SVG emitter arm.
    Rust,
    /// Vega-Lite shell-out (requires `charts-vega`).
    Vega,
}
```

```rust
impl ChartKind {
    pub const fn render_path (self) -> RenderPath;
    pub const fn name (self) -> &'static str;
}
```

```rust
pub struct FactId(pub String);
```

```rust
pub struct FactCite {
    /// Factbase entry id.
    pub id: FactId,
    /// Resolved numeric value (resolution happens at deserialize time so the
    /// emitter does not need access to the factbase).
    pub value: f64,
    /// Unit attached to the fact, used by `NumFormat::FromUnit`.
    pub unit: Unit,
}
```

```rust
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
```

```rust
pub enum CiteOrScalar {
    /// Cited from the factbase.
    Cite(FactCite),
    /// Literal scalar coordinate.
    Scalar(f64),
}
```

```rust
pub enum CiteOrText {
    /// Cited from the factbase (text rendering pulls the fact's `display`).
    Cite(FactId),
    /// Literal text.
    Text(String),
}
```

```rust
pub struct Inlines(pub Vec<String>);
```

```rust
pub enum ToneRef {
    /// `theme.chart.series[i]`.
    Indexed(usize),
    /// A named tone in the theme's palette.
    Named(String),
}
```

```rust
pub enum AxisSide {
    /// Left axis (default for non-combo charts).
    #[default]
    Left,
    /// Right axis (combo only).
    Right,
}
```

```rust
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
```

```rust
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
```

```rust
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
```

```rust
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
```

```rust
pub enum Ticks {
    /// Emitter targets ~5 ticks at 1/2/5×10ⁿ.
    #[default]
    Auto,
    /// Emitter targets the supplied tick count at 1/2/5×10ⁿ.
    Count(u8),
    /// Explicit tick values (no rounding applied).
    Explicit(Vec<f64>),
}
```

```rust
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
```

```rust
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
```

```rust
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
```

```rust
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
```

```rust
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
```

```rust
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
```

```rust
impl Chart {
    pub fn validate (&self) -> crate::Result<()>;
}
```

## `src/render/canvas.rs`

```rust
pub enum Canvas {
    /// Deck stage canvas.
    Deck(DeckCanvas),
    /// Document figure canvas.
    Doc(DocCanvas),
}
```

```rust
pub struct DeckCanvas {
    /// viewBox width.
    pub width: u32,
    /// viewBox height.
    pub height: u32,
    /// Left margin (y-left tick labels).
    pub margin_left: u32,
    /// Right margin (y-right tick labels for combo).
    pub margin_right: u32,
    /// Top margin (title + legend headroom).
    pub margin_top: u32,
    /// Bottom margin (x-tick labels).
    pub margin_bottom: u32,
}
```

```rust
pub struct DocCanvas {
    /// viewBox width.
    pub width: u32,
    /// viewBox height.
    pub height: u32,
    /// Left margin.
    pub margin_left: u32,
    /// Right margin.
    pub margin_right: u32,
    /// Top margin.
    pub margin_top: u32,
    /// Bottom margin.
    pub margin_bottom: u32,
}
```

```rust
pub struct PlotBox {
    /// Left edge.
    pub x0: f64,
    /// Top edge.
    pub y0: f64,
    /// Right edge.
    pub x1: f64,
    /// Bottom edge.
    pub y1: f64,
}
```

```rust
impl PlotBox {
    pub const fn width (&self) -> f64;
    pub const fn height (&self) -> f64;
}
```

```rust
impl Canvas {
    pub const fn width (&self) -> u32;
    pub const fn height (&self) -> u32;
    pub const fn preserve_aspect_ratio (&self) -> &'static str;
    pub fn plot_box (&self) -> PlotBox;
}
```

## `src/render/kinds/bar.rs`

> Emit the bar chart SVG.
> 
> Caller invariant (enforced by [`Chart::validate`](crate::model::Chart::validate)):
> - `chart.kind == ChartKind::Bar`
> - `chart.series` is non-empty
```rust
pub fn emit (
    chart: &Chart,
    theme: &ResolvedTheme,
    canvas: &Canvas,
    mode: ColorMode,
) -> Result<String>
```

## `src/render/kinds/column.rs`

> Emit the column chart SVG.
> 
> Caller invariant (enforced by [`Chart::validate`](crate::model::Chart::validate)):
> - `chart.kind == ChartKind::Column`
> - `chart.series` is non-empty
```rust
pub fn emit (
    chart: &Chart,
    theme: &ResolvedTheme,
    canvas: &Canvas,
    mode: ColorMode,
) -> Result<String>
```

## `src/render/kinds/combo.rs`

> Emit the combo chart SVG.
> 
> Caller invariants (enforced by [`Chart::validate`](crate::model::Chart::validate)):
> - `chart.kind == ChartKind::Combo`
> - exactly two series
> 
> The first series with `axis == Left` is treated as the column series;
> the first with `axis == Right` is treated as the line series.
```rust
pub fn emit (
    chart: &Chart,
    theme: &ResolvedTheme,
    canvas: &Canvas,
    mode: ColorMode,
) -> Result<String>
```

## `src/render/kinds/doughnut.rs`

```rust
pub fn emit (
    chart: &Chart,
    theme: &ResolvedTheme,
    canvas: &Canvas,
    mode: ColorMode,
) -> Result<String>
```

## `src/render/kinds/pie.rs`

> Emit the pie chart SVG.
> 
> Caller invariants (enforced by [`Chart::validate`](crate::model::Chart::validate)):
> - `chart.kind == ChartKind::Pie`
> - `series.len() == 1`
> - `points.len() >= 1`
```rust
pub fn emit (
    chart: &Chart,
    theme: &ResolvedTheme,
    canvas: &Canvas,
    mode: ColorMode,
) -> Result<String>
```

## `src/render/kinds/scatter.rs`

> Emit the scatter chart SVG.
> 
> Caller invariants (enforced by [`Chart::validate`](crate::model::Chart::validate)):
> - `chart.kind == ChartKind::Scatter`
> - `1+ series`
```rust
pub fn emit (
    chart: &Chart,
    theme: &ResolvedTheme,
    canvas: &Canvas,
    mode: ColorMode,
) -> Result<String>
```

## `src/render/vega.rs`

> Emit a Vega-Lite-rendered chart.
> 
> # Errors
> 
> Returns [`crate::Error::EmitterStub`] until the shell-out path lands.
```rust
pub fn emit (
    chart: &Chart,
    _theme: &ResolvedTheme,
    _canvas: &Canvas,
    _mode: ColorMode,
) -> Result<String>
```

## `src/render.rs`

> Render a [`Chart`] to an SVG byte string.
> 
> Geometry follows the canvas the caller picks:
> [`DeckCanvas`] for the deck stage (`1600×540`), [`DocCanvas`] for a
> document figure (intrinsic box per writer). The same chart spec renders
> identically across canvases up to inner-box scaling.
> 
> # Errors
> 
> - [`crate::Error::VegaRequired`]  -  kind / scale needs the Vega-Lite
>   fallback and `charts-vega` is disabled.
> - [`crate::Error::BadSeriesShape`]  -  series count violates the per-kind
>   contract.
> - [`crate::Error::EmitterStub`]  -  pure-Rust arm for this kind is not yet
>   implemented (stub list above).
> - [`crate::Error::UnresolvedTone`]  -  a series references a tone the
>   theme does not provide.
```rust
pub fn render_chart (
    chart: &Chart,
    theme: &ResolvedTheme,
    canvas: &Canvas,
    mode: ColorMode,
) -> Result<String>
```

## `src/scale.rs`

```rust
pub struct Scale {
    /// Data-space extent (`min`, `max`).
    pub domain: (f64, f64),
    /// Pixel-space extent (`px0`, `px1`).
    pub range: (f64, f64),
}
```

```rust
impl Scale {
    pub const fn new (domain: (f64, f64), range: (f64, f64)) -> Self;
    pub fn map (&self, value: f64) -> f64;
}
```

```rust
pub fn nice (min: f64, max: f64) -> (f64, f64)
```

```rust
pub fn ticks (min: f64, max: f64, target_count: u8) -> Vec<f64>
```

## `src/theme.rs`

```rust
pub enum ColorMode {
    /// Emit `var(--tone-N)` references. HTML target.
    Themed,
    /// Emit literal `#RRGGBB`. PPTX bake / document target.
    Resolved,
}
```

```rust
pub struct ResolvedTheme {
    /// Ordered series palette. `ToneRef::Indexed(i)` resolves to
    /// `series[i]`; out-of-bounds is a parse-time error.
    pub series: Vec<Tone>,
    /// Named tones referenced by `ToneRef::Named(name)`.
    pub named: Vec<NamedTone>,
    /// Theme name (used for the `--tone-*` prefix in `Themed` mode).
    pub theme_name: String,
    /// Sans serif font family token.
    pub font_sans: String,
    /// Mono font family token.
    pub font_mono: String,
}
```

```rust
pub struct Tone {
    /// CSS variable name used in `Themed` mode (e.g. `series-0`).
    pub css_var: String,
    /// Resolved hex color (e.g. `#232E54`).
    pub hex: String,
}
```

```rust
pub struct NamedTone {
    /// Tone name as referenced from a chart spec.
    pub name: String,
    /// Resolved hex color.
    pub hex: String,
}
```

```rust
impl ResolvedTheme {
    pub fn summus_stub () -> Self;
    pub fn fill_for (
        &self,
        tone: &ToneRef,
        mode: ColorMode,
        series_index: usize,
    ) -> crate::Result<String>;
}
```

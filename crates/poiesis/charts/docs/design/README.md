# Per-kind emitter designs

Each file in this directory sketches the geometry, fixed source order, and
shared-primitive reuse for one `ChartKind` emitter arm. The combo arm
(`src/render/kinds/combo.rs`) is the implemented reference; the remaining
notes track the other arms in the same source-order vocabulary.

These notes are intentionally short — each one names what's specific to the
kind and points at the shared primitives the arm reuses. They are the
substrate for the per-kind implementation fan-out (the next PRs).

## Shared primitives

Every arm consumes:

- `model::Chart` — validated spec.
- `theme::ResolvedTheme` + `theme::ColorMode` — fill / stroke resolution.
- `render::canvas::Canvas` + `PlotBox` — outer viewBox + inner plot box.
- `scale::Scale`, `scale::nice`, `scale::ticks` — domain ↔ pixel mapping.
- `format::coord`, `format::format_number` — the only paths from `f64` to SVG text.

## Source-order convention

Every arm emits groups in this fixed order so the SVG is byte-deterministic:

1. `<svg>` open
2. `<g class="gridlines">`
3. `<g class="axes">`
4. `<g class="<primary>">` — the kind-specific primary group (bars, line, slices, …)
5. `<g class="<secondary>">` — kind-specific secondary group, if any (markers, hole, …)
6. `<g class="labels">` — data labels when `chart.data_labels`
7. `<g class="x-labels">` / `<g class="legend">` — axis labels and legend
8. `</svg>` close

The combo arm follows this; new arms keep the order.

## Kinds

| Kind | File |
|---|---|
| bar | [bar.md](bar.md) |
| column | [column.md](column.md) |
| line | [line.md](line.md) |
| area | [area.md](area.md) |
| scatter | [scatter.md](scatter.md) |
| pie | [pie.md](pie.md) |
| doughnut | [doughnut.md](doughnut.md) |
| stat | [stat.md](stat.md) |
| combo | implemented — see `src/render/kinds/combo.rs` |

Vega-Lite kinds (`heatmap`, `boxplot`, `sankey`, `candlestick`) route through
`Chart::validate` and emit via the `charts-vega` feature; their design
lives with `src/render/vega.rs` (follow-up).

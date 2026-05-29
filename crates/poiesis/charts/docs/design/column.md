# column

Vertical columns. The x axis carries categories, the y axis carries values.
1+ series.

## Geometry

- x-scale: categorical. `n` bands of equal width across `plot.width()`.
- y-scale: linear, `nice_domain(values)`, `Scale::new((lo, hi), (plot.y1, plot.y0))` (inverted; SVG y grows down).
- per-column rect: `x = band_center - bar_w/2`, `y = scale.map(value)`, `width = bar_w`, `height = plot.y1 - y`.
- corners: `rx = 3`.
- multi-series: bands subdivided by series count, fills cycle through theme palette.

## Source order

Same as combo's column path:

1. `<g class="gridlines">` — horizontal gridlines at y-tick positions.
2. `<g class="axes">` — y-tick value labels (left), x-tick category labels (below).
3. `<g class="bars">` — `<rect>` per (band, series), category-major source order.
4. `<g class="labels">` — value labels on top of bar (centered, dark) or inside top (centered, white).
5. `<g class="legend">` — top-right when multi-series.

## Reuses

- The combo arm's `emit_bars` is the direct template — pull it into a shared
  `render::primitives::bars::emit_columns` when the column arm lands.
- `scale::nice` for y extent, `scale::ticks` for y ticks.

## Acceptance pointer

`B-005 §1.1` row `column`. The combo arm already implements this primitive
for its left axis; the column arm is `combo` minus the right-axis line series.

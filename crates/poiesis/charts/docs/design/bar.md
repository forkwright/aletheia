# bar

Horizontal bars. The y axis carries categories, the x axis carries values.
1+ series.

## Geometry

- y-scale: categorical. `n` bands of equal height across `plot.height()`.
- x-scale: linear, `nice_domain(values)`, `Scale::new((lo, hi), (plot.x0, plot.x1))`.
- per-bar rect: `x = plot.x0`, `y = band_top`, `width = scale.map(value) - plot.x0`, `height = bar_h`.
- bar corners: `rx = 3`.
- multi-series: bands subdivided by series count, fills cycle through theme palette.

## Source order

1. `<g class="gridlines">` — vertical gridlines at x-tick positions.
2. `<g class="axes">` — y-tick category labels (left of plot), x-tick value labels (below plot).
3. `<g class="bars">` — `<rect>` per (band, series). Fixed source order = category-major, series-minor.
4. `<g class="labels">` — value labels right-of-bar end (or inside bar for wide bars).
5. `<g class="legend">` — top-right when multi-series.

## Reuses

- `scale::nice` for x extent.
- `scale::ticks` for x ticks.
- `format::coord` for every `<rect>` attribute.
- `format::format_number(value, axis.format, point.y.unit)` for value labels.

## Acceptance pointer

`B-005 §1.1` row `bar`: 1+ series; x = value, y = category. The combo arm's
column primitives are the closest reusable template; the bar arm differs
only in axis swap.

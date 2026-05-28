# pie

Pie chart. Exactly 1 series. No axes.

## Geometry

- Total = sum of `point.y.value` across the single series.
- Angle accumulator: `θ_start = -π/2` (top center), `θ_end = θ_start + 2π * (value / total)`.
- Slice path: SVG arc `M cx,cy L <start_x>,<start_y> A r r 0 <large> 1 <end_x>,<end_y> Z`
  with `large = 1 if (θ_end - θ_start) > π else 0`.
- Fill: theme tone per slice (cycles through palette).
- Center + radius: `cx = plot.center.x`, `cy = plot.center.y`, `r = min(plot.width, plot.height) / 2 * 0.9`.

## Source order

1. `<g class="slices">` — `<path>` per point in palette index order.
2. `<g class="labels">` — value labels just outside the slice arc (when `chart.data_labels`).
3. `<g class="legend">` — top-right.

(No gridlines or axes group for pie/doughnut/stat.)

## Reuses

- `format::coord` for path coordinates.
- New: `render::primitives::pie::arc_path` — produces the `M L A Z` path string for one slice given `(cx, cy, r, θ_start, θ_end)`.

## Acceptance pointer

`B-005 §1.1` row `pie`: 1 series, no axes. Determinism: the slice order is
the source order of `series.points`; sorted-by-value variants are out of
scope until the spec asks for them.

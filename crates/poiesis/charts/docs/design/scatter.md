# scatter

Scatter plot. 1+ series. Both axes linear.

## Geometry

- x-scale: linear, `nice_domain(point.x.scalar())`.
- y-scale: linear, `nice_domain(point.y.value)`, inverted range.
- per-point marker: `<circle cx="<scale_x>" cy="<scale_y>" r="<r>" fill="<tone>"/>`.
- size encoding (optional): `r` scaled from a per-point `size` channel against a sqrt-scale; out of scope for v1 unless an offsite spec requires it.

## Source order

1. `<g class="gridlines">` — both axes (horizontal + vertical).
2. `<g class="axes">` — x-tick + y-tick value labels.
3. `<g class="markers">` — `<circle>` per (series, point), series-major source order.
4. `<g class="labels">` — point labels when `chart.data_labels` (rare for scatter).
5. `<g class="legend">` — top-right when multi-series.

## Reuses

- `scale::nice` for both axes.
- `format::coord` for marker positions.
- New: `render::primitives::scatter::emit_markers` (similar to `line`'s marker pass but without a polyline).

## Acceptance pointer

`B-005 §1.1` row `scatter`: 1+ series; both axes linear. `Point::x` is a
`Option<CiteOrScalar>`; the scatter arm requires `Some(_)` and surfaces a
typed error otherwise.

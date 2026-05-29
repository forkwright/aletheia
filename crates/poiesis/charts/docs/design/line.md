# line

Line chart. 1+ series. x axis is category or linear; y axis is linear.

## Geometry

- x-scale: category (band centers) or linear (`scale.map(point.x.scalar())`).
- y-scale: linear, `nice_domain(values)`, inverted range.
- per-series polyline: `points = scale.map((x, y)) for each point` joined by spaces.
- markers: optional `<circle cx cy r="<series.style.marker_radius>"/>` per point.
- multi-series: each series gets its own polyline + marker `<g>`, fills/strokes from theme palette.

## Source order

1. `<g class="gridlines">` — horizontal gridlines at y-tick positions.
2. `<g class="axes">` — y-tick value labels, x-tick category / value labels.
3. `<g class="lines">` — `<polyline>` per series in palette index order.
4. `<g class="markers">` — `<circle>` per (series, point), series-major source order.
5. `<g class="labels">` — value labels above each point when `chart.data_labels`.
6. `<g class="legend">` — top-right when multi-series.

## Reuses

- The combo arm's `emit_line` is the direct template — pull it into
  `render::primitives::line::emit_polyline` when the line arm lands.
- Marker radius and stroke-width are theme-tone-derived; default `r=9` `stroke-width=2`.

## Acceptance pointer

`B-005 §1.1` row `line`. The combo arm already implements polyline + circle
markers on its right axis; the line arm is `combo` with the column series
dropped and a single-y axis.

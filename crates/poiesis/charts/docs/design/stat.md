# stat

Single big-number component. Exactly 1 series. No axes.

The "stat" component is what slides reach for when the answer is just one
number plus a label — e.g. "ARR: $32M", "Headcount: 175". A tiny inline
sparkline is the only optional accessory.

## Geometry

- Layout: the plot box becomes a two-row layout. Top row (≈70 %): the big
  number, centered, `font-family = theme.font_sans`, size derived from
  `plot.height()`. Bottom row (≈30 %): the label, smaller, secondary tone.
- Optional sparkline: if `chart.series[0].points.len() > 1`, emit a
  `<polyline>` along the bottom edge of the top row — the same `line` arm
  primitive at a fixed `30 px` height.

## Source order

1. `<g class="value">` — the big number `<text>`.
2. `<g class="label">` — the smaller `<text>` (series name).
3. `<g class="sparkline">` — `<polyline>` when there's more than one point.

## Reuses

- `format::format_number` for the big number (`NumFormat::FromUnit` for the
  point's `Unit`).
- The `line` arm's polyline emitter for the sparkline.

## Acceptance pointer

`B-005 §1.1` row `stat`: 1 series. The single point's `value` drives the
big number; multi-point series light up the sparkline. The component is
schema-light by design — agents reach for it when nothing more than one
number needs to land.

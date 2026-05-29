# area

Filled area. 1+ series. Axes as `line`.

## Geometry

Identical to `line`, plus a closed `<path>` below the polyline:

- area path: `M x0,baseline L x0,y0 L x1,y1 … L xn,yn L xn,baseline Z`.
- fill: theme tone (often at reduced opacity, e.g. `fill-opacity="0.25"`).
- stroke line on top: same `<polyline>` as `line`.
- stacked variant: bottom of each band = top of previous band's band-sum;
  out of scope for v1 (Vega-Lite handles stacked area).

## Source order

1. `<g class="gridlines">`
2. `<g class="axes">`
3. `<g class="areas">` — `<path d="…" fill="<tone>" fill-opacity="0.25"/>` per series.
4. `<g class="lines">` — `<polyline>` per series (same as `line`).
5. `<g class="markers">`
6. `<g class="labels">`
7. `<g class="legend">`

## Reuses

- `line` arm primitives unchanged.
- New: `render::primitives::area::emit_path` for the closed fill.

## Acceptance pointer

`B-005 §1.1` row `area`. Geometry derives from `line` plus a closed fill;
the only new primitive is the path-builder for the area outline.

# doughnut

Doughnut chart. Exactly 1 series. No axes. Identical to `pie` with an
inner-radius cut.

## Geometry

Same angle accumulator as `pie`; each slice is two arcs (outer + inner) +
two straight segments forming an annular sector:

```text
M outer_start → A r_outer 0 large 1 outer_end
               → L inner_end → A r_inner 0 large 0 inner_start
               → Z
```

- `r_outer` as in `pie`; `r_inner = r_outer * inner_ratio`, default `inner_ratio = 0.5`.
- Optional center text (the spec's `stat` overlay) lives in `<g class="center">` when present.

## Source order

1. `<g class="slices">` — `<path>` per point in palette index order.
2. `<g class="center">` — center label when present.
3. `<g class="labels">`
4. `<g class="legend">`

## Reuses

- `render::primitives::pie::annular_path` extends `arc_path` to two radii.

## Acceptance pointer

`B-005 §1.1` row `doughnut`: 1 series, no axes. Inner-ratio is a theme
token (default 0.5); `Chart::axes` carries no meaningful state for this kind.

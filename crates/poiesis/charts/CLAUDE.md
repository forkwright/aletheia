# poiesis-charts

## At a glance

Typed chart model + deterministic SVG emitter. Implements B-005 (poiesis chart subsystem). Renders the offsite slide-3 combo chart at or above the hand-authored quality bar.

## Depth

`lib.rs` re-exports the model + the `render_chart` entry point. `ChartKind` decides the render path statically: `bar`, `column`, `line`, `area`, `combo`, `scatter`, `pie`, `doughnut`, `stat` go through the pure-Rust emitter; `heatmap`, `boxplot`, `sankey`, `candlestick`, and any `log`/`time` axis scale route to Vega-Lite behind the `charts-vega` feature. A spec that needs Vega while the feature is off fails at `Chart::validate` with `Error::VegaRequired` — never a silent degrade.

## Read first

1. `src/lib.rs` — module map + determinism contract.
2. `src/model.rs` — `Chart`, `Series`, `Axes`, `FactCite`, `ChartKind::render_path`.
3. `src/render.rs` — `render_chart` entry point + per-kind dispatch.
4. `src/render/kinds/combo.rs` — the only fully implemented arm today; the B-005 acceptance gate.
5. `src/scale.rs` — linear `Scale` + `nice()` + `ticks()` (consumed by every arm).
6. `src/format.rs` — fixed-precision number formatting (the only path from `f64` to `<text>`).

## Determinism

Three rules, all enforced in code:

| Source of nondeterminism | Where it's pinned |
|---|---|
| Float → text | `format::format_number` for data, `format::coord` for coordinates. No `format!("{}", f64)` elsewhere. |
| Element order | Per-kind arms write groups in a fixed source order (`gridlines → axes → bars → line → labels → x-labels`). No map iteration into output. |
| IDs | Content-derived or index-based. No UUIDs, no `rand`. |

The combo arm has a re-emit-must-be-byte-identical test (`output_is_deterministic_across_two_renders`). Per-kind golden snapshots via `insta` land with each follow-up arm.

## Per-kind emitter status

| Kind | Status |
|---|---|
| combo | **Implemented** — covers the B-005 acceptance gate |
| bar | Stub (`Error::EmitterStub`) — design doc in PR body |
| column | Stub — column arm shares ~70 % with combo's column primitives; extract during follow-up |
| line | Stub — line arm shares with combo's line primitives |
| area | Stub — line + closed polygon |
| scatter | Stub — Cartesian-x scale + circles only |
| pie | Stub — angle accumulator + arc emitter |
| doughnut | Stub — pie with inner-radius cut |
| stat | Stub — single big number + sparkline |

Follow-up arms reuse `Scale` + `format` + `Canvas`; only the per-arm geometry differs.

## Theme seam

`ResolvedTheme` lives in `src/theme.rs` because `poiesis-theme` (B-002) is not yet on `main`. When B-002 lands, this crate switches to `poiesis_theme::summus().resolve()` and deletes `ResolvedTheme::summus_stub`. The stub mirrors the offsite navy + teal pair (`#232E54`, `#318891`) so B-005 acceptance test 2 ("colors come only from `theme: summus`") can be exercised today.

## Patterns

- **Parse-don't-validate**: every Chart field is a newtype or closed enum. JSON ingest routes through the same constructors via `serde`.
- **No naked numbers**: `Point::y` is `FactCite`, not `f64`. Raw numbers fail parse.
- **Two color modes, identical geometry**: `ColorMode::Themed` emits `var(--tone-*)` for HTML deck; `ColorMode::Resolved` emits hex for PPTX bake / document figures.
- **Static render-path rule**: `ChartKind::render_path()` decides Rust-vs-Vega per kind; axis scale adds the Vega override at `Chart::validate`.

## Dependencies

Uses: `poiesis-core`, `serde`, `serde_json`, `snafu`, `tracing`. Dev: `insta` for golden snapshots.

Used by: nothing yet (organon adds the wiring when B-003/B-004 land).

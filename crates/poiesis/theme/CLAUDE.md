<!--
scope: poiesis-theme — theme registry, token model, and brand-asset sinks
defers_to: crates/poiesis/CLAUDE.md
tightens: this crate's conventions narrow the parent only — sink ownership, byte-stable emission, no hex/font literals outside `themes/<id>.toml`
-->

# poiesis-theme

## At a glance

Theme registry, token model, and brand-asset sinks for poiesis. One source of
brand truth (a `themes/<name>.toml` file) → three sinks (CSS custom
properties, OOXML `theme1.xml`, Pandoc doc-vars). Three `THEME/*` lint rules
mechanically enforce that specs reference tokens, never raw hex or typeface
literals.

Source: planning entry `planning/poiesis-evolution/B-002` (forkwright/kanon#985).
Locks: spec 03 of forkwright/kanon#978 (apodeixis Phase-00 bootstrap).

## Read first

1. `src/lib.rs` — module map.
2. `src/id.rs` — `ThemeId` newtype (the parse-don't-validate boundary).
3. `src/tokens.rs` — TOML-shape `Theme`, `HexColor`, color/type/space/grid/table/chart structs.
4. `src/resolved.rs` — `ResolvedTheme` (tone refs dereferenced to role hex values).
5. `src/registry.rs` — `Registry` + `themes/` directory discovery.
6. `src/sinks/css.rs` — primary sink, fully implemented, byte-stable.
7. `src/sinks/ooxml.rs` — `theme1.xml` `clrScheme` + `fontScheme` emitter.
8. `src/sinks/docvars.rs` — flat JSON + YAML doc-vars map.
9. `src/lint.rs` — `THEME/raw-color-literal`, `THEME/raw-font-literal`, `THEME/unknown-token`.
10. `themes/summus.toml` — the seed theme.

## Patterns

- **One source → three sinks.** Each sink owns its serialization. Cross-sink
  emission shares nothing beyond `ResolvedTheme`. Adding a sink is a new module
  in `src/sinks/`.
- **TOML order preserved.** `IndexMap` everywhere a brand-author cares about
  emission order (CSS variable order, OOXML `accent1..6` slots, chart series).
  This makes the output byte-stable for fingerprinting.
- **Hex lives in one place.** `[color.role]` carries the only hex values.
  `[color.tone]` and `[color.surface]` reference roles by name. The
  `HexColor` newtype normalizes case so two themes that differ only by case
  emit byte-identical CSS.
- **Discovery is filesystem-bounded.** `Registry::load_dir` reads
  `themes/<id>.toml`; non-`*.toml` files and non-`ThemeId`-shape stems are
  skipped silently so the directory may carry READMEs and JSON schemas. The
  filename stem must match `[meta].id` (hard error on mismatch).
- **Lint rules ship next to tokens.** The three `THEME/*` rules live here, not
  in `poiesis-lint`, so a theme update can land tokens + rules in the same
  PR. The QA gate ([B-008]) imports them.

## Status (B-002 scope vs. what ships)

| B-002 surface             | This PR                                  | Follow-up         |
|---------------------------|------------------------------------------|-------------------|
| `ThemeId` newtype         | shipped (`src/id.rs`)                    | re-export from [B-001] core |
| Token model               | shipped (`src/tokens.rs`)                |                   |
| `ResolvedTheme`           | shipped (`src/resolved.rs`)              | bind to `Renderer` trait in [B-001] |
| Registry + discovery      | shipped (`src/registry.rs`)              | `theme list/show/validate/compile` CLI verbs from [B-010] |
| `summus` seed theme       | shipped (`themes/summus.toml`)           |                   |
| CSS sink                  | shipped (`src/sinks/css.rs`)             | regression-byte test against offsite CSS once [B-003] lands |
| OOXML `theme1.xml`        | shipped (`src/sinks/ooxml.rs`)           | full `assets/<name>-base.pptx` raw OOXML pack belongs to [B-004]; this crate emits the `theme1.xml` body |
| Pandoc doc-vars           | shipped (`src/sinks/docvars.rs`, JSON + YAML) | generate `reference.docx/odt/template.{typ,latex}` in [B-006] |
| `THEME/*` lint rules      | shipped (`src/lint.rs`) — rule shapes + scan/check APIs | register with the [B-008] basanos engine |

## Dependencies

Uses: indexmap, jiff, regex, serde, serde_json, snafu, toml.
Used by: (none yet). The intended consumers are [B-001] core, [B-003] HTML
deck, [B-004] PPTX, [B-006] doc, and [B-008] QA gate.

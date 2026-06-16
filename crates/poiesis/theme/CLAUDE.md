<!--
scope: poiesis-theme — theme registry, token model, and brand-asset sinks
defers_to: crates/poiesis/CLAUDE.md
tightens: this crate's conventions narrow the parent only — sink ownership, byte-stable emission, no hex/font literals outside `themes/<id>.toml`
-->

# poiesis-theme

## At a glance

Theme registry, token model, and brand-asset sinks for poiesis. One source of
brand truth (a `themes/<name>.toml` file) → seven sinks (CSS custom
properties, OOXML `theme1.xml`, Pandoc doc-vars, LaTeX, PPTX, reference.docx,
Typst). Three `THEME/*` lint rules
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

- **One source → seven sinks.** Each sink owns its serialization. Cross-sink
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
  PR. The QA gate imports them.

## Theme integration boundary

Theme integration is stable at the organon tool boundary. `organon`
resolves a `ResolvedTheme` through this crate and dispatches to the
sink-specific emitters in `src/sinks/` when a tool needs themed bytes.
The core `Renderer` trait in `poiesis-core` remains format-only and is
not the theme integration boundary.

## Status (B-002 scope vs. what ships)

| B-002 surface             | This PR                                  | Follow-up         |
|---------------------------|------------------------------------------|-------------------|
| `ThemeId` newtype         | shipped (`src/id.rs`)                    | re-export from core |
| Token model               | shipped (`src/tokens.rs`)                |                   |
| `ResolvedTheme`           | shipped (`src/resolved.rs`)              | consumed by organon at the tool boundary |
| Registry + discovery      | shipped (`src/registry.rs`)              | `theme list/show/validate/compile` CLI verbs |
| `summus` seed theme       | shipped (`themes/summus.toml`)           |                   |
| CSS sink                  | shipped (`src/sinks/css.rs`)             | regression-byte test against offsite CSS |
| OOXML `theme1.xml`        | shipped (`src/sinks/ooxml.rs`)           | full `assets/<name>-base.pptx` raw OOXML pack; this crate emits the `theme1.xml` body |
| Pandoc doc-vars           | shipped (`src/sinks/docvars.rs`, JSON + YAML) | generate `reference.docx/odt/template.{typ,latex}` |
| LaTeX prelude             | shipped (`src/sinks/latex.rs`)           | `\definecolor` / `\newcommand` assets for the doc backend |
| PPTX pack                 | shipped (`src/sinks/pptx.rs`)            | packed base PPTX template with the theme baked in |
| reference.docx            | shipped (`src/sinks/reference_docx.rs`)  | Pandoc reference-doc asset for DOCX theming |
| Typst prelude             | shipped (`src/sinks/typst.rs`)           | Typst prelude for the PDF / report template path |
| `THEME/*` lint rules      | shipped (`src/lint.rs`) — rule shapes + scan/check APIs | imported by the QA gate |

## Dependencies

Uses: indexmap, jiff, regex, serde, serde_json, snafu, toml.
Used by: organon (hard dep); poiesis-charts (optional theme-bridge feature).

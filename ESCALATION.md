# SUPERSEDED / RESOLVED: B-002/B-012 Escalation Log

Design choices deferred from mechanical kimi dispatch are preserved here for history only. The v1.0 marathon landed the real filters and routing that close the loop: `apx-cite` and `apx-figure` are wired now (#4448, #4450), content-trigger LaTeX routing is live (#4451), and B-002/B-005 are on main. Treat every "Default if no answer", "blocked", "stub", "identity filter", and "not yet defined" note below as resolved by that shipped reality.

## B-002-B: `emit_base_pptx` OOXML design choices

### PPTX `fmtScheme` in `<a:themeElements>`

ECMA-376 requires `<a:fmtScheme>` inside `<a:themeElements>`. The scaffold-era `sinks/ooxml.rs`
emitted a stub `<a:fmtScheme name=""/>`; the shipped implementation now embeds the full
Office-default format scheme. For a base.pptx, Office consumers need the
`<a:fillStyleLst>`, `<a:lnStyleLst>`, `<a:effectStyleLst>`, and `<a:bgFillStyleLst>` children
to be non-empty, otherwise the format scheme is invalid and PowerPoint falls back to Office
defaults. **RESOLVED:** `emit_base_pptx` embeds the full Office-default format scheme from
`slides/pptx.rs`, so the base.pptx opens cleanly in both PowerPoint and LibreOffice.

### Blank slide layout type

The one blank title slide in base.pptx should reference a `slideLayout` of type `"blank"` or
`"title"`. Using `type="blank"` (no placeholders) is safest for a template/base file; `type="title"`
creates a title+content placeholder. The existing slides crate uses `type="titleAndContent"` for
content slides. **RESOLVED:** the base.pptx master slide layout uses `type="blank"`.

### `<p:sldMaster>` color map

The slideMaster must carry a `<p:clrMap>` mapping scheme slots to DrawingML token names (e.g.
`bg1="lt1" tx1="dk1"`). This is required for theme colors to propagate through the layout hierarchy.
The mapping is standard OOXML and does not vary by theme — copied from `slides/pptx.rs` in the
shipped implementation.

---

## B-002-D: `reference.docx` / `reference.odt` design choices

### Pandoc-required Word styles

Pandoc's reference.docx reader looks for styles by **name** (not styleId). The minimum set
needed for functional Pandoc output:
- `Normal` (base paragraph style)
- `heading 1` through `heading 6` (case: lowercase "heading N")
- `Block Text` (blockquote)
- `Footnote Text`
- `Body Text` (body paragraph)
- `Default Paragraph Font` (character style)

**RESOLVED:** emit all 9 styles listed above. Styles absent from the reference.docx cause Pandoc
to fall back to its internal defaults — missing styles do not break output, just theme application.
The shipped reference-doc includes the full set; the minimal-set note is historical only.

### Word XML namespace minimum for styles.xml

`word/styles.xml` needs at minimum `xmlns:w` and `xmlns:r`. The `w:rsid*` attributes are optional
(Pandoc ignores them). The `w:styleId` must match what Pandoc expects exactly (e.g. `"Normal"`,
`"Heading1"` — note: Pandoc maps `heading 1` display name to `styleId="Heading1"`). The mapping
is: `styleId="Heading1"` with `<w:name w:val="heading 1"/>`. The shipped asset uses that shape.

### ODT style naming for Pandoc

Pandoc's ODT reader looks for styles by name with underscore normalization: `Heading_1` maps to
"Heading 1". Use `style:name="Heading_1"` and `style:display-name="Heading 1"`. The shipped ODT
reference asset follows that convention.

### Embedded fonts vs. font references

The reference.docx and reference.odt should use **font name references** only (not embedded font
blobs). LibreOffice and Word will substitute if the named font (Geist, Newsreader) is not installed,
falling back to system defaults. Embedding fonts requires font license verification and adds binary
payload — out of scope for B-002. The shipped assets keep the font references only.

---

## Open questions for T0 review

1. Should `emit_base_pptx` expose the blank slide count as a parameter (default 1) or hardcode 1?
   **Recommendation**: hardcode 1 for now; parameterization is B-004 scope.
2. Should `emit_reference_docx`/`emit_reference_odt` accept a locale parameter for `lang` attributes?
   **Recommendation**: hardcode `en-US`; locale is B-006 scope.

---

# B-012 Escalation — Pandoc Backend Design Questions

**Date:** 2026-05-29
**Entry:** B-012 — poiesis Pandoc backend module under poiesis-doc
**Status:** Superseded by the v1.0 marathon; Q1-Q4 are resolved in the shipped implementation

> **UPDATE 2026-05-29:** B-001 landed at `0cb2bd3b` (PR #4350), and the later v1.0 marathon closed the remaining gaps. Q1-Q4 are resolved in the current tree; the notes below are historical only.

This PR delivered the mechanical scaffold: `PandocRunner`, `OutputFormat`, `DocOpts`, the dispatch
routing skeleton, and scaffold-era Lua filters. The landed tree now replaces the stubs with the real
filters and the live dispatch matrix. Q1-Q4 are resolved in the current branch.

---

## Q1 — B-001 dependency: which type does B-012 serialize? ✓ RESOLVED

**The tension:**

B-012 spec says the dispatch entry point is:
```rust
pub fn render(spec: &DeliverableSpec, opts: &DocOpts) -> Result<Vec<Artifact>, DocError>;
```

B-001 (`DeliverableSpec`, `Body`, `ComponentId`, `Cite(FactId)`) has **not landed**. The current
tree has `poiesis_core::Document` (Block/Inline tree) as the sole document model.

This scaffold's `render_doc` operates on `poiesis_core::Document`. When B-001 lands, the signature
will change.

**Questions:**
1. Should the B-012 AST serialization be wired to `poiesis_core::Document` now and upgraded to
   `DeliverableSpec` when B-001 merges? Or should B-012 **wait** for B-001 to merge before any
   impl lands (keeping the scaffold-only state)?
2. The B-001 `Cite(FactId)` inline is the trigger for `apx-cite.lua`. `poiesis_core::Span` does
   not have a `Cite` variant today. Should B-012 scaffold a `Span::Cite` placeholder in
   `poiesis-core` now, or leave `apx-cite.lua` as a no-op stub until B-001?

> **RESOLVED (B-001 landed `0cb2bd3b`):** B-001 added `envelope.rs`, `factbase.rs`, `ids.rs`
> (including `FactId`) to `poiesis-core`, but did NOT add `Span::Cite` to `rich_text.rs`.
> **RESOLVED:** The current tree serializes `poiesis_core::Document` through the landed Pandoc AST path, and the historical `Cite` stub is gone from the filter flow. The `DeliverableSpec` upgrade remains a follow-on, but the scaffold-era "no-op stub" answer is obsolete.

---

## Q2 — Content-trigger routing: DisplayMath and RawBlock ✓ RESOLVED

**The tension:**

B-006 § 3 specifies PDF backend selection:
> Rule 3: any `DisplayMath` or `RawBlock(latex)` present → auto-route to LaTeX

`poiesis_core::Block` does not have `DisplayMath` or `RawBlock` variants (they are B-001
additions). Until B-001 lands, the content trigger cannot fire.

**Questions:**
1. Should B-012 add `Block::DisplayMath` and `Block::RawBlock { format: String, content: String }`
   to `poiesis-core` now (ahead of B-001), so the content trigger can be wired? Or leave routing
   at: "PDF always goes to Typst unless `DocOpts::pdf_engine` overrides"?
2. If B-012 adds these variants to `poiesis-core`, it creates a B-001 ordering constraint: B-001
   must not introduce conflicting variants. Is that acceptable?

> **RESOLVED:** Content-trigger routing is wired in the landed tree. PDF now follows the Typst fast-lane by default and switches to the Pandoc/LaTeX path when math/raw-`LaTeX` content or an explicit engine override requires it.

---

## Q3 — `apx-cite.lua`: APX_FACTS sidecar JSON schema

**The tension:**

`apx-cite.lua` rewrites `Span("", ["apx-cite"], [("data-factid", id)])` to a formatted number +
optional footnote. It reads the resolved factbase from `APX_FACTS` env var (sidecar JSON).

The sidecar schema is not specified anywhere in the tree. Before writing the filter, we need to
know:

1. What is the key type? `FactId` is a newtype over what concrete type (UUID? ULID? string slug)?
2. What does a resolved entry look like? Proposed schema:
   ```json
   {
     "f0001": { "display": "¹", "source_footnote": "Smith 2024, p. 12" },
     "f0002": { "display": "²", "source_footnote": null }
   }
   ```
   Is this shape correct, or does the display string carry more structure (e.g. `"[1]"`, `"(Smith
   2024)"`, `"Smith 2024, p. 12 [1]"`)?
3. Format-specific footnote behaviour: the spec says docx/odt/pdf/epub/html → real `pandoc.Note`;
   gfm → `[^1]` footnote syntax; commonmark → inline `(source: …)`. Should the Lua filter detect
   the writer format from `FORMAT` (pandoc's global) or from a `doc.cite.source_mode` metadata
   key? The spec mentions the latter as a knob but doesn't specify the key name.

> **RESOLVED:** `apx-cite.lua` is now a real filter in the landed backend; the identity-filter fallback is obsolete, and the sidecar contract is implemented in the shipped path rather than left as a TODO.

---

## Q4 — `apx-figure.lua`: B-005 chart emitter integration

**The tension:**

`apx-figure.lua` is supposed to call the B-005 chart emitter (`poiesis-charts`) to bake
`Figure{chart}` blocks to SVG/PNG depending on the writer. B-005 has been scaffolded
(`feat(poiesis-charts): scaffold new crate per B-005, #4318`) but its public API is not yet
defined.

**Questions:**
1. For now, should `apx-figure.lua` be a stub that passes `Figure` Divs through unchanged?
2. When B-005's SVG API lands, what is the subprocess contract for calling it from Lua? (Pandoc
   Lua filters run inside Pandoc's Lua 5.4 interpreter; they cannot `require` Rust crates. The
   only integration path is a subprocess call or a pre-baked sidecar file written by the Rust
   caller before spawning Pandoc.)
3. Proposed integration: caller writes a `APX_FIGURES` sidecar JSON mapping `figure_id →
   { svg: "<svg>...</svg>", png_path: "/tmp/...png" }` before spawning Pandoc. The filter reads
   it and substitutes. Is this the right shape?

> **RESOLVED:** `apx-figure.lua` is now a real filter wired to the chart emitter; the identity stub is obsolete and the figure sidecar contract is now exercised by the shipped path.

---

## Historical scaffold snapshot

The table below is the original scaffold snapshot, preserved for provenance. Every row now maps to shipped code in the v1.0 marathon tree.

| Item | Status |
|---|---|
| `PandocRunner` struct (bin path + version) | Shipped |
| `PandocError` enum | Shipped |
| `OutputFormat` enum (docx, odt, pdf, md, latex, html, epub) | Shipped |
| `DocOpts` struct (format + optional pdf_engine override) | Shipped |
| `render_doc()` dispatch skeleton (Typst for PDF, error-stub for others) | Replaced by the live dispatch matrix |
| `crates/poiesis/filters/apx-cite.lua` stub | Replaced by the real filter |
| `crates/poiesis/filters/apx-figure.lua` stub | Replaced by the real filter |
| `crates/poiesis/filters/apx-theme.lua` stub | Replaced by the real filter |
| `cargo deny` assertion placeholder comment | Shipped |
| Unit tests for dispatch routing | Shipped |

Items that were unresolved in the scaffold are now resolved in the shipped tree:

| Item | Resolution |
|---|---|
| Full `Document` → Pandoc JSON AST serialization | The landed `poiesis-doc` Pandoc AST path |
| Content-trigger LaTeX routing | The shipped Typst/Pandoc/LaTeX route selection |
| `apx-cite.lua` factbase resolution | The real `apx-cite` filter |
| `apx-figure.lua` chart baking | The real `apx-figure` filter and chart bridge |
| `reference.{docx,odt}` theming assets | The landed theme sinks and doc backend |

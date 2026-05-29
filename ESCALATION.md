# B-002 poiesis-theme Escalation Log

Design choices deferred from mechanical kimi dispatch — review before merging.

## B-002-B: `emit_base_pptx` OOXML design choices

### PPTX `fmtScheme` in `<a:themeElements>`

ECMA-376 requires `<a:fmtScheme>` inside `<a:themeElements>`. The existing `sinks/ooxml.rs`
emits a stub `<a:fmtScheme name=""/>`. For a base.pptx, Office consumers need the
`<a:fillStyleLst>`, `<a:lnStyleLst>`, `<a:effectStyleLst>`, and `<a:bgFillStyleLst>` children
to be non-empty, otherwise the format scheme is invalid and PowerPoint falls back to Office
defaults. **Decision needed**: should `emit_base_pptx` embed the full Office-default format scheme
(copy from slides/pptx.rs `THEME` static) or keep the stub? Recommendation: embed the Office-
default format scheme so the base.pptx opens cleanly in both PowerPoint and LibreOffice.

### Blank slide layout type

The one blank title slide in base.pptx should reference a `slideLayout` of type `"blank"` or
`"title"`. Using `type="blank"` (no placeholders) is safest for a template/base file; `type="title"`
creates a title+content placeholder. The existing slides crate uses `type="titleAndContent"` for
content slides. **Decision**: use `type="blank"` for the base.pptx master slide layout.

### `<p:sldMaster>` color map

The slideMaster must carry a `<p:clrMap>` mapping scheme slots to DrawingML token names (e.g.
`bg1="lt1" tx1="dk1"`). This is required for theme colors to propagate through the layout hierarchy.
The mapping is standard OOXML and does not vary by theme — copy from slides/pptx.rs.

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

**Decision**: emit all 9 styles listed above. Styles absent from the reference.docx cause Pandoc
to fall back to its internal defaults — missing styles do not break output, just theme application.
Emit a minimal set (Normal + Heading 1–3) for the initial implementation; the remaining styles can
be added in a follow-up without breaking existing consumers.

### Word XML namespace minimum for styles.xml

`word/styles.xml` needs at minimum `xmlns:w` and `xmlns:r`. The `w:rsid*` attributes are optional
(Pandoc ignores them). The `w:styleId` must match what Pandoc expects exactly (e.g. `"Normal"`,
`"Heading1"` — note: Pandoc maps `heading 1` display name to `styleId="Heading1"`). The mapping
is: `styleId="Heading1"` with `<w:name w:val="heading 1"/>`.

### ODT style naming for Pandoc

Pandoc's ODT reader looks for styles by name with underscore normalization: `Heading_1` maps to
"Heading 1". Use `style:name="Heading_1"` and `style:display-name="Heading 1"`.

### Embedded fonts vs. font references

The reference.docx and reference.odt should use **font name references** only (not embedded font
blobs). LibreOffice and Word will substitute if the named font (Geist, Newsreader) is not installed,
falling back to system defaults. Embedding fonts requires font license verification and adds binary
payload — out of scope for B-002.

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
**Status:** Scaffold committed; Q1 + Q2 resolved by B-001 landing; Q3 + Q4 still need operator answers

> **UPDATE 2026-05-29:** B-001 has landed at `0cb2bd3b` (PR #4350). Q1 and Q2 are resolved —
> defaults accepted. See notes under each question. Q3 and Q4 remain open.

This PR delivers the mechanical scaffold: `PandocRunner`, `OutputFormat`, `DocOpts`, the dispatch
routing skeleton, and stub Lua filters. Q3 and Q4 below need operator answers before the full
implementation (Lua filter logic) can be finalised. Q1 and Q2 are now closed.

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
> **Default accepted:** B-012 serializes `poiesis_core::Document`; `Cite` path is a no-op stub;
> upgrade to `DeliverableSpec` is a tracked follow-on. B-012 does NOT add types to `poiesis-core`.

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

> **RESOLVED (B-001 landed `0cb2bd3b`):** B-001 did NOT add `Block::DisplayMath` or
> `Block::RawBlock` to `block.rs` (verified). **Default accepted:** Content-trigger routing stays
> stubbed; PDF always routes to Typst; `DocOpts::pdf_engine(PdfEngine::Latex)` is the only LaTeX
> override path. Full content-trigger wires in as a follow-on when Block variants land.

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

**Default if no answer:** `apx-cite.lua` is a stub that passes `Span("apx-cite")` through
unchanged (identity filter). The sidecar contract is declared as a TODO comment in the stub. Full
impl unblocked once Q1 (B-001 Cite type) and Q3 schema are answered.

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

**Default if no answer:** `apx-figure.lua` is a stub (identity). The sidecar contract is
documented as a TODO comment.

---

## What this PR ships without escalation answers

The scaffold below is correct and final regardless of the above answers:

| Item | Status |
|---|---|
| `PandocRunner` struct (bin path + version) | Shipped |
| `PandocError` enum | Shipped |
| `OutputFormat` enum (docx, odt, pdf, md, latex, html, epub) | Shipped |
| `DocOpts` struct (format + optional pdf_engine override) | Shipped |
| `render_doc()` dispatch skeleton (Typst for PDF, error-stub for others) | Shipped |
| `crates/poiesis/filters/apx-cite.lua` stub | Shipped |
| `crates/poiesis/filters/apx-figure.lua` stub | Shipped |
| `crates/poiesis/filters/apx-theme.lua` stub | Shipped |
| `cargo deny` assertion placeholder comment | Shipped |
| Unit tests for dispatch routing | Shipped |

Items blocked on answers above (not in this PR):

| Item | Blocked on |
|---|---|
| Full `Document` → Pandoc JSON AST serialization | Q1 (B-001 types) |
| Content-trigger LaTeX routing | Q2 (B-001 Block variants) |
| `apx-cite.lua` factbase resolution | Q1 + Q3 |
| `apx-figure.lua` chart baking | Q4 (B-005 API) |
| `reference.{docx,odt}` theming assets | B-002 (not yet landed) |

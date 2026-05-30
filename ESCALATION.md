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

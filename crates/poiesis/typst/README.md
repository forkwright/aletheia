# poiesis-typst

Typst-based PDF rendering for the poiesis report tooling arc.

**Typst is the primary rendering backend** for poiesis. It is more expressive
than direct PDF generation: templates, math, citations, breakable blocks,
cross-references, and page-aware layout all come for free, and compiler
diagnostics carry source locations. Other backends (ODT, XLSX, ODS, PPTX) exist
for format-specific output; when rendering a prose-oriented report to PDF,
prefer this crate.

## Public API

```rust
use poiesis_typst::{render_typst, render_template, templates, PoiesisError};
use serde_json::json;

// 1. Inline source with injected data
let pdf: Vec<u8> = render_typst(
    r#"
    #let d = json("data.json")
    = #d.title

    Hello, #d.author.
    "#,
    &json!({ "title": "Report", "author": "alice" }),
)?;

// 2. Built-in template by slug
let pdf = render_template(templates::DEFAULT, &json!({
    "title": "Quarterly Review",
    "body": ["First paragraph.", "Second paragraph."]
}))?;
```

On failure, [`PoiesisError`] carries a formatted diagnostic string including
source file, line, and column.

## Data injection

The JSON value passed to [`render_typst`] is exposed to the Typst document as a
virtual file at path `data.json`. Templates load it with Typst's built-in
`json()` function:

```typst
#let data = json("data.json")
```

No bespoke substitution layer - the injection piggybacks on Typst's own file
resolution, so templates written against it also work when edited interactively
against a real `data.json` on disk.

## Template slug convention

Built-in templates are identified by a short kebab-case slug. Each slug maps to
a `.typ` source embedded at compile time from `templates/<slug>.typ`.

| Slug      | File                        | Purpose                                        |
|-----------|-----------------------------|------------------------------------------------|
| `default` | `templates/default.typ`     | Minimal one-page report: title, body, optional table |

To add a template:

1. Drop `templates/<slug>.typ` alongside `default.typ`.
2. Register in `src/templates.rs` (`SLUGS` array + `lookup` match arm).
3. Document the expected data shape in this README.

Keep built-in templates generic. Client-specific branding belongs in the
consumer's own `.typ` source passed to [`render_typst`] directly.

## Why the library API (not the CLI)

This crate embeds the Typst compiler as a library via the `typst` + `typst-pdf`
crates, not by shelling out to the `typst` binary. Rationale:

- **Offline and reproducible.** No dependency on `typst` being on `PATH` or a
  specific version installed; the Cargo lockfile pins the exact compiler.
- **Structured diagnostics.** The library returns typed `SourceDiagnostic`
  values with spans; formatting happens in [`world::format_diagnostics`] to
  produce `file:line:col` error strings. A CLI shim would force us to parse
  stderr.
- **No temp files.** Source lives in memory; the virtual filesystem for data
  injection is constructed per render. Parallel renders do not race on shared
  scratch paths.

## Attribution

The compiler `World` implementation is adapted from a prior private project,
used with permission per issue #3450. Changes from the original:

- in-memory source instead of disk-backed root;
- synthesized `data.json` virtual file for JSON injection;
- `parking_lot::Mutex` instead of `std::sync::Mutex` (workspace lint policy);
- font discovery unchanged.

## Running tests

```bash
cargo test -p poiesis-typst
```

14 unit tests cover: minimal rendering, template round-trip, data injection,
malformed-source diagnostics, unknown-slug error, template fallback paths.

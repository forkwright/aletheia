# poiesis-lint

**Purpose:** Prose quality linting for reports. Checks banned words, citation coverage, structural patterns, required sections, and header length. Ported from `ergon_tools` sdr lint.

## Key types

| Type | Purpose |
|------|---------|
| `LintError` | Error enum for the lint pipeline |
| `banned_words::*` | Banned-word detection |
| `citations::*` | Citation coverage analysis |
| `structure::*` | Required-section and header-length checks |

## Public API surface

- `poiesis_lint::error` - `LintError` and result types
- `poiesis_lint` - Serializable report types (`Deserialize`/`Serialize`)

## When to look here

- When adjusting prose-quality rules for report generation
- When extending the citation coverage heuristic
- When adding a new required section to report templates

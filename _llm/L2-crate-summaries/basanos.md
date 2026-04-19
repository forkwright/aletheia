# basanos

**Purpose:** Planning and standards linter for kanon projects. Validates that planning documents and generated outputs conform to the kanon standards (STANDARDS.md, RUST.md, WRITING.md, etc.).

## Key types

| Type | Purpose |
|------|---------|
| `LintError` | Error enum for the lint pipeline (snafu) |
| `rules::*` | Rule implementations covering banned words, citations, structure |

## Public API surface

- `basanos::error` - `LintError` and result types
- `basanos::rules` - Rule registrations and runners

## When to look here

- When adding or modifying kanon standards rules
- When integrating `kanon lint` checks with a new file type
- When debugging lint false positives in planning documents

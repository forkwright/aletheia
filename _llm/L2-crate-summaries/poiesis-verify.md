# poiesis-verify

**Purpose:** Claim verification for reports. Validates numeric claims in a `VerifyManifest` against `Derived` (arithmetic formula) and `Ref` (cross-claim reference) sources. SQL sources are stored for auditability but not executed by this crate. Ported from `ergon_tools` sdr verify.

## Key types

| Type | Purpose |
|------|---------|
| `VerifyManifest` | Top-level manifest: claims and their sources |
| `Claim` | A single numeric claim to verify |
| `Source` | Derived (arithmetic), Ref (claim id), or SQL (stored only) |
| `Arithmetic` | Recursive-descent arithmetic formula evaluator |

## Public API surface

- `poiesis_verify::arithmetic` - Formula evaluator
- `poiesis_verify::error` - Error types
- `poiesis_verify` - `VerifyManifest`, `Claim`, `Source`

## When to look here

- When adding a new source kind (beyond Derived, Ref, SQL)
- When extending the arithmetic grammar
- When debugging claim-verification failures in generated reports

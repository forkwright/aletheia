# Design Token Standards

> Generated from `crates/basanos/src/registry/`. Do not edit generated views by hand.

## Contents

- [DESIGN/token-value-not-hardcoded](#designtoken-value-not-hardcoded)

## `DESIGN/token-value-not-hardcoded` {#designtoken-value-not-hardcoded}

- Severity: `warning`
- Scope: `universal`

Visual properties (colors, spacing, typography, border radii) must reference fleet design tokens rather than raw CSS hex codes or magic numbers. Hardcoded properties fragment the visual language: a single brand color change requires a search-and-replace across every surface rather than a token revision.

### Examples

**Good:** Reference the fleet token for danger state instead of a raw color.

```text
/* CSS */
color: var(--aima);
/* or Rust */
let danger = tokens.aima;
```

**Bad:** Hardcode a hex color that duplicates a fleet design token.

```text
/* CSS */
color: #ff3b30; /* danger red — duplicates --aima token */
```


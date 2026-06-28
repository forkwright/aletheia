# Fleet label taxonomy

Canonical issue-label set for all forkwright fleet repositories.

> **Source of truth**: `crates/basanos/src/labels.rs` — `CANONICAL_LABELS`.
> Run `kanon derive --apply --only label-taxonomy` to regenerate this file.
> Apply to GitHub repos via `mcp__kanon__label_sync` or its dry-run preview.

Four axes compose orthogonally to form a vocabulary that fleet-wide tooling
(audit pipeline, dispatch, cross-repo filing) can rely on.  Do **not** add
labels outside this set to fleet repos; extend the taxonomy in `labels.rs`
and re-derive.

## Type axis

Apply at most one type label per issue.

| Label | Color | Description |
|-------|-------|-------------|
| `bug` | `#d73a4a` | Unintended behavior or production defect |
| `documentation` | `#0075ca` | Documentation-only change or gap |
| `enhancement` | `#a2eeef` | New capability or improvement to an existing feature |
| `standards` | `#c5def5` | kanon-standards policy or basanos rule change |
| `tooling` | `#e4e669` | Build system, CI pipeline, or developer-tooling concern |

## Severity axis

Replaces the aletheia-era bare `critical` label with a five-tier scheme.
Apply exactly one severity label per issue; omit when severity is not yet
triaged.

| Label | Color | Description |
|-------|-------|-------------|
| `severity:critical` | `#b60205` | Data loss, security breach, or production-down incident |
| `severity:high` | `#e11d48` | Significant breakage with no available workaround |
| `severity:info` | `#bef264` | Informational; no immediate action required |
| `severity:low` | `#fbbf24` | Minor quality or UX degradation |
| `severity:medium` | `#f97316` | Notable impact; workaround exists |

## Area axis

Replaces aletheia-era bare crate names (`nous`, `pylon`, …) with the
`crate:*` prefix scheme already used by kanon.  Apply all area labels
that apply when an issue spans multiple crates.

| Label | Color | Description |
|-------|-------|-------------|
| `crate:angelos` | `#e4e669` | Scope: angelos MCP server library |
| `crate:archeion` | `#e4e669` | Scope: archeion forge storage substrate |
| `crate:basanos` | `#e4e669` | Scope: basanos lint engine and standards library |
| `crate:kanon` | `#e4e669` | Scope: kanon shared core and HTTP client |
| `crate:pragma` | `#e4e669` | Scope: pragma CLI binary |
| `crate:stoa` | `#e4e669` | Scope: stoa web UI and HTTP API server |

## Lifecycle axis

Apply all lifecycle labels that apply; axes are orthogonal.

| Label | Color | Description |
|-------|-------|-------------|
| `ci` | `#1d76db` | CI infrastructure or pipeline concern |
| `dependencies` | `#0075ca` | External dependency update or security patch |
| `qa-audit` | `#fbca04` | Flagged by the automated kanon audit pipeline |
| `security` | `#d93f0b` | Security advisory or hardening requirement |

## Lifecycle naming scheme: `gated-on-vX.Y.Z`

The `gated-on-vX.Y.Z` family is a **naming convention**, not a fixed label.
Create it on demand when an issue is blocked until a specific release
(e.g., `gated-on-v1.0.0`).  These labels are **not** part of `CANONICAL_LABELS`
and are **not** synced by `kanon derive`.

## Reconciliation notes

This taxonomy reconciles the divergent schemes found in the fleet as of 2026:

- **aletheia** scheme: `bug`, `enhancement`, `critical`, `qa-audit`, `ci`,
  `gated-on-v1.0.0`, `dependencies`, plus bare crate names (`nous`, `pylon`, …).
- **kanon** scheme: `severity:{critical,high,medium,low,info}`,
  `crate:<name>`, `tooling`, `standards`, `enhancement`, `documentation`.

Decisions:

- Adopt kanon's `severity:*` five-tier scheme; aletheia's bare `critical`
  maps to `severity:critical`.
- Adopt kanon's `crate:*` prefix scheme; aletheia's bare crate names
  are superseded by `crate:<name>` labels.
- Retain `qa-audit`, `ci`, `dependencies` from aletheia (no conflict).
- `gated-on-vX.Y.Z` remains a naming scheme and is excluded from the canonical set.

## Applying the taxonomy

```sh
# Verify this doc is in sync with labels.rs
kanon derive --check --only label-taxonomy

# Regenerate this doc from labels.rs
kanon derive --apply --only label-taxonomy
```

To sync labels to a GitHub repo, use the `mcp__kanon__label_sync` MCP
tool or call the equivalent `gh label create --force` loop directly.

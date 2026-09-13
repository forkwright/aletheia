# proskenion

**Purpose:** Dioxus desktop shell for Aletheia (a full root-workspace member, but not a *default* one — needs `-p proskenion`/`--workspace` since it requires GTK3/webkit2gtk; see docs/DESKTOP.md).

## Key types

| Type | Purpose |
|------|---------|
| `See L3 API index` | Current public type or boundary; see L3/source for exact fields |

## Public API surface

- Desktop app API is checked through `crates/theatron/proskenion/Cargo.toml`, opted in via `-p proskenion` (bare, no `--manifest-path` needed — it resolves against the root workspace Cargo.lock like any other member).

## When to look here

- When work touches `crates/theatron/proskenion` or downstream imports from `proskenion`.
- For exact signatures, load `_llm/L3-api-index/proskenion.md` if present, then source.

## Recent changes

forkwright/aletheia#4726 moved `proskenion` from `[workspace].exclude` into `[workspace].members` (kept out of `default-members` only); it now inherits `[workspace.dependencies]`/`[workspace.package]` instead of hand-tracking its own pins.

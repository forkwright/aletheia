# mneme

**Purpose:** Thin facade re-exporting memory and session types from eidos, graphe, episteme, and krites into a single crate import surface.

## Key types

| Type | Purpose |
|------|---------|
| `SessionStore` | Re-exported from graphe: fjall-backed session store |
| `Fact` / `Entity` | Re-exported from eidos: knowledge graph types |
| `RecallEngine` | Re-exported from episteme: 6-factor recall scoring |
| `Db` | Re-exported from krites (behind `mneme-engine` feature): Datalog engine |
| `EmbeddingProvider` | Re-exported from episteme: vector embedding trait |

## Public API surface

- `mneme::*` — single import prefix for all memory-layer types; replaces four separate crate imports
- Feature gate `mneme-engine` enables krites `Db` and related Datalog types

## When to look here

- When importing any memory-layer type in a downstream crate (nous, pylon, aletheia, etc.)
- Do not add logic here; if `src/lib.rs` exceeds 500 lines, extract to a sub-crate

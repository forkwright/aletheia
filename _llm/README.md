# _llm/ - Multi-Resolution Codebase Representation

On-demand reference for AI agents. Load the level that matches your task's scope; read source only when you need implementation detail.

CLAUDE.md = instructions (always loaded, short). This directory = reference (on demand, structured).

## Why this exists

Agents loading cold into a 48-crate workspace burn tokens on full CLAUDE.md files when they only need a fraction of the information. This directory provides a layered representation: scan the index, drill into relevant crates, read full source only when needed.

## Level map

| Level | File(s) | Size | Contents |
|-------|---------|------|----------|
| L1 | `L1-workspace.md` | ~500 tokens | Crate list, layer grouping, dependency direction |
| L2 | `L2-crate-summaries/<crate>.md` | ~200 tokens each | Purpose, key types, public API surface, recent substrate notes |
| L3 | `L3-api-index/<crate>.md` | 100–22K tokens | All `pub` fn/struct/enum/trait signatures with doc comments, extracted by tree-sitter |
| L4 | `crates/<crate>/src/` | source size | Full source, read on demand |

L1 and L2 are maintained as generated reference material. L2 files stay tight by design; use L3 or source for exact signatures.

## Loading recipes

The `nous` bootstrap assembler uses `_llm/recipes.toml` to select exact `_llm/`
reference files. The `RecipeRegistry` parser is available for other tooling, but
bootstrap resolves recipes by the names selected by [`LlmRecipe`](../crates/nous/src/bootstrap/mod.rs):

| `LlmRecipe` | Recipe name | `_llm/` sections loaded by bootstrap |
|-------------|-------------|--------------------------------------|
| `ColdStart` | `cold_start` | `L1-workspace.md` + `L2-crate-summaries/*.md` |
| `InSession` | `in_session` | `L1-workspace.md` + `current_state.toml` |
| `Refactor` | `cross_crate_refactor` | `L1-workspace.md` + `L2-crate-summaries/*.md` + `L3-api-index/*.md` |
| `None` | none | none |

`_llm/manifest.toml` is **not** injected as a section; it supplies the L3 index
path and per-crate source hashes for the staleness guard (#5404). `CLAUDE.md`
is consumed by the agent client, not loaded by bootstrap. L4 source paths
declared in recipes are read on demand by tooling, not packed into the system
prompt.

If `_llm/recipes.toml` is missing or does not define the selected recipe, the
assembler falls back to sweeping `_llm/*.md|*.toml` at the root (excluding
`manifest.toml` and `recipes.toml`) and the manifest-declared L3 directory.

Other recipes such as `edit_crate`, `add_endpoint`, `add_tool`, and `fix_bug`
are available for agent tooling but are not wired into automatic bootstrap
selection because they require task-specific parameters (e.g. `{crate}`).

## Legacy reference files (pre-L1/L2)

These TOML files were the prior representation and remain useful until L1/L2 land:

| File | Contents |
|------|---------|
| `architecture.toml` | Crate tree, layers, dependency direction (equivalent to future L1) |
| `api.toml` | CLI subcommands and HTTP endpoints |
| `decisions.toml` | Technology decisions with rationale |
| `observability.toml` | Metrics, spans, log events by crate |
| `turn-pipeline.toml` | End-to-end message flow across crates |

## Format

TOML for structured data (token-efficient, machine-parseable). Markdown for L3 (fenced rust blocks for direct rendering). Canonical sources are the `docs/` markdown files and the source itself - these are compressed views, not replacements. When in doubt, read the linked doc.

## Regenerating L3

`_llm/L3-api-index/` and `_llm/manifest.toml` are **generated, not committed** (gitignored) — they regenerate on every source change, which made `manifest.toml` a rebase-conflict magnet. Materialize them on demand with:

```bash
uv run scripts/llm-extract-l3.py
```

The script reads `Cargo.toml` workspace members, parses each `.rs` file with tree-sitter-rust, and writes one markdown file per crate to `_llm/L3-api-index/`. It also writes `_llm/manifest.toml` with per-crate source hashes and token estimates. Hand-authored `[levels.L1]`, `[levels.L2]`, `[l1]`, and `[[l2]]` manifest blocks are preserved verbatim across regeneration.

The hand-authored tiers (`L1-workspace.md`, `L2-crate-summaries/`, the legacy `*.toml`, this README) **remain committed** — they have no generator. If you ever need hand-authored `[levels.L1]`/`[levels.L2]` manifest blocks, keep them in a committed sidecar, not the generated `manifest.toml`.

The extractor runs offline: tree-sitter and tree-sitter-rust are the only runtime deps and both are pure Python wheels. No network access is required.

Running the script twice on unchanged source produces identical L3 content. The manifest `generated_at` timestamp updates on every run by design.

## Testing the extractor

```bash
uv run scripts/test_llm_extract_l3.py
```

Scaffolds a synthetic fixture crate in a temp directory and asserts the extractor's behavior: bare `pub` items captured (fn, struct, enum, trait, type_item, const_item, static_item); `pub(crate)` and private items excluded; items inside `#[cfg(test)] mod tests { ... }` excluded; doc comments attached to their item; fn bodies stripped; determinism across repeated runs; source hash stability.

## manifest.toml

Records generation metadata: schema version, timestamp, generator script, and per-crate entries with source hash and token estimate. Source hash is SHA-256 of all `.rs` files in the crate, sorted by crate-relative POSIX path, with each path's UTF-8 bytes prepended to its file bytes - use it to detect staleness without re-parsing.

## Follow-up phases

- **Phase 3**: Bootstrap assembler integration complete - `cold_start`, `in_session`, and `cross_crate_refactor` recipes wired into `nous` bootstrap
- **Phase 4**: CI hook - post-merge regeneration when source changes

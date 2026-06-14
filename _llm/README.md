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

Task-specific resolution selection is defined in [`recipes.toml`](recipes.toml).
The `nous` crate provides `RecipeRegistry` for parsing and selecting recipes at
bootstrap time. See [`CLAUDE.md`](../CLAUDE.md) for the recipe-to-task mapping.

| Task | What to load |
|------|-------------|
| Cold orientation | `_llm/L1-workspace.md` + `_llm/manifest.toml` |
| Work on a known crate | L3 for that crate |
| Cross-crate refactor | L3 for each touched crate |
| Full workspace audit | All L3 files |
| Implementation detail | L4 (source) for the specific file |

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

Records generation metadata: schema version, timestamp, generator script, and per-crate entries with source hash and token estimate. Source hash is SHA-256 of all `.rs` files in the crate concatenated in sorted path order - use it to detect staleness without re-parsing.

## Follow-up phases

- **Phase 3**: Bootstrap assembler integration - task-hint-aware loading recipes wired into `nous` bootstrap
- **Phase 4**: CI hook - post-merge regeneration when source changes

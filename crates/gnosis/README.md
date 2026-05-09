# gnosis

Machine-derived code-graph index for symbol-level cross-crate queries over the
aletheia workspace.

## What it does

Answers questions that grep and `ARCHITECTURE.md` cannot:

| Query            | Question answered                                               |
|------------------|-----------------------------------------------------------------|
| `symbol_rdeps`   | Which symbols implement or re-export `Message`?                 |
| `impl_search`    | Which types implement `Stamped`?                                |
| `reexport_chain` | Which crates re-export `Message` via `pub use`?                 |
| `crate_deps`     | What workspace crates does `nous` directly depend on?           |
| `crate_rdeps`    | What workspace crates depend on `eidos`?                        |
| `symbols_in`     | List all symbols in `eidos` (optionally filtered by kind).     |

## What it doesn't do

**Replace `architecture_fact`.** That layer holds human-curated, `EpistemicTier::Verified`
claims (`"eidos has zero internal aletheia dependencies"`). gnosis is machine-derived
(`EpistemicTier::Inferred`). It reflects the indexed code, not what the
architecture mandates. They coexist:

```
architecture_fact (existing)          code_graph_query (this crate)
─────────────────────────────         ─────────────────────────────────
ops: get/put/list/search              ops: rdeps/impl/reexports/deps
tier: Verified (human-curated)        tier: Inferred (machine-derived)
storage: flat JSON per-fact           storage: fjall index
use: "what does the design say?"      use: "what does the code actually do?"
```

gnosis can *cross-check* facts: `crate_rdeps(eidos)` returning an internal
aletheia crate would indicate a violation of the `aletheia.eidos.dependency-direction`
architectural fact.

## Cache location

Default: `~/.cache/aletheia/gnosis.fjall`

Override with `GNOSIS_CACHE_PATH` environment variable.

**Delete the directory to force a full rebuild** (next rebuild will re-parse all files).

## Rebuild trigger

**Today (v1):** Manual. Call `CodeGraph::rebuild()` or use the MCP tool:

```json
{ "op": "rebuild", "workspace": "/path/to/aletheia" }
```

**Future:** kanon-forge-sync post-receive hook (filed as follow-up to #3357).

## Cache eviction

Delete the fjall directory:

```bash
rm -rf ~/.cache/aletheia/gnosis.fjall
```

The next rebuild will re-parse all workspace source files.

## Example queries (MCP tool)

```jsonc
// Which symbols implement or re-export Message?
{ "op": "symbol_rdeps", "symbol": "Message" }

// Which types implement Stamped?
{ "op": "impl_search", "trait_name": "Stamped" }

// Which crates pub-use Message?
{ "op": "reexport_chain", "symbol": "Message" }

// What does nous depend on in the workspace?
{ "op": "crate_deps", "crate_name": "nous" }

// What depends on eidos?
{ "op": "crate_rdeps", "crate_name": "eidos" }

// All fn symbols in organon:
{ "op": "symbols_in", "crate_name": "organon", "kind": "fn" }

// Trigger incremental rebuild:
{ "op": "rebuild" }
```

## Architecture

- **`CodeGraph`** - public API handle; wraps a `Mutex<schema::Store>`.
- **`crates/gnosis/src/index.rs`** - walks workspace via `cargo metadata`, parses each
  `*.rs` with `syn::visit`, and populates the fjall index. Incremental rebuilds use the SHA-256 digest stored in `file_hashes`.
- **`crates/gnosis/src/query.rs`** - query impls against the fjall keyspaces.
- **`crates/gnosis/src/schema.rs`** - keyspace and record definitions for `symbols`, `symbol_refs`, `crate_edges`,
  `file_hashes`, and `meta`.
- **`crates/organon/src/builtins/code_graph_query.rs`** - MCP tool executor.

## Limitations (v1)

- Macro-expanded code is not indexed (syn operates pre-expansion).
- Function call sites inside macro arguments are not captured.
- gnosis tracks only direct `pub use` re-exports. Transitive chains require multiple hops.
- No background daemon. The index rebuilds on demand.

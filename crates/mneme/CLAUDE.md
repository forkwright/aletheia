# mneme

Thin facade re-exporting from four decomposed sub-crates. 264 lines of glue code.

## Architecture

Mneme was decomposed into eidos, graphe, episteme, and krites. This crate re-exports their public APIs so downstream consumers (nous, pylon, melete) depend on `mneme` without knowing about the decomposition.

## Re-exports

| Source crate | Re-exported modules | Feature gate |
|--------------|---------------------|--------------|
| `eidos` | `id`, `knowledge` | always |
| `graphe` | `backup`, `error`, `export`, `import`, `migration`, `portability`, `recovery`, `retention`, `schema`, `store`, `types` | `sqlite` (default) for most |
| `episteme` | `conflict`, `consolidation`, `embedding`, `extract`, `hnsw_index`, `instinct`, `knowledge_portability`, `knowledge_store`, `query`, `recall`, `skill`, `skills`, `vocab` | `hnsw_rs` for hnsw_index, `mneme-engine` for query |
| `krites` | `engine` | `mneme-engine` |

## Feature flags

| Feature | Default | Purpose |
|---------|---------|---------|
| `sqlite` | yes | SQLite session store (graphe backend) |
| `graph-algo` | yes | Graph algorithms in episteme + krites |
| `mneme-engine` | no | Datalog engine (krites) + typed query builder |
| `storage-fjall` | no | Fjall LSM-tree backend (requires mneme-engine) |
| `embed-candle` | no | Local ML embeddings via candle |
| `hnsw_rs` | no | Alternative HNSW vector index backend |
| `test-support` | no | MockEmbeddingProvider and test helpers |

## Where to make changes

Mneme itself has no logic. All changes go to the sub-crates:

| Task | Sub-crate |
|------|-----------|
| Add session/message field | `graphe` (types, schema, migration, store) |
| Add knowledge type | `eidos` (knowledge module) |
| Add extraction/recall logic | `episteme` |
| Add Datalog query builder | `episteme` (query module, requires mneme-engine) |
| Modify Datalog engine | `krites` |
| Add embedding provider | `episteme` (embedding module) |

## Dependencies

Uses: eidos, graphe, episteme, krites
Used by: nous, pylon, melete, aletheia (binary)

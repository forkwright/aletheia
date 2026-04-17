# krites

**Purpose:** Embedded Datalog engine with HNSW vector search, full-text search, and graph algorithms. Powers the aletheia knowledge graph.

## Key types

| Type | Purpose |
|------|---------|
| `Db` | Public engine facade: `open_mem()`, `open_fjall()`, `run()`, `with_cache()` |
| `DataValue` | Core value type: Null, Bool, Num, Str, Bytes, List, Json, Vector, Validity |
| `NamedRows` | Query result: column headers + row data |
| `QueryCache` | LRU cache with whitespace-normalized keys and hit/miss counters |
| `FixedRule` | Trait for custom graph algorithms (PageRank, community detection) |

## Public API surface

- `krites::Db` — open/close engine, run Datalog queries, manage transactions
- `krites::data` — `DataValue`, `Vector` types for query inputs/outputs
- `krites::fixed_rule` — `FixedRule` trait for custom graph algorithm extensions

## When to look here

- When querying or extending the knowledge graph with Datalog
- When adding custom graph algorithms via the `FixedRule` trait

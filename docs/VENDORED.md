# Vendored Sources

Sources absorbed into the Aletheia workspace from external projects.

## mneme-engine (from CozoDB)

| Field | Value |
|-------|-------|
| Original project | CozoDB |
| Original crate | cozo-core |
| Version | 0.7.6 |
| Source | https://github.com/cozodb/cozo |
| License | MPL-2.0 |
| Copyright | Copyright 2022-2024 Ziyang Hu and CozoDB contributors |
| Absorbed to | `crates/mneme-engine/` |

### MPL-2.0 Compliance

Per MPL-2.0 Section 3.1, source files from CozoDB retain their original license. The MPL-2.0 license is compatible with Aletheia's AGPL-3.0-or-later license per MPL-2.0 Section 3.3 (Secondary License). Copyright headers in absorbed source files are preserved verbatim.

### Modifications from Original

- Storage backends removed: rocks.rs (legacy), sqlite.rs, sled.rs, tikv.rs
- Chinese tokenizer removed: fts/cangjie/, jieba-rs dependency
- FFI/binding code removed: DbInstance, all *_str methods
- HTTP fetch utility removed: fixed_rule/utilities/jlines.rs, minreq dependency
- CSV reader utility removed: fixed_rule/utilities/csv.rs, csv dependency
- Stopwords trimmed to English-only (from 21,885 lines to ~1,303 lines)
- lib.rs rewritten: new Db facade enum replacing DbInstance
- env_logger moved to dev-dependencies

## graph-builder (from neo4j-labs/graph)

| Field | Value |
|-------|-------|
| Original project | graph (neo4j-labs) |
| Original crate | graph_builder |
| Version | 0.4.1 |
| Source | https://github.com/neo4j-labs/graph |
| License | MIT |
| Copyright | Copyright (c) neo4j-labs contributors |
| Absorbed to | `crates/graph-builder/` |

### Modifications from Original

- compat.rs removed (polyfills replaced with stdlib equivalents)
- build.rs removed (feature probes for pre-1.80 Rust no longer needed)
- Unused input formats removed: dotgraph, gdl, graph500, binary
- adj_list.rs removed (only CSR graphs used)
- rayon pinned to =1.10.0 (1.11 breaks EdgeList::edges())

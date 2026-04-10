# Feature Matrix

This document documents all compile-time feature flags across the Aletheia workspace.

## Feature Matrix by Crate

| Crate | Feature | Default | Gates |
|-------|---------|---------|-------|
| aletheia | default | yes | `tui`, `recall`, `storage-fjall`, `embed-candle` |
| aletheia | tui | yes | Terminal dashboard (koilon) |
| aletheia | recall | yes | Knowledge pipeline: KnowledgeStore, vector search, extraction persistence |
| aletheia | storage-fjall | yes | fjall LSM-tree storage backend for mneme (pure Rust, LZ4) |
| aletheia | embed-candle | yes | Local ML embeddings via candle (BAAI/bge-small-en-v1.5) |
| aletheia | mcp | no | MCP server (Model Context Protocol) via diaporeia |
| aletheia | tls | no | TLS termination for pylon HTTP gateway |
| aletheia | migrate-qdrant | no | Qdrant migration support for importing memories |
| aletheia | keyring | no | System keyring integration via symbolon |
| aletheia | energeia | no | Dispatch orchestration (phronesis port) |
| aletheia | test-core | no | Test helpers |
| aletheia | test-full | no | Full test suite |
| aletheia-agora | test-core | no | Test helpers |
| aletheia-agora | test-full | no | Full test suite |
| aletheia-dianoia | test-core | no | Test helpers |
| aletheia-dianoia | test-full | no | Full test suite |
| aletheia-diaporeia | test-core | no | Test helpers |
| aletheia-diaporeia | test-full | no | Full test suite |
| aletheia-eidos | test-core | no | Test helpers |
| aletheia-eidos | test-full | no | Full test suite |
| aletheia-energeia | storage-fjall | no | fjall storage dependency |
| aletheia-energeia | test-core | no | Test helpers with storage-fjall |
| aletheia-energeia | test-full | no | Full test suite |
| aletheia-episteme | default | yes | `graph-algo` |
| aletheia-episteme | graph-algo | yes | Graph algorithm implementations |
| aletheia-episteme | test-support | no | Test helpers and mocks |
| aletheia-episteme | mneme-engine | no | Knowledge graph engine (requires graphe/sqlite) |
| aletheia-episteme | storage-fjall | no | fjall storage backend |
| aletheia-episteme | embed-candle | no | Candle embedding models |
| aletheia-episteme | hnsw_rs | no | HNSW approximate nearest neighbor |
| aletheia-episteme | test-core | no | Core tests (mneme-engine, hnsw_rs) |
| aletheia-episteme | test-full | no | Full tests (+ embed-candle) |
| aletheia-dokimion | test-core | no | Test helpers |
| aletheia-dokimion | test-full | no | Full test suite |
| aletheia-graphe | default | yes | `sqlite` |
| aletheia-graphe | sqlite | yes | SQLite session store (rusqlite, sha2) |
| aletheia-graphe | mneme-engine | no | Error variants for tokio::task::JoinError |
| aletheia-graphe | hnsw_rs | no | HNSW algorithm support |
| aletheia-graphe | test-core | no | Test helpers |
| aletheia-graphe | test-full | no | Full test suite |
| aletheia-hermeneus | local-llm | no | OpenAI-compatible local LLM provider (vLLM, etc.) |
| aletheia-hermeneus | test-utils | no | Shared mock LLM provider (deprecated, use test-support) |
| aletheia-hermeneus | test-support | no | Shared mock LLM provider for tests |
| aletheia-hermeneus | test-core | no | Test helpers |
| aletheia-hermeneus | test-full | no | Full test suite |
| aletheia-integration-tests | default | yes | `sqlite-tests`, `knowledge-store` |
| aletheia-integration-tests | sqlite-tests | yes | SQLite-backed tests |
| aletheia-integration-tests | engine-tests | no | Mneme engine tests |
| aletheia-integration-tests | knowledge-store | yes | Knowledge store tests |
| aletheia-integration-tests | test-core | no | Core tests (engine-tests) |
| aletheia-integration-tests | test-full | no | Full tests (test-core + more) |
| aletheia-koina | test-support | no | Test helpers, crypto provider |
| aletheia-koina | test-core | no | Test helpers |
| aletheia-koina | test-full | no | Full test suite |
| aletheia-krites | default | yes | `graph-algo` |
| aletheia-krites | graph-algo | yes | Graph algorithm implementations |
| aletheia-krites | storage-fjall | no | fjall storage backend |
| aletheia-krites | test-core | no | Core tests (storage-fjall) |
| aletheia-krites | test-full | no | Full test suite |
| aletheia-melete | test-core | no | Test helpers |
| aletheia-melete | test-full | no | Full test suite |
| aletheia-mneme | default | yes | `sqlite`, `graph-algo` |
| aletheia-mneme | sqlite | yes | SQLite storage via graphe |
| aletheia-mneme | graph-algo | yes | Graph algorithms via episteme |
| aletheia-mneme | test-support | no | MockEmbeddingProvider and test helpers |
| aletheia-mneme | embed-candle | no | Candle embedding models via episteme |
| aletheia-mneme | mneme-engine | no | Mneme engine (coexists with sqlite) |
| aletheia-mneme | storage-fjall | no | fjall storage via episteme |
| aletheia-mneme | hnsw_rs | no | HNSW via episteme |
| aletheia-mneme | test-core | no | Core tests (mneme-engine, hnsw_rs, storage-fjall) |
| aletheia-mneme | test-full | no | Full tests (+ embed-candle) |
| aletheia-nous | knowledge-store | no | KnowledgeStore for vector search |
| aletheia-nous | test-support | no | Mock search implementations |
| aletheia-nous | test-core | no | Core tests (knowledge-store) |
| aletheia-nous | test-full | no | Full test suite |
| aletheia-oikonomos | test-core | no | Test helpers |
| aletheia-oikonomos | test-full | no | Full test suite |
| aletheia-organon | computer-use | no | Screen capture, action dispatch, Landlock sandbox |
| aletheia-organon | energeia | no | Energeia capability tools (dromeus, dokimasia, etc.) |
| aletheia-organon | test-support | no | MockToolExecutor and test helpers |
| aletheia-organon | test-core | no | Test helpers |
| aletheia-organon | test-full | no | Full test suite |
| aletheia-pylon | default | yes | (empty) |
| aletheia-pylon | tls | no | TLS support via axum-server |
| aletheia-pylon | knowledge-store | no | Knowledge store endpoints |
| aletheia-pylon | test-core | no | Core tests (knowledge-store) |
| aletheia-pylon | test-full | no | Full test suite |
| aletheia-symbolon | keyring | no | System keyring integration |
| aletheia-symbolon | test-support | no | Test helpers |
| aletheia-symbolon | test-core | no | Test helpers |
| aletheia-symbolon | test-full | no | Full test suite |
| aletheia-taxis | test-core | no | Test helpers |
| aletheia-taxis | test-full | no | Full test suite |
| aletheia-thesauros | test-core | no | Test helpers |
| aletheia-thesauros | test-full | no | Full test suite |

## Build Profiles

### Dev Profile

Optimized for fast incremental builds during development:

```toml
[profile.dev]
opt-level = 1
codegen-units = 16
split-debuginfo = "unpacked"
```

| Setting | Value | Purpose |
|---------|-------|---------|
| `opt-level` | 1 | Minimal optimizations for faster compilation |
| `codegen-units` | 16 | Reduces linker pressure vs default 256 while keeping parallelism |
| `split-debuginfo` | "unpacked" | Separate .dwo files, ~30% faster link time on Linux |

Dependencies are built at `opt-level = 2` for reasonable runtime performance:

```toml
[profile.dev.package."*"]
opt-level = 2
```

Proc-macro crates use `opt-level = 1` for faster cold builds:
- `syn`
- `proc-macro2`
- `serde_derive`
- `tokio-macros`
- `snafu-derive`
- `tracing-attributes`

### Release Profile

Optimized for production binaries:

```toml
[profile.release]
lto = "thin"
codegen-units = 1
strip = "symbols"
```

| Setting | Value | Purpose |
|---------|-------|---------|
| `lto` | "thin" | Link-time optimization for better performance without excessive build time |
| `codegen-units` | 1 | Maximum optimization, single codegen unit |
| `strip` | "symbols" | Remove debug symbols for smaller binaries |

## Workspace-Level vs Crate-Level Features

### Workspace-Level Configuration

The workspace `Cargo.toml` defines:
- **Workspace members**: 27 crate paths (including nested crates like `theatron`)
- **Excluded crates**: `proskenion` (requires GTK3/webkit2gtk, not available in CI)
- **Shared dependencies**: Common crate versions via `[workspace.dependencies]`
- **Lint configuration**: Rust and clippy lints applied workspace-wide

### Crate-Level Features

Features are defined per-crate in their respective `Cargo.toml` files:

- **`aletheia`** (main binary): Primary feature aggregation point. Most users interact with features here.
- **Library crates** (`aletheia-*`): Export features for composition by dependent crates.
- **Test features** (`test-core`, `test-full`, `test-support`): Hierarchical test configuration across the workspace.

### Feature Propagation

Features flow downstream through dependencies:

```
aletheia (binary)
  └─ recalls ──► aletheia-mneme/mneme-engine ──► aletheia-episteme/mneme-engine
                                          ──► aletheia-graphe/mneme-engine
                                          ──► aletheia-krites
```

## Common Feature Patterns

### Test Tiers

All crates follow a consistent test tier pattern:

| Tier | Feature | Description |
|------|---------|-------------|
| Core | `test-core` | Storage and engine tests, fast execution |
| Full | `test-full` | Superset of test-core, includes ML embedding tests |
| Support | `test-support` | Mock implementations for dependent crates |

### Optional Dependencies

Features often gate optional dependencies using the `dep:` prefix:

```toml
[features]
mcp = ["dep:aletheia-diaporeia"]
tls = ["dep:axum-server"]
keyring = ["dep:keyring"]
```

## Build Examples

```bash
# Default build (tui, recall, storage-fjall, embed-candle)
cargo build --release

# Headless server with TLS, no TUI or ML
cargo build --release --no-default-features --features tls

# Default + MCP server
cargo build --release --features mcp

# Default + MCP + local LLM
cargo build --release --features "mcp,local-llm"

# Run core tests
cargo test --workspace --features test-core

# Run all tests including ML
cargo test --workspace --features test-full
```

## Important Notes

- **`embed-candle`** must remain in default features. It has been accidentally removed three times (#1263, #1326, #1378). A compile-time test in `tests/default_features.rs` guards against recurrence.
- **`computer-use`** requires Linux kernel 5.13+ with Landlock support.
- **`mneme-engine`** coexists with `sqlite` without link conflicts (verified).
- **Graphe/sqlite** is required by `episteme/mneme-engine` because `knowledge_portability.rs` references `aletheia_graphe::portability`.

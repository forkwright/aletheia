# Feature Flag Matrix

Every feature flag defined in the workspace, how flags interact across crates, and common build recipes.

## Feature Table

| Crate | Feature | Default? | Enables | Depends on |
|-------|---------|----------|---------|------------|
| **agora** | `test-core` | no | - | - |
| **agora** | `test-full` | no | - | - |
| **aletheia** | `default` | **yes** | `tui`, `recall`, `storage-fjall`, `embed-candle`, `cc-provider` | - |
| **aletheia** | `mcp` | no | `dep:diaporeia` | - |
| **aletheia** | `recall` | no | KnowledgeStore vector search + extraction persistence | `nous/knowledge-store`, `mneme/mneme-engine`, `mneme/storage-fjall`, `pylon/knowledge-store` |
| **aletheia** | `storage-fjall` | no | fjall LSM-tree backend | `mneme/storage-fjall` |
| **aletheia** | `embed-candle` | no | Local ML embeddings via candle | `mneme/embed-candle` |
| **aletheia** | `migrate-qdrant` | no | `dep:qdrant-client` | `mneme/mneme-engine`, `mneme/storage-fjall` |
| **aletheia** | `tls` | no | TLS support | `pylon/tls` |
| **aletheia** | `keyring` | no | Keyring integration | `symbolon/keyring` |
| **aletheia** | `tui` | no | `dep:koilon` | - |
| **aletheia** | `cc-provider` | no | Claude Code subprocess provider | `hermeneus/cc-provider` |
| **aletheia** | `energeia` | no | `dep:energeia` | - |
| **aletheia** | `test-core` | no | - | `mneme/test-core`, `nous/test-core`, `pylon/test-core` |
| **aletheia** | `test-full` | no | - | `test-core`, `mneme/test-full` |
| **daemon** (oikonomos) | `default` | **yes** | *(empty)* | - |
| **daemon** | `dispatch-cron` | no | Dispatch cron integration | `energeia/storage-fjall`, `dep:energeia` |
| **daemon** | `knowledge-store` | no | Prosoche memory consistency checks | `episteme/mneme-engine` |
| **daemon** | `test-core` | no | - | - |
| **daemon** | `test-full` | no | - | - |
| **dianoia** | `test-core` | no | - | - |
| **dianoia** | `test-full` | no | - | - |
| **diaporeia** | `default` | **yes** | `knowledge-store` | - |
| **diaporeia** | `knowledge-store` | no | NousManager knowledge-store argument | `nous/knowledge-store` |
| **diaporeia** | `test-core` | no | - | - |
| **diaporeia** | `test-full` | no | - | - |
| **eidos** | `test-core` | no | - | - |
| **eidos** | `test-full` | no | - | - |
| **eidos** | `test-support` | no | `test_fixtures` module (Fact/Entity/Relationship builders) | - |
| **energeia** | `storage-fjall` | no | fjall storage for dispatch orchestration | `dep:fjall` |
| **energeia** | `test-core` | no | - | `storage-fjall` |
| **energeia** | `test-full` | no | - | - |
| **episteme** | `default` | **yes** | `graph-algo` | - |
| **episteme** | `graph-algo` | no | Graph algorithms | - |
| **episteme** | `test-support` | no | Test helpers | - |
| **episteme** | `mneme-engine` | no | Datalog knowledge store + typed query builder | `dep:krites`, `dep:tokio` |
| **episteme** | `storage-fjall` | no | fjall storage backend | `mneme-engine`, `krites/storage-fjall` |
| **episteme** | `embed-candle` | no | Candle embedding model loading | `dep:candle-core`, `dep:candle-nn`, `dep:candle-transformers`, `dep:tokenizers`, `dep:hf-hub` |
| **episteme** | `test-core` | no | - | `mneme-engine` |
| **episteme** | `test-full` | no | - | `test-core`, `embed-candle` |
| **eval** (dokimion) | `test-core` | no | - | - |
| **eval** | `test-full` | no | - | - |
| **graphe** | `default` | **yes** | *(empty)* | - |
| **graphe** | `mneme-engine` | no | Gates `tokio::task::JoinError` variants | `dep:tokio` |
| **graphe** | `test-core` | no | - | - |
| **graphe** | `test-full` | no | - | - |
| **hermeneus** | `cc-provider` | no | Claude Code subprocess provider | - |
| **hermeneus** | `test-utils` | no | Legacy mock LLM provider alias | - |
| **hermeneus** | `test-support` | no | Mock LLM provider for tests | `test-utils` |
| **hermeneus** | `test-core` | no | - | - |
| **hermeneus** | `test-full` | no | - | - |
| **integration-tests** | `default` | **yes** | `knowledge-store` | - |
| **integration-tests** | `engine-tests` | no | Engine integration tests | `mneme/mneme-engine` |
| **integration-tests** | `knowledge-store` | no | Knowledge-store integration tests | `nous/knowledge-store`, `pylon/knowledge-store` |
| **integration-tests** | `test-core` | no | - | `engine-tests` |
| **integration-tests** | `test-full` | no | - | `test-core` |
| **koina** | `default` | **yes** | *(empty)* | - |
| **koina** | `fjall` | no | fjall types / utilities | `dep:fjall`, `dep:tempfile` |
| **koina** | `test-core` | no | - | - |
| **koina** | `test-full` | no | - | - |
| **koina** | `test-support` | no | Test helpers | - |
| **krites** | `default` | **yes** | `graph-algo` | - |
| **krites** | `graph-algo` | no | Graph algorithms | - |
| **krites** | `storage-fjall` | no | fjall storage backend | `dep:fjall` |
| **krites** | `test-core` | no | - | `storage-fjall` |
| **krites** | `test-full` | no | - | - |
| **melete** | `test-core` | no | - | - |
| **melete** | `test-full` | no | - | - |
| **mneme** | `default` | **yes** | `fjall`, `graph-algo`, `mneme-engine` | - |
| **mneme** | `fjall` | no | Historical alias; graphe's fjall session backend is unconditional | - |
| **mneme** | `graph-algo` | no | Graph algorithm types | `episteme/graph-algo` |
| **mneme** | `test-support` | no | `MockEmbeddingProvider` and test helpers | `episteme/test-support` |
| **mneme** | `embed-candle` | no | Candle embedding provider | `episteme/embed-candle` |
| **mneme** | `mneme-engine` | no | Datalog knowledge engine | `dep:krites`, `graphe/mneme-engine`, `episteme/mneme-engine` |
| **mneme** | `storage-fjall` | no | fjall knowledge storage | `mneme-engine`, `episteme/storage-fjall` |
| **mneme** | `test-core` | no | - | `mneme-engine`, `storage-fjall` |
| **mneme** | `test-full` | no | - | `test-core`, `embed-candle` |
| **nous** | `knowledge-store` | no | Knowledge-store CRUD in agent pipeline | `mneme/mneme-engine` |
| **nous** | `test-support` | no | Mock search / test helpers | `hermeneus/test-utils`, `organon/test-support` |
| **nous** | `test-core` | no | - | `knowledge-store` |
| **nous** | `test-full` | no | - | `test-core` |
| **organon** | `computer-use` | no | Screen capture, Landlock sandbox (Linux 5.13+) | - |
| **organon** | `energeia` | no | Energeia capability tools | `dep:energeia` (pre-enabled with `storage-fjall`) |
| **organon** | `test-support` | no | `MockToolExecutor`, `ToolExecutorSpec`, etc. | `hermeneus/test-support`, `dep:rustls` |
| **organon** | `test-core` | no | - | - |
| **organon** | `test-full` | no | - | - |
| **pylon** | `default` | **yes** | *(empty)* | - |
| **pylon** | `tls` | no | TLS termination via `axum-server` | `dep:axum-server` |
| **pylon** | `knowledge-store` | no | Knowledge-store HTTP handlers | `nous/knowledge-store` |
| **pylon** | `test-core` | no | - | `knowledge-store` |
| **pylon** | `test-full` | no | - | - |
| **symbolon** | `default` | **yes** | *(empty)* | - |
| **symbolon** | `keyring` | no | OS keyring integration | `dep:keyring` |
| **symbolon** | `test-support` | no | Test helpers | - |
| **symbolon** | `test-core` | no | - | - |
| **symbolon** | `test-full` | no | - | - |
| **taxis** | `test-core` | no | - | - |
| **taxis** | `test-full` | no | - | - |
| **thesauros** | `test-core` | no | - | - |
| **thesauros** | `test-full` | no | - | - |
| **theatron** | *(none)* | - | - | - |
| **koilon** | `test-core` | no | - | - |
| **koilon** | `test-full` | no | - | - |
| **proskenion** | *(none)* | - | - | - |
| **skene** | `test-core` | no | - | - |
| **skene** | `test-full` | no | - | - |
| **poiesis-core** | *(none)* | - | - | - |
| **poiesis-sheet** | `default` | **yes** | `xlsx`, `ods` | - |
| **poiesis-sheet** | `xlsx` | no | XLSX backend | `dep:rust_xlsxwriter` |
| **poiesis-sheet** | `ods` | no | ODS backend | `dep:spreadsheet-ods` |
| **poiesis-slides** | `default` | **yes** | `pptx` | - |
| **poiesis-slides** | `pptx` | no | PPTX backend | `dep:ppt-rs` |
| **poiesis-text** | `default` | **yes** | `pdf`, `odt` | - |
| **poiesis-text** | `pdf` | no | PDF backend | `dep:krilla` |
| **poiesis-text** | `odt` | no | ODT backend | `dep:zip` |

## Cross-crate feature interactions

Some features in one crate cannot be used unless a downstream crate also enables a matching feature. The most important chains are listed below.

### Knowledge store (`mneme-engine` chain)

Any component that needs the Datalog knowledge store must enable the full `mneme-engine` chain:

```
aletheia/recall
  → nous/knowledge-store
      → mneme/mneme-engine
          → graphe/mneme-engine
          → episteme/mneme-engine
              → krites
```

Because of this, enabling `recall` in the `aletheia` binary automatically pulls in `mneme-engine` for the whole workspace via Cargo feature unification.

### Storage backends

| If you enable … | It requires … |
|-----------------|---------------|
| `mneme/storage-fjall` | `mneme/mneme-engine` + `episteme/storage-fjall` |
| `episteme/storage-fjall` | `episteme/mneme-engine` + `krites/storage-fjall` |
| `aletheia/storage-fjall` | `mneme/storage-fjall` |
| `energeia/storage-fjall` | `dep:fjall` |

### Embedding models (`embed-candle` chain)

```
aletheia/embed-candle
  → mneme/embed-candle
      → episteme/embed-candle
          → candle-core, candle-nn, candle-transformers, tokenizers, hf-hub
```

### Optional integrations

| Consumer feature | Dependency feature activated |
|------------------|------------------------------|
| `aletheia/mcp` | `dep:diaporeia` |
| `aletheia/tls` | `pylon/tls` (`dep:axum-server`) |
| `aletheia/keyring` | `symbolon/keyring` (`dep:keyring`) |
| `aletheia/tui` | `dep:koilon` |
| `aletheia/cc-provider` | `hermeneus/cc-provider` |
| `aletheia/energeia` | `dep:energeia` |
| `organon/energeia` | `dep:energeia` (with `storage-fjall` pre-enabled) |
| `daemon/knowledge-store` | `episteme/mneme-engine` |
| `diaporeia/knowledge-store` | `nous/knowledge-store` |

## Test feature tiers

Every workspace crate declares `test-core` and `test-full`. Most crates leave them empty; crates with storage- or ML-dependent tests wire them to the relevant engine features.

| Tier | Feature flag | Approx. tests | What it adds |
|------|--------------|---------------|--------------|
| **default** | *(none)* | ~5,400 | Pure-logic unit tests, config validation, type invariants |
| **test-core** | `--features test-core` | ~5,435 | Storage engine tests (Datalog, HNSW, fjall, knowledge store CRUD) |
| **test-full** | `--features test-full` | ~5,435 | ML embedding tests (candle model loading, vector generation) |
| **all** | `--all-features` | ~5,475 | + local-llm, computer-use, other optional features |

### Notable tier wiring

| Crate | `test-core` enables | `test-full` enables |
|-------|---------------------|---------------------|
| `aletheia` | `mneme/test-core`, `nous/test-core`, `pylon/test-core` | `test-core` + `mneme/test-full` |
| `mneme` | `mneme-engine`, `storage-fjall` | `test-core` + `embed-candle` |
| `episteme` | `mneme-engine` | `test-core` + `embed-candle` |
| `energeia` | `storage-fjall` | - |
| `krites` | `storage-fjall` | - |
| `nous` | `knowledge-store` | `test-core` |
| `pylon` | `knowledge-store` | - |
| `integration-tests` | `engine-tests` (→ `mneme/mneme-engine`) | `test-core` |

### How to run

```bash
# Fast developer loop (pure unit tests)
cargo test --workspace

# CI minimum: includes storage/engine integration tests
cargo test --workspace --features test-core

# Full suite: includes ML tests (needs ~16 GB RAM)
cargo test --workspace --features test-full

# Per-crate example
cargo test -p mneme --features test-core
cargo test -p episteme --features test-full
```

**Important:** The `aletheia` binary's default features (`recall`, `embed-candle`, `storage-fjall`) already enable `mneme-engine` and `embed-candle` for the entire workspace via Cargo feature unification. This means `cargo test --workspace` (no extra flags) will usually run engine and ML tests too. The tier distinction matters most when testing a single crate in isolation.

## Common Recipes

### "I want the smallest possible binary"

Disable the heavy defaults and keep only what you need:

```bash
cargo build -p aletheia --no-default-features
```

This drops:
- `tui` (ratatui, crossterm)
- `recall` / `storage-fjall` / `mneme-engine` (fjall, krites, Datalog)
- `embed-candle` (candle ML stack)
- `cc-provider` (Claude Code CLI delegation)

You can then opt back in selectively:

```bash
# HTTP API only, no knowledge store, no TUI
cargo build -p aletheia --no-default-features --features tls

# Add keyring auth but still skip ML
cargo build -p aletheia --no-default-features --features tls,keyring
```

### "I want full functionality"

Use the defaults (they are defaults for a reason):

```bash
cargo build -p aletheia
```

Or explicitly:

```bash
cargo build -p aletheia --features tui,recall,storage-fjall,embed-candle,cc-provider
```

### "I want to run the MCP server"

```bash
cargo build -p aletheia --features mcp
```

`mcp` is **not** in the default set because it adds auth friction and context-window overhead for local deployments.

### "I want the desktop UI"

`proskenion` is excluded from the workspace (GTK3/webkit2gtk dependency). Build it standalone:

```bash
cargo build --manifest-path crates/theatron/proskenion/Cargo.toml
```

### "I want to run integration tests"

```bash
# Default integration-test profile (knowledge-store)
cargo test -p integration-tests
```

### "I want to test a specific storage backend"

| Backend | Command |
|---------|---------|
| fjall (default) | `cargo test -p mneme --features test-core` |

### "I need the knowledge store in a downstream crate"

Enable `mneme/mneme-engine` (or a higher-level feature that implies it):

```toml
[dependencies]
mneme = { path = "../mneme", default-features = false, features = ["mneme-engine"] }
```

If you also need fjall-backed knowledge storage, use `storage-fjall` instead of `mneme-engine` (it implies `mneme-engine`).

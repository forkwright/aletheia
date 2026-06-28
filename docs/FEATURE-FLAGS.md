# Feature Flag Matrix

Every feature flag defined in the workspace, how flags interact across crates, and common build recipes.

Release feature coverage is derived from `cargo metadata` by
`scripts/release-feature-policy.py`. The only hand-maintained release policy is
`scripts/release-feature-policy.toml`, which records documented exclusions
such as network-only test tiers and the no-default feature combinations that
Cargo metadata cannot infer.

## Feature Table

| Crate | Feature | Default? | Enables | Depends on |
|-------|---------|----------|---------|------------|
| **agora** | `test-core` | no | - | - |
| **agora** | `test-full` | no | - | - |
| **aletheia** | `default` | **yes** | `tui`, `recall`, `storage-fjall`, `embed-candle`, `cc-provider` | - |
| **aletheia** | `mcp` | no | MCP server (Model Context Protocol) with recall-backed knowledge tools | `recall`, `dep:diaporeia`, `dep:rmcp` |
| **aletheia** | `recall` | no | KnowledgeStore vector search + extraction persistence | `nous/knowledge-store`, `mneme/mneme-engine`, `mneme/storage-fjall`, `pylon/knowledge-store` |
| **aletheia** | `storage-fjall` | no | fjall LSM-tree backend | `mneme/storage-fjall` |
| **aletheia** | `embed-candle` | no | Local ML embeddings via candle | `mneme/embed-candle` |
| **aletheia** | `openai-embed` | no | OpenAI-compatible and Voyage embedding providers | `mneme/openai-embed` |
| **aletheia** | `online-tests` | no | Network-dependent candle tests (HuggingFace downloads at test time) | `mneme/online-tests` |
| **aletheia** | `migrate-qdrant` | no | `dep:qdrant-client` | `mneme/mneme-engine`, `mneme/storage-fjall` |
| **aletheia** | `tls` | no | TLS support | `pylon/tls` |
| **aletheia** | `keyring` | no | Keyring integration | `symbolon/keyring` |
| **aletheia** | `tui` | no | `dep:koilon` | - |
| **aletheia** | `cc-provider` | no | Claude Code subprocess provider | `hermeneus/cc-provider` |
| **aletheia** | `codex-provider` | no | Codex subprocess provider | `hermeneus/codex-provider` |
| **aletheia** | `kimi-provider` | no | Kimi subprocess provider | `hermeneus/kimi-provider` |
| **aletheia** | `energeia` | no | Dispatch orchestration plus service-backed Energeia agent tools in the runtime registry | `dep:energeia`, `energeia/storage-fjall`, `organon/energeia` |
| **aletheia** | `bookkeeper` | no | Bookkeeper maintenance tools | `organon/bookkeeper` |
| **aletheia** | `computer-use` | no | Computer-use tools | `organon/computer-use` |
| **aletheia** | `z3` | no | Z3 SMT solver tool | `organon/z3` |
| **aletheia** | `test-core` | no | - | `mneme/test-core`, `nous/test-core`, `pylon/test-core` |
| **aletheia** | `test-full` | no | - | `test-core`, `mneme/test-full`, `online-tests` |
| **aletheia-routing** | `test-core` | no | - | - |
| **aletheia-routing** | `test-full` | no | - | `test-core` |
| **daemon** (oikonomos) | `default` | **yes** | *(empty)* | - |
| **daemon** | `knowledge-store` | no | Prosoche memory consistency checks | `episteme/mneme-engine` |
| **daemon** | `test-core` | no | - | - |
| **daemon** | `test-full` | no | - | - |
| **dianoia** | `test-core` | no | - | - |
| **dianoia** | `test-full` | no | - | - |
| **diaporeia** | `default` | **yes** | `knowledge-store` | - |
| **diaporeia** | `knowledge-store` | no | NousManager knowledge-store argument | `nous/knowledge-store`, `mneme/mneme-engine` |
| **diaporeia** | `test-core` | no | - | - |
| **diaporeia** | `test-full` | no | - | - |
| **eidos** | `test-core` | no | - | - |
| **eidos** | `test-full` | no | - | - |
| **eidos** | `test-support` | no | `test_fixtures` module (Fact/Entity/Relationship builders) | - |
| **energeia** | `storage-fjall` | no | fjall storage for dispatch orchestration | `dep:fjall`, `koina/fjall` |
| **energeia** | `test-core` | no | - | `storage-fjall` |
| **energeia** | `test-full` | no | - | `test-core` |
| **episteme** | `default` | **yes** | `graph-algo`, `reranker` | - |
| **episteme** | `graph-algo` | no | Graph algorithms | - |
| **episteme** | `reranker` | no | HTTP cross-encoder reranker | `dep:reqwest`, `dep:rustls`, `dep:tokio` |
| **episteme** | `gliner` | no | GLiNER ONNX bookkeeping provider | `dep:ort`, `dep:tokio`, `dep:tokenizers` |
| **episteme** | `nuextract` | no | NuExtract ONNX bookkeeping provider | `dep:ort`, `dep:tokio`, `dep:tokenizers` |
| **episteme** | `openai-embed` | no | OpenAI-compatible embedding provider | `dep:reqwest`, `dep:tokio`, `dep:rustls` |
| **episteme** | `online-tests` | no | Network-dependent candle tests | - |
| **episteme** | `test-support` | no | Test helpers | - |
| **episteme** | `mneme-engine` | no | Datalog knowledge store + typed query builder | `dep:krites`, `dep:tokio`, `graphe/mneme-engine` |
| **episteme** | `storage-fjall` | no | fjall storage backend | `mneme-engine`, `krites/storage-fjall` |
| **episteme** | `embed-candle` | no | Candle embedding model loading | `dep:candle-core`, `dep:candle-nn`, `dep:candle-transformers`, `dep:tokenizers`, `dep:hf-hub` |
| **episteme** | `test-core` | no | - | `mneme-engine` |
| **episteme** | `test-full` | no | - | `test-core`, `embed-candle`, `online-tests` |
| **eval** (dokimion) | `test-core` | no | - | - |
| **eval** | `test-full` | no | - | - |
| **gnosis** | `test-core` | no | - | - |
| **gnosis** | `test-full` | no | - | - |
| **graphe** | `default` | **yes** | *(empty)* | - |
| **graphe** | `mneme-engine` | no | Gates `tokio::task::JoinError` variants | `dep:tokio` |
| **graphe** | `portability` | no | Raw export/import entry points that bypass distilled-message filtering | - |
| **graphe** | `test-core` | no | - | - |
| **graphe** | `test-full` | no | - | - |
| **hermeneus** | `cc-provider` | no | Claude Code subprocess provider | - |
| **hermeneus** | `codex-provider` | no | Codex subprocess provider | - |
| **hermeneus** | `kimi-provider` | no | Kimi subprocess provider | - |
| **hermeneus** | `test-utils` | no | Legacy mock LLM provider alias | - |
| **hermeneus** | `test-support` | no | Mock LLM provider for tests | `test-utils` |
| **hermeneus** | `test-core` | no | - | - |
| **hermeneus** | `test-full` | no | - | - |
| **integration-tests** | `default` | **yes** | `knowledge-store` | - |
| **integration-tests** | `engine-tests` | no | Engine integration tests | `mneme/mneme-engine` |
| **integration-tests** | `knowledge-store` | no | Knowledge-store integration tests | `mneme/storage-fjall`, `nous/knowledge-store`, `pylon/knowledge-store` |
| **integration-tests** | `test-core` | no | - | `engine-tests` |
| **integration-tests** | `test-full` | no | - | `test-core` |
| **koina** | `default` | **yes** | *(empty)* | - |
| **koina** | `fjall` | no | fjall types / utilities | `dep:fjall`, `dep:tempfile` |
| **koina** | `test-core` | no | - | - |
| **koina** | `test-full` | no | - | - |
| **koina** | `test-support` | no | Test helpers | - |
| **krites** | `default` | **yes** | `graph-algo`, `async`, `hot-reload` | - |
| **krites** | `graph-algo` | no | Graph algorithms | - |
| **krites** | `async` | no | Async engine entry points | - |
| **krites** | `hot-reload` | no | Rule hot-reloading | `dep:notify`, `dep:arc-swap` |
| **krites** | `storage-fjall` | no | fjall storage backend | `dep:fjall` |
| **krites** | `test-core` | no | - | `storage-fjall` |
| **krites** | `test-full` | no | - | `test-core` |
| **melete** | `test-core` | no | - | - |
| **melete** | `test-full` | no | - | - |
| **mneme** | `default` | **yes** | `graph-algo`, `mneme-engine`, `portability` | - |
| **mneme** | `graph-algo` | no | Graph algorithm types | `episteme/graph-algo` |
| **mneme** | `portability` | no | Agent export/import raw entry points | `graphe/portability` |
| **mneme** | `reranker` | no | HTTP cross-encoder reranker passthrough | `episteme/reranker` |
| **mneme** | `gliner` | no | GLiNER bookkeeping provider passthrough | `episteme/gliner` |
| **mneme** | `nuextract` | no | NuExtract bookkeeping provider passthrough | `episteme/nuextract` |
| **mneme** | `test-support` | no | `MockEmbeddingProvider` and test helpers | `episteme/test-support` |
| **mneme** | `embed-candle` | no | Candle embedding provider | `episteme/embed-candle` |
| **mneme** | `openai-embed` | no | OpenAI-compatible and Voyage embedding provider passthrough | `episteme/openai-embed` |
| **mneme** | `online-tests` | no | Network-dependent candle tests passthrough | `episteme/online-tests` |
| **mneme** | `mneme-engine` | no | Datalog knowledge engine | `dep:krites`, `graphe/mneme-engine`, `episteme/mneme-engine` |
| **mneme** | `storage-fjall` | no | fjall knowledge storage | `mneme-engine`, `episteme/storage-fjall` |
| **mneme** | `test-core` | no | - | `mneme-engine`, `storage-fjall` |
| **mneme** | `test-full` | no | - | `test-core`, `embed-candle`, `online-tests` |
| **nous** | `knowledge-store` | no | Knowledge-store CRUD in agent pipeline | `mneme/mneme-engine` |
| **nous** | `pre-llm-triage` | no | Pre-LLM triage stage (intent, sensitivity, tier classification) | - |
| **nous** | `reranker` | no | Async reranker facade (`HttpReranker` / `NaiveReranker`) | `mneme/reranker` |
| **nous** | `gliner` | no | GLiNER bookkeeping provider facade | `mneme/gliner` |
| **nous** | `nuextract` | no | NuExtract bookkeeping provider facade | `mneme/nuextract` |
| **nous** | `deferred-schemas` | no | Deferred tool schemas in the execute loop | `organon/deferred-schemas` |
| **nous** | `test-support` | no | Mock search / test helpers | `hermeneus/test-utils`, `organon/test-support` |
| **nous** | `test-core` | no | - | `knowledge-store` |
| **nous** | `test-full` | no | - | `test-core` |
| **organon** | `computer-use` | no | Screen capture, Landlock sandbox (Linux 5.13+) | - |
| **organon** | `receipt_verification` | no | Receipt verification for hallucination detection | - |
| **organon** | `energeia` | no | Energeia capability tools | `dep:energeia` (pre-enabled with `storage-fjall`) |
| **organon** | `bookkeeper` | no | Bookkeeper maintenance tools | - |
| **organon** | `z3` | no | Z3 SMT solver tool | `dep:z3` |
| **organon** | `deferred-schemas` | no | Name+description-only tool schemas; full schemas via the `tool_schema` meta-tool | - |
| **organon** | `allow-absolute-file-refs` | no | Allow absolute paths in `{{file:...}}` interpolation | - |
| **organon** | `test-support` | no | `MockToolExecutor`, `ToolExecutorSpec`, etc. | `hermeneus/test-support`, `dep:rustls` |
| **organon** | `test-core` | no | - | - |
| **organon** | `test-full` | no | - | - |
| **pylon** | `default` | **yes** | *(empty)* | - |
| **pylon** | `tls` | no | TLS termination via `axum-server` | `dep:axum-server` |
| **pylon** | `knowledge-store` | no | Knowledge-store HTTP handlers | `nous/knowledge-store` |
| **pylon** | `test-core` | no | - | `knowledge-store` |
| **pylon** | `test-full` | no | - | `test-core` |
| **symbolon** | `default` | **yes** | *(empty)* | - |
| **symbolon** | `keyring` | no | OS keyring integration | `dep:keyring` |
| **symbolon** | `test-support` | no | Test helpers | - |
| **symbolon** | `test-core` | no | - | - |
| **symbolon** | `test-full` | no | - | - |
| **taxis** | `test-core` | no | - | - |
| **taxis** | `test-full` | no | - | - |
| **taxis** | `test-support` | no | `test_support` module (`EnvJail`) for integration test binaries | `dep:tempfile` |
| **thesauros** | `test-core` | no | - | - |
| **thesauros** | `test-full` | no | - | - |
| **koilon** | `test-core` | no | - | - |
| **koilon** | `test-full` | no | - | - |
| **skene** | `test-core` | no | - | - |
| **skene** | `test-full` | no | - | - |
| **poiesis-charts** | `default` | **yes** | *(empty)* | - |
| **poiesis-charts** | `theme-bridge` | no | Theme bridge | `dep:poiesis-theme` |
| **poiesis-charts** | `charts-vega` | no | Vega-Lite fallback for chart kinds the pure-Rust emitter does not own | - |
| **poiesis-printer-chromium** | `default` | **yes** | `chromium` | - |
| **poiesis-printer-chromium** | `chromium` | no | Chromium-driven printing | `dep:chromiumoxide`, `dep:tokio`, `dep:which`, `dep:futures` |
| **poiesis-sheet** | `default` | **yes** | `xlsx`, `ods`, `workbook` | - |
| **poiesis-sheet** | `xlsx` | no | XLSX backend | `dep:rust_xlsxwriter`, `dep:calamine` |
| **poiesis-sheet** | `ods` | no | ODS backend | `dep:spreadsheet-ods` |
| **poiesis-sheet** | `workbook` | no | Themed workbook assembly | `xlsx`, `dep:poiesis-theme` |
| **poiesis-slides** | `default` | **yes** | `pptx` | - |
| **poiesis-slides** | `pptx` | no | PPTX backend (hand-rolled OOXML; `quick-xml` and `zip` are unconditional deps) | - |
| **poiesis-text** | `default` | **yes** | `pdf`, `odt` | - |
| **poiesis-text** | `pdf` | no | PDF backend | `dep:krilla` |
| **poiesis-text** | `odt` | no | ODT backend | `dep:zip` |

Workspace crates not listed define no feature flags: `aletheia-classify`, `aletheia-lexica`, `aletheia-memory-mcp`, `aletheia-sessions-migrate`, `theatron`, `proskenion`, `poiesis-core`, and the remaining poiesis members (`deck`, `deck-layout`, `diff`, `doc`, `inspect`, `intake`, `lint`, `scaffold`, `theme`, `typst`, `verify`).


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
| `energeia/storage-fjall` | `dep:fjall` + `koina/fjall` |

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
| `aletheia/mcp` | `aletheia/recall`, `dep:diaporeia`, `dep:rmcp` |
| `aletheia/tls` | `pylon/tls` (`dep:axum-server`) |
| `aletheia/keyring` | `symbolon/keyring` (`dep:keyring`) |
| `aletheia/tui` | `dep:koilon` |
| `aletheia/cc-provider` | `hermeneus/cc-provider` |
| `aletheia/codex-provider` | `hermeneus/codex-provider` |
| `aletheia/kimi-provider` | `hermeneus/kimi-provider` |
| `aletheia/energeia` | `dep:energeia`, `energeia/storage-fjall`, `organon/energeia` |
| `aletheia/bookkeeper` | `organon/bookkeeper` |
| `aletheia/computer-use` | `organon/computer-use` |
| `aletheia/z3` | `organon/z3` |
| `organon/energeia` | `dep:energeia` (with `storage-fjall` pre-enabled) |
| `daemon/knowledge-store` | `episteme/mneme-engine` |
| `diaporeia/knowledge-store` | `nous/knowledge-store`, `mneme/mneme-engine` |

## Test feature tiers

Feature scope matters. `cargo test --workspace --all-features` activates every feature on every workspace member. `cargo test -p aletheia --all-features` activates only the `aletheia` package features, plus dependency features reached through its passthroughs. Use dependency features such as `organon/computer-use` directly only when building or testing that dependency crate.

Workspace crates with gated test dependencies declare `test-core` and
`test-full`. Crates with storage- or ML-dependent tests wire them to the
relevant engine features, and non-empty `test-core` features must be included
from `test-full`.

| Tier | Feature flag | Approx. tests | What it adds |
|------|--------------|---------------|--------------|
| **default** | *(none)* | ~5,400 | Pure-logic unit tests, config validation, type invariants |
| **test-core** | `--features test-core` | ~5,435 | Storage engine tests (Datalog, HNSW, fjall, knowledge store CRUD) |
| **test-full** | `--features test-full` | ~5,435 | ML embedding tests (candle model loading, vector generation) |
| **all** | `--all-features` | ~5,475 | provider subprocess adapters, computer-use, bookkeeper, z3, other optional features |

### Notable tier wiring

| Crate | `test-core` enables | `test-full` enables |
|-------|---------------------|---------------------|
| `aletheia` | `mneme/test-core`, `nous/test-core`, `pylon/test-core` | `test-core` + `mneme/test-full` + `online-tests` |
| `mneme` | `mneme-engine`, `storage-fjall` | `test-core` + `embed-candle` + `online-tests` |
| `episteme` | `mneme-engine` | `test-core` + `embed-candle` + `online-tests` |
| `energeia` | `storage-fjall` | `test-core` |
| `krites` | `storage-fjall` | `test-core` |
| `nous` | `knowledge-store` | `test-core` |
| `pylon` | `knowledge-store` | `test-core` |
| `integration-tests` | `engine-tests` (→ `mneme/mneme-engine`) | `test-core` |

### How to run

```bash
# Fast developer loop (pure unit tests)
cargo test --workspace

# CI minimum: includes storage/engine integration tests and JUnit output
cargo nextest run --profile ci --workspace --features test-core

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
- `tui` (koilon terminal dashboard: ratatui, crossterm)
- `recall` (knowledge-store wiring in nous/pylon and extraction persistence)
- `embed-candle` (candle ML stack)
- `cc-provider` (Claude Code CLI delegation)

It does **not** produce an engine-free binary: `aletheia` depends on
`mneme` with default features, so `mneme-engine` (krites, Datalog) stays
linked, and the binary depends on `fjall` unconditionally.
`--no-default-features` removes the recall wiring, ML stack, TUI, and CC
provider — not the storage/engine code.

You can then opt back in selectively:

```bash
# Minimal headless: HTTP API only, no recall wiring, no ML, no TUI
cargo build -p aletheia --no-default-features --features tls

# Add keyring auth but still skip ML
cargo build -p aletheia --no-default-features --features tls,keyring

# Minimal MCP server: stdio and HTTP MCP surfaces with recall-backed knowledge tools
cargo build -p aletheia --no-default-features --features mcp

# Full headless: everything except the TUI
cargo build -p aletheia --no-default-features --features recall,storage-fjall,embed-candle,cc-provider,tls
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
It implies `recall`, because Diaporeia exposes recall-backed knowledge tools and requires the same in-process
knowledge store wiring that Pylon uses.

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

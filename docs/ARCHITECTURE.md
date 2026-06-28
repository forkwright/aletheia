# Architecture

> Module map, dependency graph, trait boundaries, and extension points.
> Covers the Rust crate workspace.
>
> Technology choices and dependency policy: [TECHNOLOGY.md](TECHNOLOGY.md).
> Project roadmap: [PROJECT.md](PROJECT.md).

---

## Naming

Module and crate names use Greek terms reflecting their essential nature (nous = mind, mneme = memory, hermeneus = interpreter). See [lexicon.md](lexicon.md) for the full registry.

---

## Current substrate shape

The current runtime is a 48-crate workspace plus the excluded `proskenion`
desktop shell. The compact generated inventory lives in
[`_llm/L1-workspace.md`](../_llm/L1-workspace.md); this document describes the
human architecture and invariants.

Key substrate properties as of the 2026-05-08 refresh:

- **Working-memory continuity:** `nous` injects agent-curated `<key_info>` from
  the previous working checkpoint before recall/history assembly.
- **Composite loop defense:** tool execution combines `hermeneus::LoopGuard`
  signals for ping-pong, no-progress, and repeated args/result doom loops.
- **Tool receipt integrity:** `organon` emits HMAC-SHA256 receipts and an
  in-memory receipt ledger so the pipeline can detect hallucinated tool
  results.
- **Runtime-bridged MCP tools:** `crates/aletheia/src/external_tools.rs` can
  bridge configured external MCP servers into `organon::registry::ToolRegistry`
  when the binary is built with MCP support.
- **DiaporeiaServer-exposed MCP tools:** `diaporeia` exposes Aletheia's own MCP
  server surface over stdio or `/mcp` using an `rmcp` tool router; those tools
  are intentionally separate from the in-process `organon::registry::ToolRegistry`.
- **Auth facade:** `symbolon::AuthFacade` is the admin-token verification and
  revocation boundary shared by `pylon` and `diaporeia`.
- **MCP/HTTP RBAC parity:** `diaporeia` must preserve verified caller claims,
  including `nous_id` scope, and enforce the same target-agent boundaries as
  `pylon` for sessions, agent workspace resources, and knowledge operations.
- **Bounded stages:** `nous` carries per-stage timeout configuration for LLM
  execution and actor-manager operations.
- **Knowledge schema v11:** `eidos::Visibility` and `eidos::MemoryScope` travel
  through facts, Datalog schema, recall filtering, and MCP/API surfaces.
- **Operator-independent verification:** `integration-tests` owns the shared
  `TestHarness` plus a 25-scenario substrate canary suite.

---

## The binary

```text
aletheia
├── koina          -  errors, tracing, safe wrappers, fs utils
├── taxis          -  config, path resolution, oikos hierarchy, secret refs
├── mneme          -  thin facade re-exporting eidos, graphe, episteme, krites
│   ├── eidos      -  shared knowledge types (Fact, Entity, Relationship, Visibility, MemoryScope)
│   ├── graphe     -  fjall session/message store, retention, archive support
│   ├── episteme   -  knowledge pipeline: extraction, recall, consolidation, embeddings
│   └── krites     -  embedded Datalog engine + HNSW vectors (mneme-engine feature gate)
├── hermeneus      -  LLM provider registry, fallback chains, loop guard, credentials
├── organon        -  tool registry + built-in tools (see `crates/organon/src/builtins/`) + receipt ledger
├── nous           -  agent pipeline, bootstrap, recall, finalize, actor model
├── dianoia        -  planning / project orchestration
├── pylon          -  Axum HTTP gateway, SSE streaming, auth enforcement
├── diaporeia      -  MCP server interface exposed over stdio and streamable HTTP
├── symbolon       -  JWT auth, admin facade, sessions, RBAC
├── agora          -  channel registry + ChannelProvider trait
│   └── semeion    -  Signal (signal-cli subprocess)
├── daemon         -  oikonomos: per-nous background tasks, cron, prosoche
├── melete         -  context distillation, compression, token budget management
├── thesauros      -  domain pack loader (knowledge, tools, config overlays)
└── theatron       -  presentation umbrella (crates/theatron/)
    ├── core       -  shared API client, types, SSE infrastructure  (crates/theatron/skene/)
    └── tui        -  terminal dashboard                            (crates/theatron/koilon/)
```

---

## The oikos (instance structure)

Platform (tracked) vs. instance (gitignored). One directory, one boundary.

```text
aletheia/                          # git root  -  the platform
├── crates/                        # Rust workspace
├── docs/                          # platform docs
│
├── instance/                      # GITIGNORED  -  all instance state
│   ├── theke/                     # Tier 0: human + nous collaborative space
│   │   ├── USER.md               #   Canonical user profile (one copy)
│   │   ├── AGENTS.md             #   Team topology
│   │   ├── tools/                #   Tools for human + all nous
│   │   ├── research/             #   Shared research
│   │   ├── deliberations/        #   Multi-agent deliberations
│   │   └── projects/             #   Active work products
│   │
│   ├── shared/                   # Tier 1: nous-only shared
│   │   ├── tools/                #   Nous-only tools
│   │   ├── skills/               #   Extracted skill patterns
│   │   ├── hooks/                #   Global event hooks
│   │   ├── templates/            #   Prompt templates
│   │   └── coordination/         #   Blackboard, task state
│   │
│   ├── nous/                     # Tier 2: individual nous workspaces
│   │   ├── <agent-id>/           #   See WORKSPACE_FILES.md for full reference
│   │   │   ├── SOUL.md           #   Character, principles (operator-owned)
│   │   │   ├── IDENTITY.md       #   Name and emoji (required)
│   │   │   ├── GOALS.md          #   Active goals (operator-owned)
│   │   │   ├── MEMORY.md         #   Persistent knowledge (agent-writable)
│   │   │   ├── TOOLS.md          #   Tool inventory (auto-generated)
│   │   │   ├── PROSOCHE.md       #   Attention directives (auto-generated)
│   │   │   ├── CONTEXT.md        #   Session state (runtime-written)
│   │   │   ├── tools/            #   Nous-specific tools
│   │   │   ├── hooks/            #   Nous-specific hooks
│   │   │   └── memory/           #   Daily memory files

│   │
│   ├── config/                   # Deployment config
│   │   ├── aletheia.toml
│   │   ├── credentials/
│   │   └── bindings.yaml
│   │
│   ├── data/                     # Runtime stores
│   │   ├── sessions.db
│   │   └── engine/               #   Datalog engine persistent storage (embedded)
│   │
│   └── signal/                   # signal-cli data
│
└── instance.example/              # TRACKED  -  scaffold template
```

Three-tier cascading resolution: nous/{id} -> shared -> theke. Most specific wins. Presence is declaration - drop a file in the right directory, it's discovered.

The oikos hierarchy is described in [CONFIGURATION.md](CONFIGURATION.md).

---

## Rust crate workspace

48 crates in the workspace, with `proskenion` excluded and built via its own
manifest. The table below calls out the primary architecture crates; the full
generated inventory is `_llm/L1-workspace.md`.

### Crates

| Crate | Directory | Domain | Depends On |
|-------|-----------|--------|------------|
| `koina` | `crates/koina` | Errors (snafu), tracing, fs utilities, safe wrappers | nothing (leaf) |
| `taxis` | `crates/taxis` | Config loading (owned TOML cascade), path resolution, oikos hierarchy | koina |
| `eidos` | `crates/eidos` | Shared knowledge types: Fact, Entity, Relationship, EpistemicTier, Visibility, MemoryScope | nothing (leaf) |
| `graphe` | `crates/graphe` | fjall session/message store, retention, archive support | eidos, koina |
| `episteme` | `crates/episteme` | Knowledge pipeline: extraction, trace ingest, query rewrite, recall, consolidation, embedding provider | eidos, koina, graphe, krites (opt) |
| `krites` | `crates/krites` | Embedded Datalog and graph query engine with HNSW support | eidos |
| `mneme` | `crates/mneme` | Thin facade re-exporting eidos, graphe, episteme, krites | eidos, graphe, episteme, krites |
| `hermeneus` | `crates/hermeneus` | LLM provider registry, fallback chains, token redaction, provider trait, loop guard | koina, taxis |
| `organon` | `crates/organon` | Tool registry, tool definitions, tags, HMAC receipts, built-in tools, sandbox | koina, hermeneus |
| `symbolon` | `crates/symbolon` | JWT tokens, password hashing, admin auth facade, RBAC policies | koina |
| `melete` | `crates/melete` | Context distillation, compression strategies, token budget management | hermeneus |
| `agora` | `crates/agora` | Channel registry, ChannelProvider trait, Signal JSON-RPC client | koina, taxis |
| `daemon` (oikonomos) | `crates/daemon` | Per-nous background task scheduling, cron jobs, prosoche | koina |
| `dianoia` | `crates/dianoia` | Multi-phase planning orchestrator, project context tracking | nothing (leaf) |
| `thesauros` | `crates/thesauros` | Domain pack loader: external knowledge, tools, config overlays | koina, organon |
| `nous` | `crates/nous` | Agent pipeline, NousActor (tokio), working-memory injection, bootstrap, recall, execute, finalize | koina, taxis, mneme, hermeneus, organon, melete, thesauros |
| `pylon` | `crates/pylon` | Axum HTTP gateway, SSE streaming, auth middleware, meta-insights endpoints | koina, taxis, hermeneus, organon, mneme, nous, symbolon |
| `diaporeia` | `crates/diaporeia` | MCP server interface and stdio/external tool bridge for external AI agents | koina, taxis, nous, organon, mneme, symbolon |
| `skene` | `crates/theatron/skene` | Shared API client, types, SSE infrastructure for UIs | koina |
| `koilon` | `crates/theatron/koilon` | Terminal dashboard | koina, skene |
| `proskenion` | `crates/theatron/proskenion` | Dioxus desktop UI (excluded from workspace, requires GTK3) | skene |
| `aletheia` | `crates/aletheia` | Binary entrypoint (Clap CLI), wires all crates together | koina, taxis, hermeneus, organon, mneme, nous, symbolon, pylon, agora, thesauros, daemon, dianoia, dokimion, diaporeia (opt), koilon (opt) |

**Support crates** (not part of the application dependency graph):

## LLM provider routing

`hermeneus` owns the provider trait and concrete provider clients. `taxis`
owns the serialized `[[providers]]` config shape, including `providerType`,
`baseUrl`, `apiKeyEnv`, subprocess `binary`/`workdir`/`timeoutSecs`, `models`,
and `deploymentTarget`. The `aletheia` binary joins those layers in
`runtime/setup.rs` by building a provider plan and registering providers in list
order.

Current provider families:

| Provider family | Config type | Wire/runtime shape | Deployment target |
|-----------------|-------------|--------------------|-------------------|
| Anthropic cloud | `anthropic` | Anthropic Messages API | `cloud` |
| Claude Code | `claude-code` | `claude` CLI subprocess adapter (feature-gated `cc-provider`) | operator-local process, with provider egress controlled by Claude Code |
| OpenAI-compatible local/third-party | `openai-compatible` | `/v1/chat/completions` HTTP wire format | explicit per provider: usually `embedded` or `local-hosted` |
| OpenAI cloud | `openai` | `/v1/responses` by default; `apiFamily = "chat-completions"` selects the legacy endpoint | `cloud` |
| Codex OAuth | `codex-oauth` | `codex` CLI subprocess adapter (feature-gated `codex-provider`) | operator-local process |

`crates/hermeneus/src/openai/client.rs` implements both `/v1/responses` and
`/v1/chat/completions`, selected per provider via `apiFamily`. The
`openai-compatible` kind is reserved for local/proxy servers (llama.cpp,
ollama, vllm) and always uses chat completions.

Declarative `[[providers]]` entries are the supported configuration surface;
see `docs/CONFIGURATION.md#providers` for the full field reference. When the
list is non-empty, it is the complete provider-ordering contract. An
`anthropic` entry without `apiKeyEnv` uses the legacy top-level credential
chain at that entry's declared position; an empty list preserves the legacy
single-Anthropic fallback and its feature-gated startup shortcuts. Feature-gated
subprocess adapters (`claude-code`, `codex-oauth`) must be declared in the list
to participate in declarative ordering, and declarations fail startup/config
validation when the corresponding adapter feature is absent.

Local OpenAI-compatible servers continue to use the Chat Completions-compatible
shape:

```toml
[[providers]]
name = "local-qwen"
providerType = "openai-compatible"
baseUrl = "http://127.0.0.1:8088/v1"
deploymentTarget = "embedded"
models = ["Qwen3.5-35B-A3B-Q8_0"]
```

`deploymentTarget` is load-bearing for sovereignty filtering. OpenAI cloud
entries must be `cloud`; local servers must declare whether traffic terminates
on the same host (`embedded`) or an operator-trusted local endpoint
(`localhosted`).

| Crate | Directory | Domain | Depends On |
|-------|-----------|--------|------------|
| `dokimion` | `crates/eval` | Behavioral and cognitive eval framework (HTTP scenario runner) | koina |
| `integration-tests` | `crates/integration-tests` | Cross-crate integration test suite | dokimion, koina, taxis, mneme, hermeneus, nous, organon, pylon, symbolon, thesauros |

Additional workspace crates include `aletheia-classify`, `aletheia-lexica`,
`aletheia-memory-mcp`, `aletheia-routing`, `aletheia-sessions-migrate`,
`gnosis`, and the poiesis backend/helper crates (`poiesis-doc`,
`poiesis-diff`, `poiesis-inspect`, `poiesis-intake`, `poiesis-scaffold`,
`poiesis-text`, `poiesis-typst`). Keep the generated `_llm/L1-workspace.md`
list in sync when workspace membership changes.

### Mneme facade

Mneme was decomposed into four sub-crates. The `mneme` crate is a thin facade
over their downstream-facing APIs. Application crates should import memory,
session, recall, verification, tracing, and knowledge-store types through
`mneme` so the sub-crate layout can move without changing their imports. Direct
sub-crate imports are reserved for the sub-crates themselves and for specialized
tools that intentionally own a lower-level boundary.

| Sub-crate | Re-exports | Feature gate |
|-----------|------------|--------------|
| `eidos` | `bookkeeping`, `id`, `knowledge`, `meta`, `training` | always |
| `graphe` | `error`, `metrics`, `portability`, `store`, `types` | always |
| `episteme` | `consolidation`, `embedding`, `embedding_eval`, `extract`, `ingest`, `instinct`, `manifest`, `metrics`, `query_rewrite`, `recall`, `side_query`, `skill`, `skills`, `trace_ingest`, `verification` | `reranker` for reranker implementations |
| `episteme` | `knowledge_store` | `mneme-engine` |
| `krites` | `engine` | `mneme-engine` |

Downstream application crates depend on `mneme` for this public surface. When a
new downstream use needs a sub-crate item, add an explicit `mneme` re-export
instead of reaching through to the sub-crate from the application layer.

### Dependency graph

```text
                          aletheia (binary)
                  /   /   / |  \   \    \   \
                 /   /   /  |   \   \    \   \
             pylon nous agora melete organon taxis ...
             /|\ \  |\ \  |\    |      |\     |
            / | \ \ | \ \ | \   |      | \    |
  symbolon  | organon |  taxis hermeneus  koina
            |  |  \   |    |
            | hermeneus mneme (facade)
            |    |      / | \ \
            koina koina eidos graphe episteme krites
```

**Layer rules:**
- **Leaf** (no workspace deps): `koina`, `eidos`, `dianoia`
- **Low** (one workspace dep): `taxis`, `hermeneus`, `symbolon`, `krites` (eidos only), `daemon` (koina), `melete` (hermeneus), `skene` (koina), `dokimion` (koina)
- **Mid**: `graphe` (eidos + koina), `episteme` (eidos + koina + graphe + krites), `mneme` (facade), `organon` (koina + hermeneus), `agora` (koina + taxis), `thesauros` (koina + organon)
- **High**: `nous` (multiple mid+low deps), `pylon` (multiple deps including nous), `diaporeia` (MCP server, multiple deps including nous)
- **Top**: `aletheia` binary, `koilon` (koina + skene), `proskenion` (Dioxus desktop, excluded from workspace)
- **Support**: `integration-tests`

Imports flow downward only. Lower-layer crates must not depend on higher layers.

### Trait boundaries

| Trait | Crate | Purpose |
|-------|-------|---------|
| `EmbeddingProvider` | episteme | Vector embeddings from text |
| `ChannelProvider` | agora | Send/receive on a messaging channel |
| `LlmProvider` | hermeneus | LLM API calls |

---

## Adding components

### Rust crate

1. Create `crates/<name>/` with `Cargo.toml` and `src/lib.rs`
2. Add to workspace `members` in root `Cargo.toml`
3. Declare its layer in the dependency graph
4. Update this file
5. Workspace lints apply automatically

---

## Release profile

```toml
[profile.dev]
opt-level = 1

[profile.dev.package."*"]
opt-level = 2      # optimize deps in dev  -  faster iteration

[profile.release]
lto = "thin"
codegen-units = 1
strip = "symbols"
```

---

## Workspace topology metric

A dense dependency graph (many edges per node) signals that new work may fit better as a function inside an existing crate instead of as a new crate. We track the ratio of inter-crate edges to workspace crates:

```bash
./scripts/topology-metric.sh
```

**Baseline:** `crates=29, edges=102, ratio=3.51`

When adding a component, prefer extending an existing edge (dependency or trait boundary) over introducing a new node, unless the new crate provides a clean abstraction boundary or is consumed by multiple unrelated crates.

## Structural properties

- **koina, eidos, and dianoia are true leaf nodes.** No workspace deps in Rust.
- **symbolon depends only on koina** (plus external crates: reqwest, fjall, hmac/sha2/aes-gcm, argon2).
- **mneme is a thin facade.** It re-exports from eidos (types), graphe (session store), episteme (knowledge pipeline), and krites (Datalog engine). No logic of its own.
- **krites contains the Datalog+HNSW engine**, gated behind the `mneme-engine` feature.
- **Boundary: `krites` is the embedded Datalog and graph query engine.** It decides
  whether rules and facts satisfy a query; it does not judge agent behavior.
- **Boundary: `dokimion` is the behavioral and cognitive evaluation runner.** It
  runs scenarios, benchmarks, and agent-behavior checks against live Aletheia
  instances.
- **EmbeddingProvider lives in episteme**, not mneme.
- **Trait boundaries are extension points.** `EmbeddingProvider`, `ChannelProvider`, `LlmProvider` - implement the trait, swap the provider.
- **daemon depends only on koina** - lightweight scheduling, not a high-layer crate. No other application crate imports it.
- **dianoia has no workspace dependencies** - planning context fully decoupled from the agent pipeline. No other application crate imports it.
- **thesauros loads domain packs** - knowledge, tools, config overlays bundled as portable extensions. Depends on koina + organon.
- **nous requires a multi-thread Tokio runtime** (`rt-multi-thread`). The actor model and spawn-based timeout machinery depend on multiple OS threads. Single-thread runtime will deadlock.

## Preserving informative tension

Some architectural decisions look binary but aren't. When the tension between options carries information, model both states.

Examples in aletheia (correctly preserved):
- `EpistemicTier` (`Verified` / `Inferred` / `Assumed`): facts have confidence, not just yes/no
- Hot vs cold config classification
- `DegradedEmbeddingProvider`: embedding can be unavailable AND system continues

Examples audited for premature resolution:
- **Conflict resolution** (`episteme/conflict.rs`): when supersession happens, the original fact is preserved as historical evidence via `superseded_by` and `valid_to` - correctly binary.
- **Tool execution** (`organon/dispatch.rs`, `ToolResult`): `is_error: bool` is a strict success/fail collapse. Partial states (e.g. some prompts succeeded in a dispatch batch, tool produced output but also emitted warnings) are lost - premature collapse; see #3633.
- **Consolidation** (`episteme/consolidation/`): original facts are superseded, not deleted, and `consolidation_audit` records provenance. However, the consolidated `Fact` itself carries no `source_count` or multiplicity metadata, so independent-evidence signal is lost after merge - premature collapse; see #3634.
- **Actor shutdown** (`nous/manager.rs`): `ShutdownOutcome` already distinguishes `Clean` / `Panicked` / `TimedOut`, and #3472 addresses further graceful-degradation phases - correctly non-binary.

When adding a new feature, ask: does this decision collapse information that downstream code might need? If so, model the tension explicitly.

## Ergon

Tracks aletheia's main branch with zero divergence - no custom code, no feature additions. Ergon exists as a client-deliverable identity: same runtime, different branding. Changes flow from aletheia to ergon, never the reverse. Do not add ergon-specific code to this repository.

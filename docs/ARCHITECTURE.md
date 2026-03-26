# Architecture

> Module map, dependency graph, trait boundaries, and extension points.
> Covers the Rust crate workspace.
>
> Technology choices and dependency policy: [TECHNOLOGY.md](TECHNOLOGY.md).
> Project roadmap: [PROJECT.md](PROJECT.md).

---

## Naming

Module and crate names use Greek terms reflecting their essential nature (nous = mind, mneme = memory, hermeneus = interpreter). See [gnomon.md](gnomon.md) for the naming philosophy and [lexicon.md](lexicon.md) for the full registry.

---

## The binary

```text
aletheia
├── koina          -  errors, tracing, safe wrappers, fs utils
├── taxis          -  config, path resolution, oikos hierarchy, secret refs
├── mneme          -  thin facade re-exporting eidos, graphe, episteme, krites
│   ├── eidos      -  shared knowledge types (Fact, Entity, Relationship, EpistemicTier)
│   ├── graphe     -  SQLite session store: WAL, migrations, retention, backup
│   ├── episteme   -  knowledge pipeline: extraction, recall, consolidation, embeddings
│   └── krites     -  embedded Datalog engine + HNSW vectors (mneme-engine feature gate)
├── hermeneus      -  Anthropic client, model routing, credentials, provider trait
├── organon        -  tool registry + 38 built-in tools
├── nous           -  agent pipeline, bootstrap, recall, finalize, actor model
├── dianoia        -  planning / project orchestration
├── pylon          -  Axum HTTP gateway, SSE streaming
├── diaporeia      -  MCP server interface for external AI agents
├── symbolon       -  JWT auth, sessions, RBAC
├── agora          -  channel registry + ChannelProvider trait
│   └── semeion    -  Signal (signal-cli subprocess)
├── daemon         -  oikonomos: per-nous background tasks, cron, prosoche
├── melete         -  context distillation, compression, token budget management
├── thesauros      -  domain pack loader (knowledge, tools, config overlays)
└── theatron       -  presentation umbrella (crates/theatron/)
    ├── core       -  shared API client, types, SSE infrastructure  (crates/theatron/core/)
    └── tui        -  terminal dashboard                            (crates/theatron/tui/)
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
│   │   ├── templates/            #   System prompt templates
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

24 crates (23 in default workspace build, theatron-desktop excluded) in `crates/`.

### Crates

| Crate | Directory | Domain | Depends On |
|-------|-----------|--------|------------|
| `koina` | `crates/koina` | Errors (snafu), tracing, fs utilities, safe wrappers | nothing (leaf) |
| `taxis` | `crates/taxis` | Config loading (figment TOML cascade), path resolution, oikos hierarchy | koina |
| `eidos` | `crates/eidos` | Shared knowledge types: Fact, Entity, Relationship, EpistemicTier | nothing (leaf) |
| `graphe` | `crates/graphe` | SQLite session store: WAL, migrations, retention, backup, export/import | eidos, koina |
| `episteme` | `crates/episteme` | Knowledge pipeline: extraction, recall, consolidation, embedding provider | eidos, koina, graphe, krites (opt) |
| `krites` | `crates/krites` | Embedded Datalog engine with HNSW and graph support | eidos |
| `mneme` | `crates/mneme` | Thin facade re-exporting eidos, graphe, episteme, krites | eidos, graphe, episteme, krites |
| `hermeneus` | `crates/hermeneus` | Anthropic client, model routing, credential management, provider trait | koina, taxis |
| `organon` | `crates/organon` | Tool registry, tool definitions, 38 built-in tools, sandbox | koina, hermeneus |
| `symbolon` | `crates/symbolon` | JWT tokens, password hashing, RBAC policies | koina |
| `melete` | `crates/melete` | Context distillation, compression strategies, token budget management | hermeneus |
| `agora` | `crates/agora` | Channel registry, ChannelProvider trait, Signal JSON-RPC client | koina, taxis |
| `daemon` (oikonomos) | `crates/daemon` | Per-nous background task scheduling, cron jobs, prosoche | koina |
| `dianoia` | `crates/dianoia` | Multi-phase planning orchestrator, project context tracking | nothing (leaf) |
| `thesauros` | `crates/thesauros` | Domain pack loader: external knowledge, tools, config overlays | koina, organon |
| `nous` | `crates/nous` | Agent pipeline, NousActor (tokio), bootstrap, recall, execute, finalize | koina, taxis, mneme, hermeneus, organon, melete, thesauros |
| `pylon` | `crates/pylon` | Axum HTTP gateway, SSE streaming, auth middleware | koina, taxis, hermeneus, organon, mneme, nous, symbolon |
| `diaporeia` | `crates/diaporeia` | MCP server interface for external AI agents | koina, taxis, nous, organon, mneme, symbolon |
| `theatron-core` | `crates/theatron/core` | Shared API client, types, SSE infrastructure for UIs | koina |
| `theatron-tui` | `crates/theatron/tui` | Terminal dashboard | koina, theatron-core |
| `theatron-desktop` | `crates/theatron/desktop` | Dioxus desktop UI (excluded from workspace, requires GTK3) | theatron-core |
| `aletheia` | `crates/aletheia` | Binary entrypoint (Clap CLI), wires all crates together | koina, taxis, hermeneus, organon, mneme, nous, symbolon, pylon, agora, thesauros, daemon, dianoia, dokimion, diaporeia (opt), theatron-tui (opt) |

**Support crates** (not part of the application dependency graph):

| Crate | Directory | Domain | Depends On |
|-------|-----------|--------|------------|
| `dokimion` | `crates/eval` | Behavioral eval framework (HTTP scenario runner) | koina |
| `integration-tests` | `crates/integration-tests` | Cross-crate integration test suite | dokimion, koina, taxis, mneme, hermeneus, nous, organon, pylon, symbolon, thesauros |

### Mneme facade

Mneme was decomposed into four sub-crates. The `mneme` crate is now a thin facade that re-exports their public APIs:

| Sub-crate | Re-exports | Feature gate |
|-----------|------------|--------------|
| `eidos` | `id`, `knowledge` | always |
| `graphe` | `backup`, `error`, `export`, `import`, `migration`, `portability`, `recovery`, `retention`, `schema`, `store`, `types` | `sqlite` (default) for most |
| `episteme` | `conflict`, `consolidation`, `embedding`, `extract`, `hnsw_index`, `instinct`, `knowledge_portability`, `knowledge_store`, `query`, `recall`, `skill`, `skills`, `vocab` | some behind `mneme-engine` or `hnsw_rs` |
| `krites` | `engine` | `mneme-engine` |

Downstream crates depend on `mneme` and get the full API surface without knowing about the decomposition.

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
- **Low** (one workspace dep): `taxis`, `hermeneus`, `symbolon`, `krites` (eidos only), `daemon` (koina), `melete` (hermeneus), `theatron-core` (koina), `dokimion` (koina)
- **Mid**: `graphe` (eidos + koina), `episteme` (eidos + koina + graphe + krites), `mneme` (facade), `organon` (koina + hermeneus), `agora` (koina + taxis), `thesauros` (koina + organon)
- **High**: `nous` (multiple mid+low deps), `pylon` (multiple deps including nous), `diaporeia` (MCP server, multiple deps including nous)
- **Top**: `aletheia` binary, `theatron-tui` (koina + theatron-core), `theatron-desktop` (Dioxus desktop, excluded from workspace)
- **Support**: `integration-tests`

Imports flow downward only. Lower-layer crates must not depend on higher layers.

### Trait boundaries

| Trait | Crate | Purpose |
|-------|-------|---------|
| `EmbeddingProvider` | episteme | Vector embeddings from text |
| `ChannelProvider` | agora | Send/receive on a messaging channel |
| `LlmProvider` | hermeneus | LLM API calls |

### Planned crates

| Crate | Domain | Milestone |
|-------|--------|-----------|
| `prostheke` | WASM plugin host (wasmtime) | M5 |
| `autarkeia` | Agent export/import | M5 |

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

## Structural properties

- **koina, eidos, and dianoia are true leaf nodes.** No workspace deps in Rust.
- **symbolon depends only on koina** (plus external crates: reqwest, rusqlite, ring, argon2).
- **mneme is a thin facade.** It re-exports from eidos (types), graphe (session store), episteme (knowledge pipeline), and krites (Datalog engine). No logic of its own.
- **krites contains the Datalog+HNSW engine**, gated behind the `mneme-engine` feature.
- **EmbeddingProvider lives in episteme**, not mneme.
- **Trait boundaries are extension points.** `EmbeddingProvider`, `ChannelProvider`, `LlmProvider` - implement the trait, swap the provider.
- **daemon depends only on koina** - lightweight scheduling, not a high-layer crate. No other application crate imports it.
- **dianoia has no workspace dependencies** - planning context fully decoupled from the agent pipeline. No other application crate imports it.
- **thesauros loads domain packs** - knowledge, tools, config overlays bundled as portable extensions. Depends on koina + organon.
- **nous requires a multi-thread Tokio runtime** (`rt-multi-thread`). The actor model and spawn-based timeout machinery depend on multiple OS threads. Single-thread runtime will deadlock.

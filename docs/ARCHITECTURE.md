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
├── koina         — errors, tracing, safe wrappers, fs utils
├── taxis         — config, path resolution, oikos hierarchy, secret refs
├── mneme         — session store (SQLite) + knowledge engine (vendored Datalog) + candle
│   ├── store     — SQLite session store: WAL, migrations, retention
│   ├── knowledge — Datalog knowledge graph, HNSW vectors, entity relations
│   ├── embedding — EmbeddingProvider trait: candle (local default)
│   ├── extract   — LLM-driven fact extraction, entity resolution
│   ├── recall    — hybrid retrieval (vector + graph + BM25), MMR diversity
│   └── engine/   — embedded Datalog + HNSW engine (mneme-engine feature gate)
├── hermeneus     — Anthropic client, model routing, credentials, provider trait
├── organon       — tool registry + built-in tools
├── nous          — agent pipeline, bootstrap, recall, finalize, actor model
├── dianoia       — planning / project orchestration
├── pylon         — Axum HTTP gateway, SSE streaming
├── diaporeia     — MCP server interface for external AI agents
├── symbolon      — JWT auth, sessions, RBAC
├── agora         — channel registry + ChannelProvider trait
│   └── semeion   — Signal (signal-cli subprocess)
├── daemon        — oikonomos: per-nous background tasks, cron, evolution, prosoche
├── melete        — distillation, reflection, memory flush, consolidation
└── theatron      — presentation umbrella (crates/theatron/)
    └── tui       — terminal dashboard                                  (crates/theatron/tui/)
```

---

## The Oikos (instance structure)

Platform (tracked) vs. instance (gitignored). One directory, one boundary.

```text
aletheia/                          # git root — the platform
├── crates/                        # Rust workspace
├── docs/                          # platform docs
│
├── instance/                      # GITIGNORED — all instance state
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
└── instance.example/              # TRACKED — scaffold template
```

Three-tier cascading resolution: nous/{id} -> shared -> theke. Most specific wins. Presence is declaration - drop a file in the right directory, it's discovered.

The oikos hierarchy is described in [CONFIGURATION.md](CONFIGURATION.md).

---

## Rust crate workspace

Application crates in `crates/`, plus the `integration-tests` support crate.

### Crates

| Crate | Domain | Depends On |
|-------|--------|------------|
| `koina` | Errors (snafu), tracing, fs utilities, safe wrappers | nothing (leaf) |
| `taxis` | Config loading (figment YAML cascade), path resolution, oikos hierarchy | koina |
| `mneme` | Unified memory store, embedding provider trait, knowledge retrieval. Includes embedded Datalog+HNSW engine behind `mneme-engine` feature gate. | koina |
| `hermeneus` | Anthropic client, model routing, credential management, provider trait | koina |
| `organon` | Tool registry, tool definitions, built-in tool set | koina, hermeneus |
| `symbolon` | JWT tokens, password hashing, RBAC policies | koina |
| `melete` | Context distillation, compression strategies, token budget management | koina, hermeneus |
| `agora` | Channel registry, ChannelProvider trait, Signal JSON-RPC client | koina, taxis |
| `daemon` | Background task scheduling, cron jobs, lifecycle events | koina |
| `dianoia` | Multi-phase planning orchestrator, project context tracking | koina |
| `thesauros` | Domain pack loader - external knowledge, tools, config overlays | koina, organon |
| `nous` | Agent pipeline, NousActor (tokio), bootstrap, recall, execute, finalize | koina, taxis, mneme, hermeneus, organon, melete, thesauros |
| `pylon` | Axum HTTP gateway, SSE streaming, auth middleware | koina, taxis, hermeneus, organon, mneme, nous, symbolon |
| `diaporeia` | MCP server interface for external AI agents (`crates/diaporeia`) | koina, taxis, nous, organon, mneme, symbolon |
| `theatron-core` | Shared presentation types and traits for Aletheia UIs (`crates/theatron/core/`) | nothing (leaf) |
| `theatron-tui` | Terminal dashboard (`crates/theatron/tui/`) | theatron-core, reqwest (standalone UI client) |
| `aletheia` | Binary entrypoint (Clap CLI) - wires all crates together | taxis, hermeneus, organon, mneme, nous, symbolon, pylon, agora, thesauros, daemon, dianoia, theatron-tui (optional) |

**Support crates** (not part of the application dependency graph):

| Crate | Domain | Depends On |
|-------|--------|------------|
| `eval` | Behavioral eval framework (HTTP scenario runner) | nothing (leaf) |
| `integration-tests` | Cross-crate integration test suite | koina, taxis, mneme, hermeneus, nous, organon, pylon, symbolon, thesauros |

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
            | hermeneus mneme
            |    |       |
            koina koina  koina
```

**Layer rules:**
- **Leaf** (no workspace deps): `koina`
- **Low** (koina only): `taxis`, `hermeneus`, `symbolon`, `mneme` (includes embedded Datalog+HNSW engine behind feature gate)
- **Mid**: `melete` (koina + hermeneus), `organon` (koina + hermeneus), `agora` (koina + taxis), `daemon` (koina), `dianoia` (koina), `thesauros` (koina + organon)
- **High**: `nous` (multiple mid+low deps), `pylon` (multiple deps including nous), `diaporeia` (MCP server, multiple deps including nous)
- **Top**: `aletheia` binary, `tui` (terminal dashboard)
- **Support**: `eval`, `integration-tests`

Imports flow downward only. Lower-layer crates must not depend on higher layers.

### Trait boundaries

| Trait | Crate | Purpose |
|-------|-------|---------|
| `EmbeddingProvider` | mneme | Vector embeddings from text |
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
opt-level = 2      # optimize deps in dev — faster iteration

[profile.release]
lto = "thin"
codegen-units = 1
strip = "symbols"
```

---

## Structural properties

- **koina is a true leaf node.** No workspace deps in Rust.
- **symbolon depends only on koina** (plus external crates: reqwest, rusqlite, ring).
- **Datalog+HNSW engine is embedded** inside `mneme/src/engine/`, gated behind the `mneme-engine` feature.
- **Trait boundaries are extension points.** `EmbeddingProvider`, `ChannelProvider`, `LlmProvider` - implement the trait, swap the provider.
- **daemon depends only on koina** - lightweight scheduling, not a high-layer crate. No other application crate imports it.
- **dianoia depends only on koina** - planning context decoupled from the agent pipeline. No other application crate imports it.
- **thesauros loads domain packs** - knowledge, tools, config overlays bundled as portable extensions. Depends on koina + organon.
- **nous requires a multi-thread Tokio runtime** (`rt-multi-thread`). The actor model and spawn-based timeout machinery depend on multiple OS threads. Single-thread runtime will deadlock.

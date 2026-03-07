# Architecture

> Module map, dependency graph, trait boundaries, and extension points.
> Covers the Rust crate workspace.
>
> Technology choices and dependency policy: [TECHNOLOGY.md](TECHNOLOGY.md).
> Project roadmap: [PROJECT.md](PROJECT.md).

---

## Naming

Module and crate names use Greek terms reflecting their purpose (nous = mind, mneme = memory, hermeneus = interpreter). See [ALETHEIA.md](ALETHEIA.md) for philosophical grounding. Names unconceal essential natures - they don't describe implementations.

---

## The Binary

```text
aletheia
├── koina         — errors, tracing, safe wrappers, fs utils
├── taxis         — config, path resolution, oikos hierarchy, secret refs
├── mneme         — session store (SQLite) + knowledge engine (vendored Datalog) + fastembed-rs
│   ├── store     — SQLite session store: WAL, migrations, retention
│   ├── knowledge — Datalog knowledge graph, HNSW vectors, entity relations
│   ├── embedding — EmbeddingProvider trait: fastembed-rs (local default)
│   ├── extract   — LLM-driven fact extraction, entity resolution
│   ├── recall    — hybrid retrieval (vector + graph + BM25), MMR diversity
│   └── engine/   — vendored Datalog + HNSW engine (mneme-engine feature gate)
├── hermeneus     — Anthropic client, model routing, credentials, provider trait
├── organon       — tool registry + built-in tools
├── nous          — agent pipeline, bootstrap, recall, finalize, actor model
├── dianoia       — planning / project orchestration
├── pylon         — Axum HTTP gateway, SSE streaming
├── symbolon      — JWT auth, sessions, RBAC
├── agora         — channel registry + ChannelProvider trait
│   └── semeion   — Signal (signal-cli subprocess)
├── daemon        — oikonomos: per-nous background tasks, cron, evolution, prosoche
├── melete        — distillation, reflection, memory flush, consolidation
├── tui           — terminal dashboard                                  (separate workspace member at tui/)
├── prostheke     — WASM plugin host (wasmtime)                         [planned: M5]
└── autarkeia     — agent export/import                                 [planned: M5]
```

---

## The Oikos (Instance Structure)

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
│   │   ├── aletheia.yaml
│   │   ├── credentials/
│   │   └── bindings.yaml
│   │
│   ├── data/                     # Runtime stores
│   │   ├── sessions.db
│   │   └── cozo/                 #   CozoDB persistent storage (embedded)
│   │
│   └── signal/                   # signal-cli data
│
└── instance.example/              # TRACKED — scaffold template
```

Three-tier cascading resolution: nous/{id} -> shared -> theke. Most specific wins. Presence is declaration - drop a file in the right directory, it's discovered.

The oikos hierarchy is described in [CONFIGURATION.md](CONFIGURATION.md).

---

## Rust Crate Workspace

Application crates in `crates/`, plus the `integration-tests` support crate.

### Crates

| Crate | Domain | Depends On |
|-------|--------|------------|
| `koina` | Errors (snafu), tracing, fs utilities, safe wrappers | nothing (leaf) |
| `taxis` | Config loading (figment YAML cascade), path resolution, oikos hierarchy | koina |
| `mneme` | Unified memory store, embedding provider trait, knowledge retrieval. Includes vendored CozoDB engine behind `mneme-engine` feature gate. | koina |
| `hermeneus` | Anthropic client, model routing, credential management, provider trait | koina |
| `organon` | Tool registry, tool definitions, built-in tool set | koina, hermeneus |
| `symbolon` | JWT tokens, password hashing, RBAC policies | koina |
| `melete` | Context distillation, compression strategies, token budget management | koina, hermeneus |
| `agora` | Channel registry, ChannelProvider trait, Signal JSON-RPC client | koina, taxis |
| `oikonomos` | Background task scheduling, cron jobs, lifecycle events | koina |
| `dianoia` | Multi-phase planning orchestrator, project context tracking | koina |
| `thesauros` | Domain pack loader - external knowledge, tools, config overlays | koina, organon |
| `nous` | Agent pipeline, NousActor (tokio), bootstrap, recall, execute, finalize | koina, taxis, mneme, hermeneus, organon, melete, thesauros |
| `pylon` | Axum HTTP gateway, SSE streaming, auth middleware | koina, taxis, hermeneus, organon, mneme, nous, symbolon |
| `tui` | Terminal dashboard — separate workspace member at `tui/` | reqwest (standalone UI client) |
| `aletheia` | Binary entrypoint (Clap CLI) - wires all crates together | taxis, hermeneus, organon, mneme, nous, symbolon, pylon, agora, thesauros, oikonomos, dianoia, tui (optional) |

**Support crates** (not part of the application dependency graph):

| Crate | Domain | Depends On |
|-------|--------|------------|
| `dokimion` | Behavioral eval framework — HTTP scenario runner | nothing (leaf) |
| `integration-tests` | Cross-crate integration test suite | koina, taxis, mneme, hermeneus, nous, organon, pylon, symbolon, thesauros |

### Dependency Graph

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
- **Low** (koina only): `taxis`, `hermeneus`, `symbolon`, `mneme` (includes vendored CozoDB engine behind feature gate)
- **Mid**: `melete` (koina + hermeneus), `organon` (koina + hermeneus), `agora` (koina + taxis), `oikonomos` (koina), `dianoia` (koina), `thesauros` (koina + organon)
- **High**: `nous` (multiple mid+low deps), `pylon` (multiple deps including nous)
- **Top**: `aletheia` binary, `tui` (terminal dashboard)
- **Support**: `dokimion` (eval), `integration-tests`

Imports flow downward only. Lower-layer crates must not depend on higher layers.

### Trait Boundaries

| Trait | Crate | Purpose |
|-------|-------|---------|
| `EmbeddingProvider` | mneme | Vector embeddings from text |
| `ChannelProvider` | agora | Send/receive on a messaging channel |
| `LlmProvider` | hermeneus | LLM API calls |

### Planned Crates

| Crate | Domain | Milestone |
|-------|--------|-----------|
| `prostheke` | WASM plugin host (wasmtime) | M5 |
| `autarkeia` | Agent export/import | M5 |
| `theatron` | Composable operations UI | M6 |

---

## Adding Components

### Rust Crate

1. Create `crates/<name>/` with `Cargo.toml` and `src/lib.rs`
2. Add to workspace `members` in root `Cargo.toml`
3. Declare its layer in the dependency graph
4. Update this file
5. Workspace lints apply automatically

### Plugin

Plugin system (prostheke) is planned for v2. See requirements in project planning docs.

---

## Release Profile

```toml
[profile.release]
strip = true
lto = "thin"
opt-level = "z"    # size-optimized — single static binary
codegen-units = 1

[profile.dev.package."*"]
opt-level = 2      # optimize deps in dev — faster iteration
```

---

## Structural Properties

- **koina is a true leaf node.** No workspace deps in Rust.
- **symbolon depends only on koina** (plus external crates: reqwest, rusqlite, jsonwebtoken).
- **CozoDB engine is vendored** inside `mneme/src/engine/`, gated behind the `mneme-engine` feature.
- **Trait boundaries are extension points.** `EmbeddingProvider`, `ChannelProvider`, `LlmProvider` - implement the trait, swap the provider.
- **oikonomos depends only on koina** - lightweight scheduling, not a high-layer crate. No other application crate imports it.
- **dianoia depends only on koina** - planning context decoupled from the agent pipeline. No other application crate imports it.
- **thesauros loads domain packs** - knowledge, tools, config overlays bundled as portable extensions. Depends on koina + organon.

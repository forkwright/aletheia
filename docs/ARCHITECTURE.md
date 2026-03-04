# Architecture

> Module map, dependency graph, trait boundaries, and extension points.
> Covers the Rust crate workspace (target) and TypeScript runtime (current production).
>
> Technology choices and dependency policy: [TECHNOLOGY.md](TECHNOLOGY.md).
> Project roadmap: [PROJECT.md](PROJECT.md).

---

## Naming

Module and crate names use Greek terms reflecting their purpose (nous = mind, mneme = memory, hermeneus = interpreter). See [ALETHEIA.md](../ALETHEIA.md) for philosophical grounding. Names unconceal essential natures - they don't describe implementations.

---

## The Binary

```text
aletheia
├── koina         — errors, tracing, safe wrappers, fs utils
├── taxis         — config, path resolution, oikos hierarchy, secret refs
├── mneme         — unified memory (CozoDB embedded + fastembed-rs + extraction)
│   ├── store     — CozoDB: vectors, graph, relations, bi-temporal facts — single embedded DB
│   ├── embed     — EmbeddingProvider trait: fastembed-rs (local default) | HTTP API (Voyage, optional)
│   ├── extract   — LLM-driven fact extraction, entity resolution, contradiction detection
│   └── recall    — hybrid retrieval (vector + graph + BM25), MMR diversity, recollection-as-memory
├── hermeneus     — Anthropic client, model routing, credentials, provider trait
├── organon       — tool registry + built-in tools
├── nous          — agent pipeline, bootstrap, recall, finalize, actor model
│   └── roles     — tekton, theoros, zetetes, kritikos, ergates
├── dianoia       — planning / project orchestration
├── pylon         — Axum HTTP gateway, SSE, static UI serving
├── symbolon      — JWT auth, sessions, RBAC
├── agora         — channel registry + ChannelProvider trait
│   ├── semeion   — Signal (signal-cli subprocess)
│   └── slack     — Slack (raw API + WebSocket)
├── daemon           — oikonomos: per-nous background tasks, cron, evolution, prosoche
├── melete        — distillation, reflection, memory flush, consolidation
├── prostheke     — WASM plugin host (wasmtime)                         [planned: M5]
└── autarkeia     — agent export/import                                 [planned: M5]
```

---

## The Oikos (Instance Structure)

Platform (tracked) vs. instance (gitignored). One directory, one boundary.

```text
aletheia/                          # git root — the platform
├── crates/                        # Rust workspace
├── ui/                            # Svelte 5 frontend
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
│   │   └── _example/             #   Template for new agents
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
| `symbolon` | JWT tokens, password hashing, RBAC policies | nothing (leaf) |
| `melete` | Context distillation, compression strategies, token budget management | koina, hermeneus |
| `agora` | Channel registry, ChannelProvider trait, Signal JSON-RPC client | koina, taxis |
| `oikonomos` | Background task scheduling, cron jobs, lifecycle events | koina |
| `dianoia` | Multi-phase planning orchestrator, project context tracking | koina |
| `thesauros` | Domain pack loader - external knowledge, tools, config overlays | koina, organon |
| `nous` | Agent pipeline, NousActor (tokio), bootstrap, recall, execute, finalize | koina, taxis, mneme, hermeneus, organon, thesauros |
| `pylon` | Axum HTTP gateway, SSE streaming, static UI serving, auth middleware | koina, taxis, hermeneus, organon, mneme, nous, symbolon |
| `aletheia` | Binary entrypoint (Clap CLI) - wires all crates together | taxis, hermeneus, organon, mneme, nous, symbolon, pylon, agora, thesauros, oikonomos, dianoia |

**Support crates** (not part of the application dependency graph):

| Crate | Domain | Depends On |
|-------|--------|------------|
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
- **Leaf** (no workspace deps): `koina`, `symbolon`
- **Low** (leaf deps only): `taxis`, `hermeneus`, `melete`, `agora`, `mneme` (includes vendored CozoDB engine behind feature gate)
- **Mid**: `organon` (koina + hermeneus), `oikonomos` (koina), `dianoia` (koina), `thesauros` (koina + organon)
- **High**: `nous` (multiple mid+low deps), `pylon` (multiple deps including nous)
- **Top**: `aletheia` binary
- **Support**: `integration-tests`

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

## TypeScript Runtime (Current Production)

All modules in `infrastructure/runtime/src/`:

| Module | Domain |
|--------|--------|
| `koina` | Logger, errors, event bus, safe wrappers, crypto |
| `taxis` | Config loading + Zod validation |
| `mneme` | Session store (better-sqlite3, migrations) |
| `hermeneus` | Anthropic SDK, provider routing, token counting |
| `organon` | Built-in tools, skills registry, MCP client |
| `nous` | Agent bootstrap, turn pipeline, competence model |
| `melete` | Distillation, reflection, memory flush |
| `symbolon` | Split-token authentication, JWT, sessions, RBAC |
| `dianoia` | Multi-phase planning orchestrator |
| `agora` | Channel registry, CLI commands |
| `semeion` | Signal client, listener, commands, TTS |
| `pylon` | Hono HTTP gateway, MCP, Web UI routes |
| `prostheke` | Plugin system, lifecycle hooks |
| `oikonomos` | Cron scheduler, watchdog, update checker |
| `portability` | Agent import/export (AgentFile format) |

### Initialization Order

```text
taxis → mneme → hermeneus → organon → nous → dianoia → prostheke → oikonomos
                                           ↑
                                     (semeion and pylon wired at runtime start)
```

### Dependency Rules

| Module | May Import | Must Not Import |
|--------|-----------|-----------------|
| `koina` | nothing (leaf) | everything |
| `taxis` | `koina` | everything else |
| `mneme` | `koina`, `taxis` | everything else |
| `hermeneus` | `koina`, `taxis` | everything else |
| `organon` | `koina`, `taxis`, `hermeneus` | everything else |
| `nous` | `koina`, `taxis`, `mneme`, `hermeneus`, `organon`, `melete`, `portability` | `semeion`, `pylon`, `prostheke`, `oikonomos`, `dianoia`, `symbolon` |
| `melete` | `koina`, `taxis`, `mneme`, `hermeneus` | everything else |
| `symbolon` | `koina` | everything else (stateless utilities) |
| `dianoia` | `koina`, `taxis`, `mneme`, `hermeneus`, `organon`, `nous` | `semeion`, `pylon`, `prostheke`, `oikonomos` |
| `agora` | `koina`, `taxis`, `semeion` | everything else |
| `semeion` | `koina`, `taxis`, `mneme`, `hermeneus`, `organon`, `nous`, `dianoia` | `pylon`, `prostheke`, `oikonomos` |
| `pylon` | `koina`, `taxis`, `mneme`, `hermeneus`, `organon`, `nous`, `dianoia`, `semeion`, `symbolon`, `oikonomos` | `prostheke`, `melete`, `portability` |
| `prostheke` | `koina`, `organon` | everything else |
| `oikonomos` | `koina`, `taxis`, `mneme`, `hermeneus`, `nous`, `melete` | everything else |
| `portability` | `koina`, `taxis`, `mneme`, `hermeneus`, `organon`, `nous` | everything else |

---

## Adding Components

### Rust Crate

1. Create `crates/<name>/` with `Cargo.toml` and `src/lib.rs`
2. Add to workspace `members` in root `Cargo.toml`
3. Declare its layer in the dependency graph
4. Update this file
5. Workspace lints apply automatically

### TypeScript Module

1. Create `src/<name>/` with entry file
2. Define its layer in the dependency graph
3. Wire into initialization in `aletheia.ts`
4. Update this file

### Plugin

See [docs/PLUGINS.md](PLUGINS.md).

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

- **koina is a true leaf node** in both stacks. No `index.ts` in TS - import from specific files. No workspace deps in Rust.
- **symbolon is zero-dependency** in both stacks. Takes `Database.Database` as constructor argument in TS.
- **CozoDB engine is vendored** inside `mneme/src/engine/`, gated behind the `mneme-engine` feature.
- **Trait boundaries are extension points.** `EmbeddingProvider`, `ChannelProvider`, `LlmProvider` - implement the trait, swap the provider.
- **oikonomos depends only on koina** - lightweight scheduling, not a high-layer crate. No other application crate imports it.
- **dianoia depends only on koina** - planning context decoupled from the agent pipeline. No other application crate imports it.
- **thesauros loads domain packs** - knowledge, tools, config overlays bundled as portable extensions. Depends on koina + organon.

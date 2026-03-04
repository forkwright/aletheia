# Architecture

> Module map, dependency graph, trait boundaries, and extension points.
> Covers the Rust crate workspace (target) and TypeScript runtime (current production).

---

## Naming

Module and subsystem names follow the naming philosophy in [gnomon.md](gnomon.md). Names unconceal essential natures, not describe implementations. Check the layer test (L1–L4) and anti-patterns before naming new components.

---

## Rust Crate Workspace

Application crates in `crates/`, plus support crates (`graph-builder`, `integration-tests`, `mneme-bench`).

### Crates

| Crate | Domain | Depends On |
|-------|--------|------------|
| `koina` | Errors (snafu), tracing, fs utilities, safe wrappers | nothing (leaf) |
| `taxis` | Config loading (figment YAML cascade), path resolution, oikos hierarchy | koina |
| `mneme-engine` | CozoDB embedded database: vectors, graph, relations, bi-temporal facts | nothing (vendored) |
| `mneme` | Unified memory store, embedding provider trait, knowledge retrieval | koina, mneme-engine (optional) |
| `hermeneus` | Anthropic client, model routing, credential management, provider trait | koina |
| `organon` | Tool registry, tool definitions, built-in tool set | koina, hermeneus |
| `symbolon` | JWT tokens, password hashing, RBAC policies | nothing (leaf) |
| `melete` | Context distillation, compression strategies, token budget management | koina, hermeneus |
| `agora` | Channel registry, ChannelProvider trait, Signal JSON-RPC client | koina, taxis |
| `daemon` | Background task scheduling, cron jobs, lifecycle events | koina |
| `dianoia` | Multi-phase planning orchestrator, project context tracking | koina |
| `thesauros` | Domain pack loader — external knowledge, tools, config overlays | koina, organon |
| `nous` | Agent pipeline, NousActor (tokio), bootstrap, recall, execute, finalize | koina, taxis, mneme, hermeneus, organon, thesauros |
| `pylon` | Axum HTTP gateway, SSE streaming, static UI serving, auth middleware | koina, taxis, hermeneus, organon, mneme, nous, symbolon |
| `aletheia` | Binary entrypoint (Clap CLI) — wires all crates together | taxis, hermeneus, organon, mneme, nous, symbolon, pylon, agora, thesauros, daemon, dianoia |

**Support crates** (not part of the application dependency graph):

| Crate | Domain | Depends On |
|-------|--------|------------|
| `graph-builder` | CSR graph construction for mneme-engine | nothing (standalone) |
| `integration-tests` | Cross-crate integration test suite | koina, taxis, mneme, hermeneus, nous, organon, pylon, symbolon, thesauros |
| `mneme-bench` | CozoDB benchmark harness | nothing (standalone, excluded from workspace) |

### Dependency Graph

```
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
                         |
                    mneme-engine (optional)
```

**Layer rules:**
- **Leaf** (no workspace deps): `koina`, `symbolon`, `mneme-engine`, `graph-builder`
- **Low** (leaf deps only): `taxis`, `hermeneus`, `melete`, `agora`, `mneme`
- **Mid**: `organon` (koina + hermeneus), `daemon` (koina), `dianoia` (koina), `thesauros` (koina + organon)
- **High**: `nous` (multiple mid+low deps), `pylon` (multiple deps including nous)
- **Top**: `aletheia` binary
- **Support**: `integration-tests`, `mneme-bench`

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
| `semeion` | Signal client, listener, commands, TTS |
| `pylon` | Hono HTTP gateway, MCP, Web UI routes |
| `prostheke` | Plugin system, lifecycle hooks |
| `daemon` | Cron scheduler, watchdog, update checker |
| `portability` | Agent import/export (AgentFile format) |

### Initialization Order

```
taxis → mneme → hermeneus → organon → nous → dianoia → prostheke → daemon
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
| `nous` | `koina`, `taxis`, `mneme`, `hermeneus`, `organon`, `melete`, `portability` | `semeion`, `pylon`, `prostheke`, `daemon`, `dianoia`, `symbolon` |
| `melete` | `koina`, `taxis`, `mneme`, `hermeneus` | everything else |
| `symbolon` | `koina` | everything else (stateless utilities) |
| `dianoia` | `koina`, `taxis`, `mneme`, `hermeneus`, `organon`, `nous` | `semeion`, `pylon`, `prostheke`, `daemon` |
| `semeion` | `koina`, `taxis`, `mneme`, `hermeneus`, `organon`, `nous`, `dianoia` | `pylon`, `prostheke`, `daemon` |
| `pylon` | `koina`, `taxis`, `mneme`, `hermeneus`, `organon`, `nous`, `dianoia`, `semeion`, `symbolon`, `daemon` | `prostheke`, `melete`, `portability` |
| `prostheke` | `koina`, `organon` | everything else |
| `daemon` | `koina`, `taxis`, `mneme`, `hermeneus`, `nous`, `melete` | everything else |
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

## Key Structural Properties

- **koina is a true leaf node** in both stacks. No `index.ts` in TS — import from specific files. No workspace deps in Rust.
- **symbolon is zero-dependency** in both stacks. Takes `Database.Database` as constructor argument in TS.
- **mneme-engine is vendored.** CozoDB absorbed into workspace. Optional dependency of `mneme`.
- **Trait boundaries are extension points.** `EmbeddingProvider`, `ChannelProvider`, `ModelProvider` — implement the trait, swap the provider.
- **daemon depends only on koina** — lightweight scheduling, not a high-layer crate. Must not be imported by other application crates.
- **dianoia depends only on koina** — planning context is decoupled from the agent pipeline. Must not be imported by other application crates.
- **thesauros loads domain packs** — knowledge, tools, config overlays bundled as portable extensions. Depends on koina + organon.

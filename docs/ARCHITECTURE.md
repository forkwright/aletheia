# Architecture

> Module map, dependency graph, trait boundaries, and extension points.
> Covers both the Rust crate workspace (target architecture) and the TypeScript runtime (current production).
> Last updated: 2026-03-01

---

## Naming

Module and subsystem names follow the naming philosophy documented in **[gnomon.md](gnomon.md)**. Names unconceal essential natures, not describe implementations. Each name should pass the layer test (L1 practical through L4 reflexive) and compose with existing names in the system topology.

When adding a new module, check gnomon.md's process section and anti-patterns before choosing a name.

---

## Rust Crate Workspace

11 application crates in `crates/`, plus `graph-builder` (build tool), `integration-tests` (test harness), `mneme-bench` (benchmarks, excluded from default build).

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
| `nous` | Agent pipeline, NousActor (tokio), bootstrap, recall, execute, finalize | koina, taxis, mneme, hermeneus, organon |
| `melete` | Context distillation, compression strategies, token budget management | koina, hermeneus |
| `agora` | Channel registry, ChannelProvider trait, Signal JSON-RPC client | koina, taxis |
| `pylon` | Axum HTTP gateway, SSE streaming, static UI serving, auth middleware | koina, taxis, hermeneus, organon, mneme, nous, symbolon |

### Dependency Graph

```
                        pylon
                     /  | |  \  \
                   /    | |   \   \
                nous  symbolon  |   |
              / | | \         mneme |
            /   |  \  \        |    |
       taxis organon melete    |    |
          |     |      |       |    |
        koina  hermeneus    mneme-engine
                                    agora
                                   /    \
                                taxis  koina
```

Imports are directional. Higher-layer crates may depend on lower layers. Lower-layer crates must not depend on higher layers. `koina` and `symbolon` are true leaf nodes with no internal dependencies.

### Trait Boundaries

These traits define the extension points between crates. Implement the trait, swap the provider.

| Trait | Crate | Purpose | Implementations |
|-------|-------|---------|-----------------|
| `EmbeddingProvider` | mneme | Vector embeddings from text | `FastEmbedProvider` (local, default), HTTP API (optional) |
| `ChannelProvider` | agora | Send/receive on a messaging channel | `SignalProvider` (signal-cli JSON-RPC) |
| `ModelProvider` | hermeneus | LLM API calls | `AnthropicProvider` |

### Planned Crates (Not Yet Built)

These exist as TypeScript modules in the current runtime. They will become Rust crates as the rewrite progresses.

| Crate | Domain | Milestone |
|-------|--------|-----------|
| `daemon` | Per-nous background tasks, cron, prosoche integration | M4 |
| `dianoia` | Multi-phase planning orchestrator | M4 |
| `prostheke` | WASM plugin host (wasmtime) | M5 |
| `autarkeia` | Agent export/import | M5 |

---

## TypeScript Runtime (Current Production)

All 14 modules in `infrastructure/runtime/src/`:

| Module | Domain | Files | Public Surface |
|--------|--------|-------|----------------|
| `koina` | Shared utilities, leaf node | 22 | `createLogger`, `AletheiaError` hierarchy, `trySafe`/`trySafeAsync`, `eventBus`, crypto, PII scanner, hooks |
| `taxis` | Config loading, Zod validation, paths | 7 | `loadConfig`, `AletheiaConfigSchema`, `paths`, `resolveNous`, `scaffoldAgent` |
| `mneme` | Session store, SQLite, migrations | 4 | `SessionStore`, `makeDb`, session/thread/message CRUD |
| `hermeneus` | Anthropic SDK, provider routing, token counting | 11 | `AnthropicProvider`, `createDefaultRouter`, `ProviderRouter`, token counter, pricing |
| `organon` | 48 built-in tools, skills registry, MCP client | 30 | `ToolRegistry`, `ToolHandler`, `SkillRegistry`, `McpClientManager` |
| `nous` | Agent bootstrap, turn pipeline, competence model | 36 | `NousManager`, turn orchestration, `CompetenceModel`, `UncertaintyTracker`, pipeline config |
| `melete` | Distillation, reflection, memory flush | 16 | `distillSession`, `reflectOnAgent`, `weeklyReflection`, `MemoryFlushTarget` |
| `semeion` | Signal client, TTS, commands, listener | 20 | `SignalClient`, `createDefaultRegistry` (commands), `startListener`, `DaemonHandle` |
| `pylon` | Hono HTTP gateway, MCP server, Web UI routes | 9 | `createGateway`, `startGateway`, MCP handlers, UI routes |
| `prostheke` | Plugin system, lifecycle hooks | 5 | `PluginRegistry`, plugin loader, hook dispatch |
| `daemon` | Cron scheduler, watchdog, update checker, reflection cron | 14 | `CronScheduler`, `Watchdog`, `startUpdateChecker`, reflection/evolution jobs |
| `dianoia` | Multi-phase planning orchestrator | 34 | `DianoiaOrchestrator`, `PlanningStore`, `ResearchOrchestrator`, `RequirementsOrchestrator`, `RoadmapOrchestrator`, `ExecutionOrchestrator`, `GoalBackwardVerifier`, `CheckpointSystem` |
| `symbolon` | Split-token authentication, JWT, sessions, RBAC, passwords | 13 | `AuthSessionStore`, `AuditLog`, `createAuthMiddleware`, `createAuthRoutes`, `signToken`, `verifyToken`, `hashPassword`, `rbac` |
| `portability` | Agent import/export (AgentFile format) | 5 | `exportAgent`, `importAgent`, `AgentFile`, portability CLI |

### Initialization Order

Derived from `infrastructure/runtime/src/aletheia.ts`:

```
taxis → mneme → hermeneus → organon → nous → dianoia → prostheke → daemon
                                           ↑
                                     (semeion and pylon initialized in startRuntime, wired at runtime)
```

**createRuntime() sequence:**

1. `taxis` -- load and validate config (`loadConfig`)
2. `koina/encryption` -- init encryption (depends on config)
3. `mneme` -- open SQLite store (`new SessionStore`)
4. `hermeneus` -- create provider router (`createDefaultRouter`)
5. `organon` -- create tool registry (`new ToolRegistry`), register all built-in tools
6. `nous` -- create manager (`new NousManager`); wires mneme, hermeneus, organon
7. `dianoia` -- create planning store and orchestrators; wired into nous manager
8. `prostheke` -- create plugin registry (`new PluginRegistry`)
9. `nous/competence` -- create competence model and uncertainty tracker; wired into nous manager

**startRuntime() continuation (after createRuntime):**

10. `prostheke` -- discover and load plugins
11. `koina/hooks` -- register declarative YAML hooks from disk
12. `semeion` -- initialize Signal client, listener, commands (if configured)
13. `pylon` -- create HTTP gateway, mount routes; wires auth, semeion, daemon refs
14. `daemon` -- start cron scheduler, watchdog, update checker

**Auth initialization** -- `symbolon` module is stateless utilities. `AuthSessionStore` and `AuditLog` are instantiated in `startGateway` via `pylon/server.ts` using the existing mneme SQLite `Database` handle.

### Dependency Rules

Imports are directional. Higher-layer modules may import lower layers. Lower-layer modules must not import higher layers.

| Module | May Import | Must Not Import |
|--------|-----------|-----------------|
| `koina` | (nothing, leaf node) | Any other module |
| `taxis` | `koina` | everything else |
| `mneme` | `koina`, `taxis` | everything else |
| `hermeneus` | `koina`, `taxis` | everything else |
| `organon` | `koina`, `taxis`, `hermeneus` | everything else |
| `nous` | `koina`, `taxis`, `mneme`, `hermeneus`, `organon`, `melete`, `portability` | `semeion`, `pylon`, `prostheke`, `daemon`, `dianoia`, `symbolon` |
| `melete` | `koina`, `taxis`, `mneme`, `hermeneus` | `organon`, `nous`, `semeion`, `pylon`, `prostheke`, `daemon`, `dianoia`, `symbolon`, `portability` |
| `symbolon` | `koina` | everything else (stateless utilities) |
| `dianoia` | `koina`, `taxis`, `mneme`, `hermeneus`, `organon`, `nous` | `semeion`, `pylon`, `prostheke`, `daemon`, `symbolon`, `melete`, `portability` |
| `semeion` | `koina`, `taxis`, `mneme`, `hermeneus`, `organon`, `nous`, `dianoia` | `pylon`, `prostheke`, `daemon`, `symbolon`, `melete`, `portability` |
| `pylon` | `koina`, `taxis`, `mneme`, `hermeneus`, `organon`, `nous`, `dianoia`, `semeion`, `symbolon`, `daemon` | `prostheke`, `melete`, `portability` |
| `prostheke` | `koina`, `organon` | `taxis`, `mneme`, `hermeneus`, `nous`, `melete`, `semeion`, `pylon`, `daemon`, `dianoia`, `symbolon`, `portability` |
| `daemon` | `koina`, `taxis`, `mneme`, `hermeneus`, `nous`, `melete` | `organon`, `semeion`, `pylon`, `prostheke`, `dianoia`, `symbolon`, `portability` |
| `portability` | `koina`, `taxis`, `mneme`, `hermeneus`, `organon`, `nous` | `melete`, `semeion`, `pylon`, `prostheke`, `daemon`, `dianoia`, `symbolon` |

---

## Adding Components

### Adding a Rust Crate

1. Create `crates/<name>/` with `Cargo.toml` and `src/lib.rs`
2. Add to workspace `members` in root `Cargo.toml`
3. Declare its layer in the dependency graph (what it depends on, who may depend on it)
4. Update this file: add row to Crates table, verify the dependency graph, note any new traits
5. All workspace lints apply automatically via `[workspace.lints]`

### Adding a TypeScript Module

1. Create `src/<name>/` with entry file (avoid `index.ts` for leaf modules)
2. Define its layer in the dependency graph (what it imports, who may import it)
3. Wire into initialization sequence in `aletheia.ts` (or `startRuntime` for services)
4. Update this file: add row to Modules table, Dependency Rules table, and update Initialization Order
5. The `.claude/rules/architecture.md` agent context automatically informs dispatched agents of the updated boundary rules once this file is updated

### Adding an Event

1. Follow `noun:verb` format (e.g., `distill:before`, `plugin:loaded`)
2. Define the event constant in the relevant module's event file or at point of emission
3. Keep the event name greppable -- use the module name as the noun for module lifecycle events
4. Document in `docs/STANDARDS.md#rule-event-name-format`

### Adding a Plugin

Plugins live outside the runtime at `~/.aletheia/plugins/<id>/`. They integrate via `prostheke`:

1. Create a plugin manifest (`plugin.json`) and entry file implementing `PluginDefinition`
2. The plugin receives `PluginApi` giving access to tool registration and event subscription
3. See `docs/PLUGINS.md` for the full plugin authoring guide

---

## Key Structural Properties

**koina is a true leaf node** in both Rust and TypeScript. In TS, it has no `index.ts` by design. All imports must reference the specific file (e.g., `../koina/logger.js`). This prevents circular dependencies and ensures only the needed symbols are loaded.

**symbolon is a zero-dependency utilities module** in both stacks. TS version: `symbolon/tokens.ts`, `symbolon/passwords.ts`, `symbolon/rbac.ts` import only from `node:crypto` and `hono`. `symbolon/sessions.ts` and `symbolon/audit.ts` take `Database.Database` as a constructor argument. Rust version: standalone crate with no workspace dependencies.

**dianoia routes are a seam** (TS only). `dianoia/routes.ts` imports a type from `pylon/routes/deps.ts` to satisfy the Hono route handler signature. The route file is owned by dianoia but mounted by pylon. If this coupling becomes a problem, the route file can move to `pylon/routes/plans.ts` with no behavioral change.

**daemon imports nous and melete** (TS only). The cron and reflection jobs need `NousManager` (to dispatch messages to agents) and melete functions (to run reflection cycles). This makes daemon a high-layer module despite its name suggesting infrastructure. Daemon must not be imported by other modules.

**mneme-engine is vendored.** The CozoDB database engine is absorbed into the workspace as a vendored crate. It has no workspace dependencies and is an optional dependency of `mneme` (behind the `mneme-engine` feature flag).

**Trait boundaries are the extension points.** `EmbeddingProvider`, `ChannelProvider`, and `ModelProvider` define where the system can be extended without modifying existing crates. New providers implement the trait and register at startup.
# Architecture

> Module map, initialization order, directed dependency graph, and extension points
> for all runtime modules. Updated when modules are added or boundaries change.
> Last updated: 2026-02-25

---

## Naming

Module and subsystem names follow the naming philosophy documented in **[gnomon.md](gnomon.md)**. Names unconceal essential natures — the fundamental character of a thing that persists across implementation changes. Each name should pass the layer test (L1 practical through L4 reflexive) and compose with existing names in the system topology.

When adding a new module, check gnomon.md's process section and anti-patterns before choosing a name.

---

## Modules

All 14 modules in `infrastructure/runtime/src/`:

| Module | Domain | Files | Public Surface |
|--------|--------|-------|----------------|
| `koina` | Shared utilities — leaf node | 22 | `createLogger`, `AletheiaError` hierarchy, `trySafe`/`trySafeAsync`, `eventBus`, crypto, PII scanner, hooks |
| `taxis` | Config loading, Zod validation, paths | 7 | `loadConfig`, `AletheiaConfigSchema`, `paths`, `resolveNous`, `scaffoldAgent` |
| `mneme` | Session store, SQLite, migrations | 4 | `SessionStore`, `makeDb`, session/thread/message CRUD |
| `hermeneus` | Anthropic SDK, provider routing, token counting | 11 | `AnthropicProvider`, `createDefaultRouter`, `ProviderRouter`, token counter, pricing |
| `organon` | 48 built-in tools, skills registry, MCP client | 30 | `ToolRegistry`, `ToolHandler`, `SkillRegistry`, `McpClientManager` |
| `nous` | Agent bootstrap, turn pipeline, competence model | 36 | `NousManager`, turn orchestration, `CompetenceModel`, `UncertaintyTracker`, pipeline config |
| `melete` | Disciplined practice — distillation, reflection, memory flush | 16 | `distillSession`, `reflectOnAgent`, `weeklyReflection`, `MemoryFlushTarget` |
| `semeion` | Signal client, TTS, commands, listener | 20 | `SignalClient`, `createDefaultRegistry` (commands), `startListener`, `DaemonHandle` |
| `pylon` | Hono HTTP gateway, MCP server, Web UI routes | 9 | `createGateway`, `startGateway`, MCP handlers, UI routes |
| `prostheke` | Plugin system, lifecycle hooks | 5 | `PluginRegistry`, plugin loader, hook dispatch |
| `daemon` | Cron scheduler, watchdog, update checker, reflection cron | 14 | `CronScheduler`, `Watchdog`, `startUpdateChecker`, reflection/evolution jobs |
| `dianoia` | Multi-phase planning orchestrator | 34 | `DianoiaOrchestrator`, `PlanningStore`, `ResearchOrchestrator`, `RequirementsOrchestrator`, `RoadmapOrchestrator`, `ExecutionOrchestrator`, `GoalBackwardVerifier`, `CheckpointSystem` |
| `symbolon` | Split-token authentication — JWT, sessions, RBAC, passwords | 13 | `AuthSessionStore`, `AuditLog`, `createAuthMiddleware`, `createAuthRoutes`, `signToken`, `verifyToken`, `hashPassword`, `rbac` |
| `portability` | Agent import/export (AgentFile format) | 5 | `exportAgent`, `importAgent`, `AgentFile`, portability CLI |

---

## Initialization Order

Derived from `infrastructure/runtime/src/aletheia.ts`:

```
taxis → mneme → hermeneus → organon → nous → dianoia → prostheke → daemon
                                           ↑
                                     (semeion and pylon initialized in startRuntime, wired at runtime)
```

**createRuntime() sequence:**

1. `taxis` — load and validate config (`loadConfig`)
2. `koina/encryption` — init encryption (depends on config)
3. `mneme` — open SQLite store (`new SessionStore`)
4. `hermeneus` — create provider router (`createDefaultRouter`)
5. `organon` — create tool registry (`new ToolRegistry`), register all built-in tools
6. `nous` — create manager (`new NousManager`); wires mneme, hermeneus, organon
7. `dianoia` — create planning store and orchestrators; wired into nous manager
8. `prostheke` — create plugin registry (`new PluginRegistry`)
9. `nous/competence` — create competence model and uncertainty tracker; wired into nous manager

**startRuntime() continuation (after createRuntime):**

10. `prostheke` — discover and load plugins
11. `koina/hooks` — register declarative YAML hooks from disk
12. `semeion` — initialize Signal client, listener, commands (if configured)
13. `pylon` — create HTTP gateway, mount routes; wires auth, semeion, daemon refs
14. `daemon` — start cron scheduler, watchdog, update checker

**Auth initialization** — `symbolon` module is stateless utilities. `AuthSessionStore` and `AuditLog` are instantiated in `startGateway` via `pylon/server.ts` using the existing mneme SQLite `Database` handle.

---

## Dependency Rules

Imports are directional. Higher-layer modules may import lower layers. Lower-layer modules must not import higher layers (prevents initialization cycles and tight coupling).

All rows verified by reading each module's entry files.

| Module | May Import | Must Not Import |
|--------|-----------|-----------------|
| `koina` | (nothing — leaf node) | Any other module |
| `taxis` | `koina` | `mneme`, `hermeneus`, `organon`, `nous`, `melete`, `semeion`, `pylon`, `prostheke`, `daemon`, `dianoia`, `symbolon`, `portability` |
| `mneme` | `koina`, `taxis` | `hermeneus`, `organon`, `nous`, `melete`, `semeion`, `pylon`, `prostheke`, `daemon`, `dianoia`, `symbolon`, `portability` |
| `hermeneus` | `koina`, `taxis` | `mneme`, `organon`, `nous`, `melete`, `semeion`, `pylon`, `prostheke`, `daemon`, `dianoia`, `symbolon`, `portability` |
| `organon` | `koina`, `taxis`, `hermeneus` | `nous`, `melete`, `semeion`, `pylon`, `prostheke`, `daemon`, `dianoia`, `symbolon`, `portability` |
| `nous` | `koina`, `taxis`, `mneme`, `hermeneus`, `organon`, `melete` | `semeion`, `pylon`, `daemon`, `symbolon`, `portability` |
| `melete` | `koina`, `taxis`, `mneme`, `hermeneus`, `nous` (reflection only) | `semeion`, `pylon`, `prostheke`, `daemon`, `symbolon`, `portability` |
| `semeion` | `koina`, `taxis`, `mneme`, `nous`, `organon`, `daemon` (watchdog type) | `pylon`, `prostheke`, `symbolon`, `portability` |
| `pylon` | `koina`, `taxis`, `mneme`, `hermeneus`, `nous`, `organon`, `semeion`, `symbolon`, `daemon`, `dianoia`, `melete` | `prostheke`, `portability` |
| `prostheke` | `koina`, `taxis`, `organon` | `mneme`, `hermeneus`, `nous`, `melete`, `semeion`, `pylon`, `daemon`, `dianoia`, `symbolon`, `portability` |
| `daemon` | `koina`, `taxis`, `mneme`, `hermeneus`, `nous`, `melete` | `semeion`, `pylon`, `prostheke`, `symbolon`, `portability` |
| `dianoia` | `koina`, `taxis`, `mneme`, `organon`, `pylon` (routes type import only) | `hermeneus`, `nous`, `melete`, `semeion`, `prostheke`, `daemon`, `symbolon`, `portability` |
| `symbolon` | `koina` (node:crypto only — no aletheia module imports) | All aletheia modules |
| `portability` | `koina`, `taxis`, `mneme` | `hermeneus`, `organon`, `nous`, `melete`, `semeion`, `pylon`, `prostheke`, `daemon`, `dianoia`, `symbolon` |

**Notes:**

- `melete` has a narrow upward reference to `nous/competence.ts` for reflection jobs — this is a known coupling point. The dependency flows through `daemon/reflection-cron.ts` which imports both.
- `semeion` imports `daemon/watchdog.ts` as a type import for the watchdog alert function injected via commands.
- `dianoia` has one type import from `pylon/routes/deps.ts` in `dianoia/routes.ts` — the planning routes are mounted by pylon, and the route file is technically in dianoia but wired into pylon at startup. This is an accepted architectural exception.
- `symbolon` is a stateless utilities module — it imports only from `node:crypto` and `hono`. It has no aletheia module dependencies by design.

---

## Extension Points

### Adding a Tool

1. Create `infrastructure/runtime/src/organon/built-in/my-tool.ts` implementing `ToolHandler`
2. Export it as a named constant: `export const myTool: ToolHandler = { definition: {...}, execute: ... }`
3. Register in `aletheia.ts`: `tools.register(myTool)` (or as factory if it needs deps)
4. If the tool is synchronous on all branches: use `execute(input): Promise<string>` with `return Promise.resolve(result)` — not `async execute()` with no `await` (triggers `require-await`)
5. Throw `ToolError` or appropriate `AletheiaError` subclass, never bare `Error`
6. Categories: `"essential"` (always loaded) or `"available"` (on-demand via `enable_tool`, expires after 5 unused turns)

### Adding a Command (Signal)

1. Register in `createDefaultRegistry()` in `src/semeion/commands.ts`
2. Provide `name`, `aliases`, `description`, `adminOnly`, and `async execute(args, ctx)` returning a response string
3. `CommandContext` provides: `sender`, `client`, `store`, `config`, `manager`, `watchdog`, `skills`

### Adding a Module

1. Create `infrastructure/runtime/src/my-module/` with a focused domain responsibility
2. Determine its layer in the dependency graph (what it imports, who may import it)
3. Wire into initialization sequence in `aletheia.ts` (or `startRuntime` for services)
4. Update this file (ARCHITECTURE.md): add row to Modules table, Dependency Rules table, and update Initialization Order
5. The `.claude/rules/architecture.md` agent context automatically informs dispatched agents of the updated boundary rules once this file is updated

### Adding an Event

1. Follow `noun:verb` format (e.g., `distill:before`, `plugin:loaded`)
2. Define the event constant in the relevant module's event file or at point of emission
3. Keep the event name greppable — use the module name as the noun for module lifecycle events
4. Document in `docs/STANDARDS.md#rule-event-name-format`

### Adding a Plugin

Plugins live outside the runtime at `~/.aletheia/plugins/<id>/`. They integrate via `prostheke`:

1. Create a plugin manifest (`plugin.json`) and entry file implementing `PluginDefinition`
2. The plugin receives `PluginApi` giving access to tool registration and event subscription
3. See `docs/PLUGINS.md` for the full plugin authoring guide

---

## Key Structural Properties

**koina is a true leaf node.** It has no `index.ts` by design — all imports must reference the specific file (e.g., `../koina/logger.js`, not `../koina/index.js`). This prevents circular dependencies and ensures only the needed symbols are loaded.

**symbolon is a zero-dependency utilities module.** `symbolon/tokens.ts`, `symbolon/passwords.ts`, `symbolon/rbac.ts` import only from `node:crypto` and `hono`. `symbolon/sessions.ts` and `symbolon/audit.ts` take `Database.Database` as a constructor argument. This design allows symbolon to be tested and used independently of the runtime.

**dianoia routes are a seam.** `dianoia/routes.ts` imports a type from `pylon/routes/deps.ts` to satisfy the Hono route handler signature. The route file is owned by dianoia but mounted by pylon. If this coupling becomes a problem, the route file can move to `pylon/routes/plans.ts` with no behavioral change.

**daemon imports nous and melete.** The cron and reflection jobs need `NousManager` (to dispatch messages to agents) and melete functions (to run reflection cycles). This makes daemon a high-layer module despite its name suggesting infrastructure. Daemon must not be imported by other modules — it is a leaf in the upward direction.

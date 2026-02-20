# Development Guide

Reference for contributors working on the Aletheia runtime.

---

## Prerequisites

| Tool | Version | Purpose |
|------|---------|---------|
| Node.js | >= 22.12 | Runtime target (ES2022, native fetch) |
| npm | (bundled) | Package management |
| tsdown | 0.20+ | Bundler (installed as devDependency) |
| TypeScript | 5.7+ | Type checking (installed as devDependency) |
| oxlint | 1.48+ | Linting (installed as devDependency) |
| vitest | 3.0+ | Testing (installed as devDependency) |

Optional:

- Docker / Docker Compose -- for supporting services (Qdrant, Neo4j, Langfuse)
- signal-cli -- for Signal messaging channel
- Chromium -- for the browser tool (`CHROMIUM_PATH` or `ENABLE_BROWSER` env)

---

## Building from Source

```bash
cd infrastructure/runtime
npm install
npx tsdown
```

Build output lands in `dist/` as a single ESM bundle:

```
dist/
  entry.mjs        # ~450KB, single-file executable
  entry.mjs.map    # Source map
```

The build configuration lives in `tsdown.config.ts`:

```ts
entry: ["src/entry.ts"],
format: "esm",
target: "node22",
clean: true,
outDir: "dist",
sourcemap: true,
```

The production binary is symlinked: `/usr/local/bin/aletheia` -> `infrastructure/runtime/dist/entry.mjs`.

For development without building, use `tsx` for direct TypeScript execution:

```bash
npm run dev    # tsx src/entry.ts
```

---

## Module Architecture

The runtime is organized into named modules, each owning a single concern. Initialization order matters -- later modules depend on earlier ones.

```
taxis -> mneme -> hermeneus -> organon -> semeion -> pylon -> prostheke -> daemon
```

### taxis -- Configuration

`src/taxis/`

Loads and validates `aletheia.json` configuration using Zod schemas. Resolves filesystem paths (sessions DB, shared directory, agent workspaces). Single source of truth for all typed config via `AletheiaConfig`.

Files: `schema.ts` (Zod schemas + types), `loader.ts` (config loading + agent resolution), `paths.ts` (canonical path resolution).

### mneme -- Session Storage

`src/mneme/`

SQLite-backed session store using `better-sqlite3`. Manages conversation history, message persistence, token counting, routing cache, contact approval (pairing), blackboard, interaction signals, working state, agent notes, and session archival. 10 embedded migrations.

Files: `store.ts` (SessionStore class), `schema.ts` (DB schema types).

### hermeneus -- LLM Routing and Pricing

`src/hermeneus/`

Anthropic SDK integration layer. Handles model selection, complexity-based routing (tiered model selection), token estimation, and provider abstraction. Manages the request/response cycle with the Anthropic API including prompt caching headers.

Files: `anthropic.ts` (SDK wrapper + type definitions), `router.ts` (ProviderRouter), `complexity.ts` (complexity scoring + model selection), `pricing.ts` (cost estimation), `token-counter.ts` (token budget math).

### organon -- Tools, Skills, and Self-Authoring

`src/organon/`

Tool registry with dynamic loading, on-demand activation, and automatic expiry. 33 built-in tools (`built-in/`), the skill system (`skills.ts`), self-authoring tools (`self-author.ts`), reversibility tagging (`reversibility.ts`), and result truncation (`truncate.ts`).

Files: `registry.ts` (ToolRegistry class), `skills.ts` (SkillRegistry), `self-author.ts` (runtime tool creation by agents), `reversibility.ts` (action safety classification), `built-in/*.ts` (individual tools).

### nous -- Agent Management

`src/nous/`

Agent lifecycle, turn execution, and bootstrap assembly. The `NousManager` orchestrates message handling -- routing inbound messages to the correct agent, assembling system prompts, executing tool loops, and managing distillation triggers.

Files: `manager.ts` (NousManager), `bootstrap.ts` (system prompt assembly), `bootstrap-diff.ts` (prompt change detection), `circuit-breaker.ts` (input/response quality gates), `competence.ts` (competence model), `uncertainty.ts` (uncertainty tracker), `ephemeral.ts` (spawn sessions), `trace.ts` (request tracing).

### distillation -- Context Compression

`src/distillation/`

Session history summarization pipeline. Extracts facts before summarizing, handles chunked long histories, and manages the distillation lifecycle. Triggered by token budget thresholds.

Files: `pipeline.ts` (orchestration), `extract.ts` (fact extraction), `summarize.ts` (summarization), `chunked-summarize.ts` (long history handling), `hooks.ts` (plugin integration), `similarity-pruning.ts` (dedup).

### semeion -- Signal Messaging and I/O

`src/semeion/`

Signal protocol client, message listener, command registry, text-to-speech, and message formatting. Handles inbound/outbound Signal messages, link preprocessing, media attachments, and the `!command` system.

Files: `client.ts` (SignalClient HTTP wrapper), `listener.ts` (SSE message listener), `sender.ts` (outbound delivery), `commands.ts` (CommandRegistry + built-in commands), `preprocess.ts` (link expansion), `format.ts` (message formatting), `tts.ts` (Piper TTS integration), `daemon.ts` (signal-cli process management).

### pylon -- Gateway, MCP, and Web UI

`src/pylon/`

Hono-based HTTP gateway serving the REST API, MCP (Model Context Protocol) SSE endpoints, streaming message API, and the Svelte web UI. All external access enters through pylon.

Files: `server.ts` (Hono app + REST API routes), `mcp.ts` (MCP SSE transport), `ui.ts` (web UI static serving + SSE events).

### prostheke -- Plugin System

`src/prostheke/`

Plugin loading and lifecycle management. Plugins can register tools and hook into lifecycle events (start, shutdown, before/after turn, before/after distillation, config reload).

Files: `types.ts` (PluginDefinition, hooks, manifest types), `loader.ts` (filesystem plugin loading), `registry.ts` (PluginRegistry).

### daemon -- Cron and Watchdog

`src/daemon/`

Cron job scheduler and service health watchdog. Cron triggers agent turns on schedule (heartbeat, consolidation, pattern extraction, adversarial testing). Watchdog monitors dependent services and alerts via Signal.

Files: `cron.ts` (CronScheduler), `watchdog.ts` (Watchdog + ServiceProbe), `update-check.ts` (periodic update check against GitHub releases).

### koina -- Shared Utilities

`src/koina/`

Cross-cutting concerns: structured logging (tslog), cryptographic helpers, filesystem utilities, error codes, typed errors, and the event bus.

Files: `logger.ts` (tslog wrapper), `crypto.ts`, `fs.ts`, `errors.ts`, `error-codes.ts`, `event-bus.ts`, `safe.ts` (non-fatal error boundaries via `trySafe`/`trySafeAsync`).

---

## Running Tests

Unit tests use vitest with the `forks` pool. Tests live alongside source files as `*.test.ts`.

```bash
# Run all unit tests
npm test

# Watch mode
npm run test:watch

# With coverage (v8 provider, thresholds enforced)
npm run test:coverage

# Integration tests only (separate config, 30s timeout)
npm run test:integration
```

Coverage thresholds (enforced in `vitest.config.ts`):

| Metric | Threshold |
|--------|-----------|
| Statements | 80% |
| Branches | 78% |
| Functions | 90% |
| Lines | 80% |

Test configuration:

- Root: `src/`
- Unit tests: `**/*.test.ts` (excludes `*.integration.test.ts`)
- Integration tests: `**/*.integration.test.ts` (separate config: `vitest.integration.config.ts`)
- Timeout: 10s (unit), 30s (integration)
- Pool: `forks` (process isolation)
- `passWithNoTests: false` -- builds fail if test files produce zero tests

---

## Pre-commit Hooks

The repo uses a git hook at `.githooks/pre-commit`:

```bash
cd "$(git rev-parse --show-toplevel)/infrastructure/runtime" || exit 0
npm run typecheck && npm run lint:check
```

To enable the hook after cloning:

```bash
git config core.hooksPath .githooks
```

The `precommit` npm script runs the full check suite (typecheck + lint + tests):

```bash
npm run precommit
# Equivalent to: npm run typecheck && npm run lint:check && npm run test -- --reporter=dot
```

### oxlint Configuration

Defined in `.oxlintrc.json`:

- Plugin: `typescript`
- All `correctness` rules: error
- `eqeqeq`: error (strict equality required)
- `typescript/no-floating-promises`: error
- `typescript/no-misused-promises`: error
- `typescript/no-explicit-any`: warn
- `no-unused-vars`, `no-empty`, `no-empty-function`, `require-await`: warn
- Ignored paths: `dist/`, `coverage/`, `test-results/`, `node_modules/`

---

## Code Style Conventions

### TypeScript Strictness

`tsconfig.json` enables maximum strictness:

- `strict: true`
- `exactOptionalPropertyTypes: true`
- `noUncheckedIndexedAccess: true`
- `noPropertyAccessFromIndexSignature: true`
- `noUnusedLocals: true` / `noUnusedParameters: true`
- `noFallthroughCasesInSwitch: true`
- `isolatedModules: true`

### File Headers

Every file starts with a single-line comment describing its purpose:

```ts
// Tool registry -- register, resolve, filter by policy, dynamic loading with expiry
```

No JSDoc blocks, no creation dates, no author info.

### Module Imports

- Use `.js` extensions in all import paths (required by NodeNext module resolution)
- Group imports: node builtins, then local modules
- Use `type` imports where possible: `import type { Foo } from "./bar.js"`

### Naming

- Files: `kebab-case.ts`
- Classes: `PascalCase` (e.g., `ToolRegistry`, `NousManager`)
- Interfaces: `PascalCase` (e.g., `ToolHandler`, `CommandContext`)
- Functions: `camelCase` (e.g., `createLogger`, `loadConfig`)
- Constants: `UPPER_SNAKE_CASE` for true constants, `camelCase` for config-derived values
- Logger instances: `const log = createLogger("module:submodule")`

### Index Access

Bracket notation required for string-keyed record access (enforced by `noPropertyAccessFromIndexSignature`):

```ts
// Correct
const value = record["key"];

// Incorrect (compiler error)
const value = record.key;
```

### Error Handling

- Use the typed error system from `koina/errors.ts`
- Error messages include context: `Failed to ${action}: ${err.message}`
- `instanceof Error` checks before accessing `.message` or `.stack`

### Testing

- Test files adjacent to source: `foo.ts` / `foo.test.ts`
- Integration tests use `.integration.test.ts` suffix
- Use `vi.fn()`, `vi.stubGlobal()`, `vi.stubEnv()` for mocking
- Test structure: `describe` per module/class, `it` per behavior

---

## Adding New Tools

Tools are registered in the `ToolRegistry` and exposed to agents during conversation turns.

### 1. Create the tool file

Add a new file in `src/organon/built-in/`:

```ts
// src/organon/built-in/my-tool.ts
// Brief description of what this tool does
import type { ToolHandler, ToolContext } from "../registry.js";

export const myTool: ToolHandler = {
  definition: {
    name: "my_tool",
    description: "What this tool does -- shown to the agent.",
    input_schema: {
      type: "object",
      properties: {
        param: {
          type: "string",
          description: "Description of the parameter",
        },
      },
      required: ["param"],
    },
  },
  async execute(
    input: Record<string, unknown>,
    context: ToolContext,
  ): Promise<string> {
    const param = input["param"] as string;
    // Implementation here
    return JSON.stringify({ result: param });
  },
};
```

The `ToolHandler` interface:

- `definition` -- Anthropic tool definition (name, description, JSON Schema input)
- `execute` -- receives parsed input and a `ToolContext` (nousId, sessionId, workspace, depth)
- `category` -- optional: `"essential"` (always available) or `"available"` (on-demand via `enable_tool`)

### 2. Register in aletheia.ts

Import and register the tool in `src/aletheia.ts` inside `createRuntime()`:

```ts
import { myTool } from "./organon/built-in/my-tool.js";

// Essential tool (always in context):
tools.register(myTool);

// On-demand tool (agent must call enable_tool first):
tools.register({ ...myTool, category: "available" as const });
```

### 3. Write tests

Create `src/organon/built-in/my-tool.test.ts`:

```ts
import { describe, it, expect } from "vitest";
import { myTool } from "./my-tool.js";

describe("myTool", () => {
  it("has valid definition", () => {
    expect(myTool.definition.name).toBe("my_tool");
    expect(myTool.definition.input_schema.required).toContain("param");
  });

  it("executes correctly", async () => {
    const result = await myTool.execute(
      { param: "test" },
      { nousId: "test", sessionId: "s1", workspace: "/tmp" },
    );
    expect(result).toContain("test");
  });
});
```

### Tool Categories

- **essential** -- always included in the tool list for every turn. Use sparingly (each tool consumes token budget).
- **available** -- hidden by default. Agents see the names listed in `enable_tool`'s description and can activate them on demand. Tools auto-expire after 5 unused turns.

### Wired Tools

Some tools need runtime references (config, store, manager). These use factory functions:

```ts
export function createMyTool(store: SessionStore): ToolHandler {
  return {
    definition: { /* ... */ },
    async execute(input, context) {
      // Can access `store` via closure
    },
  };
}
```

Register them after the dependency is constructed in `createRuntime()`.

---

## Adding New Built-in Commands

Signal commands are prefixed with `!` and handled before messages reach agents.

### 1. Register in createDefaultRegistry

Edit `src/semeion/commands.ts` and add a new `registry.register()` call inside `createDefaultRegistry()`:

```ts
registry.register({
  name: "mycommand",
  aliases: ["mc"],                    // Optional short forms
  description: "What this command does",
  adminOnly: false,                   // true = owner-only
  async execute(args: string, ctx: CommandContext): Promise<string> {
    // args = everything after "!mycommand "
    // ctx provides: sender, client, store, config, manager, watchdog, skills
    return "Response text sent back to the user";
  },
});
```

The `CommandHandler` interface:

- `name` -- primary trigger (e.g., `"mycommand"` responds to `!mycommand`)
- `aliases` -- optional array of alternative triggers
- `description` -- shown in `!help` output
- `adminOnly` -- if true, only the config-defined owner can invoke it
- `execute(args, ctx)` -- returns a string response. `args` is the text after the command name, trimmed.

### 2. Available context

`CommandContext` provides:

| Field | Type | Description |
|-------|------|-------------|
| `sender` | string | Signal number of the sender |
| `senderName` | string | Contact name |
| `isGroup` | boolean | Whether the message came from a group |
| `accountId` | string | Signal account ID |
| `target` | SendTarget | Reply target (recipient or group) |
| `client` | SignalClient | Signal HTTP client |
| `store` | SessionStore | Session database |
| `config` | AletheiaConfig | Full runtime config |
| `manager` | NousManager | Agent manager |
| `watchdog` | Watchdog or null | Service health monitor |
| `skills` | SkillRegistry or null | Loaded skills |

### 3. Write tests

Add tests in `src/semeion/commands.test.ts` or `commands-full.test.ts`:

```ts
it("responds to !mycommand", async () => {
  const registry = createDefaultRegistry();
  const match = registry.match("!mycommand some args");
  expect(match).not.toBeNull();
  expect(match!.handler.name).toBe("mycommand");
  expect(match!.args).toBe("some args");
});
```

---

## CLI Commands

The runtime exposes a CLI via Commander (`src/entry.ts`):

```
aletheia gateway start [-c config]    Start the gateway
aletheia gateway run [-c config]      Alias for gateway start
aletheia doctor [-c config]           Validate configuration
aletheia status [-u url] [-t token]   System health check
aletheia send -a <id> -m <text>       Send a message to an agent
aletheia sessions [-a agent]          List sessions
aletheia cron list                    List cron jobs
aletheia cron trigger <id>            Manually trigger a cron job
aletheia update [version] [--edge|--check|--rollback]  Self-update with rollback
aletheia replay <session-id>          Replay session history (--live for re-execution)
```

---

## Project Layout

```
infrastructure/runtime/
  src/
    entry.ts              CLI entry point
    aletheia.ts           Runtime wiring (createRuntime, startRuntime)
    taxis/                Configuration
    mneme/                Session storage
    hermeneus/            LLM routing
    organon/              Tools and skills
      built-in/           Individual tool implementations
    nous/                 Agent management
    distillation/         Context compression
    semeion/              Signal messaging
    pylon/                HTTP gateway
    prostheke/            Plugin system
    daemon/               Cron and watchdog
    koina/                Shared utilities
  dist/                   Build output
  tsdown.config.ts        Bundle configuration
  tsconfig.json           TypeScript configuration
  vitest.config.ts        Unit test configuration
  vitest.integration.config.ts  Integration test configuration
  .oxlintrc.json          Linter configuration
  package.json            Dependencies and scripts
```

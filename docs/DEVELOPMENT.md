# Development Guide

## Prerequisites

| Tool | Version | Purpose |
|------|---------|---------|
| Node.js | >= 22.12 | Runtime |
| tsdown | 0.20+ | Bundler (devDep) |
| TypeScript | 5.7+ | Type checking (devDep) |
| vitest | 3.0+ | Testing (devDep) |

Optional: Docker (Qdrant, Neo4j, Langfuse), signal-cli, Chromium (browser tool).

---

## Building

```bash
cd infrastructure/runtime && npm install && npx tsdown
```

Output: `dist/entry.mjs` (~450KB ESM bundle) + source map.

For dev without building: `npm run dev` (tsx).

---

## Module Architecture

Initialization order: `taxis → mneme → hermeneus → organon → semeion → pylon → prostheke → daemon`

| Module | Domain | Key Files |
|--------|--------|-----------|
| `taxis` | Config loading + Zod validation | `schema.ts`, `loader.ts`, `paths.ts` |
| `mneme` | Session store (better-sqlite3, 10 migrations) | `store.ts`, `schema.ts` |
| `hermeneus` | Anthropic SDK, provider router, token counting | `anthropic.ts`, `router.ts`, `complexity.ts`, `pricing.ts` |
| `organon` | 33 built-in tools, skills, self-authoring | `registry.ts`, `skills.ts`, `built-in/*.ts` |
| `nous` | Agent bootstrap, turn pipeline, working state | `manager.ts`, `bootstrap.ts`, `working-state.ts`, `pipeline/` |
| `distillation` | Context summarization | `pipeline.ts`, `extract.ts`, `summarize.ts` |
| `semeion` | Signal client, listener, commands, TTS | `client.ts`, `listener.ts`, `commands.ts` |
| `pylon` | Hono HTTP gateway, MCP, Web UI | `server.ts`, `mcp.ts`, `ui.ts` |
| `prostheke` | Plugin system (lifecycle hooks) | `types.ts`, `loader.ts`, `registry.ts` |
| `daemon` | Cron, watchdog, update checker | `cron.ts`, `watchdog.ts`, `update-check.ts` |
| `koina` | Logger, errors, event bus, safe wrappers, crypto | `logger.ts`, `errors.ts`, `event-bus.ts`, `safe.ts` |

---

## Testing

```bash
npm test                    # Unit tests
npm run test:watch          # Watch mode
npm run test:coverage       # Coverage (thresholds enforced)
npm run test:integration    # Integration (30s timeout)
```

Tests live alongside source as `*.test.ts`. Integration tests use `.integration.test.ts`.

Coverage thresholds: 80% statements, 78% branches, 90% functions, 80% lines.

---

## Code Style

Full conventions in [CONTRIBUTING.md](../CONTRIBUTING.md#code-standards). Key rules:

### TypeScript

Strict mode with `exactOptionalPropertyTypes`, `noUncheckedIndexedAccess`, `noPropertyAccessFromIndexSignature`. Bracket notation for index access: `record["key"]`.

### File Headers

One-line comment per file: `// Pipeline runner — composes stages for streaming and non-streaming turn execution`

### Import Order

```typescript
import { join } from "node:path";           // 1. Node builtins
import { Hono } from "hono";               // 2. External
import { createLogger } from "../koina/logger.js";  // 3. Internal
import type { TurnState } from "./types.js";        // 4. Local
```

### Error Handling

```typescript
// Typed errors
throw new PipelineError("Stage failed", { code: "PIPELINE_STAGE_FAILED", context: { stage, sessionId } });

// Non-critical operations
const result = trySafe("skill extraction", () => extractSkill(data), null);
```

### Naming

Files `kebab-case`, classes `PascalCase`, functions `camelCase` verb-first, constants `UPPER_SNAKE`, events `noun:verb`, booleans `is`/`has`/`should` prefix.

---

## Adding Tools

Tools live in `src/organon/built-in/`. Each exports a `ToolHandler`:

```typescript
export const myTool: ToolHandler = {
  definition: {
    name: "my_tool",
    description: "What this tool does",
    input_schema: { type: "object", properties: { param: { type: "string" } }, required: ["param"] },
  },
  async execute(input: Record<string, unknown>, context: ToolContext): Promise<string> {
    return JSON.stringify({ result: input["param"] });
  },
};
```

Register in `src/aletheia.ts`: `tools.register(myTool)`.

For tools needing runtime deps (store, config), use factory functions:

```typescript
export function createMyTool(store: SessionStore): ToolHandler { ... }
```

Categories: `"essential"` (always available) or `"available"` (on-demand via `enable_tool`, expires after 5 unused turns).

---

## Adding Signal Commands

Register in `createDefaultRegistry()` in `src/semeion/commands.ts`:

```typescript
registry.register({
  name: "mycommand",
  aliases: ["mc"],
  description: "What this command does",
  adminOnly: false,
  async execute(args: string, ctx: CommandContext): Promise<string> {
    return "Response text";
  },
});
```

`CommandContext` provides: `sender`, `client`, `store`, `config`, `manager`, `watchdog`, `skills`.

---

## CLI

```
aletheia gateway start [-c config]
aletheia doctor [-c config]
aletheia status [-u url] [-t token]
aletheia send -a <id> -m <text>
aletheia sessions [-a agent]
aletheia update [version] [--edge|--check|--rollback]
aletheia cron list|trigger <id>
aletheia replay <session-id> [--live]
```

---

## API Endpoints

| Method | Path | Purpose |
|--------|------|---------|
| GET | `/health` | Health check |
| GET | `/api/status` | Agent list + version |
| GET | `/api/metrics` | Full metrics |
| GET | `/api/agents` | All agents |
| GET | `/api/agents/:id` | Agent + recent sessions + usage |
| GET | `/api/agents/:id/identity` | Name + emoji |
| GET | `/api/sessions` | Session list |
| GET | `/api/sessions/:id/history` | Message history |
| POST | `/api/sessions/send` | Send message |
| POST | `/api/sessions/stream` | Streaming message (SSE) |
| POST | `/api/sessions/:id/archive` | Archive session |
| POST | `/api/sessions/:id/distill` | Trigger distillation |
| GET | `/api/events` | SSE event stream |
| GET | `/api/costs/summary` | Token usage + cost |
| GET | `/api/costs/session/:id` | Per-session costs |
| GET | `/api/cron` | Cron jobs |
| POST | `/api/cron/:id/trigger` | Trigger cron job |
| GET | `/api/skills` | Skills directory |
| GET | `/api/contacts/pending` | Pending contacts |
| POST | `/api/contacts/:code/approve` | Approve contact |
| POST | `/api/contacts/:code/deny` | Deny contact |
| GET | `/api/config` | Config summary |

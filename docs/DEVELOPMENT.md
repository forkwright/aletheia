# Development Guide

## Prerequisites

### Rust

Rust stable (2024 edition), cargo, clippy (bundled).

### TypeScript

Node.js >= 22.12. Dev dependencies (tsdown, TypeScript, vitest) installed via `npm install`.

Optional: Docker/Podman (Qdrant, Neo4j, Langfuse), signal-cli, Chromium (browser tool).

---

## Building

### Rust

```bash
cargo build                     # debug
cargo build --release           # release (thin LTO, stripped)
cargo clippy --workspace        # lint — zero warnings, pedantic
```

Workspace lints: pedantic clippy with select allows. `dbg!`, `todo!`, `unimplemented!` denied. `unsafe` denied workspace-wide.

### TypeScript

```bash
cd infrastructure/runtime && npm install && npx tsdown
```

Output: `dist/entry.mjs` (~450KB ESM bundle). For dev without building: `npm run dev` (tsx).

---

## Testing

### Rust

```bash
cargo test --workspace                          # all
cargo test -p aletheia-nous                     # single crate
cargo test -p aletheia-nous -- actor            # filter by name
cargo test -p aletheia-integration-tests        # cross-crate
```

Tests live alongside source in `#[cfg(test)] mod tests`. Integration tests in `crates/integration-tests/`.

### TypeScript

```bash
npx vitest run                          # all
npx vitest run src/path/file.test.ts    # specific
```

Tests live alongside source as `*.test.ts`. Integration tests use `.integration.test.ts`.

---

## Code Style

Full reference: [STANDARDS.md](STANDARDS.md).

### Rust

- Edition 2024, `unsafe` denied, pedantic clippy
- Errors via `snafu` with context selectors
- `pub(crate)` by default
- `expect("invariant description")` over bare `unwrap()`
- Property-based testing with `proptest` where applicable

### TypeScript

- Strict mode with `exactOptionalPropertyTypes`, `noUncheckedIndexedAccess`
- Bracket notation for index access: `record["key"]`
- `.js` import extensions required

### Universal

- File headers: one-line comment describing purpose
- No inline comments except genuinely non-obvious *why* explanations
- No creation dates, author info, or AI generation indicators
- Conventional commits: `feat(scope):`, `fix(scope):`

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

Register in `src/aletheia.ts`. Categories: `"essential"` (always available) or `"available"` (on-demand, expires after 5 unused turns).

---

## CLI

```
aletheia start [--no-memory]     # start memory services + gateway
aletheia stop [--all]            # stop gateway (--all includes containers)
aletheia restart                 # restart gateway
aletheia logs [-f]               # follow logs
aletheia tui                     # terminal UI
aletheia status                  # live metrics
aletheia doctor                  # validate config + connectivity
aletheia send -a <id> -m <text>  # send message
aletheia sessions [-a agent]     # list sessions
aletheia update [version]        # self-update
aletheia cron list|trigger <id>  # manage cron
```

---

## API Endpoints

| Method | Path | Purpose |
|--------|------|---------|
| GET | `/health` | Health check |
| GET | `/api/status` | Agent list + version |
| GET | `/api/metrics` | Full metrics |
| GET | `/api/agents` | All agents |
| GET | `/api/agents/:id` | Agent detail |
| GET | `/api/sessions` | Session list |
| GET | `/api/sessions/:id/history` | Message history |
| POST | `/api/sessions/send` | Send message |
| POST | `/api/sessions/stream` | Streaming message (SSE) |
| POST | `/api/sessions/:id/archive` | Archive session |
| POST | `/api/sessions/:id/distill` | Trigger distillation |
| GET | `/api/events` | SSE event stream |
| GET | `/api/costs/summary` | Token usage + cost |
| GET | `/api/cron` | Cron jobs |
| POST | `/api/cron/:id/trigger` | Trigger cron job |
| GET | `/api/skills` | Skills directory |
| GET | `/api/config` | Config summary |

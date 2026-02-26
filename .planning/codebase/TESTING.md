# Testing Patterns

**Analysis Date:** 2026-02-24

## Test Framework

**Runner:** Vitest v4.0.18+
- Config: `vitest.config.ts` (main), `vitest.fast.config.ts` (agent-friendly), `vitest.integration.config.ts` (long-running)
- No Playwright tests in this codebase — playwright-core is a runtime dependency, not for testing

**Assertion Library:** Vitest built-in (Jest-compatible)

**Run Commands:**
```bash
npm test                    # All unit tests (excludes .integration.test.ts)
npm run test:watch          # Watch mode
npm run test:coverage       # Coverage report (enforces thresholds)
npm run test:integration    # Integration tests only (.integration.test.ts, 30s timeout)
npm run test:fast           # Fast suite (excludes heavy files, 5s timeout)
```

**Test Output:**
- Default reporter + JSON reporter to `test-results/results.json`
- Coverage report: `coverage/` directory (text, JSON, LCOV formats)

## Test File Organization

**Location:** Co-located with implementation
- `module.ts` paired with `module.test.ts` in same directory
- Example: `mneme/store.ts` has `mneme/store.test.ts` (788 lines)

**Naming:**
- Unit tests: `*.test.ts`
- Integration tests: `*.integration.test.ts`
- Full/heavy tests: `*-full.test.ts` (excluded from fast suite)
- Examples: `cron.test.ts`, `cron-full.test.ts`, `store.test.ts`, `manager-streaming.test.ts`

**Directory Structure:**
```
src/
├── koina/
│   ├── errors.ts
│   ├── errors.test.ts      (not found — errors are tested via module tests)
│   ├── logger.ts
│   ├── hooks.ts
│   └── hooks.test.ts       (558 lines)
├── mneme/
│   ├── store.ts
│   └── store.test.ts       (788 lines)
├── nous/
│   ├── manager.ts
│   ├── manager.test.ts     (280 lines)
│   ├── manager-streaming.test.ts  (340 lines)
│   └── pipeline/
│       ├── runner.ts
│       ├── stages/
│       │   └── execute.test.ts (293 lines)
├── distillation/
│   ├── summarize.test.ts   (56 lines)
│   ├── reflect.test.ts     (444 lines)
│   ├── pipeline.test.ts    (356 lines)
└── daemon/
    ├── cron.ts
    ├── cron.test.ts        (basic)
    └── cron-full.test.ts   (comprehensive)
```

## Test Structure

**Suite Organization:**
```typescript
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

describe("FeatureName", () => {
  let state: SomeType;

  beforeEach(() => {
    state = initializeState();
  });

  afterEach(() => {
    state?.cleanup();
  });

  it("does X when Y", () => {
    // arrange
    const input = makeInput();

    // act
    const result = doSomething(input);

    // assert
    expect(result).toBe(expected);
  });

  it("handles error Z", () => {
    const input = makeBadInput();
    expect(() => doSomething(input)).toThrow("specific error");
  });
});
```

**Patterns:**
- Setup: `beforeEach()` initializes test state
- Teardown: `afterEach()` cleans up resources (close stores, stop schedulers, unmock)
- Descriptive test names: `"does X when Y"`, `"handles error Z"`
- Arrange-Act-Assert pattern
- One primary assertion per test (can have secondary assertions for related checks)

**Examples from codebase:**

**Store tests** (`mneme/store.test.ts`):
```typescript
beforeEach(() => {
  store = new SessionStore(":memory:");
});

afterEach(() => {
  store.close();
});

it("creates and retrieves a session", () => {
  const session = store.createSession("syn", "main");
  expect(session.id).toMatch(/^ses_/);
  expect(session.nousId).toBe("syn");
  expect(session.sessionKey).toBe("main");
  expect(session.status).toBe("active");
});
```

**Manager tests** (`nous/manager.test.ts`):
```typescript
let manager: NousManager;
let store: SessionStore;
let router: ProviderRouter;
let tools: ToolRegistry;

beforeEach(() => {
  store = new SessionStore(":memory:");
  router = mockRouter();
  tools = new ToolRegistry();
  manager = new NousManager(makeConfig(), store, router, tools);
});

afterEach(() => {
  store?.close();
  manager?.stop();
});

it("loads enabled jobs on start", () => {
  manager.start();
  const status = manager.getStatus();
  expect(status).toHaveLength(1);
  expect(status[0]!.id).toBe("heartbeat");
});
```

## Mocking

**Framework:** `vi` from Vitest (compatible with Jest `jest.fn()`)

**Patterns:**

**Mocking functions:**
```typescript
const mockRouter = (text: string) => {
  return {
    complete: vi.fn().mockResolvedValue({
      content: [{ type: "text", text }],
      stopReason: "end_turn",
      usage: { inputTokens: 100, outputTokens: 50, cacheReadTokens: 0, cacheWriteTokens: 0 },
      model: "test",
    }),
  } as never;
};
```

**Mocking modules:**
```typescript
vi.mock("./logger.js", () => ({
  createLogger: () => ({
    info: vi.fn(),
    warn: vi.fn(),
    error: vi.fn(),
    debug: vi.fn(),
  }),
}));
```

**Mock assertions:**
```typescript
const router = mockRouter("Response");
await summarizeMessages(router, [{ role: "user", content: "hello" }], {}, "test-model");

const callArgs = (router.complete as ReturnType<typeof vi.fn>).mock.calls[0]![0];
expect(JSON.stringify(callArgs)).toContain("expected-value");
expect(target.addMemories).toHaveBeenCalledWith("syn", ["fact1", "fact2"]);
expect(target.addMemories).toHaveBeenCalledTimes(2);
```

**What to Mock:**
- External APIs and providers (use `vi.fn().mockResolvedValue()`)
- Database calls (use `:memory:` SQLite instead when possible)
- I/O operations (file system, network)
- Expensive computations
- System modules (timers, logger)

**What NOT to Mock:**
- Core business logic (test real implementation)
- Database schema validation (use `:memory:` SQLite)
- Error classes and error handling
- Event bus (test real event propagation)
- Store operations (use real SessionStore with `:memory:`)

## Fixtures and Factories

**Test Data:**
```typescript
function makeConfig(jobs: Array<Record<string, unknown>> = []) {
  return {
    cron: { jobs },
  } as never;
}

function makeManager() {
  return {
    handleMessage: vi.fn().mockResolvedValue({ text: "ok" }),
  } as never;
}

const emptyExtraction = {
  facts: [], decisions: [], openItems: [], keyEntities: [], contradictions: [],
};
```

**Patterns:**
- Factory functions with sensible defaults: `make<Type>(overrides)`
- Immutable fixtures to prevent test pollution
- Type-cast as `never` to bypass strict typing (acceptable for test doubles)

**Location:**
- Fixtures defined at top of test file or in `beforeEach()` blocks
- Reusable fixtures extracted to shared test utilities (not found in this codebase, but pattern available)

## Coverage

**Requirements:** Enforced via Vitest thresholds
- Statements: 80%
- Branches: 78%
- Functions: 90%
- Lines: 80%

**View Coverage:**
```bash
npm run test:coverage
cat coverage/index.html  # Open in browser
```

**Coverage Config** (`vitest.config.ts`):
```typescript
coverage: {
  provider: "v8",
  reporter: ["text", "json", "lcov"],
  reportsDirectory: "../coverage",
  thresholds: {
    statements: 80,
    branches: 78,
    functions: 90,
    lines: 80,
  },
  include: ["**/*.ts"],
  exclude: ["**/*.test.ts", "**/*.integration.test.ts", "entry.ts"],
}
```

## Test Types

**Unit Tests:** Co-located `*.test.ts`
- Scope: Single module or feature
- Approach: Mock external dependencies
- Timeout: 10 seconds (default)
- Examples:
  - `cron.test.ts` — tests basic CronScheduler behavior
  - `summarize.test.ts` — tests summarization with mocked router
  - `hooks.test.ts` — tests hook loading and execution

**Integration Tests:** `*.integration.test.ts`
- Scope: Multiple modules working together
- Approach: Real dependencies where possible (real SQLite, real API calls)
- Timeout: 30 seconds (extended)
- Examples:
  - `cron-full.test.ts` — tests full cron pipeline with real scheduling
  - `manager-streaming.test.ts` — tests full turn pipeline with streaming
  - `server-full.test.ts` — tests HTTP server with real middleware

**Heavy Tests:** `*-full.test.ts`
- Scope: Complex scenarios, full state machines
- Excluded from `npm run test:fast` (agent/local development)
- Run in CI only or with `npm test`
- Examples:
  - `cron-full.test.ts` (schedule parsing, tick execution)
  - `reflect.test.ts` (444 lines — full distillation pipeline)
  - `server-full.test.ts` (397 lines — HTTP endpoints)

## Common Patterns

**Async Testing:**
```typescript
it("handles async operations", async () => {
  const result = await summarizeMessages(router, messages, extraction, "test-model");
  expect(result).toBe("summary text");
});

// With mock rejection
it("retries on failure", async () => {
  const target = makeTarget();
  (target.addMemories as ReturnType<typeof vi.fn>)
    .mockRejectedValueOnce(new Error("network"))
    .mockResolvedValueOnce({ added: 2, errors: 0 });

  const result = await flushToMemory(target, "syn", extraction, 3);
  expect(result.flushed).toBe(2);
  expect(target.addMemories).toHaveBeenCalledTimes(2);
});
```

**Error Testing:**
```typescript
it("throws typed error", () => {
  expect(() => {
    throw new ConfigError("invalid", { code: "CONFIG_VALIDATION_FAILED" });
  }).toThrow();
});

// Or async:
it("rejects with error", async () => {
  await expect(badAsyncFn()).rejects.toThrow("specific message");
});
```

**State Mutation Testing:**
```typescript
it("updates context in-place", () => {
  const ctx = { turnId: "t_1", sessionId: "ses_1" };
  updateTurnContext({ nousId: "syn" });
  expect(getTurnContext()).toMatchObject({ nousId: "syn" });
});
```

**Regex Matching:**
```typescript
it("generates correct IDs", () => {
  const session = store.createSession("syn", "main");
  expect(session.id).toMatch(/^ses_/);
  expect(nextRun).toMatch(/^\d{4}-\d{2}-\d{2}T/);  // ISO string
});
```

## Vitest Configuration Variants

**vitest.config.ts** (default, full suite):
- Includes all tests except `.integration.test.ts`
- Tests 10s timeout
- Reporters: default + JSON
- Pool: forks (conservative defaults)

**vitest.fast.config.ts** (agent-friendly):
- Excludes: `*.integration.test.ts`, `*-full.test.ts`, `manager.test.ts`, `manager-streaming.test.ts`, `store.test.ts`, `server-stream.test.ts`
- Timeout: 5 seconds
- Reporter: dot (minimal output)
- Use: `npm run test:fast` during development/agent sessions

**vitest.integration.config.ts** (slow tests):
- Includes only `*.integration.test.ts`
- Timeout: 30 seconds
- Run: `npm run test:integration`

## Pre-commit Testing

**Hook:** `.githooks/pre-commit` runs typecheck + lint, NOT tests
- CI handles full test runs (slower, duplicates local work)
- Local full-suite runs: 84+ seconds, frequently timeout, not recommended
- Use `npm run test:fast` locally, CI runs `npm run test:coverage`

**Agent Task Testing:**
- Use `npm run test:fast` only
- Never run full suite during agent sessions
- Tests are CI's responsibility

---

*Testing analysis: 2026-02-24*

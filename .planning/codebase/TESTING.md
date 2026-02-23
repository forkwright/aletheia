# Testing Patterns

**Analysis Date:** 2026-02-23

## Test Framework

**Runner:**
- vitest 4.0.18
- Config: `infrastructure/runtime/vitest.config.ts` (main), `vitest.fast.config.ts` (local dev), `vitest.integration.config.ts` (integration only)
- TypeScript support via @types/node and built-in tsconfig

**Assertion Library:**
- vitest built-in `expect()` API

**Run Commands:**
```bash
npx vitest run                              # Full suite (CI mode)
npx vitest                                  # Watch mode (local)
npm run test:coverage                       # Coverage report with thresholds
npm run test:fast                           # Fast subset (excludes heavy tests)
npm run test:integration                    # Integration tests only
npx vitest run src/path/to/specific.test.ts # Single test file
```

**Coverage requirements** (enforced):
- Statements: 80%
- Branches: 78%
- Functions: 90%
- Lines: 80%

Coverage reports: `coverage/` directory (text, json, lcov formats)

## Test File Organization

**Location:**
- Co-located with source: tests live in same directory as code
- Example: `src/daemon/retention.ts` has test at `src/daemon/retention.test.ts`

**Naming:**
- Unit tests: `*.test.ts`
- Integration tests: `*.integration.test.ts`
- Full-suite tests (slow, heavy): `*-full.test.ts`
- Fast config excludes heavy tests — see `vitest.fast.config.ts` for list

**File discovery:**
- Included: `**/*.test.ts` (main config)
- Excluded: `**/*.integration.test.ts`, `**/*-full.test.ts`, `entry.ts`

## Test Structure

**Suite organization:**
Test files use `describe()` for logical grouping, often one `describe` per function being tested.

From `infrastructure/runtime/src/daemon/retention.test.ts`:
```typescript
describe("runRetention", () => {
  it("calls all store methods with correct args", () => {
    const store = makeStore();
    const privacy = makePrivacy();
    runRetention(store, privacy);

    expect(store.purgeDistilledMessages).toHaveBeenCalledWith(90);
    expect(store.purgeArchivedSessionMessages).toHaveBeenCalledWith(180);
    expect(store.truncateToolResults).toHaveBeenCalledWith(500);
    expect(store.deleteEphemeralSessions).toHaveBeenCalled();
  });

  it("returns counts from store methods", () => {
    const store = makeStore({ distilled: 10, archived: 20, truncated: 5, ephemeral: 3 });
    const result = runRetention(store, makePrivacy());

    expect(result.distilledMessagesDeleted).toBe(10);
    expect(result.archivedMessagesDeleted).toBe(20);
    expect(result.toolResultsTruncated).toBe(5);
    expect(result.ephemeralSessionsDeleted).toBe(3);
  });

  it("isolates failures — one store method throwing does not block others", () => {
    const store = makeStore();
    (store.purgeDistilledMessages as ReturnType<typeof vi.fn>).mockImplementation(() => {
      throw new Error("disk full");
    });

    const result = runRetention(store, makePrivacy());
    expect(result.distilledMessagesDeleted).toBe(0);
    expect(result.archivedMessagesDeleted).toBe(5);
  });
});
```

**Patterns:**
- One logical test per `it()` block
- Descriptive test names with context: "calls all store methods with correct args", "isolates failures"
- Arrange-act-assert: setup (create mocks), execute (call function), verify (expect assertions)
- Each test is independent; no shared state between tests

## Mocking

**Framework:** vitest's `vi` module

**Patterns from test files:**

**Basic mock factory** (co-located in test file):
```typescript
function makeStore(returns?: Partial<Record<string, number>>) {
  return {
    purgeDistilledMessages: vi.fn().mockReturnValue(returns?.distilled ?? 3),
    purgeArchivedSessionMessages: vi.fn().mockReturnValue(returns?.archived ?? 5),
    truncateToolResults: vi.fn().mockReturnValue(returns?.truncated ?? 2),
    deleteEphemeralSessions: vi.fn().mockReturnValue(returns?.ephemeral ?? 1),
  } as unknown as SessionStore;
}
```

**Mock with resolved promise:**
```typescript
function mockRouter(responseText: string) {
  return {
    complete: vi.fn().mockResolvedValue({
      content: [{ type: "text", text: responseText }],
      stopReason: "end_turn",
      usage: { inputTokens: 100, outputTokens: 50, cacheReadTokens: 0, cacheWriteTokens: 0 },
      model: "test",
    }),
  } as never;
}
```

**Mock module (vi.mock)**:
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

**Implementation override:**
```typescript
const store = makeStore();
(store.purgeDistilledMessages as ReturnType<typeof vi.fn>).mockImplementation(() => {
  throw new Error("disk full");
});
```

## Fixtures and Factories

**Test data factories:**
Factories are inline in test files, not in separate fixtures directory. Named `make*`:

```typescript
function makePrivacy(overrides?: Partial<PrivacySettings["retention"]>): PrivacySettings {
  return {
    retention: {
      distilledMessageMaxAgeDays: 90,
      archivedSessionMaxAgeDays: 180,
      toolResultMaxChars: 500,
      ...overrides,
    },
    hardenFilePermissions: true,
    pii: {},
  } as PrivacySettings;
}

function makeConfig() {
  return {
    gateway: { port: 18789, auth: { mode: "none" as const, token: undefined } },
    agents: {
      list: [
        { id: "syn", name: "Syn", model: "claude-sonnet", workspace: "/tmp/syn" },
      ],
      default: "syn",
    },
    cron: { jobs: [] },
    signal: { accounts: [] },
  } as never;
}
```

**Location:**
- Factories defined at top of test file, before tests
- Reusable across multiple test cases
- Support optional overrides for test-specific values

**Pattern:**
- Factory returns complete, valid test data
- Overrides allow customization without factory modification
- Cast to `never` or `as unknown as Type` when mocking complex interfaces

## Coverage

**Requirements:** 80% statements, 78% branches, 90% functions, 80% lines

**View coverage:**
```bash
npm run test:coverage
# Output: text report + coverage/coverage-final.json + coverage/lcov.info
```

**Thresholds enforced** (vitest config):
```javascript
coverage: {
  provider: "v8",
  thresholds: {
    statements: 80,
    branches: 78,
    functions: 90,
    lines: 80,
  },
}
```

**Excluded from coverage:**
- `**/*.test.ts` — test files themselves
- `**/*.integration.test.ts` — integration tests
- `entry.ts` — main entry point

## Test Types

**Unit Tests:**
- Scope: Single function or small module
- Test behavior, not implementation details
- Mock dependencies, not internal logic
- Example: `src/daemon/retention.test.ts` tests `runRetention()` in isolation with mocked `SessionStore`

**Integration Tests:**
- Naming: `*.integration.test.ts`
- Run separately: `npm run test:integration`
- Can use real dependencies (databases, file system)
- Example: `src/mneme/store.test.ts` (creates real SQLite DB)

**E2E Tests:**
- Not used for unit/integration coverage
- Some playwright-based tests may exist for UI testing
- Not part of standard test suite

## Common Patterns

**Async testing:**
```typescript
it("parses JSON from LLM response", async () => {
  const router = mockRouter(`json response`);
  const result = await extractFromMessages(router, [
    { role: "user", content: "hello" },
  ], "test-model");

  expect(result.facts).toEqual(["extracted fact"]);
});
```

Uses `async/await` directly; vitest awaits promise automatically.

**Error testing:**
```typescript
it("returns empty arrays on malformed response", async () => {
  const router = mockRouter("I couldn't parse that, sorry.");
  const result = await extractFromMessages(router, [
    { role: "user", content: "hello" },
  ], "test-model");

  expect(result.facts).toEqual([]);
  expect(result.decisions).toEqual([]);
  expect(result.openItems).toEqual([]);
});
```

Tests graceful degradation (no exception thrown, empty fallback returned).

**Isolation with error handling:**
```typescript
it("isolates failures — one store method throwing does not block others", () => {
  const store = makeStore();
  (store.purgeDistilledMessages as ReturnType<typeof vi.fn>).mockImplementation(() => {
    throw new Error("disk full");
  });

  const result = runRetention(store, makePrivacy());

  expect(result.distilledMessagesDeleted).toBe(0);
  expect(result.archivedMessagesDeleted).toBe(5);
  expect(result.toolResultsTruncated).toBe(2);
  expect(result.ephemeralSessionsDeleted).toBe(1);
});
```

Verifies that one failure doesn't cascade; other operations complete successfully.

**Multiple assertions per behavior** (not typical per-test rule, but sometimes needed):
```typescript
it("calls all store methods with correct args", () => {
  const store = makeStore();
  runRetention(store, makePrivacy());

  expect(store.purgeDistilledMessages).toHaveBeenCalledWith(90);
  expect(store.purgeArchivedSessionMessages).toHaveBeenCalledWith(180);
  expect(store.truncateToolResults).toHaveBeenCalledWith(500);
  expect(store.deleteEphemeralSessions).toHaveBeenCalled();
});
```

Multiple assertions allowed when testing a single behavior (e.g., method call contract).

## Test Behavior Not Implementation

**Good test (behavior):**
```typescript
it("returns counts from store methods", () => {
  const store = makeStore({ distilled: 10, archived: 20, truncated: 5, ephemeral: 3 });
  const result = runRetention(store, makePrivacy());

  expect(result.distilledMessagesDeleted).toBe(10);
  expect(result.archivedMessagesDeleted).toBe(20);
});
```

Tests: Given certain store responses, retention returns correct counts.

**Bad test (implementation detail):**
```typescript
it("calls purgeDistilledMessages first", () => {
  const store = makeStore();
  let order = [];
  store.purgeDistilledMessages.mockImplementation(() => {
    order.push("distilled");
  });
  // ... rest of setup
  expect(order[0]).toBe("distilled");
});
```

Tests order of operations (implementation), not the contract.

## Local Development Guidance

**For agent/developer use:**
- Run `npm run test:fast` for local development — excludes heavy tests like store (real SQLite) and full-suite manager tests
- Never run `npm test` locally unless debugging a CI failure; full suite takes 84+ seconds and frequently times out
- Pre-commit hook (`npm run precommit`) runs typecheck, lint, and fast tests
- Use `npx vitest run src/path/to/specific.test.ts` for targeted testing of specific functionality

**Large test files to avoid locally:**
- `src/mneme/store.test.ts` — real SQLite setup
- `src/nous/manager-streaming.test.ts` — full streaming pipeline
- `src/pylon/server-full.test.ts` — complete server lifecycle
- Any file matching `*-full.test.ts`

---

*Testing analysis: 2026-02-23*

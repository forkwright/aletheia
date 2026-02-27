# Spec 40: Testing Strategy — Coverage Targets, Integration Patterns, Contract Tests

**Status:** Draft
**Origin:** Issue #297
**Module:** Cross-cutting

---

## Problem

Testing exists (vitest for runtime, pytest for sidecar) but there is no documented strategy: no coverage targets, no integration test patterns, no definition of what a "complete" test suite looks like. Agents writing tests have no guidance on what to test or how much is enough.

## Current State

| Layer | Framework | Coverage | Integration | E2E |
|-------|-----------|----------|-------------|-----|
| Runtime (TypeScript) | vitest | ~65% lines (est) | Some (CI job) | None |
| UI (Svelte) | vitest + @testing-library | ~20% (est) | None | None |
| Memory sidecar (Python) | pytest | ~40% (est) | None | None |
| TUI (Rust) | cargo test | ~0% | None | None |

## Coverage Targets (CI-enforced)

| Layer | Line Coverage | Branch Coverage |
|-------|--------------|-----------------|
| Runtime (core modules) | ≥ 80% | ≥ 70% |
| Runtime (overall) | ≥ 70% | ≥ 60% |
| Memory sidecar | ≥ 75% | — |
| UI | ≥ 50% | — |

Core modules: koina, mneme, hermeneus, organon, symbolon.

## Unit Test Patterns

Test behavior, not implementation:

```typescript
// Good: tests observable behavior
it("rejects expired tokens", async () => {
  const token = createExpiredToken();
  const result = await auth.verify(token);
  expect(result.ok).toBe(false);
  expect(result.error.code).toBe("TOKEN_EXPIRED");
});
```

## Integration Test Patterns

- Contract tests between runtime ↔ sidecar API
- Session lifecycle tests (create → message → distill → resume)
- Tool execution round-trip tests

## Phases

1. Baseline measurement: actual coverage numbers across all layers
2. CI enforcement: coverage gates in GitHub Actions
3. Integration test harness: shared fixtures, test containers
4. Contract tests: runtime ↔ sidecar API boundary
5. E2E smoke tests: session lifecycle through API

## Open Questions

- Mutation testing (Stryker) — see #280, worth the CI time?
- Snapshot testing for UI components vs. behavioral tests
- Test data management strategy

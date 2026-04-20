# dokimion (eval)

**Purpose:** Behavioral eval framework: scenario-based HTTP testing against a live Aletheia instance, including cognitive evals for recall, sycophancy, and adversarial prompts.

## Key types

| Type | Purpose |
|------|---------|
| `Scenario` | Trait: `meta()` + `run(client) -> Result<()>` |
| `ScenarioRunner` | Orchestrates scenario execution with filtering and timeouts |
| `RunConfig` | Base URL, token, filter, fail-fast, timeout, JSON output |
| `RunReport` | Aggregated pass/fail/skip counts and per-scenario results |
| `EvalClient` | HTTP client for health, nous, session, knowledge API calls |

## Public API surface

- `dokimion::scenario` - `Scenario` trait, `ScenarioMeta`, `ScenarioOutcome`
- `dokimion::runner` - `ScenarioRunner`, `RunConfig`, `RunReport`
- `dokimion::client` - `EvalClient` for live instance API calls

## When to look here

- When adding new behavioral eval scenarios (implement `Scenario` trait, register in scenario registry)
- When extending cognitive eval coverage (recall, sycophancy, adversarial)

# dokimion

**Purpose:** Behavioral eval framework - scenario-based API testing against a live Aletheia instance.

## Key types

| Type | Purpose |
|------|---------|
| `RunConfig` | Configuration for a scenario run (base URL, token, filter, timeout, JSON output) |
| `RunReport` | Aggregated pass/fail/skip counts and per-scenario results |
| `ScenarioRunner` | Runs behavioral scenarios against a live Aletheia instance |
| `Scenario` | Trait implemented by each behavioral scenario |
| `ScenarioMeta` | Metadata (id, description, category, auth/nous requirements) |
| `ScenarioOutcome` | Pass / fail / skip result for a single scenario |
| `ScenarioResult` | Pair of `ScenarioMeta` and `ScenarioOutcome` |
| `EvalProvider` | Trait for scenario sources; `BuiltinProvider` and `CompositeProvider` implement it |
| `EvalClient` | HTTP client for talking to the target instance during evaluation |

## Public API surface

- `dokimion::runner` - `RunConfig`, `RunReport`, `ScenarioRunner`
- `dokimion::scenario` - `Scenario`, `ScenarioMeta`, `ScenarioOutcome`, `ScenarioResult`
- `dokimion::provider` - `EvalProvider`, `BuiltinProvider`, `CompositeProvider`
- `dokimion::client` - `EvalClient` (re-exported from `benchmarks`)
- `dokimion::report` - `print_report`, `print_report_json`, `emit_eval_report`
- `dokimion::persistence` - JSONL training-data output helpers
- `dokimion::benchmarks` - LongMemEval / LoCoMo dataset loaders and baselines
- `dokimion::error` - eval-specific `Error` and `Result`

## When to look here

- When work touches `crates/eval` or downstream imports from `dokimion`.
- For exact signatures, load `_llm/L3-api-index/dokimion.md` if present, then source.

## Recent changes

Scenario runner moved from benchmark-centric `RunnerConfig` / `BenchmarkRunnerConfig` to a generic `ScenarioRunner` / `RunConfig` model. Scenario filtering now supports `category_filter` in addition to substring filtering.

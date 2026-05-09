# dokimion

**Purpose:** Behavioral eval framework - scenario-based API testing against a live instance.

## Key types

| Type | Purpose |
|------|---------|
| `BenchmarkRunnerConfig` | Current public type or boundary; see L3/source for exact fields |
| `BenchmarkDatasetConfig` | Current public type or boundary; see L3/source for exact fields |
| `RunnerConfig` | Current public type or boundary; see L3/source for exact fields |
| `ScenarioOutcome` | Current public type or boundary; see L3/source for exact fields |

## Public API surface

- `dokimion::benchmarks/baselines` - public items from `src/benchmarks/baselines.rs`
- `dokimion::benchmarks/judge` - public items from `src/benchmarks/judge.rs`
- `dokimion::benchmarks/locomo` - public items from `src/benchmarks/locomo.rs`
- `dokimion::benchmarks/longmemeval` - public items from `src/benchmarks/longmemeval.rs`
- `dokimion::benchmarks/metrics` - public items from `src/benchmarks/metrics.rs`

## When to look here

- When work touches `crates/eval` or downstream imports from `dokimion`.
- For exact signatures, load `_llm/L3-api-index/dokimion.md` if present, then source.

## Recent changes

RecallBenchmarkScenario was decoupled and scenario config now carries question_timeout, ISO-8601 helpers, and TriggerConfig.

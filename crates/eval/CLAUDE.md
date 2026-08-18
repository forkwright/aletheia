# eval (dokimion)

## At a glance

Behavioral eval framework for scenario-based API testing. Depends on koina. Entry point: `src/lib.rs` (Scenario, ScenarioRunner).

## Depth

Behavioral eval framework: scenario-based API testing against a live Aletheia instance. 4.9K lines.

## Read first

1. `src/scenario.rs`: Scenario trait, ScenarioMeta, ScenarioOutcome
2. `src/runner.rs`: ScenarioRunner orchestration, RunConfig, RunReport
3. `src/scenarios/mod.rs`: Scenario registry (health, auth, nous, session, conversation)
4. `src/cognitive/mod.rs`: Cognitive evals (recall, sycophancy, adversarial, self-assessment)
5. `src/client.rs`: EvalClient HTTP wrapper for Aletheia API

## Key types

| Type | Path | Purpose |
|------|------|---------|
| `Scenario` | `scenario.rs` | Trait: `meta()` + `run(client) -> Result<()>` |
| `ScenarioMeta` | `scenario.rs` | ID, description, category, auth/nous requirements |
| `ScenarioOutcome` | `scenario.rs` | Enum: Passed, Failed, Skipped |
| `ScenarioRunner` | `runner.rs` | Orchestrates scenario execution with filtering and timeouts |
| `RunConfig` | `runner.rs` | Base URL, token, filter, fail-fast, timeout, JSON output, model override |
| `RunReport` | `runner.rs` | Aggregated pass/fail/skip counts and per-scenario results |
| `EvalClient` | `client.rs` | HTTP client for health, nous, session, knowledge API calls |
| `EvalRecord` | `persistence.rs` | JSONL record for training data persistence |
| `TriggerConfig` | `triggers.rs` | Configurable scheduling for eval triggers |
| `ParsedSseEvent` | `sse.rs` | Parsed SSE stream event for real-time eval output |

## Scenario categories

| Category | Module | Tests |
|----------|--------|-------|
| Health | `scenarios/health.rs` | Liveness, readiness checks |
| Auth | `scenarios/auth.rs` | Token validation, unauthorized access |
| Nous | `scenarios/nous.rs` | Agent listing, status |
| Session | `scenarios/session.rs` | Session lifecycle CRUD |
| Conversation | `scenarios/conversation.rs` | Message send, SSE streaming |
| Cognitive | `cognitive/` | Recall@k, sycophancy, adversarial, self-assessment |

## Patterns

- **Scenario trait**: each scenario defines metadata and an async run method against EvalClient.
- **Filter execution**: `RunConfig.filter` substring-matches scenario IDs.
- **Skip logic**: scenarios auto-skip when auth token or nous agent is unavailable.
- **Colored output**: `owo-colors` + `supports-color` for terminal report formatting.
- **Two retrieval-quality measures, not one.** `cognitive/recall.rs`'s
  `recall-at-k-benchmark` scenario is smoke-only whenever the operator has
  not set `ALETHEIA_RECALL_RELEVANT_IDS` — it falls back to synthetic
  document IDs and a fixed query, and its `ScenarioClassification` is
  `Smoke` in that state so it can never masquerade as an assertive result.
  The comparable ground-truth measure is the `benchmarks/` module (LoCoMo /
  LongMemEval): `BenchmarkRunner` scores each question's retrieval against
  the dataset's own parsed evidence refs (`recall_at_k`, `ndcg_at_k`,
  `mrr_at_k`, `hallucination_rate` on `QuestionResult`), falling back to
  normalized-content-hash matching only when a dataset carries no evidence
  refs at all.

## Eval vs. unit/integration tests

- **Unit tests** (`#[cfg(test)]` inline, or `crates/eval/src/**/*_tests.rs`): pure
  logic with no live instance -- scoring math, provenance construction,
  manifest parsing. No network, no `EvalClient`.
- **`crates/integration-tests/tests/eval_harness.rs`**: exercises the eval
  harness itself (scenario registry, runner orchestration, report/coverage
  output) against a real `aletheia` instance driven by
  `hermeneus::test_utils::MockProvider` -- proves the harness works, not
  that the model behaves well. `canary-*` categories are excluded from a
  full harness run there because they exercise a real LLM and would fail
  against the mock.
- **`aletheia eval` (this crate's scenarios, `src/scenarios/`)**: behavioral
  checks against a live instance. Health/auth/session-shape scenarios run
  fine against `MockProvider`; `canary-*` and the cognitive evals need a
  real provider to mean anything. This is where "did this agent behavior
  improve, regress, or get more expensive" questions live -- not in
  `cargo test`.

Put a check in `cargo test` when it verifies the harness's own code. Put it
in an eval scenario when it verifies agent or model behavior against a
running instance.

## Common tasks

| Task | Where |
|------|-------|
| Add behavioral scenario | New file in `src/scenarios/`, implement Scenario trait, register in `scenarios/mod.rs` |
| Add cognitive eval | New file in `src/cognitive/`, register in `cognitive/mod.rs` |
| Add API client method | `src/client.rs` (EvalClient impl) |
| Modify report output | `src/report.rs` (print_report function) |
| Add eval trigger type | `src/triggers.rs` (TriggerSchedule enum) |

## Dependencies

Uses: koina, reqwest, serde_json, tokio, snafu, owo-colors
Used by: integration-tests, aletheia (binary)

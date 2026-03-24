# eval (dokimion)

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
| `RunConfig` | `runner.rs` | Base URL, token, filter, fail-fast, timeout, JSON output |
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

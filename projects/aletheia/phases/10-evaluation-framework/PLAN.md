# Phase 10: Evaluation framework

## Goal
Behavioral evaluation system with scenario-based API testing against live instances.

## Success criteria
- Eval runner executes 100 scenarios in under 5 minutes
- Scenario definitions are declarative YAML with request/response matching
- Regression detection flags any metric drop > 2pp from baseline
- Eval results produce a structured report with pass/fail counts

## Falsification

| Criterion | Falsifier |
|-----------|-----------|
| Eval runner executes 100 scenarios in under 5 minutes | Benchmark shows 100 scenarios take >= 5 minutes |
| Scenario definitions are declarative YAML with request/response matching | YAML parser rejects valid scenario or accepts invalid one |
| Regression detection flags any metric drop > 2pp from baseline | Synthetic 3pp regression is not flagged |
| Eval results produce a structured report with pass/fail counts | Report JSON is missing count fields or has incorrect totals |

## Scope

### In scope
- dokimion crate: scenario runner, report generation
- Cognitive evals (memory recall, reasoning quality)
- Benchmark infrastructure (LongMemEval, LoCoMo)

### Out of scope
- Automated model selection via eval scores
- A/B testing framework

## Requirements
- REQ-01: Scenarios can target any HTTP endpoint or internal function
- REQ-02: Response matching supports JSON path, regex, and exact string
- REQ-03: Baseline is stored per-branch and updated on merge
- REQ-04: Reports are emitted as JSON and human-readable markdown

## Decisions

| Decision | Choice | Rationale |
|----------|--------|-----------|
| Scenario format | YAML over TOML | Better multiline string support for prompts |
| Runner | Async tokio over synchronous | Matches production runtime |

## Open questions
- Should evals run in CI or only locally? (Resolved: both, with lighter suite in CI)

## Dependencies
- Phase 09 complete
- Live instance or test fixture

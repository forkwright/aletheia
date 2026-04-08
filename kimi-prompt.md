# Task: Implement canary prompt suite for dokimion

## Context

Aletheia issue #2294 (W-12). The dokimion eval framework now has an `EvalProvider` trait (PR #2567) that allows pluggable scenario sources. This task creates a `CanaryProvider` with 20-30 representative prompts for regression testing dispatch quality.

## Standards

Read the AGENTS.md file in the repo root for project standards. Skip the Setup section.

## Background

The eval framework lives in `crates/eval/`. Key files:
- `src/provider.rs` — `EvalProvider` trait with `provide()` and `name()` methods
- `src/scenario.rs` — `Scenario` trait with `meta()` and `run()` methods
- `src/scenarios/` — built-in scenarios (health, auth, nous, session, conversation)

## What to Build

### 1. Canary scenarios module (`crates/eval/src/scenarios/canary.rs`)

Define 20-30 canary prompt scenarios covering major capability axes:

**Recall (5 scenarios):**
- Insert fact → query it back → verify exact match
- Insert 3 related facts → search by semantic query → verify recall
- Insert fact with high confidence → insert contradicting fact → verify conflict detection
- Insert temporal facts → query by time range → verify ordering
- Search with empty knowledge → verify graceful empty result

**Tool use (5 scenarios):**
- File read tool → verify content returned
- File write → read back → verify roundtrip
- Web search tool → verify structured results
- Multi-tool chain (read → transform → write) → verify end state
- Tool with invalid input → verify error handling

**Session lifecycle (5 scenarios):**
- Create session → send message → get history → verify consistency
- Multi-turn conversation → verify context preservation
- Session close → reopen → verify state restored
- Concurrent messages → verify ordering
- Session with large context → verify distillation triggers

**Knowledge extraction (5 scenarios):**
- Send technical description → verify fact extraction
- Send contradiction → verify conflict flagged
- Send update to existing knowledge → verify revision
- Send ambiguous statement → verify low confidence
- Send meta-knowledge (about the system) → verify categorization

**Conflict resolution (3-5 scenarios):**
- Present two valid approaches → verify balanced analysis
- Present clear error → verify direct correction
- Present request outside scope → verify boundary acknowledgment

### 2. Canary provider (`crates/eval/src/scenarios/canary.rs`)

```rust
pub struct CanaryProvider;

impl EvalProvider for CanaryProvider {
    fn provide(&self) -> Vec<Box<dyn Scenario>> { /* ... */ }
    fn name(&self) -> &str { "canary" }
}

pub fn canary_scenarios() -> Vec<Box<dyn Scenario>> { /* ... */ }
```

### 3. Registration

Add `canary_scenarios()` to the `all_scenarios()` function in `crates/eval/src/scenarios/mod.rs`.

## Constraints

- Only modify code in `crates/eval/`
- Each scenario implements the `Scenario` trait manually (no macro needed — it was removed)
- Use `tracing::info_span!` on each scenario's run method
- Categories should be: "canary-recall", "canary-tool", "canary-session", "canary-knowledge", "canary-conflict"
- Scenario IDs should be descriptive: e.g., "canary-recall-insert-query-roundtrip"

## Validation Gate

```bash
cargo check -p aletheia-dokimion
cargo test -p aletheia-dokimion
```

## Completion

1. `git add -A`
2. `git commit -m "feat(dokimion): canary prompt suite with 25 regression scenarios"`
3. `git push origin feat/canary-suite`
4. `gh pr create --title "feat(dokimion): canary prompt suite (W-12)" --body "25 canary scenarios across 5 capability axes: recall, tool use, session lifecycle, knowledge extraction, conflict resolution. Uses EvalProvider trait. Closes #2294"`

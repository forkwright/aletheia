# Orchestration Standards

> Generated from `crates/basanos/src/registry/`. Do not edit generated views by hand.

## Contents

- [DISPATCH/acceptance-verifier](#dispatchacceptance-verifier)
- [DISPATCH/blast-zone-declared](#dispatchblast-zone-declared)
- [DISPATCH/no-time-estimates](#dispatchno-time-estimates)
- [HANDOFF/structured-over-prose](#handoffstructured-over-prose)
- [ORCHESTRATION/bare-forge-ref](#orchestrationbare-forge-ref)
- [ORCHESTRATION/dispatch-anti-pattern](#orchestrationdispatch-anti-pattern)
- [ORCHESTRATION/issues-go-to-forge](#orchestrationissues-go-to-forge)
- [PLAN/preamble-required](#planpreamble-required)

## `DISPATCH/acceptance-verifier` {#dispatchacceptance-verifier}

- Severity: `error`
- Scope: `project:dispatch-prompts`
- Enforcer: `DISPATCH/acceptance-verifier`

Every dispatch prompt must include a machine-checkable acceptance verifier — a heading (`## Acceptance verifier`, `## Verifier probe`, or equivalent) followed by a fenced code block containing a shell one-liner or MCP tool invocation whose exit-zero truth signals task complete. Prose acceptance criteria without an executable probe do not satisfy this rule.

### Examples

**Good:** Include an acceptance verifier section with a fenced probe block.

```text
## Acceptance verifier
```bash
cargo check -p basanos 2>&1 | grep -q Finished
```
```

**Bad:** Include only prose acceptance criteria without an executable probe.

```text
## Acceptance criteria
- [ ] Tests pass
- [ ] No clippy warnings
```

## `DISPATCH/blast-zone-declared` {#dispatchblast-zone-declared}

- Severity: `error`
- Scope: `project:dispatch-prompts`
- Enforcer: `DISPATCH/blast-zone-declared`

Every dispatch prompt must declare its blast zone — the set of files or directories the dispatched agent is authorized to modify. Without an explicit blast zone, parallel dispatches against the same crate cause merge conflicts or divergent edits. Declare via a `## Blast zone` heading or a `blast_zone:` YAML-frontmatter key.

### Examples

**Good:** Declare the blast zone as a heading with path entries.

```text
## Blast zone
- crates/basanos/src/rules/orchestration/
```

**Bad:** Omit the blast zone declaration entirely.

```text
## Goals
Refactor the orchestration lint rules.
```

## `DISPATCH/no-time-estimates` {#dispatchno-time-estimates}

- Severity: `error`
- Scope: `project:dispatch-prompts`
- Enforcer: `DISPATCH/no-time-estimates`

Dispatch prompts and plan files must not include time estimates for development work. Estimates for software work are inaccurate and erode trust when the real duration diverges. Describe scope structurally (dependencies, blast zone size, number of crates) rather than temporally.

### Examples

**Good:** Describe scope structurally rather than temporally.

```text
Touches 3 crates; depends on kanon#148 (dispatch registry).
```

**Bad:** Include a numeric time estimate in the prompt.

```text
This should take about 2 days to complete.
```

## `HANDOFF/structured-over-prose` {#handoffstructured-over-prose}

- Severity: `warning`
- Scope: `project:orchestration-artifacts`
- Enforcer: `HANDOFF/structured-over-prose`

Cross-agent handoff documents should use machine-readable JSONL instead of markdown prose where possible. Prose handoff summaries are lossy and non-queryable. JSONL handoff files belong at `/tmp/kanon-orchestration/handoff-<ISO8601>.jsonl`.

### Examples

**Good:** Record handoff events as JSONL at the canonical path.

```text
{"event":"merged","pr_number":201,"branch":"lane/feat-foo"}
```

**Bad:** Write a markdown prose summary as the handoff document.

```text
# Handoff
I merged PRs #201 and #202 and cleaned up stale worktrees.
```

## `ORCHESTRATION/bare-forge-ref` {#orchestrationbare-forge-ref}

- Severity: `warning`
- Scope: `project:planning-docs`
- Enforcer: `ORCHESTRATION/bare-forge-ref`

Planning documents and project markdown must not cite bare `forge:owner/repo#N` references after the GitHub cutover. The forge identifier space is no longer live; unmapped references force every future triage session to re-derive the forge→GitHub mapping. Replace recovered refs with their GitHub issue number or annotate `[forge-only, not recovered]` when no mapping exists.

### Examples

**Good:** Reference a recovered issue by its GitHub number.

```text
See #265 for the peer-tenancy invariant.
```

**Bad:** Leave a bare forge reference in a planning document.

```text
`forge:forkwright/kanon#411`
```

## `ORCHESTRATION/dispatch-anti-pattern` {#orchestrationdispatch-anti-pattern}

- Severity: `error`
- Scope: `project:dispatch-diffs`
- Enforcer: `ORCHESTRATION/dispatch-anti-pattern`

Dispatch reviews must block high-risk Rust additions before merge. Added lines must not introduce panic shortcuts, blanket lint suppressions, console debug output, or placeholder macros that bypass normal review discipline.

### Examples

**Good:** Propagate errors with context in added Rust dispatch code.

```text
let payload = parse_payload(input)?;
```

**Bad:** Add a panic-prone shortcut to dispatch-reviewed Rust code.

```text
let payload = parse_payload(input).unwrap();
```

## `ORCHESTRATION/issues-go-to-forge` {#orchestrationissues-go-to-forge}

- Severity: `warning`
- Scope: `project:dispatch-prompts`
- Enforcer: `ORCHESTRATION/issues-go-to-forge`

Dispatch prompts and templates must create tracker issues through the forge-native MCP entry point. Shelling out to `gh issue create`, `gh issue open`, or `gh issue new` bypasses repository routing and can recreate tracker drift.

### Examples

**Good:** Create issues through the forge-native MCP entry point.

```text
mcp__kanon__issue_create(title: "Backfill registry rule", body: "...")
```

**Bad:** Shell out to GitHub issue commands from dispatch prompts.

```text
gh issue create --title "Backfill registry rule" --body-file issue.md
```

## `PLAN/preamble-required` {#planpreamble-required}

- Severity: `warning`
- Scope: `project:plan-files`
- Enforcer: `PLAN/preamble-required`

Plan files (`PLAN.md` in phase directories, files under `plans/`) must carry a YAML preamble with `plan_id`, `status`, and `stages` fields. The preamble makes plans machine-readable by `kanon plan {status,verify}`, which runs per-stage verification commands. Narrative-only plans accumulate silently and cannot be tooling-audited.

### Examples

**Good:** Open PLAN.md with a YAML preamble carrying plan_id, status, and stages.

```text
---
plan_id: phase-14-coherence
status: executing
stages:
  S1: { name: registry, status: pending, verify: "kanon dispatch list --json | jq 'length >= 0'" }
---
```

**Bad:** Write a PLAN.md with only narrative prose and no preamble.

```text
# Phase 14 Plan

Goals: improve orchestration lint coverage.
```


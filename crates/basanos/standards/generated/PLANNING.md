# Planning Standards

> Generated from `crates/basanos/src/registry/`. Do not edit generated views by hand.

## Contents

- [PLANNING/derive-live-facts](#planningderive-live-facts)
- [PLANNING/falsification-criterion](#planningfalsification-criterion)

## `PLANNING/derive-live-facts` {#planningderive-live-facts}

- Severity: `warning`
- Scope: `universal`
- See also: `WRITING/derivable-count`

Planning documents must not embed facts that are cheaply derivable from live sources: test counts, PR counts, issue totals, CI status, queue depths. These values drift immediately after commit. Either reference the live source or omit the number. Planning docs manually maintain only judgment: decisions, rationale, intent, scope, blockers, and next-step handoff notes.

### Examples

**Good:** Reference the live source for counts instead of embedding a frozen number.

```text
Current test count: see `cargo nextest run --list | wc -l`
```

**Bad:** Embed a test count that will drift as the codebase evolves.

```text
Current test count: 842 (as of 2026-06-01)
```

## `PLANNING/falsification-criterion` {#planningfalsification-criterion}

- Severity: `warning`
- Scope: `universal`
- See also: `WRITING/stale-date`

Every phase plan success criterion must include a falsification clause: what observation would prove the criterion wrong or revert the work? Unfalsifiable success criteria ("system is fast and reliable") cannot be measured and cannot drive a decision to ship or revert. The falsifier is the thing that forces honest measurement.

### Examples

**Good:** Include a measurable claim and the observation that would prove it wrong.

```text
Success criterion: P99 query latency under 50 ms with 10k concurrent connections.
Falsifier: a load test showing P99 >= 50 ms at that concurrency.
```

**Bad:** State a success criterion with no way to falsify it.

```text
Success criterion: the system is fast and reliable.
```


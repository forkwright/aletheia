# Operations Standards

> Generated from `crates/basanos/src/registry/`. Do not edit generated views by hand.

## Contents

- [OPERATIONS/runbook-required](#operationsrunbook-required)

## `OPERATIONS/runbook-required` {#operationsrunbook-required}

- Severity: `warning`
- Scope: `universal`
- See also: `VERIFICATION/claim-requires-fixture`

Every production alerting rule must link to a runbook via a runbook_url annotation. A runbook must describe: what the alert means, how to diagnose the root cause, and what steps to take to resolve it. An alert without a runbook forces responders to improvise under pressure, which increases MTTR.

### Examples

**Good:** Attach a runbook to every alerting rule, reachable from the alert annotation.

```text
# Prometheus alerting rule
annotations:
  runbook_url: https://wiki.internal/runbooks/kanon-oom
```

**Bad:** Ship an alert with no runbook link, leaving responders to improvise.

```text
# Prometheus alerting rule — no runbook_url annotation
```


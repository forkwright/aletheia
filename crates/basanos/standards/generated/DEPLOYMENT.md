# Deployment Standards

> Generated from `crates/basanos/src/registry/`. Do not edit generated views by hand.

## Contents

- [DEPLOYMENT/single-merge-gate](#deploymentsingle-merge-gate)

## `DEPLOYMENT/single-merge-gate` {#deploymentsingle-merge-gate}

- Severity: `error`
- Scope: `universal`
- See also: `CI/no-op-command`

Every PR must pass all automated checks before merge. No exceptions, no manual overrides, no --admin bypasses. The gate is the contract between the author and the fleet: a green gate means the change is safe to ship. Bypassing it creates invisible risk that the fleet has no way to detect.

### Examples

**Good:** Require all automated checks to pass before any PR can merge.

```text
# .kanon-ci.toml
[gate]
require_all = true
# PR merge blocked until lint, tests, and security scan pass
```

**Bad:** Allow manual override to merge a PR that failed automated checks.

```text
git merge --admin origin/pr-123  # bypasses required status checks
```


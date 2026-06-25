# Behavioral Parameter Standards

> Generated from `crates/basanos/src/registry/`. Do not edit generated views by hand.

## Contents

- [PARAMETERS/safe-default-required](#parameterssafe-default-required)

## `PARAMETERS/safe-default-required` {#parameterssafe-default-required}

- Severity: `warning`
- Scope: `universal`
- See also: `ENVIRONMENT/fail-fast-on-invalid-config`

Every non-secret runtime-tunable parameter (thresholds, weights, timing, capacity limits, rollout percentages) must degrade to a known-good default when its parsed value is invalid, and must log the fallback at warn level with the observed value and the default chosen. Silent use of wrong values is worse than crashing.

### Examples

**Good:** Degrade to a known-good default and log a warning when a runtime parameter is invalid.

```text
let timeout = parse_timeout(raw).unwrap_or_else(|e| {
    tracing::warn!(raw, error = %e, fallback = DEFAULT_TIMEOUT_MS, "invalid timeout; using default");
    DEFAULT_TIMEOUT_MS
});
```

**Bad:** Silently use a zero or nonsensical value when a parameter parse fails.

```text
let timeout = raw.parse::<u64>().unwrap_or(0); // silent wrong value
```


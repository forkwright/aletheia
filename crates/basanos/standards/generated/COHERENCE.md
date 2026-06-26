# Coherence Standards

> Generated from `crates/basanos/src/registry/`. Do not edit generated views by hand.

## Contents

- [COHERENCE/dimensional-coherence](#coherencedimensional-coherence)
- [COHERENCE/single-source-of-truth](#coherencesingle-source-of-truth)

## `COHERENCE/dimensional-coherence` {#coherencedimensional-coherence}

- Severity: `warning`
- Scope: `universal`
- See also: `COHERENCE/single-source-of-truth`

A component must pass all four altitude tests: L1 literal (does it do what its name says?), L2 structural (does it fit the architecture?), L3 philosophical (does it embody system values?), L4 reflexive (does it exhibit the property it implements?). A component that passes only L1 is a label; a component that passes all four is a gnomon.

### Examples

**Good:** Name a component after what it does so all four altitudes pass.

```text
/// Storage layer that caches computed blast-radius graphs.
struct BlastRadiusCache { ... }
```

**Bad:** Name a component after an aspiration it does not exhibit.

```text
/// Resilient storage layer.
struct ResilientStore { /* no retry, no fallback, panics on error */ }
```

## `COHERENCE/single-source-of-truth` {#coherencesingle-source-of-truth}

- Severity: `warning`
- Scope: `universal`
- See also: `STANDARDS/duplicated-prose`, `COHERENCE/dimensional-coherence`

Every fact, rule, or definition lives in exactly one place. Duplicate definitions drift; the second copy becomes the wrong value as soon as the first changes. Extract shared constants, functions, and types to the lowest common ancestor and reference from there.

### Examples

**Good:** Define the timeout once and reference it from every consumer.

```text
// config.rs
pub const REQUEST_TIMEOUT_MS: u64 = 30_000;
// client.rs
use crate::config::REQUEST_TIMEOUT_MS;
Duration::from_millis(REQUEST_TIMEOUT_MS)
```

**Bad:** Hardcode the same timeout value in two different modules.

```text
// client.rs
Duration::from_millis(30_000)
// retry.rs
Duration::from_millis(30_000) // must match client.rs
```


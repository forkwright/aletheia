# Documentation Standards

> Additive to README.md. Read that first. Everything here covers the comment system and doc comment rules.

---

## Zero-Comment Default

Most code should have zero inline comments. Self-documenting names and clear structure are the standard. Inline comments exist only for genuinely non-obvious *why* explanations.

Never include:
- Creation dates, author attributions, changelog entries
- AI generation indicators
- "Upgraded from X" or migration notes
- Comments restating what the code does

## Structured Comment Tags

When a comment is warranted, use exactly one of these prefixes. No freeform comments outside this system.

| Tag | Purpose | Issue required |
|-----|---------|:--------------:|
| `WHY:` | Non-obvious design decision. Explains rationale, not mechanism. | No |
| `WARNING:` | Fragile coupling, dangerous assumption, will-break-if. | No |
| `NOTE:` | Non-obvious context that doesn't fit other categories. | No |
| `PERF:` | Performance-critical path, deliberate optimization, or known bottleneck. | No |
| `SAFETY:` | Precedes unsafe or dangerous operations. Explains why invariants hold. | No |
| `INVARIANT:` | Documents a maintained invariant at a call site or type definition. | No |
| `TODO(#NNN):` | Planned work. Must reference a tracking issue. | **Yes** |
| `FIXME(#NNN):` | Known defect or temporary workaround. Must reference a tracking issue. | **Yes** |

Usage:
```
// WHY: Datalog engine returns results as JSON arrays, not named columns.
// Positional indexing is intentional and matches their wire format.

// WARNING: This timeout must exceed the LLM provider's own timeout,
// or we'll cancel requests that are still in-flight upstream.

// PERF: Pre-allocated buffer avoids per-turn heap allocation.
// Measured 3x throughput improvement in session replay benchmarks.

// SAFETY: The pointer is valid because we hold the arena lock and
// the allocation lifetime is tied to the arena's drop.

// INVARIANT: session.turns is always sorted by timestamp ascending.
// Callers depend on this for binary search in recall.

// TODO(#342): Replace linear scan with bloom filter after mneme v2.

// FIXME(#118): Workaround for upstream bug in serde_yml. Remove
// when we migrate to serde_yaml 0.9+.
```

## Banned Patterns

- Bare `// TODO` or `// FIXME` without an issue number — invisible debt
- `// HACK`, `// XXX`, `// TEMP` — use `FIXME(#NNN)` with a tracking issue
- `// NOTE:` as a substitute for clear code — rewrite the code first
- Commented-out code blocks — delete them, git has history
- End-of-line comments explaining what a line does — rename the variable instead

## Doc Comments

Doc comments (`///` in Rust, `/** */` in TS/Kotlin, `<summary>` in C#, docstrings in Python) apply to:

- Public API items that cross module boundaries
- Functions that can panic or throw unexpectedly (document when/why)
- Functions with non-obvious error conditions
- `unsafe` functions — mandatory safety contract documentation

Not required on:
- Private/internal functions with self-documenting signatures
- Test functions (the name IS the documentation)
- Trivial getters, builders, or standard trait implementations

One-line file headers (module-level doc comment) are encouraged: describe the module's purpose in a single sentence.

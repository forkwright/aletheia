# Error Handling Standards

> Additive to README.md. Read that first. Everything here covers error propagation, fail-fast policy, and resource lifecycle.

---

Every error must:

1. **Carry context** — what operation failed, with what inputs
2. **Be typed** — callers can match on error kind, not parse strings
3. **Propagate** — chain errors with context, never swallow the cause
4. **Surface** — log at the point of *handling*, not the point of *origin*

## Fail Fast

- Panic/crash on programmer bugs: violated invariants, impossible states, corrupted data
- Return errors for anything the caller could reasonably handle or report
- Prefer descriptive assertions over silent fallbacks: `expect("session must exist after authentication")` over bare `unwrap()`
- Never panic in library code for recoverable conditions

## No Silent Catch

- Every catch/except/match block must: log, propagate, return a meaningful value, or explain why it discards
- `/* intentional: reason */` for deliberate discard — never an empty catch body
- If you're catching to ignore, you're hiding a bug

## No Sentinel Values

- Do not return `null`/`None`/`-1`/empty string to signal failure
- Use the language's error type: `Result`, exceptions, `sealed class` error hierarchies
- Invalid data cannot exist past the point of construction

## Resource Lifecycle

Acquired resources must have a defined cleanup path. Use RAII (`Drop` in Rust), `defer` (Go), `with`/context managers (Python), `using` (C#), `use` (Kotlin). Never rely on garbage collection or finalizers for resource cleanup. Database connections, file handles, and sockets are released as soon as work completes.

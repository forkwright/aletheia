# Concurrency Standards

> Additive to README.md. Read that first. Everything here covers task ownership, shared state, and testing concurrent code.

---

## Ownership

Every spawned task, goroutine, thread, or async operation must have a defined owner responsible for its lifecycle. Fire-and-forget is banned — if you spawn it, something must join, cancel, or supervise it.

## Shared State

- **Prefer message passing** (channels, actors) over shared memory and locks
- When shared mutable state is necessary, synchronize all access. Document which lock guards which data.
- Prefer higher-level constructs (channels, executors, actors) over raw mutexes and atomics. Use atomics only for single counters or flags, not for coordinating state.
- Never hold a lock across an await point, an I/O operation, or a callback

## Thread Safety Contracts

Public types that may be used concurrently must declare their safety guarantee: immutable (always safe), thread-safe (synchronized internally), conditionally thread-safe (caller must synchronize), or not thread-safe (single-threaded only).

## Ordering

Never rely on execution order between concurrent units unless explicitly synchronized. Code that "works because the goroutine is always fast enough" is a race condition.

## Testing Concurrent Code

Concurrency bugs live in interleavings, not in text. Static analysis and code review catch a fraction. Use the tools:
- **Sanitizers:** TSan (C++, Rust via `-Z sanitizer=thread`), `go test -race`
- **Model checkers:** `loom` (Rust), `jcstress` (JVM) for lock-free algorithms
- **Stress tests:** Run concurrent tests under high contention with randomized timing. Single-pass success proves nothing; 10,000-pass success builds confidence.
- **Deterministic replay:** Seed-based schedulers for reproducing intermittent failures

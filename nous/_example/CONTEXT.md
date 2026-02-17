# Context

This file holds transient, session-scoped state. It is cleared or overwritten at session boundaries.

Use it for:
- Active task state that shouldn't persist beyond the current session
- Temporary working notes during multi-step operations
- Runtime-injected context (e.g., current date, active channel, session metadata)

This file is in the "dynamic" cache group — it is never cached between API turns, ensuring the nous always sees the latest state.

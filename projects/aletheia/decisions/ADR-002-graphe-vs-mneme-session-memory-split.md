<!-- Operator-review-pending: agent-drafted under DIRECTIVE v20; T0 metis review required before status moves Proposed → Accepted -->
# ADR-002: graphe vs mneme session memory split

## Status

Proposed

## Context

Aletheia separates session persistence from the wider memory facade. `graphe` is the low-level session record crate. It owns sessions, messages, usage records, notes, blackboard rows, fjall storage, and the portability schema for exported session history. `mneme` is a curated facade that re-exports selected session, knowledge, embedding, recall, ingestion, and Datalog-engine surfaces from decomposed sub-crates.

The split is explicit in crate-level documentation:

```text
crates/graphe/src/lib.rs:2
//! aletheia-graphe: session persistence layer
//!
//! Graphe (Γραφή): "writing, record." Manages sessions, messages, and usage
//! tracking via a fjall LSM-tree store (pure Rust, zero C dependencies).
```

```text
crates/mneme/src/lib.rs:4
//! Mneme (Μνήμη): "memory." Curated facade that re-exports from the extracted
//! sub-crates: graphe (session persistence), episteme (knowledge pipeline),
//! eidos (types), and krites (Datalog engine).
```

`graphe` also documents the concrete session store data model. Its key schema is session-native: session ids, logical session keys, message sequences, usage records, distillations, notes, blackboard entries, and counters. It is not a general semantic memory graph:

```text
crates/graphe/src/store/fjall_store.rs:9
//! | Partition       | Key pattern                                            | Value                    |
//! |-----------------|--------------------------------------------------------|--------------------------|
//! | `sessions`      | `{session_id}`                                         | JSON `Session`           |
//! | `messages`      | `{session_id}:{seq_padded_20}`                         | JSON `Message`           |
//! | `usage`         | `{session_id}:{turn_seq_padded_20}`                    | JSON `UsageRecord`       |
```

The domain types are likewise session history types:

```text
crates/graphe/src/types.rs:145
/// A session record persisted in the store.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
```

```text
crates/graphe/src/types.rs:196
/// A single message within a session's conversation history.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
```

`mneme` crosses that boundary by re-exporting selected graphe types through a stable import path:

```text
crates/mneme/src/lib.rs:109
/// Session store — fjall LSM-tree backend.
///
/// # Facade surface
///
/// [`SessionStore`](store::SessionStore)
pub mod store {
    pub use graphe::store::SessionStore;
}
```

```text
crates/mneme/src/lib.rs:132
pub mod types {
    pub use graphe::types::{
        AgentNote, BlackboardRow, Message, Role, Session, SessionMetrics, SessionOrigin,
        SessionStatus, SessionType, UsageRecord,
```

The facade also states why it exists: downstream crates import `mneme::` instead of knowing every decomposed storage and memory crate. That is a stability contract, not permission to put all memory behavior in one crate:

```text
crates/mneme/src/lib.rs:17
//! 1. **API stability**: downstream application crates import from `mneme`
//!    instead of from `eidos`/`graphe`/`episteme`/`krites` directly.
```

## Decision

**Keep `graphe` as the session graph and persistence crate, and keep `mneme` as the curated memory facade that re-exports `graphe` session surfaces plus knowledge-memory surfaces from the other memory sub-crates.**

What crosses from `graphe` to `mneme` is the stable public session API needed by application crates: `SessionStore`, session/message/usage/note/blackboard types, graphe error types, session metrics registration, and portability DTOs. These exports let pylon, nous, daemon, and other consumers import `mneme::store::SessionStore` and `mneme::types::Session` without binding themselves to the internal crate layout.

What does not cross is graphe's storage implementation detail. Fjall partitions, key formats, counters, write locks, backend modules, and distillation-row internals remain in `graphe`. Likewise, semantic knowledge behavior, embeddings, recall ranking, ingestion, trace ingest, verification, and the optional Datalog/graph engine remain in the episteme/krites side of the memory system and are surfaced through `mneme` only as explicitly curated modules.

The boundary is therefore asymmetric. `graphe` can compile as a focused session persistence crate. `mneme` depends on `graphe` and re-exports selected session types, but should not accumulate its own persistence logic. The existing alarm threshold in `mneme` is part of the decision: if the facade starts gaining logic instead of re-exporting stable surfaces, the logic belongs in a sub-crate.

## Consequences

**Positive:**

- **Session storage remains focused.** Graphe can optimize fjall schemas, sequence scans, session lifecycle states, and portability without carrying knowledge-ingestion or recall concerns.
- **Downstream imports stay stable.** Consumers can use `mneme::` for session and memory concepts while the internal decomposition changes. That reduces churn when storage or knowledge internals move.
- **Feature gates stay centralized.** Optional engine behavior such as Datalog/graph support can be gated once in mneme rather than duplicated across every application crate.
- **The distinction clarifies data ownership.** Session history is not the same as semantic memory. Messages, usage records, and session lifecycle belong to graphe; facts, entities, relationships, embeddings, and recall belong to the memory pipeline exposed through mneme.

**Negative:**

- **There are two names to learn.** New contributors must understand that graphe is the implementation crate for session records while mneme is the import facade and broader memory surface.
- **Facade drift is a risk.** A convenience facade can become a dumping ground. The documented line-count alarm and "re-export only" discipline must be preserved.
- **Some internal users may reach around the facade.** Low-level crates can import graphe directly when they truly own session persistence details, but application crates should generally use mneme. That distinction requires review attention.
- **Portability spans both concepts.** Graphe's `AgentFile` includes sessions and optional memory/knowledge data, so export/import code must avoid treating portability DTOs as permission to merge storage responsibilities.

## References

- [forkwright/aletheia#4039](https://github.com/forkwright/aletheia/issues/4039) - ADR canary issue requesting ADR-002.
- `crates/graphe/src/lib.rs:2` - graphe crate purpose as session persistence.
- `crates/graphe/src/store/fjall_store.rs:9` - fjall key schema for session/message/usage records.
- `crates/mneme/src/lib.rs:4` and `crates/mneme/src/lib.rs:17` - mneme facade scope and API stability rationale.
- `crates/mneme/src/lib.rs:109` and `crates/mneme/src/lib.rs:132` - curated re-exports of graphe session store and session types.
- Michael Nygard, "Documenting Architecture Decisions" - lightweight decision record practice used by this ADR.

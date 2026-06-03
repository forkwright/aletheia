# ADR-006: Agent export/import fidelity contract (v2)

## Status

Accepted

## Context

`aletheia export-agent` and `aletheia import-agent` are documented as a backup and portability mechanism. Before #4163 the implementation was silently lossy in four independently-observable ways:

**Loss A — distilled messages dropped from export.**
`get_history` (graphe `fjall_store.rs`) filters `msg.is_distilled == true` before returning. Export called this path, so the distilled tail of every session silently vanished. An agent restored from a backup came back amnesiac: the distillation context that shaped its behaviour was gone.

**Loss B — structured fields hardcoded to `None`.**
`ExportedSession.working_state`, `ExportedSession.distillation_priming`, and the top-level `memory` and `knowledge` slots were always serialized as `None`. Working state (task stack, focus) was live data in the blackboard; typed knowledge (Facts, Entities, Relationships) was live data in the knowledge store. Neither reached the export file.

**Loss C — session metadata reset on import.**
`create_session` assigns `created_at = now`, `updated_at = now`, `status = Active`, and `metrics = zeroed`. Import called this path. Archived sessions were resurrected as Active; timestamps moved to the import moment; distillation counts and token estimates were zeroed.

**Loss D — per-message metadata dropped on import.**
`append_message` assigns `created_at = now` and forces `is_distilled = false`. Import called this path. Message timestamps moved to the import moment; the distillation flag was stripped from every message.

The existing round-trip test (`export_agent_output_round_trips_through_import`) asserted only that a single non-distilled message's content survived. That weak assertion was why the bug shipped: it could not detect any of the four losses.

The v1 format had no version field that would let consumers detect and reject stale exports; any file produced before the fix would be silently imported with the old behaviour. This required a version bump that gates the importer.

## Decision

**aletheia v2 agent files round-trip faithfully. The format version is bumped to `AGENT_FILE_VERSION = 2`. Importers reject v1 files with an explicit error.**

### Changes landed (PRs #4349 → #4363)

**graphe (PR1 #4349):** Added two raw I/O primitives.

- `get_history_raw(session_id, limit)` — scans the messages partition without filtering `is_distilled`, returning every row in seq order. This is the authoritative export path; runtime recall continues to use `get_history`.
- `insert_message_raw(msg)` — writes a message at its declared `seq`, preserving `created_at` and `is_distilled`, advancing `next_seq` and `msg_id` counters to `max(current, supplied)` so subsequent `append_message` calls cannot collide. Refuses if the owning session does not exist.
- `import_session(session, force)` — writes a session record verbatim (status, timestamps, metrics, origin), constructing indexes from the supplied `updated_at` so list scans see the imported session at its true age. Returns an error (without `force`) if the session id already exists or the `(nous_id, session_key)` slot belongs to a different id.

**export_agent (PR2 #4351):**

- Calls `get_history_raw` instead of `get_history` — Loss A closed.
- Reads `ws:{nous_id}:{session_id}` from the blackboard and serializes it into `ExportedSession.working_state` — Loss B (working_state) closed.
- Exports typed knowledge (Facts, Entities, Relationships) via `export_knowledge` into the top-level `knowledge` slot — Loss B (knowledge) closed.
- Serializes `session.status`, `session.created_at`, `session.updated_at`, and session metrics into `ExportedSession` — groundwork for Loss C.
- `distillation_priming` remains `None`: no live producer exists. This is a documented schema slot reserved for a future distillation-priming store.
- `memory` (HNSW vectors + opaque graph) remains `None`: no portable vector serialization path exists yet. Tracked as a v2 known gap (see Consequences).

**import_agent (PR3 #4357):**

- Calls `import_session` instead of `create_session` — Loss C closed.
- Calls `insert_message_raw` instead of `append_message` — Loss D closed.
- Parses `status` string → `SessionStatus` enum; unknown values warn and default to `Active`.
- Hydrates `working_state` back into the blackboard via `blackboard_write` after each session.
- Version guard: rejects files with `version < AGENT_FILE_VERSION` with a human-readable error citing #4163 and instructing re-export.

**Portability schema (graphe `portability.rs`):** `AGENT_FILE_VERSION` bumped from 1 to 2. Docstring on the constant names the v1 loss inventory and the v2 fidelity guarantee.

**Tests (PR4 #4363):** Four targeted tests replace the weak phase-0 pinning tests:

- `export_preserves_distilled_messages_4163_a` — export produces all messages (distilled and non-distilled) with correct `is_distilled` flags.
- `export_preserves_working_state_4163_b` — working state from the blackboard survives into the export file.
- `import_preserves_session_status_4163_c` — archived status, timestamps, and distillation/message/token metrics are identical after import.
- `import_preserves_message_metadata_4163_d` — per-message `seq`, `is_distilled`, `created_at`, `role`, `content`, and `token_estimate` are identical after import.
- `roundtrip_preserves_typed_knowledge_4163` (recall-gated) — Facts, Entities, Relationships round-trip.
- `roundtrip_is_byte_stable_4163` (recall-gated) — a full export → import → re-export cycle produces identical JSON on every field except `exported_at` and `generator`.

### v2 known gaps (documented, not silent)

| Gap | Reason | Tracking |
|-----|--------|----------|
| `memory.vectors` (HNSW embeddings) | No portable vector serialization path; embeddings are regenerated from text on import. Exporting raw float arrays would be large and version-sensitive. | Follow-up; schema slot reserved. |
| `memory.graph` (opaque graph blob) | No stable serialization for the knowledge-graph snapshot distinct from `knowledge` (typed). | Follow-up; schema slot reserved. |
| `distillation_priming` | No live producer in the current codebase. | Populate when the distillation-priming store materializes. |

These gaps are explicit `None` in the export, documented in code comments, and the format version communicates the v2 contract. Consumers that need the missing data should either wait for a follow-up version bump or extract directly from the source instance.

## Consequences

**Positive:**

- **Faithful backup.** An agent restored from a v2 export has its full distillation history, task state, typed knowledge, timestamps, and session status intact. The "amnesiac restore" class of bug is closed for all currently-live data.
- **Version gate prevents silent data loss.** Importing a v1 file produces an explicit error; the operator is directed to re-export from the source instance. No silent partial restore.
- **Raw primitives serve future importers.** `get_history_raw` and `import_session`/`insert_message_raw` are general enough to serve migration tooling, snapshot restore, and eventual multi-instance sync without a new API surface.
- **Stable contract for downstream tooling.** The TypeScript and Python ecosystem integrations can rely on the v2 schema for backup/restore pipelines.

**Negative:**

- **v1 files are non-migratable without re-export.** There is no in-place migration path. Users with v1 backups must have access to the source instance to re-export. This is the correct trade: attempting to import v1 data with v2 semantics would silently produce partially-correct state (metrics would be zeroed, timestamps wrong), which is worse than a clear error.
- **`memory` gap persists.** Agents with large embedding stores cannot fully back up and restore without direct fjall copy or waiting for a v3 vector-export path.
- **`distillation_priming` gap persists.** If this store materializes and is not yet in the export, a restore would be partially lossy. When that producer lands it must also land a corresponding export path in the same PR.

## References

- forkwright/aletheia#4163 (issue: silently lossy export/import)
- forkwright/aletheia#4290 (PR0: phase-0 pinning tests)
- forkwright/aletheia#4349 (PR1: graphe raw entry points)
- forkwright/aletheia#4351 (PR2: export population)
- forkwright/aletheia#4357 (PR3: faithful import consumer)
- forkwright/aletheia#4363 (PR4: fidelity round-trip tests)
- `crates/graphe/src/store/fjall_store.rs` — `get_history_raw`, `insert_message_raw`, `import_session`
- `crates/graphe/src/portability.rs` — `AgentFile`, `AGENT_FILE_VERSION`
- `crates/aletheia/src/commands/agent_io.rs` — `export_agent`, `import_agent`
- ADR-003 (graphe vs mneme session/memory split) — the store boundary that `get_history_raw` crosses

# SQLite v32 → fjall field mapping

Maps every column from the legacy SQLite v32 sessions DB to its fjall
location. Included via `include_str!` into `migrate::FIELD_MAPPING_DOC`
so the binary can print it to operators verbatim.

## sessions

| SQLite column            | Fjall location                          | Notes                              |
|--------------------------|-----------------------------------------|------------------------------------|
| `id`                     | `Session::id`                           | direct                             |
| `nous_id`                | `Session::nous_id`                      | direct                             |
| `session_key`            | `Session::session_key`                  | direct                             |
| `parent_session_id`      | `Session::origin::parent_session_id`    | direct                             |
| `status`                 | `Session::status`                       | enum `active|archived|distilled`   |
| `model`                  | `Session::model`                        | direct                             |
| `token_count_estimate`   | `Session::metrics::token_count_estimate`| direct                             |
| `message_count`          | `Session::metrics::message_count`       | direct                             |
| `created_at`             | `Session::created_at`                   | direct (ISO 8601)                  |
| `updated_at`             | `Session::updated_at`                   | direct (ISO 8601)                  |
| `last_input_tokens`      | `Session::metrics::last_input_tokens`   | direct                             |
| `bootstrap_hash`         | `Session::metrics::bootstrap_hash`      | direct                             |
| `distillation_count`     | `Session::metrics::distillation_count`  | direct                             |
| `thinking_enabled`       | `migration_legacy:{id}:thinking_enabled` | preserved out-of-band when ≠ 0    |
| `thinking_budget`        | `migration_legacy:{id}:thinking_budget`  | preserved out-of-band when ≠ 10000 |
| `thread_id`              | `Session::origin::thread_id`            | direct                             |
| `transport`              | `Session::origin::transport`            | direct                             |
| `working_state`          | `migration_legacy:{id}:working_state`   | preserved out-of-band when not NULL|
| `session_type`           | `Session::session_type`                 | enum `primary|background|ephemeral`|
| `last_distilled_at`      | `Session::metrics::last_distilled_at`   | direct                             |
| `computed_context_tokens`| `Session::metrics::computed_context_tokens` | direct                         |
| `distillation_priming`   | `migration_legacy:{id}:distillation_priming` | preserved out-of-band when not NULL |
| `display_name`           | `Session::origin::display_name`         | direct                             |

Each session is also indexed under:

- `idx:nous:{nous_id}:upd:{updated_at}:{id}` → `""`
- `idx:key:{nous_id}:{session_key}` → `{id}` bytes

## messages

| SQLite column     | Fjall location              | Notes                          |
|-------------------|-----------------------------|--------------------------------|
| `id`              | `Message::id`               | preserved as legacy auto-id    |
| `session_id`      | `Message::session_id`       | direct                         |
| `seq`             | `Message::seq`              | direct                         |
| `role`            | `Message::role`             | enum `system|user|assistant|tool_result` |
| `content`         | `Message::content`          | direct                         |
| `tool_call_id`    | `Message::tool_call_id`     | direct                         |
| `tool_name`       | `Message::tool_name`        | direct                         |
| `token_estimate`  | `Message::token_estimate`   | direct                         |
| `is_distilled`    | `Message::is_distilled`     | i64 → bool (0=false, ≠0=true)  |
| `created_at`      | `Message::created_at`       | direct                         |

Per-message key: `{session_id}:{seq:020}` in `messages` partition.
Per-session: `next_seq:{session_id}` set to MAX(seq) as big-endian u64.
Global seed: `counters/msg_id` set to MAX(messages.id).

## usage

| SQLite column          | Fjall location                | Notes        |
|------------------------|-------------------------------|--------------|
| `id`                   | `migration_legacy:usage:{session_id}:{turn_seq:020}:id` | legacy auto-id |
| `session_id`           | `UsageRecord::session_id`     | direct       |
| `turn_seq`             | `UsageRecord::turn_seq`       | direct       |
| `input_tokens`         | `UsageRecord::input_tokens`   | direct       |
| `output_tokens`        | `UsageRecord::output_tokens`  | direct       |
| `cache_read_tokens`    | `UsageRecord::cache_read_tokens` | direct    |
| `cache_write_tokens`   | `UsageRecord::cache_write_tokens` | direct   |
| `model`                | `UsageRecord::model`          | direct       |
| `created_at`           | `migration_legacy:usage:{session_id}:{turn_seq:020}:created_at` | legacy timestamp |

`turn_seq` identifies the runtime usage record per session; legacy-only
`id` and `created_at` are preserved in `migration_legacy` for audit/replay
fidelity.

Per-row key: `{session_id}:{turn_seq:020}` in `usage` partition.

## distillations

| SQLite column      | Fjall location                            | Notes |
|--------------------|-------------------------------------------|-------|
| `id`               | `migration_legacy:distillations:{session_id}:{local_id:020}:id` | legacy auto-id |
| `session_id`       | `DistillationRecord::session_id`          | direct |
| `messages_before`  | `DistillationRecord::messages_before`     | direct |
| `messages_after`   | `DistillationRecord::messages_after`      | direct |
| `tokens_before`    | `DistillationRecord::tokens_before`       | direct |
| `tokens_after`     | `DistillationRecord::tokens_after`        | direct |
| `facts_extracted`  | `migration_legacy:distillations:{session_id}:{local_id:020}:facts_extracted` | legacy extraction count |
| `model`            | `DistillationRecord::model`               | direct |
| `created_at`       | `DistillationRecord::created_at`          | direct |

`id` and `facts_extracted` do not appear on the runtime
`DistillationRecord` shape, so they are preserved in `migration_legacy`.

Per-row key: `{session_id}:{local_id:020}` where `local_id` is the
1-based position of the row within its session's distillation list,
matching the runtime's `record_distillation` per-session sequence.

Global counter `counters/dist_id` is seeded to the max per-session
count so future inserts strictly exceed every key already present.

## agent_notes

| SQLite column   | Fjall location           | Notes |
|-----------------|--------------------------|-------|
| `id`            | `AgentNote::id`          | preserved as legacy global id |
| `session_id`    | `AgentNote::session_id`  | direct |
| `nous_id`       | `AgentNote::nous_id`     | direct |
| `category`      | `AgentNote::category`    | direct |
| `content`       | `AgentNote::content`     | direct |
| `created_at`    | `AgentNote::created_at`  | direct |

Two keys per note in the `notes` partition:

- Local: `{session_id}:{local_id:020}` → JSON `AgentNote`
- Global: `gid:{global_id:020}` → `{session_id}:{local_id:020}` bytes

`local_id` is the 1-based position within the session; `global_id`
is the legacy auto-increment `id`. `counters/note_global_id` is
seeded to MAX(notes.id) and `counters/note_local_id` is seeded to
the max per-session count.

## blackboard

| SQLite column    | Fjall location                | Notes |
|------------------|-------------------------------|-------|
| `id`             | `migration_legacy:blackboard:{key}:id` | legacy row id |
| `key`            | `BlackboardRow::key`          | direct |
| `value`          | `BlackboardRow::value`        | direct |
| `author_nous_id` | `BlackboardRow::author_nous_id` | direct |
| `ttl_seconds`    | `BlackboardRow::ttl_seconds`  | direct |
| `created_at`     | `BlackboardRow::created_at`   | direct |
| `expires_at`     | `BlackboardRow::expires_at`   | direct |

`key` is the unique identifier in the new layout; legacy `id` is preserved
in `migration_legacy`.

Per-row key: `{key}` in `blackboard` partition.

## Provenance stamping

Every migrated `Session` is stamped with an `ArtefactMeta` whose:

- `producer = "aletheia-sessions-migrate@<version>"`
- `schema_version = 1`
- `row_counts["messages" | "usage" | "distillations" | "notes"]` reflect
  the rows actually written for that session.

This makes the migration source visible to any later auditor.

# Phase 02: Memory system

## Goal
Conversations persist across sessions with SQLite-backed session storage and shared knowledge types.

## Success criteria
- Session store supports 10K conversations with p99 read latency under 10ms
- WAL mode enabled with automatic checkpointing
- Knowledge types (Fact, Entity, Relationship) are serializable and versioned
- Backup produces a restorable snapshot without stopping the database

## Falsification

| Criterion | Falsifier |
|-----------|-----------|
| Session store supports 10K conversations with p99 read latency under 10ms | Benchmark shows p99 >= 10ms for 10K session read workload |
| WAL mode enabled with automatic checkpointing | `PRAGMA journal_mode` returns `delete` instead of `wal` |
| Knowledge types (Fact, Entity, Relationship) are serializable and versioned | Deserialization of v1 Fact fails after v2 schema change without migration path |
| Backup produces a restorable snapshot without stopping the database | Backup file is corrupted or restore requires exclusive lock |

## Scope

### In scope
- graphe crate: SQLite session store, migrations, retention
- eidos crate: shared knowledge types (Fact, Entity, Relationship, EpistemicTier)
- Backup/restore utilities

### Out of scope
- Vector search and embeddings (Phase 03)
- Datalog reasoning engine (Phase 03)

## Requirements
- REQ-01: SQLite connection pool size is configurable via taxis
- REQ-02: Migrations are idempotent and reversible where possible
- REQ-03: Retention policy deletes sessions older than N days
- REQ-04: Backup uses online streaming copy

## Decisions

| Decision | Choice | Rationale |
|----------|--------|-----------|
| Store backend | SQLite over PostgreSQL | Single binary deployment, no external service |
| Schema migrations | rusqlite_migration over sqlx | Simpler, no compile-time query checking needed |

## Open questions
- Should retention be configurable per-nous or global? (Resolved: per-nous via config cascade)

## Dependencies
- Phase 01 complete
- SQLite 3.35+ (RETURNING clause support)

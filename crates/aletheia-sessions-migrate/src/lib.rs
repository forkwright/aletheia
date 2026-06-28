//! One-shot `SQLite` v32 → fjall sessions-store migrator for legacy aletheia
//! 0.15.x instances.
//!
//! # Background
//!
//! Aletheia 0.15.x persisted sessions in `SQLite` (`sessions.db`, single file
//! with WAL). PR #3119 introduced the fjall-backed `SessionStore` and PR
//! #3446 deleted the `SQLite` implementation entirely. Operators who never
//! upgraded between those points were left holding a `SQLite` file and no
//! migration tool.
//!
//! This crate is that migration tool. It reads a v32 `SQLite` session DB
//! read-only and writes its content to a fresh fjall keyspace whose
//! key/value layout matches `crates/graphe/src/store/fjall_store.rs`
//! exactly — so once migrated, the resulting fjall directory is
//! indistinguishable from one a 0.21.x aletheia instance produced.
//!
//! # Schema invariants
//!
//! - The source DB must have `PRAGMA user_version = 32`.
//! - All required tables must exist: `sessions`, `messages`, `usage`,
//!   `distillations`, `agent_notes`, `blackboard`.
//! - Field mapping is documented in [`migrate::FIELD_MAPPING_DOC`] and
//!   verified by tests in `tests/round_trip_integrity.rs`.
//!
//! # Field-mapping policy: NO silent data loss
//!
//! A handful of session columns from the legacy schema (`thinking_enabled`,
//! `thinking_budget`, `working_state`, `distillation_priming`) have no
//! corresponding field on the new `Session` type. When such a column carries
//! a non-default value, the migrator preserves it under the
//! `migration_legacy` partition keyed by `{session_id}:{column_name}`. The
//! data is never silently dropped.
//!
//! Likewise, messages whose parent session row is missing in the legacy
//! schema (an artefact of #2959, `REFERENCES sessions(id)` without
//! cascade) are preserved under synthesised `orphan-recovery` sessions —
//! see [`migrate`] for the policy.
//!
//! # Migration verification
//!
//! Source `schema_version` is asserted equal to [`schema::REQUIRED_USER_VERSION`]
//! (currently `32`). After staging, verification compares deterministic
//! key/value entry counts and SHA-256 hashes for every migrated fjall
//! partition before the destination is published. Any mismatch aborts with a
//! non-zero exit status. See [`verify::run_verification`].

#![deny(missing_docs)]

pub mod dest;
pub mod error;
pub mod migrate;
pub mod schema;
pub mod source;
pub mod verify;

pub use error::{Error, Result};
pub use migrate::{
    MigrationPlan, MigrationReport, StagedMigration, run_migration, stage_migration,
};
pub use verify::{VerificationReport, run_verification};

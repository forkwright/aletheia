//! Auth store.
//!
//! Selects a backend at compile time via feature flags:
//!
//! - `fjall` (default): pure-Rust LSM-tree store via the `fjall` crate.
//! - `sqlite`: WAL-mode SQLite via `rusqlite`.
//!
//! Both backends expose the same [`AuthStore`] type with an identical method
//! set. Code that depends on `symbolon` should import from this module and
//! never reach into backend-specific sub-modules.

// ── Fjall backend ──────────────────────────────────────────────────────────────
// WHY: when both fjall and sqlite are active (workspace feature unification),
// prefer sqlite so AuthStore is unambiguous and fjall helpers aren't dead code.
// Fjall module only compiles when sqlite is absent.
#[cfg(all(feature = "fjall", not(feature = "sqlite")))]
mod fjall_store;
#[cfg(all(feature = "fjall", not(feature = "sqlite")))]
pub(crate) use fjall_store::AuthStore;

// ── SQLite backend ─────────────────────────────────────────────────────────────
#[cfg(feature = "sqlite")]
mod sqlite_store;
#[cfg(feature = "sqlite")]
pub(crate) use sqlite_store::AuthStore;

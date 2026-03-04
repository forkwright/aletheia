//! aletheia-mneme — session store and memory engine
//!
//! Mneme (Μνήμη) — "memory." Manages sessions, messages, and usage tracking
//! via embedded `SQLite` (`rusqlite`). Future: `CozoDB` for vectors + graph.
//!
//! Depends on `aletheia-koina` for types and errors.

#[cfg(feature = "mneme-engine")]
#[allow(
    unsafe_code,
    dead_code,
    private_interfaces,
    unexpected_cfgs,
    unused_imports,
    unreachable_code,
    unused_variables,
    clippy::pedantic,
    clippy::mutable_key_type,
    clippy::type_complexity,
    clippy::too_many_arguments,
    clippy::non_canonical_partial_ord_impl,
    clippy::neg_cmp_op_on_partial_ord,
)]
pub mod engine;

#[cfg(feature = "sqlite")]
pub mod backup;
pub mod embedding;
pub mod error;
pub mod extract;
pub mod knowledge;
pub mod knowledge_store;
#[cfg(feature = "sqlite")]
pub mod migration;
#[cfg(feature = "mneme-engine")]
pub mod query;
pub mod recall;
#[cfg(feature = "sqlite")]
pub mod retention;
#[cfg(feature = "sqlite")]
pub mod schema;
#[cfg(feature = "sqlite")]
pub mod store;
pub mod types;

#[cfg(all(test, feature = "sqlite"))]
mod assertions {
    use super::store::SessionStore;
    use static_assertions::assert_impl_all;

    assert_impl_all!(SessionStore: Send);
}

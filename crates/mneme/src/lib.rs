//! aletheia-mneme — session store and memory engine
//!
//! Mneme (Μνήμη) — "memory." Manages sessions, messages, and usage tracking
//! via embedded `SQLite` (`rusqlite`). Future: `CozoDB` for vectors + graph.
//!
//! Depends on `aletheia-koina` for types and errors.

pub mod error;
pub mod knowledge;
pub mod knowledge_store;
#[cfg(feature = "sqlite")]
pub mod schema;
#[cfg(feature = "sqlite")]
pub mod store;
pub mod types;

#[cfg(all(test, feature = "sqlite"))]
mod assertions {
    use static_assertions::assert_impl_all;
    use super::store::SessionStore;

    assert_impl_all!(SessionStore: Send);
}

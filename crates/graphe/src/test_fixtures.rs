//! Shared test fixtures for graphe session store tests.
//!
//! Centralises the `test_store` helper that was duplicated across every
//! test module in `store/tests/`, `backup_tests`, `export_tests`, and
//! `import_tests/`.

use crate::store::SessionStore;

/// Open an in-memory `SessionStore` for use in tests.
pub(crate) fn test_store() -> SessionStore {
    SessionStore::open_in_memory().expect("open in-memory session store for test")
}

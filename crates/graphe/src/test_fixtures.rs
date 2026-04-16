//! Shared test fixtures for graphe session store tests.

use crate::store::SessionStore;

/// Open an in-memory `SessionStore` for use in tests.
pub(crate) fn test_store() -> SessionStore {
    #![expect(clippy::expect_used, reason = "test helper")]
    SessionStore::open_in_memory().expect("open in-memory session store for test")
}

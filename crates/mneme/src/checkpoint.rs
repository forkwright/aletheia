//! Working-memory checkpoint storage contract.
//!
//! WHY: The working-checkpoint hook is enabled by default in `nous` config,
//! but the Aletheia runtime currently passes no store, so the injector hook
//! no-ops and the organon tool reports "working checkpoint store unavailable"
//! (#4688). Defining the storage surface in the memory facade lets downstream
//! runtime code wire a durable implementation without leaking `graphe` or
//! `episteme` internals.

use std::collections::HashMap;
use std::sync::Mutex;

/// Storage backend for agent-curated working-memory checkpoints.
///
/// WHY: Checkpoints are small, agent-readable key/value notes that should
/// survive across turns and be injectable into later context. A dedicated
/// trait keeps the hook/tool decoupled from any specific backend.
pub trait WorkingCheckpointStore: Send + Sync {
    /// Error returned by store operations.
    type Error: std::error::Error + Send + Sync + 'static;

    /// Persist or overwrite a checkpoint value under `key`.
    ///
    /// WHY: updates are unconditional writes; conflict resolution lives in
    /// the tool/hook layer.
    fn update(&self, key: &str, value: &str) -> Result<(), Self::Error>;

    /// Read the checkpoint stored under `key`, if any.
    fn get(&self, key: &str) -> Result<Option<String>, Self::Error>;

    /// List all checkpoint keys currently stored.
    fn list_keys(&self) -> Result<Vec<String>, Self::Error>;
}

/// In-memory implementation of [`WorkingCheckpointStore`] for tests and
/// lightweight runtimes.
#[derive(Debug, Default)]
pub struct InMemoryCheckpointStore {
    inner: Mutex<HashMap<String, String>>,
}

impl InMemoryCheckpointStore {
    /// Create a new empty in-memory checkpoint store.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }
}

/// Error type for the in-memory checkpoint store.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InMemoryCheckpointError {
    /// The store lock was poisoned by a panicking thread.
    Poisoned,
}

impl std::fmt::Display for InMemoryCheckpointError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Poisoned => f.write_str("checkpoint store lock poisoned"),
        }
    }
}

impl std::error::Error for InMemoryCheckpointError {}

impl WorkingCheckpointStore for InMemoryCheckpointStore {
    type Error = InMemoryCheckpointError;

    fn update(&self, key: &str, value: &str) -> Result<(), Self::Error> {
        let mut guard = self.inner.lock().map_err(|_| InMemoryCheckpointError::Poisoned)?;
        guard.insert(key.to_owned(), value.to_owned());
        Ok(())
    }

    fn get(&self, key: &str) -> Result<Option<String>, Self::Error> {
        let guard = self.inner.lock().map_err(|_| InMemoryCheckpointError::Poisoned)?;
        Ok(guard.get(key).cloned())
    }

    fn list_keys(&self) -> Result<Vec<String>, Self::Error> {
        let guard = self.inner.lock().map_err(|_| InMemoryCheckpointError::Poisoned)?;
        Ok(guard.keys().cloned().collect())
    }
}

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test assertions")]
mod tests {
    use super::*;

    #[test]
    fn in_memory_store_roundtrip() {
        let store = InMemoryCheckpointStore::new();
        store.update("goal", "finish audit").expect("update succeeds");
        assert_eq!(store.get("goal").expect("get succeeds"), Some("finish audit".to_owned()));
    }

    #[test]
    fn in_memory_store_update_overwrites() {
        let store = InMemoryCheckpointStore::new();
        store.update("goal", "first").expect("update succeeds");
        store.update("goal", "second").expect("update succeeds");
        assert_eq!(store.get("goal").expect("get succeeds"), Some("second".to_owned()));
    }

    #[test]
    fn in_memory_store_lists_keys() {
        let store = InMemoryCheckpointStore::new();
        store.update("a", "1").expect("update succeeds");
        store.update("b", "2").expect("update succeeds");
        let mut keys = store.list_keys().expect("list succeeds");
        keys.sort();
        assert_eq!(keys, vec!["a", "b"]);
    }
}

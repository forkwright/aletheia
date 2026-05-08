//! Retention executor for session-scoped cleanup.

use std::sync::Arc;

use mneme::store::SessionStore;
use oikonomos::maintenance::{RetentionExecutor, RetentionSummary};
use tokio::sync::Mutex;

/// Bridges the daemon retention task to the fjall-backed session store.
pub(crate) struct SessionRetentionAdapter {
    store: Arc<Mutex<SessionStore>>,
}

impl SessionRetentionAdapter {
    pub(crate) fn new(store: Arc<Mutex<SessionStore>>) -> Self {
        Self { store }
    }
}

impl RetentionExecutor for SessionRetentionAdapter {
    fn execute_retention(&self) -> oikonomos::error::Result<RetentionSummary> {
        let store = self.store.blocking_lock();
        let cleaned = store.cleanup_expired_entries().map_err(|e| {
            oikonomos::error::TaskFailedSnafu {
                task_id: "retention-execution",
                reason: format!("blackboard cleanup failed: {e}"),
            }
            .build()
        })?;
        let blackboard_entries_cleaned = u32::try_from(cleaned).map_err(|e| {
            oikonomos::error::TaskFailedSnafu {
                task_id: "retention-execution",
                reason: format!("blackboard cleanup count overflow: {e}"),
            }
            .build()
        })?;

        Ok(RetentionSummary {
            blackboard_entries_cleaned,
            ..RetentionSummary::default()
        })
    }
}

#[cfg(test)]
mod tests {
    use oikonomos::maintenance::RetentionExecutor as _;

    use super::*;

    fn retention_error(reason: impl Into<String>) -> oikonomos::error::Error {
        oikonomos::error::TaskFailedSnafu {
            task_id: "retention-execution",
            reason: reason.into(),
        }
        .build()
    }

    #[tokio::test]
    async fn retention_adapter_executes_blackboard_cleanup() -> oikonomos::error::Result<()> {
        let store =
            Arc::new(Mutex::new(SessionStore::open_in_memory().map_err(|e| {
                retention_error(format!("session store open failed: {e}"))
            })?));

        let adapter = SessionRetentionAdapter::new(Arc::clone(&store));
        let summary = tokio::task::spawn_blocking(move || adapter.execute_retention())
            .await
            .map_err(|e| retention_error(format!("retention task join failed: {e}")))??;

        assert_eq!(summary.blackboard_entries_cleaned, 0);
        let entries = store
            .lock()
            .await
            .blackboard_list()
            .map_err(|e| retention_error(format!("blackboard list failed: {e}")))?;
        assert!(entries.is_empty());
        Ok(())
    }
}

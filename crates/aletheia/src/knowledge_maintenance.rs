//! Stub implementation of `KnowledgeMaintenanceExecutor` for the binary crate.
//!
//! Each method returns an empty `MaintenanceReport`. Actual logic is added by
//! subsequent feature prompts (F.1–F.8).

use std::sync::Arc;

use aletheia_mneme::knowledge_store::KnowledgeStore;
use aletheia_oikonomos::maintenance::{KnowledgeMaintenanceExecutor, MaintenanceReport};

/// Bridges the daemon's `KnowledgeMaintenanceExecutor` trait to the concrete
/// `KnowledgeStore`. All methods are blocking (CozoDB is sync).
pub(crate) struct KnowledgeMaintenanceAdapter {
    #[expect(dead_code, reason = "store will be used once F.1–F.8 stubs are replaced")]
    store: Arc<KnowledgeStore>,
}

impl KnowledgeMaintenanceAdapter {
    pub(crate) fn new(store: Arc<KnowledgeStore>) -> Self {
        Self { store }
    }
}

impl KnowledgeMaintenanceExecutor for KnowledgeMaintenanceAdapter {
    fn refresh_decay_scores(
        &self,
        _nous_id: &str,
    ) -> aletheia_oikonomos::error::Result<MaintenanceReport> {
        Ok(MaintenanceReport::default())
    }

    fn deduplicate_entities(
        &self,
        _nous_id: &str,
    ) -> aletheia_oikonomos::error::Result<MaintenanceReport> {
        Ok(MaintenanceReport::default())
    }

    fn recompute_graph_scores(
        &self,
        _nous_id: &str,
    ) -> aletheia_oikonomos::error::Result<MaintenanceReport> {
        Ok(MaintenanceReport::default())
    }

    fn refresh_embeddings(
        &self,
        _nous_id: &str,
    ) -> aletheia_oikonomos::error::Result<MaintenanceReport> {
        Ok(MaintenanceReport::default())
    }

    fn garbage_collect(
        &self,
        _nous_id: &str,
    ) -> aletheia_oikonomos::error::Result<MaintenanceReport> {
        Ok(MaintenanceReport::default())
    }

    fn maintain_indexes(
        &self,
        _nous_id: &str,
    ) -> aletheia_oikonomos::error::Result<MaintenanceReport> {
        Ok(MaintenanceReport::default())
    }

    fn health_check(
        &self,
        _nous_id: &str,
    ) -> aletheia_oikonomos::error::Result<MaintenanceReport> {
        Ok(MaintenanceReport::default())
    }
}

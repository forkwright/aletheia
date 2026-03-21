//! Agent registration state.

use theatron_core::id::NousId;

/// A registered agent entry from the pylon server.
#[derive(Debug, Clone)]
pub struct AgentEntry {
    /// Agent identifier.
    pub id: NousId,
    /// Human-readable agent name.
    pub name: String,
    /// Agent status label (idle, busy, etc.).
    pub status: String,
}

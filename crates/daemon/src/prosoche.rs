//! Prosoche (προσοχή) — "directed attention." Periodic check-in that monitors
//! calendar, tasks, and system health for a nous.

use serde::{Deserialize, Serialize};

/// Prosoche attention check runner.
#[derive(Debug, Clone)]
pub struct ProsocheCheck {
    nous_id: String,
}

/// Result of a prosoche check.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProsocheResult {
    pub items: Vec<AttentionItem>,
    pub checked_at: String,
}

/// A single item requiring attention.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AttentionItem {
    pub category: AttentionCategory,
    pub summary: String,
    pub urgency: Urgency,
}

/// Categories of attention items.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AttentionCategory {
    Calendar,
    Task,
    SystemHealth,
    Custom(String),
}

/// Urgency level for attention items.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub enum Urgency {
    Low,
    Medium,
    High,
    Critical,
}

impl ProsocheCheck {
    pub fn new(nous_id: impl Into<String>) -> Self {
        Self {
            nous_id: nous_id.into(),
        }
    }

    /// Run the attention check. Returns items needing attention.
    ///
    /// Currently returns an empty result — actual checks (gcal, taskwarrior,
    /// system health) require tool execution which will be wired when daemon
    /// integrates with the nous pipeline.
    #[expect(clippy::unused_async, reason = "will perform async I/O once wired to nous pipeline")]
    pub async fn run(&self) -> crate::error::Result<ProsocheResult> {
        tracing::info!(nous_id = %self.nous_id, "prosoche check completed");
        Ok(ProsocheResult {
            items: Vec::new(),
            checked_at: jiff::Timestamp::now().to_string(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn prosoche_returns_empty_result() {
        let check = ProsocheCheck::new("test-nous");
        let result = check.run().await.expect("should succeed");
        assert!(result.items.is_empty());
        assert!(!result.checked_at.is_empty());
    }
}

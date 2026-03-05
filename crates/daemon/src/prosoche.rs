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
    /// Items requiring the nous's attention.
    pub items: Vec<AttentionItem>,
    /// ISO 8601 timestamp when the check was performed.
    pub checked_at: String,
}

/// A single item requiring attention.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AttentionItem {
    /// What kind of attention is needed.
    pub category: AttentionCategory,
    /// Human-readable description of the item.
    pub summary: String,
    /// How urgently this needs attention.
    pub urgency: Urgency,
}

/// Categories of attention items.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AttentionCategory {
    /// Calendar event or deadline.
    Calendar,
    /// Pending task or overdue item.
    Task,
    /// System health issue (disk, memory, service status).
    SystemHealth,
    /// Application-defined attention category.
    Custom(String),
}

/// Urgency level for attention items.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub enum Urgency {
    /// Informational, no action needed soon.
    Low,
    /// Should be addressed within the current session.
    Medium,
    /// Needs attention soon (within hours).
    High,
    /// Requires immediate action.
    Critical,
}

impl AttentionItem {
    /// Short label for this item's category (used in prompt formatting).
    pub fn category_label(&self) -> &str {
        match &self.category {
            AttentionCategory::Calendar => "calendar",
            AttentionCategory::Task => "task",
            AttentionCategory::SystemHealth => "health",
            AttentionCategory::Custom(s) => s,
        }
    }
}

impl ProsocheCheck {
    /// Create a prosoche check for the given nous.
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
    #[expect(
        clippy::unused_async,
        reason = "will perform async I/O once wired to nous pipeline"
    )]
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

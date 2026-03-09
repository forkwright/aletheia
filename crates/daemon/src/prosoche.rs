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

    #[test]
    fn prosoche_check_new() {
        let check = ProsocheCheck::new("alice-nous");
        // Verify nous_id is stored by running the check (which logs it).
        // We can also verify via Debug output.
        let debug = format!("{check:?}");
        assert!(
            debug.contains("alice-nous"),
            "ProsocheCheck should store the nous_id"
        );
    }

    #[test]
    fn attention_item_category_label_calendar() {
        let item = AttentionItem {
            category: AttentionCategory::Calendar,
            summary: "meeting".to_owned(),
            urgency: Urgency::Medium,
        };
        assert_eq!(item.category_label(), "calendar");
    }

    #[test]
    fn attention_item_category_label_task() {
        let item = AttentionItem {
            category: AttentionCategory::Task,
            summary: "review PR".to_owned(),
            urgency: Urgency::Low,
        };
        assert_eq!(item.category_label(), "task");
    }

    #[test]
    fn attention_item_category_label_health() {
        let item = AttentionItem {
            category: AttentionCategory::SystemHealth,
            summary: "disk full".to_owned(),
            urgency: Urgency::Critical,
        };
        assert_eq!(item.category_label(), "health");
    }

    #[test]
    fn attention_item_category_label_custom() {
        let item = AttentionItem {
            category: AttentionCategory::Custom("foo".to_owned()),
            summary: "custom item".to_owned(),
            urgency: Urgency::Low,
        };
        assert_eq!(item.category_label(), "foo");
    }

    #[test]
    fn urgency_ordering() {
        assert!(Urgency::Low < Urgency::Medium);
        assert!(Urgency::Medium < Urgency::High);
        assert!(Urgency::High < Urgency::Critical);
    }

    #[test]
    fn prosoche_result_serialization() {
        let result = ProsocheResult {
            items: vec![AttentionItem {
                category: AttentionCategory::Task,
                summary: "test".to_owned(),
                urgency: Urgency::High,
            }],
            checked_at: "2026-01-01T00:00:00Z".to_owned(),
        };
        let json = serde_json::to_string(&result).expect("serialize");
        let back: ProsocheResult = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back.items.len(), 1);
        assert_eq!(back.checked_at, "2026-01-01T00:00:00Z");
    }

    #[test]
    fn attention_item_serialization() {
        let item = AttentionItem {
            category: AttentionCategory::Calendar,
            summary: "standup".to_owned(),
            urgency: Urgency::Medium,
        };
        let json = serde_json::to_string(&item).expect("serialize");
        assert!(json.contains("Calendar"));
        assert!(json.contains("standup"));
        assert!(json.contains("Medium"));
    }
}

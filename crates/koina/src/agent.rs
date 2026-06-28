//! Shared agent lifecycle types.

use std::fmt;

use serde::{Deserialize, Serialize};

/// Canonical lifecycle status for a nous agent across HTTP and first-party clients.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum AgentLifecycle {
    /// Agent is administratively disabled and should not accept operator work.
    Disabled,
    /// Agent is currently processing a turn or other foreground work.
    Active,
    /// Agent is live and ready to accept work.
    #[default]
    Idle,
    /// Agent is paused or sleeping, with wake-up required before normal work.
    Dormant,
    /// Agent degraded itself after repeated failures and rejects new work until recovery.
    Degraded,
    /// Manager has detected a failed actor and is restarting or revalidating it.
    Recovering,
    /// Agent is retained but intentionally removed from active operator workflows.
    Archived,
    /// Agent state could not be queried or the actor is unexpectedly unavailable.
    Error,
}

impl AgentLifecycle {
    /// Stable lowercase wire value for this lifecycle state.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Disabled => "disabled",
            Self::Active => "active",
            Self::Idle => "idle",
            Self::Dormant => "dormant",
            Self::Degraded => "degraded",
            Self::Recovering => "recovering",
            Self::Archived => "archived",
            Self::Error => "error",
        }
    }

    /// Parse a stable wire value.
    #[must_use]
    pub fn from_wire(value: &str) -> Option<Self> {
        match value {
            "disabled" => Some(Self::Disabled),
            "active" | "working" | "streaming" => Some(Self::Active),
            "idle" => Some(Self::Idle),
            "dormant" => Some(Self::Dormant),
            "degraded" => Some(Self::Degraded),
            "recovering" => Some(Self::Recovering),
            "archived" => Some(Self::Archived),
            "error" | "unknown" => Some(Self::Error),
            _ => None,
        }
    }
}

impl fmt::Display for AgentLifecycle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lifecycle_wire_values_are_stable() {
        let cases = [
            (AgentLifecycle::Disabled, "disabled"),
            (AgentLifecycle::Active, "active"),
            (AgentLifecycle::Idle, "idle"),
            (AgentLifecycle::Dormant, "dormant"),
            (AgentLifecycle::Degraded, "degraded"),
            (AgentLifecycle::Recovering, "recovering"),
            (AgentLifecycle::Archived, "archived"),
            (AgentLifecycle::Error, "error"),
        ];

        for (status, expected) in cases {
            assert_eq!(status.as_str(), expected);
            assert_eq!(status.to_string(), expected);
            assert_eq!(AgentLifecycle::from_wire(expected), Some(status));
        }
    }

    #[test]
    fn legacy_client_activity_words_parse_as_active() {
        assert_eq!(
            AgentLifecycle::from_wire("working"),
            Some(AgentLifecycle::Active)
        );
        assert_eq!(
            AgentLifecycle::from_wire("streaming"),
            Some(AgentLifecycle::Active)
        );
    }

    #[test]
    fn legacy_unknown_word_parses_as_error() {
        assert_eq!(
            AgentLifecycle::from_wire("unknown"),
            Some(AgentLifecycle::Error)
        );
    }
}

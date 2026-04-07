//! Pipeline stage tracking for transparent routing.
//!
//! WHY: Neurodivergent operators need to know what the system is doing at all
//! times (#2411). Without visible pipeline stage, the UI feels opaque --
//! "is it thinking? recalling? stuck?" Transparent routing reduces anxiety
//! and supports trust calibration.

use theatron_core::id::NousId;

/// Observable pipeline stage for the active agent.
///
/// Maps to the server-side turn lifecycle:
/// - `TurnStart` -> `Bootstrap` (brief) -> `Recalling` (context retrieval)
/// - `TextDelta` -> `Thinking` (generation in progress)
/// - `ToolStart` -> `Executing` (tool use)
/// - `TurnComplete` -> `Idle`
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub(crate) enum PipelineStage {
    /// No active turn. Agent is available.
    Idle,
    /// Turn started, awaiting first content. Context is being assembled.
    Bootstrap,
    /// Agent is retrieving memories and context for the turn.
    Recalling,
    /// Agent is generating a response (text deltas arriving).
    Thinking,
    /// Agent is executing a tool call.
    Executing {
        /// Name of the tool being executed.
        tool_name: String,
    },
    /// Turn completed, response delivered.
    Complete,
}

impl PipelineStage {
    /// Human-readable label for display in the routing indicator.
    #[must_use]
    pub(crate) fn label(&self) -> &str {
        match self {
            Self::Idle => "ready",
            Self::Bootstrap => "bootstrapping\u{2026}",
            Self::Recalling => "recalling\u{2026}",
            Self::Thinking => "thinking\u{2026}",
            Self::Executing { tool_name } => {
                // NOTE: Returns the tool_name directly; the component
                // formats as "running {tool_name}..."
                tool_name.as_str()
            }
            Self::Complete => "done",
        }
    }

    /// Whether the stage represents active work (not idle/complete).
    #[must_use]
    pub(crate) fn is_active(&self) -> bool {
        !matches!(self, Self::Idle | Self::Complete)
    }

    /// CSS color token for the stage indicator dot.
    #[must_use]
    pub(crate) fn dot_color(&self) -> &'static str {
        match self {
            Self::Idle => "var(--text-muted)",
            Self::Bootstrap | Self::Recalling => "var(--aporia)",
            Self::Thinking => "var(--status-success)",
            Self::Executing { .. } => "var(--accent)",
            Self::Complete => "var(--text-muted)",
        }
    }
}

/// Current routing state for the active conversation.
///
/// Combines the agent identity with the pipeline stage for a single
/// status line: "Syn \u{00b7} thinking..." or "Arc \u{00b7} running read_file..."
#[derive(Debug, Clone)]
pub(crate) struct RoutingState {
    /// Agent handling the current conversation.
    pub agent_name: String,
    /// Agent identifier.
    pub agent_id: NousId,
    /// Current pipeline stage.
    pub stage: PipelineStage,
}

impl RoutingState {
    /// Format as a display string: "Agent \u{00b7} stage".
    #[must_use]
    pub(crate) fn display(&self) -> String {
        match &self.stage {
            PipelineStage::Executing { tool_name } => {
                format!("{} \u{00b7} running {tool_name}\u{2026}", self.agent_name)
            }
            stage => format!("{} \u{00b7} {}", self.agent_name, stage.label()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pipeline_stage_labels() {
        assert_eq!(PipelineStage::Idle.label(), "ready");
        assert_eq!(PipelineStage::Bootstrap.label(), "bootstrapping\u{2026}");
        assert_eq!(PipelineStage::Recalling.label(), "recalling\u{2026}");
        assert_eq!(PipelineStage::Thinking.label(), "thinking\u{2026}");
        assert_eq!(PipelineStage::Complete.label(), "done");
        assert_eq!(
            PipelineStage::Executing {
                tool_name: "read_file".to_string()
            }
            .label(),
            "read_file"
        );
    }

    #[test]
    fn pipeline_stage_is_active() {
        assert!(!PipelineStage::Idle.is_active());
        assert!(PipelineStage::Bootstrap.is_active());
        assert!(PipelineStage::Recalling.is_active());
        assert!(PipelineStage::Thinking.is_active());
        assert!(PipelineStage::Executing {
            tool_name: "x".to_string()
        }
        .is_active());
        assert!(!PipelineStage::Complete.is_active());
    }

    #[test]
    fn pipeline_stage_dot_colors_are_css_vars() {
        for stage in [
            PipelineStage::Idle,
            PipelineStage::Bootstrap,
            PipelineStage::Thinking,
            PipelineStage::Complete,
        ] {
            assert!(
                stage.dot_color().starts_with("var("),
                "dot_color should use CSS variables: {}",
                stage.dot_color()
            );
        }
    }

    #[test]
    fn routing_state_display_format() {
        let state = RoutingState {
            agent_name: "Syn".to_string(),
            agent_id: NousId::from("syn"),
            stage: PipelineStage::Thinking,
        };
        assert_eq!(state.display(), "Syn \u{00b7} thinking\u{2026}");
    }

    #[test]
    fn routing_state_display_tool_execution() {
        let state = RoutingState {
            agent_name: "Syn".to_string(),
            agent_id: NousId::from("syn"),
            stage: PipelineStage::Executing {
                tool_name: "read_file".to_string(),
            },
        };
        assert_eq!(state.display(), "Syn \u{00b7} running read_file\u{2026}");
    }

    #[test]
    fn routing_state_display_idle() {
        let state = RoutingState {
            agent_name: "Arc".to_string(),
            agent_id: NousId::from("arc"),
            stage: PipelineStage::Idle,
        };
        assert_eq!(state.display(), "Arc \u{00b7} ready");
    }
}

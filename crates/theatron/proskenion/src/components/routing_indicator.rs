//! Transparent routing indicator for neurodivergent UX.
//!
//! WHY: AuDHD operators need to know what the system is doing at all times
//! (#2411). Without a visible pipeline stage, the UI feels opaque -- "is it
//! thinking? recalling? stuck?" A small status line below the chat shows
//! "Syn · recalling..." or "Syn · thinking..." so the operator can trust
//! the system without anxious guessing.
//!
//! Reads from `Signal<Option<RoutingState>>` provided in the layout. The
//! SSE event processing pipeline updates this signal as turn_start,
//! text_delta, tool_start, and turn_complete events arrive.

use dioxus::prelude::*;

use crate::state::pipeline::{PipelineStage, RoutingState};

/// Height of the routing indicator bar.
const INDICATOR_STYLE: &str = "\
    display: flex; \
    align-items: center; \
    gap: var(--space-2); \
    padding: var(--space-1) var(--space-4); \
    font-size: var(--text-xs); \
    color: var(--text-secondary); \
    background: var(--bg-surface-dim); \
    border-top: 1px solid var(--border-separator); \
    min-height: 24px; \
    flex-shrink: 0;\
";

/// Pulsing dot style for active stages. The animation is informational
/// (indicates ongoing work), not decorative -- it persists.
const DOT_ACTIVE_STYLE: &str = "\
    width: 6px; \
    height: 6px; \
    border-radius: var(--radius-full); \
    flex-shrink: 0; \
    animation: routing-pulse 2s ease-in-out infinite;\
";

/// Static dot for idle/complete states -- no animation.
const DOT_IDLE_STYLE: &str = "\
    width: 6px; \
    height: 6px; \
    border-radius: var(--radius-full); \
    flex-shrink: 0;\
";

/// Transparent routing indicator shown below the chat input.
///
/// Displays the active agent name and current pipeline stage. Hidden
/// when no routing state is available (e.g. before first agent load).
#[component]
pub(crate) fn RoutingIndicator() -> Element {
    let routing_signal = use_context::<Signal<Option<RoutingState>>>();
    let routing = routing_signal.read();

    let Some(ref state) = *routing else {
        return rsx! {};
    };

    let display_text = state.display();
    let dot_color = state.stage.dot_color();
    let is_active = state.stage.is_active();

    let dot_style = if is_active {
        DOT_ACTIVE_STYLE
    } else {
        DOT_IDLE_STYLE
    };

    rsx! {
        div {
            style: "{INDICATOR_STYLE}",
            role: "status",
            "aria-live": "polite",
            "aria-label": "Agent routing status: {display_text}",

            // Status dot
            span {
                style: "{dot_style} background: {dot_color};",
                "aria-hidden": "true",
            }

            // Status text
            span {
                "{display_text}"
            }
        }
    }
}

/// Update the routing state signal from chat streaming events.
///
/// Call this from the chat view's streaming loop to keep the routing
/// indicator in sync with the pipeline. Transitions:
///
/// - `TurnStart` -> `Bootstrap` (brief) -> `Recalling`
/// - First `TextDelta` -> `Thinking`
/// - `ToolStart` -> `Executing { tool_name }`
/// - `TurnComplete` -> `Complete` (auto-clears to `Idle` after 2s)
pub(crate) fn update_routing_stage(
    routing: &mut Signal<Option<RoutingState>>,
    stage: PipelineStage,
    agent_name: &str,
    agent_id: &skene::id::NousId,
) {
    let current = routing.read();
    let needs_update = match &*current {
        Some(state) => state.stage != stage || state.agent_id != *agent_id,
        None => true,
    };
    drop(current);

    if needs_update {
        routing.set(Some(RoutingState {
            agent_name: agent_name.to_string(),
            agent_id: agent_id.clone(),
            stage,
        }));
    }
}

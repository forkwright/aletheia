//! Distillation progress indicator.
//!
//! Shows a progress bar with stage label while an agent is performing context
//! distillation (memory compaction). Auto-hides when complete or no distillation
//! is active. Reads `Signal<EventState>` from context.

use dioxus::prelude::*;
use theatron_core::id::NousId;

use crate::state::events::{DistillationProgress, EventState};

const CONTAINER_STYLE: &str = "\
    display: flex; \
    align-items: center; \
    gap: 10px; \
    padding: 6px 16px; \
    background: #1a1a2e; \
    border-top: 1px solid #333;\
";

const LABEL_STYLE: &str = "\
    font-size: 12px; \
    color: #888; \
    min-width: 90px;\
";

const TRACK_STYLE: &str = "\
    flex: 1; \
    height: 3px; \
    background: #2a2a3a; \
    border-radius: 2px; \
    overflow: hidden;\
";

const BAR_STYLE: &str = "\
    height: 3px; \
    border-radius: 2px; \
    background: #4a4aff;\
";

const BAR_COMPLETE_STYLE: &str = "\
    height: 3px; \
    border-radius: 2px; \
    background: #22c55e;\
";

/// Map a distillation stage name to a progress bar fill percentage.
fn stage_pct(label: &str) -> u8 {
    match label {
        "distilling" => 10,
        "summarizing" => 30,
        "extracting" => 50,
        "compacting" => 70,
        "finalizing" => 90,
        "complete" => 100,
        _ => 20,
    }
}

/// Distillation progress indicator.
///
/// Takes `nous_id` as a prop to look up distillation state for a specific
/// agent. Hidden when no distillation is active for that agent.
#[component]
pub(crate) fn DistillationIndicatorView(nous_id: NousId) -> Element {
    let event_state = use_context::<Signal<EventState>>();

    let progress: Option<DistillationProgress> = event_state
        .read()
        .distillation
        .get(&nous_id)
        .cloned();

    let Some(progress) = progress else {
        return rsx! { div {} };
    };

    let label = progress.label().to_string();
    let pct = stage_pct(&label);
    let bar_style = if matches!(progress, DistillationProgress::Complete) {
        BAR_COMPLETE_STYLE
    } else {
        BAR_STYLE
    };
    let display_label = match &progress {
        DistillationProgress::Started => "Distilling…".to_string(),
        DistillationProgress::Stage { stage } => {
            let mut s = stage.clone();
            if let Some(c) = s.get_mut(0..1) {
                c.make_ascii_uppercase();
            }
            format!("{s}…")
        }
        DistillationProgress::Complete => "Complete".to_string(),
    };

    rsx! {
        div {
            style: "{CONTAINER_STYLE}",
            span {
                style: "{LABEL_STYLE}",
                title: "Context distillation in progress",
                "{display_label}"
            }
            div {
                style: "{TRACK_STYLE}",
                div {
                    style: "{bar_style} width: {pct}%;",
                }
            }
        }
    }
}

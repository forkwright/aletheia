//! Tool execution status indicator component.

use dioxus::prelude::*;

use crate::state::tools::ToolStatus;

const ICON_PENDING: &str = "\u{25CB}"; // ○
const ICON_SUCCESS: &str = "\u{2713}"; // ✓
const ICON_ERROR: &str = "\u{2717}"; // ✗

const PENDING_STYLE: &str = "\
    color: var(--text-muted); \
    font-size: var(--text-base);\
";

// WHY: CSS animation may not work in Blitz/Dioxus desktop webview;
// the pulsing opacity is a signal-driven fallback defined in the component.
const RUNNING_STYLE: &str = "\
    color: var(--accent); \
    font-size: var(--text-base);\
";

const SUCCESS_STYLE: &str = "\
    color: var(--status-success); \
    font-size: var(--text-base);\
";

const ERROR_STYLE: &str = "\
    color: var(--status-error); \
    font-size: var(--text-base);\
";

/// Render a tool status icon with color appropriate to the current state.
#[component]
pub(crate) fn ToolStatusIcon(status: ToolStatus) -> Element {
    match status {
        ToolStatus::Pending => rsx! {
            span { style: "{PENDING_STYLE}", "{ICON_PENDING}" }
        },
        ToolStatus::Running => {
            // WHY: signal-driven animation as fallback for CSS @keyframes
            // limitations in Blitz. Toggles opacity on a 500ms interval.
            let pulse = use_signal(|| true);
            let opacity = if *pulse.read() { "1.0" } else { "0.4" };
            let style = format!("{RUNNING_STYLE} opacity: {opacity};");

            use_future(move || {
                let mut pulse = pulse;
                async move {
                    loop {
                        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                        let current = *pulse.read();
                        pulse.set(!current);
                    }
                }
            });

            rsx! {
                span { style: "{style}", "\u{25CF}" } // ● filled circle
            }
        }
        ToolStatus::Success => rsx! {
            span { style: "{SUCCESS_STYLE}", "{ICON_SUCCESS}" }
        },
        ToolStatus::Error => rsx! {
            span { style: "{ERROR_STYLE}", "{ICON_ERROR}" }
        },
    }
}

/// CSS color string for a tool status, using design tokens.
#[cfg_attr(not(test), expect(dead_code, reason = "used in tests"))]
fn status_color(status: ToolStatus) -> &'static str {
    match status {
        ToolStatus::Pending => "var(--text-muted)",
        ToolStatus::Running => "var(--accent)",
        ToolStatus::Success => "var(--status-success)",
        ToolStatus::Error => "var(--status-error)",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_color_returns_distinct_values() {
        let colors: Vec<&str> = [
            ToolStatus::Pending,
            ToolStatus::Running,
            ToolStatus::Success,
            ToolStatus::Error,
        ]
        .iter()
        .map(|s| status_color(*s))
        .collect();

        // WHY: verify no two statuses share the same color.
        for (i, a) in colors.iter().enumerate() {
            for b in colors.iter().skip(i + 1) {
                assert_ne!(a, b, "status colors must be distinct");
            }
        }
    }
}

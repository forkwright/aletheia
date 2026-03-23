//! Tool execution status indicator component.

use dioxus::prelude::*;

use crate::state::tools::ToolStatus;

const ICON_PENDING: &str = "\u{25CB}"; // ○
const ICON_SUCCESS: &str = "\u{2713}"; // ✓
const ICON_ERROR: &str = "\u{2717}";   // ✗

const PENDING_STYLE: &str = "\
    color: #666; \
    font-size: 14px;\
";

// WHY: CSS animation may not work in Blitz/Dioxus desktop webview;
// the pulsing opacity is a signal-driven fallback defined in the component.
const RUNNING_STYLE: &str = "\
    color: #4a4aff; \
    font-size: 14px;\
";

const SUCCESS_STYLE: &str = "\
    color: #22c55e; \
    font-size: 14px;\
";

const ERROR_STYLE: &str = "\
    color: #ef4444; \
    font-size: 14px;\
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

/// Select the style for a status, for use in contexts where the full
/// component is not needed (e.g. inline text styling).
#[must_use]
pub(crate) fn status_color(status: ToolStatus) -> &'static str {
    match status {
        ToolStatus::Pending => "#666",
        ToolStatus::Running => "#4a4aff",
        ToolStatus::Success => "#22c55e",
        ToolStatus::Error => "#ef4444",
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

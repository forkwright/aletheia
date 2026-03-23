//! Dual-handle timeline scrubber for selecting a date range.

use dioxus::prelude::*;

const SCRUBBER_CONTAINER: &str = "\
    display: flex; \
    flex-direction: column; \
    gap: 8px; \
    padding: 12px 16px; \
    background: #1a1a2e; \
    border: 1px solid #333; \
    border-radius: 8px;\
";

const TRACK_ROW: &str = "\
    position: relative; \
    height: 32px; \
    display: flex; \
    align-items: center;\
";

const TRACK_BG: &str = "\
    position: absolute; \
    left: 0; \
    right: 0; \
    height: 4px; \
    background: #2a2a3a; \
    border-radius: 2px;\
";

const HANDLE_STYLE: &str = "\
    width: 14px; \
    height: 14px; \
    border-radius: 50%; \
    background: #9A7B4F; \
    border: 2px solid #c4a06a; \
    cursor: pointer; \
    position: absolute; \
    top: 50%; \
    transform: translate(-50%, -50%); \
    z-index: 2;\
";

const DATE_LABEL: &str = "\
    font-size: 11px; \
    color: #888;\
";

const DATE_INPUT: &str = "\
    background: #12110f; \
    border: 1px solid #333; \
    border-radius: 4px; \
    padding: 4px 8px; \
    color: #e0e0e0; \
    font-size: 12px; \
    font-family: inherit;\
";

/// Horizontal range scrubber with since/until handles and date inputs.
#[component]
pub(crate) fn TimelineScrubber(
    min_date: String,
    max_date: String,
    since: String,
    until: String,
    on_since_change: EventHandler<String>,
    on_until_change: EventHandler<String>,
) -> Element {
    let total_days = days_between_simple(&min_date, &max_date).max(1);
    let since_days = days_between_simple(&min_date, &since);
    let until_days = days_between_simple(&min_date, &until);

    let since_pct = (since_days as f64 / total_days as f64 * 100.0).clamp(0.0, 100.0);
    let until_pct = (until_days as f64 / total_days as f64 * 100.0).clamp(0.0, 100.0);
    let highlight_left = since_pct;
    let highlight_width = (until_pct - since_pct).max(0.0);

    rsx! {
        div {
            style: "{SCRUBBER_CONTAINER}",

            // Date input row
            div {
                style: "display: flex; align-items: center; gap: 12px;",
                span { style: "{DATE_LABEL}", "From" }
                input {
                    style: "{DATE_INPUT}",
                    r#type: "date",
                    value: "{since}",
                    min: "{min_date}",
                    max: "{until}",
                    onchange: move |evt: Event<FormData>| {
                        on_since_change.call(evt.value().clone());
                    },
                }
                span { style: "{DATE_LABEL}", "To" }
                input {
                    style: "{DATE_INPUT}",
                    r#type: "date",
                    value: "{until}",
                    min: "{since}",
                    max: "{max_date}",
                    onchange: move |evt: Event<FormData>| {
                        on_until_change.call(evt.value().clone());
                    },
                }
            }

            // Visual track
            div {
                style: "{TRACK_ROW}",

                // Background track
                div { style: "{TRACK_BG}" }

                // Highlighted range
                div {
                    style: "position: absolute; left: {highlight_left}%; width: {highlight_width}%; height: 4px; background: #9A7B4F; border-radius: 2px; opacity: 0.6;",
                }

                // Since handle
                div {
                    style: "{HANDLE_STYLE} left: {since_pct}%;",
                    title: "Since: {since}",
                }

                // Until handle
                div {
                    style: "{HANDLE_STYLE} left: {until_pct}%;",
                    title: "Until: {until}",
                }
            }

            // Date range labels
            div {
                style: "display: flex; justify-content: space-between;",
                span { style: "{DATE_LABEL}", "{min_date}" }
                span { style: "{DATE_LABEL}", "{max_date}" }
            }
        }
    }
}

/// Simple day count between two `YYYY-MM-DD` strings.
fn days_between_simple(start: &str, end: &str) -> u32 {
    let s = parse_date_to_days(start);
    let e = parse_date_to_days(end);
    e.saturating_sub(s)
}

/// Convert `YYYY-MM-DD` to approximate days since epoch for relative positioning.
fn parse_date_to_days(date: &str) -> u32 {
    let parts: Vec<&str> = date.split('-').collect();
    if parts.len() != 3 {
        return 0;
    }
    let y: u32 = parts[0].parse().unwrap_or(0);
    let m: u32 = parts[1].parse().unwrap_or(1);
    let d: u32 = parts[2].parse().unwrap_or(1);
    // WHY: Rough approximation is sufficient for slider positioning.
    y * 365 + m * 30 + d
}

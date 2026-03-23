//! Wave band component for execution progress display.

use dioxus::prelude::*;

use crate::state::execution::{Wave, WaveStatus};

const BAND_BASE: &str = "\
    border-radius: 8px; \
    border: 1px solid; \
    padding: 12px 16px; \
    margin-bottom: 8px;\
";

const BAND_HEADER: &str = "\
    display: flex; \
    align-items: center; \
    justify-content: space-between; \
    margin-bottom: 8px;\
";

const WAVE_LABEL: &str = "\
    font-size: 14px; \
    font-weight: 600; \
    color: #e0e0e0;\
";

const BADGE_BASE: &str = "\
    display: inline-block; \
    font-size: 10px; \
    font-weight: 600; \
    padding: 2px 8px; \
    border-radius: 10px; \
    text-transform: uppercase; \
    letter-spacing: 0.3px;\
";

const TIME_STYLE: &str = "\
    font-size: 11px; \
    color: #666;\
";

const PROGRESS_TRACK: &str = "\
    height: 4px; \
    background: #2a2a3a; \
    border-radius: 2px; \
    margin-bottom: 10px; \
    overflow: hidden;\
";

const PLANS_ROW: &str = "\
    display: flex; \
    gap: 12px; \
    flex-wrap: wrap;\
";

/// Wave band container showing wave header, progress bar, and child plan cards.
#[component]
pub(crate) fn WaveBand(wave: Wave, children: Element) -> Element {
    let (bg, border_color) = band_colors(wave.status);
    let band_style = format!("{BAND_BASE} background: {bg}; border-color: {border_color};");
    let badge_style = status_badge_style(wave.status);
    let label = status_label(wave.status);
    let progress = wave.progress_pct();

    rsx! {
        div {
            style: "{band_style}",

            // Header
            div {
                style: "{BAND_HEADER}",
                div {
                    style: "display: flex; align-items: center; gap: 8px;",
                    span { style: "{WAVE_LABEL}", "Wave {wave.wave_number}" }
                    span { style: "{badge_style}", "{label}" }
                }
                div {
                    style: "display: flex; align-items: center; gap: 12px;",
                    if let Some(ref start) = wave.start_time {
                        span { style: "{TIME_STYLE}", "Started: {start}" }
                    }
                    if let Some(ref end) = wave.end_time {
                        span { style: "{TIME_STYLE}", "Ended: {end}" }
                    }
                    span { style: "font-size: 12px; color: #aaa;", "{progress}%" }
                }
            }

            // Progress bar
            div {
                style: "{PROGRESS_TRACK}",
                div {
                    style: "height: 100%; background: {progress_color(wave.status)}; width: {progress}%; border-radius: 2px; transition: width 0.3s;",
                }
            }

            // Plan cards row
            div {
                style: "{PLANS_ROW}",
                {children}
            }
        }
    }
}

fn band_colors(status: WaveStatus) -> (&'static str, &'static str) {
    match status {
        WaveStatus::Active => ("#1a1a2e", "#4a9aff"),
        WaveStatus::Complete => ("#0f1a0f", "#2a4a2a"),
        WaveStatus::Pending => ("#151520", "#2a2a3a"),
    }
}

fn progress_color(status: WaveStatus) -> &'static str {
    match status {
        WaveStatus::Active => "#4a9aff",
        WaveStatus::Complete => "#22c55e",
        WaveStatus::Pending => "#444",
    }
}

#[must_use]
pub(crate) fn status_badge_style(status: WaveStatus) -> String {
    let (bg, color) = match status {
        WaveStatus::Active => ("#1e1e5a", "#4a9aff"),
        WaveStatus::Complete => ("#0f2a0f", "#22c55e"),
        WaveStatus::Pending => ("#2a2a3a", "#666"),
    };
    format!("{BADGE_BASE} background: {bg}; color: {color};")
}

#[must_use]
pub(crate) fn status_label(status: WaveStatus) -> &'static str {
    match status {
        WaveStatus::Pending => "Pending",
        WaveStatus::Active => "Active",
        WaveStatus::Complete => "Complete",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn band_colors_differ_by_status() {
        let active = band_colors(WaveStatus::Active);
        let complete = band_colors(WaveStatus::Complete);
        let pending = band_colors(WaveStatus::Pending);
        assert_ne!(active, complete);
        assert_ne!(complete, pending);
    }

    #[test]
    fn status_labels_are_distinct() {
        let labels: Vec<_> = [
            WaveStatus::Pending,
            WaveStatus::Active,
            WaveStatus::Complete,
        ]
        .iter()
        .map(|s| status_label(*s))
        .collect();
        let unique: std::collections::HashSet<_> = labels.iter().collect();
        assert_eq!(
            unique.len(),
            labels.len(),
            "all wave labels must be distinct"
        );
    }

    #[test]
    fn progress_color_differs_by_status() {
        assert_ne!(
            progress_color(WaveStatus::Active),
            progress_color(WaveStatus::Complete)
        );
        assert_ne!(
            progress_color(WaveStatus::Complete),
            progress_color(WaveStatus::Pending)
        );
    }
}

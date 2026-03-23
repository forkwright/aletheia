//! Coverage bar component with color-coded threshold display.

use dioxus::prelude::*;

const BAR_OUTER_STYLE: &str = "\
    width: 100%; \
    height: 8px; \
    background: #2a2a3a; \
    border-radius: 4px; \
    overflow: hidden;\
";

const LABEL_ROW_STYLE: &str = "\
    display: flex; \
    justify-content: space-between; \
    align-items: center; \
    margin-bottom: 4px; \
    font-size: 12px; \
    color: #aaa;\
";

const NO_REQS_STYLE: &str = "font-size: 12px; color: #555; font-style: italic;";

/// Progress bar showing requirement coverage for a category.
///
/// `coverage` is `None` when no requirements exist for the category,
/// which renders a "No requirements defined" placeholder instead of a 0% bar.
#[component]
pub(crate) fn CoverageBar(
    /// Coverage percentage 0–100. `None` = no requirements defined.
    coverage: Option<u8>,
    /// Display label for this bar (e.g., `"Overall"`, `"v1"`, `"v2"`).
    label: String,
) -> Element {
    let Some(pct) = coverage else {
        return rsx! {
            div {
                style: "margin-bottom: 12px;",
                div {
                    style: "{LABEL_ROW_STYLE}",
                    span { "{label}" }
                }
                span { style: "{NO_REQS_STYLE}", "No requirements defined" }
            }
        };
    };

    let color = coverage_color(pct);
    let bar_inner_style = format!(
        "height: 100%; width: {pct}%; background: {color}; border-radius: 4px; \
         transition: width 0.3s ease;"
    );

    rsx! {
        div {
            style: "margin-bottom: 12px;",
            div {
                style: "{LABEL_ROW_STYLE}",
                span { "{label}" }
                span {
                    style: "color: {color}; font-weight: 600;",
                    "{pct}%"
                }
            }
            div {
                style: "{BAR_OUTER_STYLE}",
                div { style: "{bar_inner_style}" }
            }
        }
    }
}

/// Color for a coverage percentage.
///
/// - `>80%` → green
/// - `50–80%` → amber
/// - `<50%` → red
#[must_use]
pub(crate) fn coverage_color(pct: u8) -> &'static str {
    if pct > 80 {
        "#22c55e"
    } else if pct >= 50 {
        "#f59e0b"
    } else {
        "#ef4444"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn coverage_color_green_above_80() {
        assert_eq!(coverage_color(81), "#22c55e");
        assert_eq!(coverage_color(100), "#22c55e");
    }

    #[test]
    fn coverage_color_amber_at_boundary() {
        assert_eq!(coverage_color(80), "#f59e0b");
        assert_eq!(coverage_color(50), "#f59e0b");
        assert_eq!(coverage_color(65), "#f59e0b");
    }

    #[test]
    fn coverage_color_red_below_50() {
        assert_eq!(coverage_color(49), "#ef4444");
        assert_eq!(coverage_color(0), "#ef4444");
    }
}

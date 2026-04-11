//! Confidence bar component with color-coded thresholds.

use dioxus::prelude::*;

use crate::state::memory::confidence_color;

const BAR_OUTER_STYLE: &str = "\
    height: 8px; \
    background: var(--border); \
    border-radius: var(--radius-sm); \
    overflow: hidden; \
    flex: 1;\
";

const WRAPPER_STYLE: &str = "\
    display: flex; \
    align-items: center; \
    gap: var(--space-2);\
";

/// Horizontal confidence bar filled proportionally to a 0.0--1.0 value.
///
/// Color: green (>0.7), amber (0.4--0.7), red (<0.4).
/// Shows numeric percentage alongside the bar.
#[component]
pub(crate) fn ConfidenceBar(
    /// Confidence value between 0.0 and 1.0.
    value: f64,
    /// Optional fixed width for the bar container. Defaults to flex: 1.
    #[props(default = None)]
    width: Option<&'static str>,
) -> Element {
    let clamped = value.clamp(0.0, 1.0);
    #[expect(
        clippy::cast_sign_loss,
        reason = "clamped to 0.0–1.0, always non-negative"
    )]
    #[expect(
        clippy::cast_possible_truncation,
        reason = "percentage 0–100 fits in u8"
    )]
    #[expect(clippy::as_conversions, reason = "percentage 0–100 from clamped 0.0–1.0")]
    let pct = (clamped * 100.0) as u8;
    let color = confidence_color(clamped);

    let bar_inner = format!(
        "height: 100%; width: {pct}%; background: {color}; border-radius: var(--radius-sm); \
         transition: width var(--transition-measured);"
    );

    let outer = match width {
        Some(w) => format!("{BAR_OUTER_STYLE} width: {w};"),
        None => BAR_OUTER_STYLE.to_string(),
    };

    rsx! {
        div {
            style: "{WRAPPER_STYLE}",
            div {
                style: "{outer}",
                div { style: "{bar_inner}" }
            }
            span {
                style: "font-size: var(--text-xs); color: {color}; font-weight: var(--weight-semibold); min-width: 36px; text-align: right;",
                "{pct}%"
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::state::memory::confidence_color;

    #[test]
    fn confidence_bar_color_green() {
        assert_eq!(confidence_color(0.8), "var(--status-success)");
        assert_eq!(confidence_color(1.0), "var(--status-success)");
    }

    #[test]
    fn confidence_bar_color_amber() {
        assert_eq!(confidence_color(0.5), "var(--status-warning)");
        assert_eq!(confidence_color(0.7), "var(--status-warning)");
    }

    #[test]
    fn confidence_bar_color_red() {
        assert_eq!(confidence_color(0.0), "var(--status-error)");
        assert_eq!(confidence_color(0.39), "var(--status-error)");
    }
}

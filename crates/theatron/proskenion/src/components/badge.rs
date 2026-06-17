//! Shared badge style helpers for wave, checkpoint, and tool approval components.

/// Base CSS for inline status badges.
pub(crate) const BADGE_BASE: &str = "\
    display: inline-block; \
    font-size: var(--text-xs); \
    font-weight: var(--weight-semibold); \
    padding: var(--space-1) var(--space-2); \
    border-radius: var(--radius-lg); \
    text-transform: uppercase; \
    letter-spacing: 0.4px;\
";

/// Build a badge style string with the given background and foreground color tokens.
#[must_use]
pub(crate) fn status_badge_style(bg: &str, color: &str) -> String {
    format!("{BADGE_BASE} background: {bg}; color: {color};")
}

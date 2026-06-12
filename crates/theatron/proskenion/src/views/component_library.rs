//! Dev-only component library reference. Only compiled in debug builds (`#[cfg(debug_assertions)]`). Not part of the operator nav.

use dioxus::prelude::*;

use crate::components::chart::{ChartEntry, DonutChart, HorizBarChart};
use crate::components::confidence_bar::ConfidenceBar;
use crate::components::coverage_bar::CoverageBar;
use crate::components::markdown::Markdown;
use crate::components::thinking::ThinkingPanel;
use crate::components::tool_status::ToolStatusIcon;
use crate::state::tools::ToolStatus;

const PAGE_STYLE: &str = "\
    display: flex; \
    flex-direction: column; \
    gap: var(--space-6); \
    height: 100%; \
    overflow-y: auto; \
    padding: var(--space-6); \
    background: var(--bg); \
    color: var(--text-primary);\
";

const HEADER_STYLE: &str = "\
    display: flex; \
    justify-content: space-between; \
    align-items: flex-start; \
    gap: var(--space-4); \
    border-bottom: 1px solid var(--border-separator); \
    padding-bottom: var(--space-4);\
";

const GRID_STYLE: &str = "\
    display: grid; \
    grid-template-columns: repeat(auto-fit, minmax(360px, 1fr)); \
    gap: var(--space-4);\
";

const SECTION_STYLE: &str = "\
    background: var(--bg-surface); \
    border: 1px solid var(--border); \
    border-radius: var(--radius-lg); \
    padding: var(--space-4); \
    box-shadow: var(--shadow-card); \
    min-width: 0; \
    contain: layout;\
";

const SECTION_TITLE_STYLE: &str = "\
    margin: 0 0 var(--space-3); \
    font-size: var(--text-md); \
    font-weight: var(--weight-semibold); \
    color: var(--text-primary);\
";

const ROW_STYLE: &str = "\
    display: flex; \
    flex-wrap: wrap; \
    align-items: center; \
    gap: var(--space-2); \
    margin-bottom: var(--space-3);\
";

const LABEL_STYLE: &str = "\
    min-width: 92px; \
    color: var(--text-muted); \
    font-size: var(--text-xs); \
    font-family: var(--font-mono); \
";

const SWATCH_STYLE: &str = "\
    width: 96px; \
    height: 56px; \
    border: 1px solid var(--border); \
    border-radius: var(--radius-md); \
    display: flex; \
    align-items: flex-end; \
    padding: var(--space-2); \
    font-size: var(--text-xs); \
    color: var(--text-primary); \
";

/// Routed reference page showing the proskenion component surface in both themes.
#[component]
pub(crate) fn ComponentLibrary() -> Element {
    rsx! {
        div {
            style: "{PAGE_STYLE}",
            header {
                style: "{HEADER_STYLE}",
                div {
                    h1 {
                        style: "margin: 0; font-size: var(--text-2xl); font-weight: var(--weight-bold);",
                        "Component Library"
                    }
                    p {
                        style: "margin: var(--space-2) 0 0; color: var(--text-secondary); max-width: 760px; line-height: var(--leading-normal);",
                        "Reference states for proskenion surfaces, controls, feedback, data display, chat, planning, and tool execution."
                    }
                }
                div {
                    style: "display: flex; gap: var(--space-2); flex-wrap: wrap; justify-content: flex-end;",
                    StatusPill { label: "default" }
                    StatusPill { label: "hover" }
                    StatusPill { label: "active" }
                    StatusPill { label: "disabled" }
                    StatusPill { label: "focus" }
                }
            }

            div {
                style: "{GRID_STYLE}",
                ThemeFrame { theme: "dark", title: "Dark Theme" }
                ThemeFrame { theme: "light", title: "Light Theme" }
            }
        }
    }
}

#[component]
fn ThemeFrame(theme: &'static str, title: &'static str) -> Element {
    rsx! {
        div {
            "data-theme": "{theme}",
            // WHY: layout containment isolates each theme frame's layout so a
            // scroll repaint or unrelated re-render doesn't force the sibling
            // frame to recalc. No paint containment: the inner section cards
            // cast box-shadow that must paint past their box. PERF: bounds
            // layout recalc when rendering the full surface twice.
            style: "\
                background: var(--bg); \
                color: var(--text-primary); \
                border: 1px solid var(--border); \
                border-radius: var(--radius-lg); \
                padding: var(--space-4); \
                display: flex; \
                flex-direction: column; \
                gap: var(--space-4); \
                contain: layout;\
            ",
            h2 {
                style: "margin: 0; font-size: var(--text-lg); color: var(--text-primary);",
                "{title}"
            }
            SurfaceSection {}
            ControlSection {}
            FeedbackSection {}
            DataSection {}
            MessageSection {}
        }
    }
}

#[component]
fn SurfaceSection() -> Element {
    rsx! {
        section {
            style: "{SECTION_STYLE}",
            h3 { style: "{SECTION_TITLE_STYLE}", "Surfaces" }
            div {
                style: "{ROW_STYLE}",
                div { style: "{SWATCH_STYLE} background: var(--bg);", "--bg" }
                div { style: "{SWATCH_STYLE} background: var(--bg-surface);", "--surface" }
                div { style: "{SWATCH_STYLE} background: var(--bg-surface-bright);", "--bright" }
                div { style: "{SWATCH_STYLE} background: var(--bg-surface-dim);", "--dim" }
            }
            div {
                style: "{ROW_STYLE}",
                TokenChip { name: "primary", value: "var(--text-primary)" }
                TokenChip { name: "secondary", value: "var(--text-secondary)" }
                TokenChip { name: "muted", value: "var(--text-muted)" }
                TokenChip { name: "accent", value: "var(--accent)" }
            }
        }
    }
}

#[component]
fn ControlSection() -> Element {
    rsx! {
        section {
            style: "{SECTION_STYLE}",
            h3 { style: "{SECTION_TITLE_STYLE}", "Controls" }
            StateRow {
                label: "buttons",
                ButtonDemo { label: "Default", style_kind: "default", disabled: false }
                ButtonDemo { label: "Hover", style_kind: "hover", disabled: false }
                ButtonDemo { label: "Active", style_kind: "active", disabled: false }
                ButtonDemo { label: "Disabled", style_kind: "disabled", disabled: true }
                ButtonDemo { label: "Focus", style_kind: "focus", disabled: false }
            }
            StateRow {
                label: "inputs",
                input {
                    style: "flex: 1; min-width: 180px; background: var(--input-bg); color: var(--text-primary); border: 1px solid var(--input-border); border-radius: var(--radius-md); padding: var(--space-2) var(--space-3);",
                    value: "Default input",
                    readonly: true,
                }
                input {
                    style: "flex: 1; min-width: 180px; background: var(--input-bg); color: var(--text-primary); border: 1px solid var(--input-border-focus); box-shadow: var(--shadow-glow); border-radius: var(--radius-md); padding: var(--space-2) var(--space-3);",
                    value: "Focused input",
                    readonly: true,
                }
                input {
                    style: "flex: 1; min-width: 180px; background: var(--bg-surface-dim); color: var(--text-muted); border: 1px solid var(--border); border-radius: var(--radius-md); padding: var(--space-2) var(--space-3);",
                    value: "Disabled input",
                    disabled: true,
                }
            }
            StateRow {
                label: "badges",
                ToneBadge { label: "success", bg: "var(--status-success-bg)", fg: "var(--status-success)" }
                ToneBadge { label: "warning", bg: "var(--status-warning-bg)", fg: "var(--status-warning)" }
                ToneBadge { label: "error", bg: "var(--status-error-bg)", fg: "var(--status-error)" }
                ToneBadge { label: "info", bg: "var(--status-info-bg)", fg: "var(--status-info)" }
            }
        }
    }
}

#[component]
fn FeedbackSection() -> Element {
    rsx! {
        section {
            style: "{SECTION_STYLE}",
            h3 { style: "{SECTION_TITLE_STYLE}", "Feedback" }
            StateRow {
                label: "status",
                ToolStatusIcon { status: ToolStatus::Pending }
                ToolStatusIcon { status: ToolStatus::Running }
                ToolStatusIcon { status: ToolStatus::Success }
                ToolStatusIcon { status: ToolStatus::Error }
            }
            StateRow {
                label: "confidence",
                div { style: "width: 260px;", ConfidenceBar { value: 0.82, width: Some("180px") } }
                div { style: "width: 260px;", ConfidenceBar { value: 0.57, width: Some("180px") } }
                div { style: "width: 260px;", ConfidenceBar { value: 0.31, width: Some("180px") } }
            }
            div {
                style: "display: grid; grid-template-columns: repeat(auto-fit, minmax(180px, 1fr)); gap: var(--space-3);",
                CoverageBar { coverage: Some(94), label: "Complete".to_string() }
                CoverageBar { coverage: Some(63), label: "Partial".to_string() }
                CoverageBar { coverage: Some(28), label: "At risk".to_string() }
                CoverageBar { coverage: None, label: "Empty".to_string() }
            }
        }
    }
}

#[component]
fn DataSection() -> Element {
    // WHY: Chart data is fixed reference content. Memoizing keeps the
    // Vec/String allocations off the per-render path so re-renders triggered by
    // unrelated global signal ticks stay cheap. PERF: re-render no longer
    // rebuilds six ChartEntry values per frame.
    let entries = use_memo(|| {
        vec![
            chart_entry("Tokens", 48.0, "var(--accent)", Some("48k")),
            chart_entry("Tools", 32.0, "var(--status-info)", Some("32")),
            chart_entry("Errors", 8.0, "var(--status-error)", Some("8")),
        ]
    });
    let donut = use_memo(|| {
        vec![
            chart_entry("Prompt", 58.0, "var(--accent)", None),
            chart_entry("Completion", 31.0, "var(--status-success)", None),
            chart_entry("Tools", 11.0, "var(--status-info)", None),
        ]
    });

    rsx! {
        section {
            style: "{SECTION_STYLE}",
            h3 { style: "{SECTION_TITLE_STYLE}", "Data Display" }
            div {
                style: "display: grid; grid-template-columns: minmax(0, 1fr) 180px; gap: var(--space-4); align-items: center;",
                HorizBarChart {
                    entries: entries.read().clone(),
                    max_value: 60.0,
                    show_value: true,
                    on_click: None::<EventHandler<String>>,
                }
                DonutChart {
                    segments: donut.read().clone(),
                    size_px: 128,
                    center_label: "Usage".to_string(),
                }
            }
        }
    }
}

#[component]
fn MessageSection() -> Element {
    rsx! {
        section {
            style: "{SECTION_STYLE}",
            h3 { style: "{SECTION_TITLE_STYLE}", "Message and Reasoning" }
            div {
                style: "display: flex; flex-direction: column; gap: var(--space-3);",
                div {
                    style: "\
                        background: var(--bg-surface-bright); \
                        border: 1px solid var(--border); \
                        border-left: 3px solid var(--role-assistant); \
                        border-radius: var(--radius-lg); \
                        padding: var(--space-3) var(--space-4);\
                    ",
                    Markdown { content: "**Assistant message** with `inline code` and a short list:\n\n- confirm state\n- ship verifier".to_string() }
                }
                div {
                    style: "\
                        background: var(--bg-surface); \
                        border: 1px solid var(--border); \
                        border-left: 3px solid var(--accent); \
                        border-radius: var(--radius-lg); \
                        padding: var(--space-3) var(--space-4);\
                    ",
                    "User message bubble with long text wrapping across multiple lines without changing the column width."
                }
                ThinkingPanel {
                    content: "Reconcile local evidence, choose the smallest route, then verify the result.".to_string(),
                    is_streaming: true,
                }
            }
        }
    }
}

#[component]
fn StateRow(label: &'static str, children: Element) -> Element {
    rsx! {
        div {
            style: "{ROW_STYLE}",
            span { style: "{LABEL_STYLE}", "{label}" }
            {children}
        }
    }
}

#[component]
fn ButtonDemo(label: &'static str, style_kind: &'static str, disabled: bool) -> Element {
    let style = match style_kind {
        "hover" => {
            "background: var(--accent-hover); color: var(--text-inverse); border: 1px solid var(--accent-hover); transform: translateY(-1px);"
        }
        "active" => {
            "background: var(--accent-dim); color: var(--text-inverse); border: 1px solid var(--accent-dim);"
        }
        "disabled" => {
            "background: var(--bg-surface-dim); color: var(--text-muted); border: 1px solid var(--border); cursor: not-allowed;"
        }
        "focus" => {
            "background: var(--accent); color: var(--text-inverse); border: 1px solid var(--border-focused); box-shadow: var(--shadow-glow);"
        }
        _ => {
            "background: var(--accent); color: var(--text-inverse); border: 1px solid var(--accent);"
        }
    };

    rsx! {
        button {
            disabled,
            style: "{style} border-radius: var(--radius-md); padding: var(--space-2) var(--space-3); font-size: var(--text-sm); font-weight: var(--weight-semibold); min-width: 92px;",
            "{label}"
        }
    }
}

#[component]
fn StatusPill(label: &'static str) -> Element {
    rsx! {
        span {
            style: "font-size: var(--text-xs); color: var(--text-secondary); border: 1px solid var(--border); border-radius: var(--radius-full); padding: var(--space-1) var(--space-2); background: var(--bg-surface);",
            "{label}"
        }
    }
}

#[component]
fn ToneBadge(label: &'static str, bg: &'static str, fg: &'static str) -> Element {
    rsx! {
        span {
            style: "background: {bg}; color: {fg}; border: 1px solid {fg}; border-radius: var(--radius-md); padding: var(--space-1) var(--space-2); font-size: var(--text-xs); font-weight: var(--weight-semibold);",
            "{label}"
        }
    }
}

#[component]
fn TokenChip(name: &'static str, value: &'static str) -> Element {
    rsx! {
        div {
            style: "display: flex; align-items: center; gap: var(--space-2); border: 1px solid var(--border); border-radius: var(--radius-md); padding: var(--space-2); background: var(--bg-surface-bright);",
            span { style: "width: 12px; height: 12px; border-radius: var(--radius-sm); background: {value}; border: 1px solid var(--border);" }
            span { style: "font-size: var(--text-xs); color: var(--text-secondary); font-family: var(--font-mono);", "{name}" }
        }
    }
}

fn chart_entry(label: &str, value: f64, color: &str, sub_label: Option<&str>) -> ChartEntry {
    ChartEntry {
        label: label.to_string(),
        value,
        color: color.to_string(),
        sub_label: sub_label.map(str::to_string),
    }
}

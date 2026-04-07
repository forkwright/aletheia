//! Component library visual reference page.
//!
//! Renders every reusable component from `crate::components` in a labeled,
//! browsable reference page. Serves as the source of truth for all desktop
//! UI work.

use dioxus::prelude::*;

use crate::components::checkpoint_card::CheckpointCard;
use crate::components::confidence_bar::ConfidenceBar;
use crate::components::connection_indicator::IndicatorColor;
use crate::components::option_card::OptionCard;
use crate::components::resize_handle::{ResizeHandle, ResizeDir, ResizeState};
use crate::components::theme_toggle::ThemeToggle;
use crate::components::toast::ToastItem;
use crate::components::tool_approval::ToolApproval;
use crate::components::tool_status::ToolStatusIcon;
use crate::state::checkpoints::{Checkpoint, CheckpointStatus, CheckpointRequirement, CheckpointArtifact};
use crate::state::discussion::DiscussionOption;
use crate::state::toasts::{Severity, Toast, ToastAction};
use crate::state::tools::{RiskLevel, ToolApprovalState, ToolStatus};
use crate::theme::ThemeMode;

/// Main reference view component.
#[component]
pub(crate) fn Reference() -> Element {
    // Provide mock context for components that need it
    use_context_provider(|| Signal::new(ThemeMode::Dark));
    
    rsx! {
        div {
            style: "
                display: flex;
                flex-direction: column;
                height: 100%;
                overflow-y: auto;
                padding: var(--space-6);
                gap: var(--space-8);
                background: var(--bg);
                color: var(--text-primary);
                font-family: var(--font-body);
            ",
            
            // Header
            div {
                style: "
                    border-bottom: 1px solid var(--border);
                    padding-bottom: var(--space-4);
                    margin-bottom: var(--space-4);
                ",
                h1 {
                    style: "
                        font-family: var(--font-display);
                        font-size: var(--text-3xl);
                        font-weight: var(--weight-bold);
                        color: var(--text-primary);
                        margin: 0;
                    ",
                    "Component Library Reference"
                }
                p {
                    style: "
                        font-size: var(--text-sm);
                        color: var(--text-secondary);
                        margin: var(--space-2) 0 0 0;
                    ",
                    "Visual reference for all reusable UI components. Issue #2412."
                }
            }
            
            // Typography Section
            TypographySection {}
            
            // Buttons Section
            ButtonsSection {}
            
            // Inputs Section
            InputsSection {}
            
            // Cards Section
            CardsSection {}
            
            // Status Indicators Section
            StatusIndicatorsSection {}
            
            // Data Display Section
            DataDisplaySection {}
            
            // Layout Components Section
            LayoutSection {}
            
            // Feedback Components Section
            FeedbackSection {}
        }
    }
}

#[component]
fn SectionHeader(title: &'static str) -> Element {
    rsx! {
        h2 {
            style: "
                font-family: var(--font-display);
                font-size: var(--text-2xl);
                font-weight: var(--weight-semibold);
                color: var(--text-primary);
                margin: 0 0 var(--space-4) 0;
                padding-bottom: var(--space-2);
                border-bottom: 1px solid var(--border-separator);
            ",
            "{title}"
        }
    }
}

#[component]
fn ComponentLabel(name: &'static str) -> Element {
    rsx! {
        div {
            style: "
                font-family: var(--font-mono);
                font-size: var(--text-xs);
                color: var(--text-muted);
                text-transform: uppercase;
                letter-spacing: 0.5px;
                margin-bottom: var(--space-2);
            ",
            "{name}"
        }
    }
}

#[component]
fn ShowcaseItem(name: &'static str, children: Element) -> Element {
    rsx! {
        div {
            style: "
                background: var(--bg-surface);
                border: 1px solid var(--border);
                border-radius: var(--radius-lg);
                padding: var(--space-4);
            ",
            ComponentLabel { name }
            {children}
        }
    }
}

// =============================================================================
// Typography Section
// =============================================================================

#[component]
fn TypographySection() -> Element {
    rsx! {
        section {
            SectionHeader { title: "Typography" }
            
            div {
                style: "
                    display: grid;
                    grid-template-columns: repeat(auto-fit, minmax(300px, 1fr));
                    gap: var(--space-4);
                ",
                
                ShowcaseItem { name: "Display Font (Cormorant Garamond)",
                    div {
                        style: "font-family: var(--font-display);",
                        h1 { style: "font-size: var(--text-3xl); margin: 0;", "Heading 1" }
                        h2 { style: "font-size: var(--text-2xl); margin: var(--space-2) 0 0 0;", "Heading 2" }
                        h3 { style: "font-size: var(--text-xl); margin: var(--space-2) 0 0 0;", "Heading 3" }
                    }
                }
                
                ShowcaseItem { name: "Body Font (System UI)",
                    div {
                        p { style: "font-size: var(--text-lg); margin: 0;", "Large text (text-lg)" }
                        p { style: "font-size: var(--text-base); margin: var(--space-2) 0 0 0;", "Base text (text-base)" }
                        p { style: "font-size: var(--text-sm); margin: var(--space-2) 0 0 0;", "Small text (text-sm)" }
                        p { style: "font-size: var(--text-xs); margin: var(--space-2) 0 0 0;", "Extra small (text-xs)" }
                    }
                }
                
                ShowcaseItem { name: "Monospace (IBM Plex Mono)",
                    div {
                        style: "font-family: var(--font-mono);",
                        div { style: "color: var(--code-fg);", "code block styling" }
                        div { style: "color: var(--syntax-keyword); margin-top: var(--space-1);", "keyword color" }
                        div { style: "color: var(--syntax-string); margin-top: var(--space-1);", "string color" }
                        div { style: "color: var(--syntax-function); margin-top: var(--space-1);", "function color" }
                    }
                }
                
                ShowcaseItem { name: "Font Weights",
                    div {
                        div { style: "font-weight: var(--weight-normal);", "Normal (400)" }
                        div { style: "font-weight: var(--weight-medium); margin-top: var(--space-1);", "Medium (500)" }
                        div { style: "font-weight: var(--weight-semibold); margin-top: var(--space-1);", "Semibold (600)" }
                        div { style: "font-weight: var(--weight-bold); margin-top: var(--space-1);", "Bold (700)" }
                    }
                }
            }
        }
    }
}

// =============================================================================
// Buttons Section
// =============================================================================

#[component]
fn ButtonsSection() -> Element {
    let primary_btn = "
        background: var(--accent);
        color: var(--text-inverse);
        border: none;
        border-radius: var(--radius-md);
        padding: var(--space-2) var(--space-4);
        font-size: var(--text-sm);
        font-weight: var(--weight-semibold);
        cursor: pointer;
        transition: background var(--transition-quick);
    ";
    
    let secondary_btn = "
        background: var(--bg-surface-bright);
        color: var(--text-primary);
        border: 1px solid var(--border);
        border-radius: var(--radius-md);
        padding: var(--space-2) var(--space-4);
        font-size: var(--text-sm);
        font-weight: var(--weight-medium);
        cursor: pointer;
    ";
    
    let destructive_btn = "
        background: var(--status-error);
        color: var(--text-inverse);
        border: none;
        border-radius: var(--radius-md);
        padding: var(--space-2) var(--space-4);
        font-size: var(--text-sm);
        font-weight: var(--weight-semibold);
        cursor: pointer;
    ";
    
    let ghost_btn = "
        background: transparent;
        color: var(--text-secondary);
        border: none;
        border-radius: var(--radius-md);
        padding: var(--space-2) var(--space-4);
        font-size: var(--text-sm);
        font-weight: var(--weight-medium);
        cursor: pointer;
    ";
    
    let disabled_btn = "
        background: var(--bg-surface-dim);
        color: var(--text-muted);
        border: none;
        border-radius: var(--radius-md);
        padding: var(--space-2) var(--space-4);
        font-size: var(--text-sm);
        font-weight: var(--weight-medium);
        cursor: not-allowed;
    ";

    rsx! {
        section {
            SectionHeader { title: "Buttons" }
            
            div {
                style: "
                    display: grid;
                    grid-template-columns: repeat(auto-fit, minmax(200px, 1fr));
                    gap: var(--space-4);
                ",
                
                ShowcaseItem { name: "Primary Button",
                    button { style: "{primary_btn}", "Primary Action" }
                }
                
                ShowcaseItem { name: "Secondary Button",
                    button { style: "{secondary_btn}", "Secondary Action" }
                }
                
                ShowcaseItem { name: "Destructive Button",
                    button { style: "{destructive_btn}", "Destructive Action" }
                }
                
                ShowcaseItem { name: "Ghost Button",
                    button { style: "{ghost_btn}", "Ghost Action" }
                }
                
                ShowcaseItem { name: "Disabled Button",
                    button { style: "{disabled_btn}", disabled: true, "Disabled" }
                }
                
                ShowcaseItem { name: "Button Group",
                    div {
                        style: "display: flex; gap: var(--space-2);",
                        button { style: "{primary_btn}", "Save" }
                        button { style: "{secondary_btn}", "Cancel" }
                        button { style: "{destructive_btn}", "Delete" }
                    }
                }
            }
        }
    }
}

// =============================================================================
// Inputs Section
// =============================================================================

#[component]
fn InputsSection() -> Element {
    let input_style = "
        background: var(--input-bg);
        border: 1px solid var(--input-border);
        border-radius: var(--radius-md);
        padding: var(--space-2) var(--space-3);
        color: var(--text-primary);
        font-size: var(--text-sm);
        font-family: var(--font-body);
        width: 100%;
        box-sizing: border-box;
    ";
    
    let textarea_style = "
        background: var(--input-bg);
        border: 1px solid var(--input-border);
        border-radius: var(--radius-md);
        padding: var(--space-2) var(--space-3);
        color: var(--text-primary);
        font-size: var(--text-sm);
        font-family: var(--font-body);
        width: 100%;
        min-height: 80px;
        resize: vertical;
        box-sizing: border-box;
    ";

    rsx! {
        section {
            SectionHeader { title: "Inputs" }
            
            div {
                style: "
                    display: grid;
                    grid-template-columns: repeat(auto-fit, minmax(300px, 1fr));
                    gap: var(--space-4);
                ",
                
                ShowcaseItem { name: "Text Input",
                    input {
                        style: "{input_style}",
                        r#type: "text",
                        placeholder: "Enter text...",
                    }
                }
                
                ShowcaseItem { name: "Text Input (Focused)",
                    input {
                        style: "{input_style} border-color: var(--input-border-focus); outline: none;",
                        r#type: "text",
                        value: "Focused input",
                    }
                }
                
                ShowcaseItem { name: "Textarea",
                    textarea {
                        style: "{textarea_style}",
                        placeholder: "Enter multiple lines...",
                    }
                }
                
                ShowcaseItem { name: "Select",
                    select {
                        style: "{input_style}",
                        option { "Option 1" }
                        option { "Option 2" }
                        option { "Option 3" }
                    }
                }
                
                ShowcaseItem { name: "Disabled Input",
                    input {
                        style: "{input_style} opacity: 0.5; cursor: not-allowed;",
                        r#type: "text",
                        value: "Disabled",
                        disabled: true,
                    }
                }
            }
        }
    }
}

// =============================================================================
// Cards Section
// =============================================================================

#[component]
fn CardsSection() -> Element {
    // Mock checkpoint data
    let checkpoint = Checkpoint {
        id: "chk-123".to_string(),
        project_id: "proj-1".to_string(),
        title: "Review API Changes".to_string(),
        description: "Review and approve the breaking changes to the public API.".to_string(),
        context: "This checkpoint gates the release of v2.0.".to_string(),
        status: CheckpointStatus::Pending,
        requirements: vec![
            CheckpointRequirement { id: "req-1".to_string(), title: "Documentation updated".to_string(), met: true },
            CheckpointRequirement { id: "req-2".to_string(), title: "Tests passing".to_string(), met: false },
        ],
        artifacts: vec![
            CheckpointArtifact { label: "PR".to_string(), value: "#452".to_string() },
        ],
        decision: None,
    };
    
    let option = DiscussionOption {
        id: "opt-1".to_string(),
        title: "Use PostgreSQL".to_string(),
        description: "Store all data in a relational database.".to_string(),
        rationale: "Better consistency guarantees.".to_string(),
        pros: vec!["ACID compliance".to_string(), "Mature ecosystem".to_string()],
        cons: vec!["Scaling challenges".to_string()],
        recommended: true,
    };

    rsx! {
        section {
            SectionHeader { title: "Cards" }
            
            div {
                style: "
                    display: grid;
                    grid-template-columns: repeat(auto-fit, minmax(350px, 1fr));
                    gap: var(--space-4);
                ",
                
                ShowcaseItem { name: "Checkpoint Card (Pending)",
                    div {
                        style: "max-width: 400px;",
                        CheckpointCard {
                            checkpoint: checkpoint.clone(),
                            project_id: "proj-1".to_string(),
                            on_action_complete: EventHandler::new(|_| {}),
                        }
                    }
                }
                
                ShowcaseItem { name: "Option Card (Recommended)",
                    div {
                        style: "max-width: 400px;",
                        OptionCard {
                            option: option.clone(),
                            selected: false,
                            on_select: EventHandler::new(|_| {}),
                        }
                    }
                }
                
                ShowcaseItem { name: "Option Card (Selected)",
                    div {
                        style: "max-width: 400px;",
                        OptionCard {
                            option: option.clone(),
                            selected: true,
                            on_select: EventHandler::new(|_| {}),
                        }
                    }
                }
                
                ShowcaseItem { name: "Generic Card Styles",
                    div {
                        style: "display: flex; flex-direction: column; gap: var(--space-3);",
                        
                        div {
                            style: "
                                background: var(--bg-surface);
                                border: 1px solid var(--border);
                                border-radius: var(--radius-lg);
                                padding: var(--space-4);
                            ",
                            "Default Card"
                        }
                        
                        div {
                            style: "
                                background: var(--bg-surface-bright);
                                border: 1px solid var(--border-focused);
                                border-radius: var(--radius-lg);
                                padding: var(--space-4);
                            ",
                            "Elevated Card (focused border)"
                        }
                        
                        div {
                            style: "
                                background: var(--status-success-bg);
                                border: 1px solid var(--status-success);
                                border-radius: var(--radius-lg);
                                padding: var(--space-4);
                                color: var(--status-success);
                            ",
                            "Success Card"
                        }
                        
                        div {
                            style: "
                                background: var(--status-error-bg);
                                border: 1px solid var(--status-error);
                                border-radius: var(--radius-lg);
                                padding: var(--space-4);
                                color: var(--status-error);
                            ",
                            "Error Card"
                        }
                    }
                }
            }
        }
    }
}

// =============================================================================
// Status Indicators Section
// =============================================================================

#[component]
fn StatusIndicatorsSection() -> Element {
    rsx! {
        section {
            SectionHeader { title: "Status Indicators" }
            
            div {
                style: "
                    display: grid;
                    grid-template-columns: repeat(auto-fit, minmax(250px, 1fr));
                    gap: var(--space-4);
                ",
                
                ShowcaseItem { name: "Connection Indicator",
                    div {
                        style: "display: flex; flex-direction: column; gap: var(--space-2);",
                        
                        ConnectionIndicatorDot { color: IndicatorColor::Green, label: "Connected" }
                        ConnectionIndicatorDot { color: IndicatorColor::Yellow, label: "Reconnecting (2)" }
                        ConnectionIndicatorDot { color: IndicatorColor::Red, label: "Disconnected" }
                    }
                }
                
                ShowcaseItem { name: "Tool Status Icons",
                    div {
                        style: "display: flex; align-items: center; gap: var(--space-4);",
                        ToolStatusIcon { status: ToolStatus::Pending }
                        ToolStatusIcon { status: ToolStatus::Running }
                        ToolStatusIcon { status: ToolStatus::Success }
                        ToolStatusIcon { status: ToolStatus::Error }
                    }
                }
                
                ShowcaseItem { name: "Confidence Bar",
                    div {
                        style: "display: flex; flex-direction: column; gap: var(--space-3);",
                        ConfidenceBar { value: 0.85 }
                        ConfidenceBar { value: 0.55 }
                        ConfidenceBar { value: 0.25 }
                    }
                }
                
                ShowcaseItem { name: "Severity Colors",
                    div {
                        style: "display: flex; flex-direction: column; gap: var(--space-2);",
                        
                        SeverityBadge { severity: Severity::Info, label: "Info" }
                        SeverityBadge { severity: Severity::Success, label: "Success" }
                        SeverityBadge { severity: Severity::Warning, label: "Warning" }
                        SeverityBadge { severity: Severity::Error, label: "Error" }
                    }
                }
                
                ShowcaseItem { name: "Risk Level Badges",
                    div {
                        style: "display: flex; flex-direction: column; gap: var(--space-2);",
                        
                        RiskBadge { risk: RiskLevel::Low }
                        RiskBadge { risk: RiskLevel::Medium }
                        RiskBadge { risk: RiskLevel::High }
                        RiskBadge { risk: RiskLevel::Critical }
                    }
                }
                
                ShowcaseItem { name: "Semantic Colors",
                    div {
                        style: "display: flex; flex-wrap: wrap; gap: var(--space-2);",
                        
                        ColorSwatch { name: "--accent", color: "var(--accent)" }
                        ColorSwatch { name: "--status-success", color: "var(--status-success)" }
                        ColorSwatch { name: "--status-warning", color: "var(--status-warning)" }
                        ColorSwatch { name: "--status-error", color: "var(--status-error)" }
                        ColorSwatch { name: "--status-info", color: "var(--status-info)" }
                        ColorSwatch { name: "--aima", color: "var(--aima)" }
                        ColorSwatch { name: "--aporia", color: "var(--aporia)" }
                        ColorSwatch { name: "--natural", color: "var(--natural)" }
                    }
                }
            }
        }
    }
}

#[component]
fn ConnectionIndicatorDot(color: IndicatorColor, label: &'static str) -> Element {
    let color_css = color.css();
    rsx! {
        div {
            style: "display: flex; align-items: center; gap: 6px;",
            span {
                style: "color: {color_css}; font-size: 10px;",
                "●"
            }
            span { style: "font-size: var(--text-sm);", "{label}" }
        }
    }
}

#[component]
fn SeverityBadge(severity: Severity, label: &'static str) -> Element {
    let bg = severity.css_bg();
    let color = severity.css_color();
    rsx! {
        div {
            style: "
                display: inline-block;
                font-size: var(--text-xs);
                font-weight: var(--weight-semibold);
                padding: var(--space-1) var(--space-3);
                border-radius: var(--radius-full);
                background: {bg};
                color: {color};
                text-transform: uppercase;
                letter-spacing: 0.5px;
            ",
            "{label}"
        }
    }
}

#[component]
fn RiskBadge(risk: RiskLevel) -> Element {
    let (bg, color) = match risk {
        RiskLevel::Low => ("var(--status-success-bg)", "var(--status-success)"),
        RiskLevel::Medium => ("var(--status-warning-bg)", "var(--status-warning)"),
        RiskLevel::High => ("var(--status-error-bg)", "var(--status-error)"),
        RiskLevel::Critical => ("var(--aima-bg)", "var(--aima)"),
    };
    let label = risk.label();
    rsx! {
        div {
            style: "
                display: inline-block;
                font-size: var(--text-xs);
                font-weight: var(--weight-semibold);
                padding: var(--space-1) var(--space-3);
                border-radius: var(--radius-full);
                background: {bg};
                color: {color};
                text-transform: uppercase;
                letter-spacing: 0.5px;
            ",
            "{label} Risk"
        }
    }
}

#[component]
fn ColorSwatch(name: &'static str, color: &'static str) -> Element {
    rsx! {
        div {
            style: "display: flex; align-items: center; gap: var(--space-2);",
            div {
                style: "
                    width: 32px;
                    height: 32px;
                    border-radius: var(--radius-md);
                    background: {color};
                    border: 1px solid var(--border);
                ",
            }
            span {
                style: "font-family: var(--font-mono); font-size: var(--text-xs); color: var(--text-secondary);",
                "{name}"
            }
        }
    }
}

// =============================================================================
// Data Display Section
// =============================================================================

#[component]
fn DataDisplaySection() -> Element {
    rsx! {
        section {
            SectionHeader { title: "Data Display" }
            
            div {
                style: "
                    display: grid;
                    grid-template-columns: repeat(auto-fit, minmax(250px, 1fr));
                    gap: var(--space-4);
                ",
                
                ShowcaseItem { name: "Code Block",
                    pre {
                        style: "
                            background: var(--code-bg);
                            border: 1px solid var(--border);
                            border-radius: var(--radius-md);
                            padding: var(--space-3);
                            font-family: var(--font-mono);
                            font-size: var(--text-sm);
                            color: var(--code-fg);
                            margin: 0;
                            overflow-x: auto;
                        ",
                        code {
                            "fn main() {{\n    println!(\"Hello\");\n}}"
                        }
                    }
                }
                
                ShowcaseItem { name: "Syntax Highlighting",
                    div {
                        style: "
                            background: var(--code-bg);
                            border: 1px solid var(--border);
                            border-radius: var(--radius-md);
                            padding: var(--space-3);
                            font-family: var(--font-mono);
                            font-size: var(--text-sm);
                        ",
                        div {
                            span { style: "color: var(--syntax-keyword);", "let " }
                            span { style: "color: var(--syntax-function);", "x" }
                            span { style: "color: var(--text-primary);", " = " }
                            span { style: "color: var(--syntax-number);", "42" }
                            span { style: "color: var(--text-primary);", ";" }
                        }
                        div {
                            style: "margin-top: var(--space-1);",
                            span { style: "color: var(--syntax-comment);", "// A comment" }
                        }
                    }
                }
                
                ShowcaseItem { name: "Table",
                    table {
                        style: "
                            width: 100%;
                            border-collapse: collapse;
                            font-size: var(--text-sm);
                        ",
                        thead {
                            tr {
                                th {
                                    style: "
                                        text-align: left;
                                        padding: var(--space-2);
                                        border-bottom: 1px solid var(--border);
                                        color: var(--text-secondary);
                                        font-weight: var(--weight-semibold);
                                    ",
                                    "Name"
                                }
                                th {
                                    style: "
                                        text-align: left;
                                        padding: var(--space-2);
                                        border-bottom: 1px solid var(--border);
                                        color: var(--text-secondary);
                                        font-weight: var(--weight-semibold);
                                    ",
                                    "Status"
                                }
                            }
                        }
                        tbody {
                            tr {
                                td {
                                    style: "padding: var(--space-2); border-bottom: 1px solid var(--border-separator);",
                                    "Item 1"
                                }
                                td {
                                    style: "padding: var(--space-2); border-bottom: 1px solid var(--border-separator);",
                                    span { style: "color: var(--status-success);", "●" }
                                }
                            }
                            tr {
                                td {
                                    style: "padding: var(--space-2); border-bottom: 1px solid var(--border-separator);",
                                    "Item 2"
                                }
                                td {
                                    style: "padding: var(--space-2); border-bottom: 1px solid var(--border-separator);",
                                    span { style: "color: var(--status-warning);", "●" }
                                }
                            }
                        }
                    }
                }
                
                ShowcaseItem { name: "Progress Indicators",
                    div {
                        style: "display: flex; flex-direction: column; gap: var(--space-3);",
                        
                        // Progress bar
                        div {
                            div {
                                style: "font-size: var(--text-xs); color: var(--text-secondary); margin-bottom: var(--space-1);",
                                "Progress (75%)"
                            }
                            div {
                                style: "
                                    height: 8px;
                                    background: var(--bg-surface-dim);
                                    border-radius: var(--radius-full);
                                    overflow: hidden;
                                ",
                                div {
                                    style: "
                                        height: 100%;
                                        width: 75%;
                                        background: var(--accent);
                                        border-radius: var(--radius-full);
                                        transition: width 0.3s ease;
                                    ",
                                }
                            }
                        }
                        
                        // Spinner-like indicator
                        div {
                            style: "display: flex; align-items: center; gap: var(--space-2);",
                            div {
                                style: "
                                    width: 16px;
                                    height: 16px;
                                    border: 2px solid var(--border);
                                    border-top-color: var(--accent);
                                    border-radius: var(--radius-full);
                                    animation: spin 1s linear infinite;
                                ",
                            }
                            span { style: "font-size: var(--text-sm); color: var(--text-secondary);", "Loading..." }
                        }
                    }
                }
            }
        }
    }
}

// =============================================================================
// Layout Section
// =============================================================================

#[component]
fn LayoutSection() -> Element {
    rsx! {
        section {
            SectionHeader { title: "Layout Components" }
            
            div {
                style: "
                    display: grid;
                    grid-template-columns: repeat(auto-fit, minmax(250px, 1fr));
                    gap: var(--space-4);
                ",
                
                ShowcaseItem { name: "Theme Toggle",
                    ThemeToggle {}
                }
                
                ShowcaseItem { name: "Resize Handle",
                    div {
                        style: "
                            display: flex;
                            height: 60px;
                            background: var(--bg-surface);
                            border-radius: var(--radius-md);
                            overflow: hidden;
                        ",
                        div {
                            style: "
                                flex: 1;
                                display: flex;
                                align-items: center;
                                justify-content: center;
                                background: var(--bg-surface-dim);
                                font-size: var(--text-xs);
                                color: var(--text-muted);
                            ",
                            "Panel A"
                        }
                        ResizeHandle {
                            dir: ResizeDir::Horizontal,
                            state: ResizeState {
                                size: Signal::new(100.0),
                                is_dragging: Signal::new(false),
                                drag_origin: Signal::new(0.0),
                                drag_start_size: Signal::new(0.0),
                                min_size: 50.0,
                                max_size: 300.0,
                                default_size: 100.0,
                            },
                        }
                        div {
                            style: "
                                flex: 1;
                                display: flex;
                                align-items: center;
                                justify-content: center;
                                background: var(--bg-surface-dim);
                                font-size: var(--text-xs);
                                color: var(--text-muted);
                            ",
                            "Panel B"
                        }
                    }
                }
                
                ShowcaseItem { name: "Spacing Scale",
                    div {
                        style: "display: flex; flex-direction: column; gap: var(--space-2);",
                        
                        SpacingDemo { name: "space-1 (4px)", size: "var(--space-1)" }
                        SpacingDemo { name: "space-2 (8px)", size: "var(--space-2)" }
                        SpacingDemo { name: "space-3 (12px)", size: "var(--space-3)" }
                        SpacingDemo { name: "space-4 (16px)", size: "var(--space-4)" }
                    }
                }
                
                ShowcaseItem { name: "Border Radius",
                    div {
                        style: "display: flex; flex-wrap: wrap; gap: var(--space-3); align-items: center;",
                        
                        RadiusDemo { name: "sm", radius: "var(--radius-sm)" }
                        RadiusDemo { name: "md", radius: "var(--radius-md)" }
                        RadiusDemo { name: "lg", radius: "var(--radius-lg)" }
                        RadiusDemo { name: "xl", radius: "var(--radius-xl)" }
                        RadiusDemo { name: "full", radius: "var(--radius-full)" }
                    }
                }
            }
        }
    }
}

#[component]
fn SpacingDemo(name: &'static str, size: &'static str) -> Element {
    rsx! {
        div {
            style: "display: flex; align-items: center; gap: var(--space-3);",
            div {
                style: "
                    width: {size};
                    height: 16px;
                    background: var(--accent);
                    border-radius: var(--radius-sm);
                ",
            }
            span {
                style: "font-family: var(--font-mono); font-size: var(--text-xs); color: var(--text-secondary);",
                "{name}"
            }
        }
    }
}

#[component]
fn RadiusDemo(name: &'static str, radius: &'static str) -> Element {
    rsx! {
        div {
            style: "text-align: center;",
            div {
                style: "
                    width: 48px;
                    height: 48px;
                    background: var(--accent);
                    border-radius: {radius};
                    margin: 0 auto var(--space-1) auto;
                ",
            }
            span {
                style: "font-family: var(--font-mono); font-size: var(--text-xs); color: var(--text-secondary);",
                "{name}"
            }
        }
    }
}

// =============================================================================
// Feedback Section
// =============================================================================

#[component]
fn FeedbackSection() -> Element {
    let info_toast = Toast {
        id: 1,
        severity: Severity::Info,
        title: "File saved".to_string(),
        body: Some("Your changes have been saved.".to_string()),
        action: None,
        auto_dismiss: Some(std::time::Duration::from_secs(5)),
    };
    
    let success_toast = Toast {
        id: 2,
        severity: Severity::Success,
        title: "Connection established".to_string(),
        body: None,
        action: None,
        auto_dismiss: Some(std::time::Duration::from_secs(5)),
    };
    
    let warning_toast = Toast {
        id: 3,
        severity: Severity::Warning,
        title: "Session expiring soon".to_string(),
        body: Some("Your session will expire in 5 minutes.".to_string()),
        action: None,
        auto_dismiss: Some(std::time::Duration::from_secs(10)),
    };
    
    let error_toast = Toast {
        id: 4,
        severity: Severity::Error,
        title: "Connection failed".to_string(),
        body: Some("Unable to connect to the server.".to_string()),
        action: Some(ToastAction { label: "Retry".to_string(), action_id: "retry".to_string() }),
        auto_dismiss: None,
    };
    
    let approval = ToolApprovalState {
        turn_id: theatron_core::id::TurnId::from("turn-123"),
        tool_id: theatron_core::id::ToolId::new("tool-456"),
        tool_name: "write_file".to_string(),
        input: serde_json::json!({"path": "/tmp/test.txt"}),
        risk: RiskLevel::High,
        reason: "This will overwrite an existing file.".to_string(),
        resolved: false,
    };

    rsx! {
        section {
            SectionHeader { title: "Feedback Components" }
            
            div {
                style: "
                    display: grid;
                    grid-template-columns: repeat(auto-fit, minmax(350px, 1fr));
                    gap: var(--space-4);
                ",
                
                ShowcaseItem { name: "Toast - Info",
                    ToastItem { toast: info_toast }
                }
                
                ShowcaseItem { name: "Toast - Success",
                    ToastItem { toast: success_toast }
                }
                
                ShowcaseItem { name: "Toast - Warning",
                    ToastItem { toast: warning_toast }
                }
                
                ShowcaseItem { name: "Toast - Error (with action)",
                    ToastItem { toast: error_toast }
                }
                
                ShowcaseItem { name: "Tool Approval (High Risk)",
                    div {
                        style: "max-width: 400px;",
                        ToolApproval {
                            approval: approval,
                            on_approve: EventHandler::new(|_| {}),
                            on_deny: EventHandler::new(|_| {}),
                        }
                    }
                }
                
                ShowcaseItem { name: "Inline Alert Styles",
                    div {
                        style: "display: flex; flex-direction: column; gap: var(--space-2);",
                        
                        div {
                            style: "
                                padding: var(--space-3);
                                background: var(--status-info-bg);
                                border: 1px solid var(--status-info);
                                border-radius: var(--radius-md);
                                color: var(--text-primary);
                                font-size: var(--text-sm);
                            ",
                            "ℹ️ This is an informational message."
                        }
                        
                        div {
                            style: "
                                padding: var(--space-3);
                                background: var(--status-success-bg);
                                border: 1px solid var(--status-success);
                                border-radius: var(--radius-md);
                                color: var(--text-primary);
                                font-size: var(--text-sm);
                            ",
                            "✓ Operation completed successfully."
                        }
                        
                        div {
                            style: "
                                padding: var(--space-3);
                                background: var(--status-warning-bg);
                                border: 1px solid var(--status-warning);
                                border-radius: var(--radius-md);
                                color: var(--text-primary);
                                font-size: var(--text-sm);
                            ",
                            "⚠️ Please review before continuing."
                        }
                        
                        div {
                            style: "
                                padding: var(--space-3);
                                background: var(--status-error-bg);
                                border: 1px solid var(--status-error);
                                border-radius: var(--radius-md);
                                color: var(--text-primary);
                                font-size: var(--text-sm);
                            ",
                            "✗ An error occurred."
                        }
                    }
                }
            }
        }
    }
}

//! Fact list: readable rows of what the agent remembers, with sovereignty
//! badges, stated-vs-inferred visual ranking, and inline curation.

use dioxus::prelude::*;

use crate::state::memory::{
    Fact, FactListErrorKind, FactListState, FactListStore, FactReviewMode, FactSort,
    confidence_color, format_confidence,
};
use crate::state::sessions::format_relative_time;
use crate::views::memory::curation::{
    AdjustConfidenceDialog, ChangeSensitivityDialog, ForgetFactDialog, RestoreFactDialog,
};

const CONTAINER_STYLE: &str = "\
    display: flex; \
    flex-direction: column; \
    height: 100%; \
    overflow: hidden;\
";

const SORT_BAR_STYLE: &str = "\
    display: flex; \
    align-items: center; \
    gap: var(--space-2); \
    padding: var(--space-2) var(--space-1); \
    border-bottom: 1px solid var(--border); \
    font-size: var(--text-xs); \
    color: var(--text-secondary);\
";

const SORT_SELECT_STYLE: &str = "\
    background: var(--bg-surface); \
    border: 1px solid var(--border); \
    border-radius: var(--radius-sm); \
    padding: var(--space-1) var(--space-2); \
    color: var(--text-primary); \
    font-size: var(--text-xs); \
    cursor: pointer;\
";

const SCROLL_AREA_STYLE: &str = "\
    flex: 1; \
    overflow-y: auto; \
    padding: var(--space-2) var(--space-1); \
    display: flex; \
    flex-direction: column; \
    gap: var(--space-2);\
";

// WHY: a stated (operator-told/verified) fact gets a solid accent left border;
// an inferred/assumed one gets a muted dotted border — what you told the agent
// visually outranks what it guessed.
const ROW_BASE_STYLE: &str = "\
    display: flex; \
    flex-direction: column; \
    gap: var(--space-2); \
    padding: var(--space-3); \
    border-radius: var(--radius-md); \
    background: var(--bg-surface);\
";

const ROW_HEAD_STYLE: &str = "\
    display: flex; \
    align-items: flex-start; \
    gap: var(--space-2);\
";

const GLYPH_STYLE: &str = "\
    font-size: var(--text-sm); \
    line-height: var(--leading-normal); \
    flex-shrink: 0;\
";

const CONTENT_STYLE: &str = "\
    font-size: var(--text-base); \
    color: var(--text-primary); \
    line-height: var(--leading-normal); \
    flex: 1; \
    word-break: break-word;\
";

const BADGES_ROW_STYLE: &str = "\
    display: flex; \
    align-items: center; \
    gap: var(--space-1); \
    flex-wrap: wrap;\
";

const BADGE_STYLE: &str = "\
    font-size: var(--text-xs); \
    padding: 1px var(--space-2); \
    border-radius: var(--radius-lg); \
    font-weight: var(--weight-medium); \
    white-space: nowrap;\
";

const META_ROW_STYLE: &str = "\
    display: flex; \
    align-items: center; \
    gap: var(--space-3); \
    font-size: var(--text-xs); \
    color: var(--text-secondary); \
    flex-wrap: wrap;\
";

const CURATION_ROW_STYLE: &str = "\
    display: flex; \
    align-items: center; \
    gap: var(--space-1); \
    margin-left: auto;\
";

const ACTION_BTN_STYLE: &str = "\
    background: none; \
    border: 1px solid var(--border); \
    border-radius: var(--radius-sm); \
    color: var(--text-secondary); \
    font-size: var(--text-xs); \
    padding: 1px var(--space-2); \
    cursor: pointer; \
    transition: background-color var(--transition-quick), \
                color var(--transition-quick), \
                border-color var(--transition-quick);\
";

const EMPTY_STYLE: &str = "\
    display: flex; \
    align-items: center; \
    justify-content: center; \
    flex: 1; \
    color: var(--text-muted); \
    font-size: var(--text-base); \
    padding: var(--space-6);\
";

const FORGOTTEN_TAG_STYLE: &str = "\
    font-size: var(--text-xs); \
    color: var(--status-error); \
    font-weight: var(--weight-medium);\
";

const ERROR_BANNER_STYLE: &str = "\
    display: flex; \
    align-items: center; \
    justify-content: space-between; \
    gap: var(--space-3); \
    padding: var(--space-3); \
    margin-bottom: var(--space-2); \
    border: 1px solid var(--status-error); \
    border-radius: var(--radius-md); \
    background: var(--status-error-bg); \
    color: var(--text-primary); \
    font-size: var(--text-sm);\
";

const RETRY_BTN_STYLE: &str = "\
    background: var(--bg-surface); \
    color: var(--text-primary); \
    border: 1px solid var(--border); \
    border-radius: var(--radius-md); \
    padding: var(--space-1) var(--space-3); \
    font-size: var(--text-sm); \
    cursor: pointer; \
    flex-shrink: 0;\
";

const LEGACY_BADGE_STYLE: &str = "\
    font-size: var(--text-xs); \
    padding: 1px var(--space-2); \
    border-radius: var(--radius-lg); \
    background: var(--status-warning-bg); \
    color: var(--status-warning); \
    font-weight: var(--weight-medium);\
";

/// Which curation dialog is open, keyed by fact.
#[derive(Clone, PartialEq)]
enum CurationDialog {
    None,
    Forget {
        id: String,
        content: String,
    },
    Restore {
        id: String,
        content: String,
    },
    Confidence {
        id: String,
        content: String,
        value: f64,
    },
    Sensitivity {
        id: String,
        content: String,
        sensitivity: crate::state::memory::FactSensitivity,
    },
}

/// A single colored token chip.
#[component]
fn Badge(label: String, color: &'static str) -> Element {
    rsx! {
        span {
            style: "{BADGE_STYLE} background: {color}22; color: {color};",
            "{label}"
        }
    }
}

/// Human-readable title for a fetch/decode error kind.
fn error_title(kind: FactListErrorKind) -> &'static str {
    match kind {
        FactListErrorKind::Connection => "Connection failed",
        FactListErrorKind::Non2xx(_) => "Server error",
        FactListErrorKind::Decode => "Decode failed",
        FactListErrorKind::Unavailable => "Memory unavailable",
    }
}

/// Fact list panel: sort control, rows, and inline curation dialogs.
#[component]
pub(crate) fn FactList(
    list_store: Signal<FactListStore>,
    on_sort_change: EventHandler<FactSort>,
    /// Fired after any mutation succeeds, so the parent can refetch.
    on_mutated: EventHandler<()>,
    /// Fired when the operator asks to retry a failed fetch.
    on_retry: EventHandler<()>,
) -> Element {
    let mut dialog = use_signal(|| CurationDialog::None);

    let store = list_store.read();
    let state = store.state.clone();
    let sort = store.sort;
    let review_mode = store.review_mode;
    let visible: Vec<Fact> = store.visible().into_iter().cloned().collect();
    let active_count = store.active_count;
    let total_count = store.total_count;
    let legacy_array = store.legacy_array;
    let has_data = !store.facts.is_empty();
    drop(store);

    let shown = visible.len();
    let forgotten_count = total_count.saturating_sub(active_count);
    let count_label = match review_mode {
        FactReviewMode::Active => format!("{shown} of {active_count} active facts"),
        FactReviewMode::Forgotten => format!("{shown} of {forgotten_count} forgotten facts"),
        FactReviewMode::All => format!("{shown} of {total_count} facts ({active_count} active)"),
    };

    let current_error = if let FactListState::Error(err) = state.clone() {
        Some(err)
    } else {
        None
    };

    rsx! {
        div {
            style: "{CONTAINER_STYLE}",
            if let Some(ref err) = current_error {
                div {
                    style: "{ERROR_BANNER_STYLE}",
                    div {
                        style: "display: flex; align-items: center; gap: var(--space-2);",
                        span { style: "color: var(--status-error); font-weight: var(--weight-semibold);", "⚠" }
                        div {
                            style: "display: flex; flex-direction: column; gap: 2px; flex: 1;",
                            span { style: "font-weight: var(--weight-medium);", "{error_title(err.kind)}" }
                            span { style: "font-size: var(--text-xs); color: var(--text-secondary);", "{err.message}" }
                        }
                    }
                    button {
                        style: "{RETRY_BTN_STYLE}",
                        onclick: move |_| on_retry.call(()),
                        "Retry"
                    }
                }
            }

            if state == FactListState::Loading && !has_data {
                div {
                    style: "{EMPTY_STYLE}",
                    "Loading memories…"
                }
            } else if state == FactListState::Empty {
                div {
                    style: "{EMPTY_STYLE} flex-direction: column; gap: var(--space-3);",
                    span { "No memories found." }
                    button {
                        style: "{RETRY_BTN_STYLE}",
                        onclick: move |_| on_retry.call(()),
                        "Refresh"
                    }
                }
            } else if matches!(state, FactListState::Error(_)) && !has_data {
                div {
                    style: "{EMPTY_STYLE} flex-direction: column; gap: var(--space-3);",
                    span { "Could not load memories." }
                    button {
                        style: "{RETRY_BTN_STYLE}",
                        onclick: move |_| on_retry.call(()),
                        "Retry"
                    }
                }
            } else {
                div {
                    style: "{SORT_BAR_STYLE}",
                    span { "Sort:" }
                    select {
                        style: "{SORT_SELECT_STYLE}",
                        value: "{sort.label()}",
                        onchange: move |evt: Event<FormData>| {
                            let label = evt.value();
                            for s in FactSort::ALL {
                                if s.label() == label {
                                    on_sort_change.call(*s);
                                    break;
                                }
                            }
                        },
                        for s in FactSort::ALL {
                            option { value: "{s.label()}", selected: *s == sort, "{s.label()}" }
                        }
                    }
                    if legacy_array {
                        span {
                            style: "{LEGACY_BADGE_STYLE}",
                            title: "Loaded from legacy array format",
                            "legacy"
                        }
                    }
                    span {
                        style: "margin-left: auto;",
                        "{count_label}"
                    }
                }

                if state == FactListState::Loading {
                    div {
                        style: "{SORT_BAR_STYLE} border-bottom: none; justify-content: center; color: var(--text-muted);",
                        "Refreshing…"
                    }
                }

                if visible.is_empty() {
                    div {
                        style: "{EMPTY_STYLE}",
                        "No facts match these filters yet."
                    }
                } else {
                div {
                    style: "{SCROLL_AREA_STYLE}",
                    for fact in visible.iter() {
                        {
                            let stated = fact.tier.is_stated();
                            // INVARIANT: stated facts read with a solid accent
                            // border; guessed facts read muted + dotted.
                            let border = if stated {
                                "border-left: 3px solid var(--accent);"
                            } else {
                                "border-left: 3px dotted var(--border-separator);"
                            };
                            let glyph_color = fact.tier.color();
                            let glyph = fact.tier.glyph();
                            let content = fact.content.clone();
                            let id = fact.id.clone();
                            let confidence = fact.confidence;
                            let conf_color = confidence_color(confidence);
                            let conf_label = format_confidence(confidence);
                            let recorded = fact.recorded_at.clone();
                            let nous = fact.nous_id.clone();
                            let access = fact.access_count;
                            let forgotten = fact.is_forgotten;
                            let row_opacity = if forgotten { "opacity: 0.55;" } else { "" };
                            let source_session = fact.source_session_id.clone();
                            let valid_from = fact.valid_from.clone();
                            let valid_to = fact.valid_to.clone();
                            let last_accessed = fact.last_accessed_at.clone();
                            let stability = fact.stability_hours;
                            let scope = fact.scope;
                            let project_id = fact.project_id.clone();
                            let superseded_by = fact.superseded_by.clone();
                            let forgotten_at = fact.forgotten_at.clone();
                            let forget_reason = fact.forget_reason;

                            let ft_label = fact.fact_type.label().to_string();
                            let ft_color = fact.fact_type.color();
                            let tier_label = fact.tier.label();
                            let tier_color = fact.tier.color();
                            let sens = fact.sensitivity;
                            let sens_label = sens.label().to_string();
                            let sens_color = sens.color();
                            let vis_label = fact.visibility.label().to_string();
                            let vis_color = fact.visibility.color();

                            rsx! {
                                div {
                                    key: "{id}",
                                    style: "{ROW_BASE_STYLE} {border} {row_opacity}",
                                    div {
                                        style: "{ROW_HEAD_STYLE}",
                                        span {
                                            style: "{GLYPH_STYLE} color: {glyph_color};",
                                            title: if stated { "Stated / verified" } else { "Inferred / assumed" },
                                            "{glyph}"
                                        }
                                        span { style: "{CONTENT_STYLE}", "{content}" }
                                    }

                                    div {
                                        style: "{BADGES_ROW_STYLE}",
                                        Badge { label: ft_label, color: ft_color }
                                        Badge { label: tier_label.to_string(), color: tier_color }
                                        Badge { label: sens_label, color: sens_color }
                                        Badge { label: vis_label, color: vis_color }
                                    }

                                    div {
                                        style: "{META_ROW_STYLE}",
                                        span {
                                            style: "color: {conf_color}; font-weight: var(--weight-semibold);",
                                            "● {conf_label}"
                                        }
                                        span { "{format_relative_time(&recorded)}" }
                                        if !nous.is_empty() {
                                            span { title: "Owning nous", "{nous}" }
                                        }
                                        if access > 0 {
                                            span { "{access} recalls" }
                                        }
                                        if let Some(ref ts) = last_accessed {
                                            span { title: "Last recalled", "last {format_relative_time(ts)}" }
                                        }
                                        if !valid_from.is_empty() {
                                            span { title: "Valid from", "valid from {format_relative_time(&valid_from)}" }
                                        }
                                        if !valid_to.is_empty() && !valid_to.starts_with("9999") {
                                            span { title: "Valid to", "valid to {format_relative_time(&valid_to)}" }
                                        }
                                        if stability > 0.0 {
                                            span { title: "FSRS stability in hours", "stability {stability:.0}h" }
                                        }
                                        if let Some(ref session) = source_session {
                                            span { title: "Source session", "session {session}" }
                                        }
                                        if let Some(scope) = scope {
                                            span { title: "Memory scope", "{scope.label()}" }
                                        }
                                        if let Some(ref project) = project_id {
                                            span { title: "Project partition", "{project}" }
                                        }
                                        if let Some(ref sup) = superseded_by {
                                            span { title: "Superseded by", "superseded by {sup}" }
                                        }
                                        if let Some(reason) = forget_reason {
                                            span { title: "Forget reason", "forgotten: {reason.label()}" }
                                        } else if forgotten {
                                            span { style: "{FORGOTTEN_TAG_STYLE}", "forgotten" }
                                        }
                                        if let Some(ref ts) = forgotten_at {
                                            span { title: "Forgotten at", "forgotten {format_relative_time(ts)}" }
                                        }

                                        div {
                                            style: "{CURATION_ROW_STYLE}",
                                            if forgotten {
                                                button {
                                                    style: "{ACTION_BTN_STYLE}",
                                                    onclick: {
                                                        let id = id.clone();
                                                        let content = content.clone();
                                                        move |_| dialog.set(CurationDialog::Restore {
                                                            id: id.clone(), content: content.clone(),
                                                        })
                                                    },
                                                    "Restore"
                                                }
                                            } else {
                                                button {
                                                    style: "{ACTION_BTN_STYLE}",
                                                    onclick: {
                                                        let id = id.clone();
                                                        let content = content.clone();
                                                        move |_| dialog.set(CurationDialog::Confidence {
                                                            id: id.clone(), content: content.clone(), value: confidence,
                                                        })
                                                    },
                                                    "Confidence"
                                                }
                                                button {
                                                    style: "{ACTION_BTN_STYLE}",
                                                    onclick: {
                                                        let id = id.clone();
                                                        let content = content.clone();
                                                        move |_| dialog.set(CurationDialog::Sensitivity {
                                                            id: id.clone(), content: content.clone(), sensitivity: sens,
                                                        })
                                                    },
                                                    "Sensitivity"
                                                }
                                                button {
                                                    style: "{ACTION_BTN_STYLE} color: var(--status-error); border-color: var(--status-error)44;",
                                                    onclick: {
                                                        let id = id.clone();
                                                        let content = content.clone();
                                                        move |_| dialog.set(CurationDialog::Forget {
                                                            id: id.clone(), content: content.clone(),
                                                        })
                                                    },
                                                    "Forget"
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        // ── Inline curation dialogs ──
        {
            let close = move |_| dialog.set(CurationDialog::None);
            let done = move |_| {
                dialog.set(CurationDialog::None);
                on_mutated.call(());
            };
            match dialog.read().clone() {
                CurationDialog::None => rsx! {},
                CurationDialog::Forget { id, content } => rsx! {
                    ForgetFactDialog { fact_id: id, content, on_close: close, on_done: done }
                },
                CurationDialog::Restore { id, content } => rsx! {
                    RestoreFactDialog { fact_id: id, content, on_close: close, on_done: done }
                },
                CurationDialog::Confidence { id, content, value } => rsx! {
                    AdjustConfidenceDialog { fact_id: id, content, initial: value, on_close: close, on_done: done }
                },
                CurationDialog::Sensitivity { id, content, sensitivity } => rsx! {
                    ChangeSensitivityDialog { fact_id: id, content, initial: sensitivity, on_close: close, on_done: done }
                },
            }
        }
    }
}

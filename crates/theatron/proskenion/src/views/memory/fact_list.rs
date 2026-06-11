//! Fact list: readable rows of what the agent remembers, with sovereignty
//! badges, stated-vs-inferred visual ranking, and inline curation.

use dioxus::prelude::*;

use crate::state::memory::{Fact, FactListStore, FactSort, confidence_color, format_confidence};
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

/// Fact list panel: sort control, rows, and inline curation dialogs.
#[component]
pub(crate) fn FactList(
    list_store: Signal<FactListStore>,
    on_sort_change: EventHandler<FactSort>,
    /// Fired after any mutation succeeds, so the parent can refetch.
    on_mutated: EventHandler<()>,
) -> Element {
    let mut dialog = use_signal(|| CurationDialog::None);

    let store = list_store.read();
    let sort = store.sort;
    let visible: Vec<Fact> = store.visible().into_iter().cloned().collect();
    let total = store.total;
    drop(store);

    let shown = visible.len();

    rsx! {
        div {
            style: "{CONTAINER_STYLE}",
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
                span {
                    style: "margin-left: auto;",
                    "{shown} of {total} facts"
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
                                        if forgotten {
                                            span { style: "{FORGOTTEN_TAG_STYLE}", "forgotten" }
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

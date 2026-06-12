//! Fact search + filter bar: text search, fact-type chips, trust-tier chips,
//! and a recency window — the readable replacement for the entity-class and
//! opaque-confidence dropdowns.

use dioxus::prelude::*;

use crate::state::memory::{FactListStore, FactRecency, FactReviewMode, FactTier, FactType};

const BAR_STYLE: &str = "\
    display: flex; \
    flex-direction: column; \
    gap: var(--space-2); \
    padding-bottom: var(--space-2);\
";

const SEARCH_INPUT_STYLE: &str = "\
    flex: 1; \
    min-width: 200px; \
    background: var(--bg-surface); \
    border: 1px solid var(--border); \
    border-radius: var(--radius-md); \
    padding: var(--space-2) var(--space-3); \
    color: var(--text-primary); \
    font-size: var(--text-base);\
";

const GROUP_STYLE: &str = "\
    display: flex; \
    align-items: center; \
    gap: var(--space-2); \
    flex-wrap: wrap;\
";

const GROUP_LABEL_STYLE: &str = "\
    font-size: var(--text-xs); \
    color: var(--text-muted); \
    text-transform: uppercase; \
    letter-spacing: 0.4px; \
    margin-right: var(--space-1);\
";

const CHIP_BASE_STYLE: &str = "\
    font-size: var(--text-xs); \
    padding: var(--space-1) var(--space-3); \
    border-radius: var(--radius-lg); \
    cursor: pointer; \
    border: 1px solid var(--border); \
    background: var(--bg-surface); \
    color: var(--text-secondary); \
    transition: background-color var(--transition-quick), \
                color var(--transition-quick), \
                border-color var(--transition-quick);\
";

const CLEAR_ALL_STYLE: &str = "\
    color: var(--accent); \
    font-size: var(--text-xs); \
    cursor: pointer; \
    background: none; \
    border: none; \
    padding: var(--space-1) var(--space-2); \
    margin-left: auto;\
";

/// Style for a toggle chip: filled with the supplied accent when active.
fn chip_style(active: bool, accent: &str) -> String {
    if active {
        format!(
            "{CHIP_BASE_STYLE} background: {accent}22; color: {accent}; border-color: {accent}66;"
        )
    } else {
        CHIP_BASE_STYLE.to_string()
    }
}

/// Search + filter bar for the fact list.
#[component]
pub(crate) fn FactFilters(
    list_store: Signal<FactListStore>,
    on_search_change: EventHandler<String>,
    on_filter_change: EventHandler<()>,
    on_clear_all: EventHandler<()>,
) -> Element {
    let store = list_store.read();
    let search_query = store.search_query.clone();
    let active_types = store.type_filter.clone();
    let active_tiers = store.tier_filter.clone();
    let recency = store.recency;
    let review_mode = store.review_mode;
    let has_filters = store.has_active_filters();
    drop(store);

    rsx! {
        div {
            style: "{BAR_STYLE}",
            div {
                style: "{GROUP_STYLE}",
                input {
                    style: "{SEARCH_INPUT_STYLE}",
                    r#type: "text",
                    placeholder: "Search what I remember...",
                    value: "{search_query}",
                    oninput: move |evt: Event<FormData>| {
                        let query = evt.value().clone();
                        list_store.write().search_query = query.clone();
                        // WHY: 300ms debounce avoids a request per keystroke; the
                        // last timer to fire wins as it reads the latest query.
                        spawn(async move {
                            tokio::time::sleep(std::time::Duration::from_millis(300)).await;
                            on_search_change.call(query);
                        });
                    },
                }
                if has_filters {
                    button {
                        style: "{CLEAR_ALL_STYLE}",
                        onclick: move |_| on_clear_all.call(()),
                        "Clear all"
                    }
                }
            }

            // ── Fact-type chips ──
            div {
                style: "{GROUP_STYLE}",
                span { style: "{GROUP_LABEL_STYLE}", "Type" }
                for ft in FactType::FILTERABLE {
                    {
                        let active = active_types.contains(ft);
                        let style = chip_style(active, ft.color());
                        let ft_clone = ft.clone();
                        rsx! {
                            span {
                                key: "ft-{ft.wire()}",
                                style: "{style}",
                                onclick: move |_| {
                                    let mut s = list_store.write();
                                    if let Some(pos) = s.type_filter.iter().position(|t| t == &ft_clone) {
                                        s.type_filter.remove(pos);
                                    } else {
                                        s.type_filter.push(ft_clone.clone());
                                    }
                                    drop(s);
                                    on_filter_change.call(());
                                },
                                "{ft.label()}"
                            }
                        }
                    }
                }
            }

            // ── Trust-tier chips ──
            div {
                style: "{GROUP_STYLE}",
                span { style: "{GROUP_LABEL_STYLE}", "Trust" }
                for tier in FactTier::FILTERABLE {
                    {
                        let t = *tier;
                        let active = active_tiers.contains(&t);
                        let style = chip_style(active, t.color());
                        rsx! {
                            span {
                                key: "tier-{t.wire()}",
                                style: "{style}",
                                onclick: move |_| {
                                    let mut s = list_store.write();
                                    if let Some(pos) = s.tier_filter.iter().position(|x| *x == t) {
                                        s.tier_filter.remove(pos);
                                    } else {
                                        s.tier_filter.push(t);
                                    }
                                    drop(s);
                                    on_filter_change.call(());
                                },
                                "{t.label()}"
                            }
                        }
                    }
                }
            }

            // -- Review mode --
            div {
                style: "{GROUP_STYLE}",
                span { style: "{GROUP_LABEL_STYLE}", "Review" }
                for mode in FactReviewMode::ALL {
                    {
                        let m = *mode;
                        let active = review_mode == m;
                        let style = chip_style(active, "var(--accent)");
                        rsx! {
                            span {
                                key: "review-{m.label()}",
                                style: "{style}",
                                onclick: move |_| {
                                    list_store.write().review_mode = m;
                                    on_filter_change.call(());
                                },
                                "{m.label()}"
                            }
                        }
                    }
                }
            }

            // ── Recency window ──
            div {
                style: "{GROUP_STYLE}",
                span { style: "{GROUP_LABEL_STYLE}", "Age" }
                for window in FactRecency::ALL {
                    {
                        let w = *window;
                        let active = recency == w;
                        let style = chip_style(active, "var(--accent)");
                        rsx! {
                            span {
                                key: "age-{w.label()}",
                                style: "{style}",
                                onclick: move |_| {
                                    list_store.write().recency = w;
                                    // NOTE: recency is a client-side window over
                                    // recorded_at, so no refetch is required.
                                    on_filter_change.call(());
                                },
                                "{w.label()}"
                            }
                        }
                    }
                }
            }
        }
    }
}

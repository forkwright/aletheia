//! Facts list: flat fact browser shown while the knowledge graph has no entities.

use dioxus::prelude::*;

use crate::state::memory::format_confidence;
use crate::state::sessions::format_relative_time;

/// Maximum facts fetched for the fallback list.
pub(crate) const FACTS_FETCH_LIMIT: usize = 200;

/// Parsed `/api/v1/knowledge/facts` response plus a load marker.
#[derive(Debug, Clone, Default, PartialEq, serde::Deserialize)]
pub(crate) struct FactsSnapshot {
    #[serde(default)]
    pub facts: Vec<FactRow>,
    #[serde(default)]
    pub total: usize,
    /// True once a fetch has completed; gates the empty-state copy.
    #[serde(skip)]
    pub loaded: bool,
}

/// One fact from the knowledge API (temporal/provenance fields arrive flattened).
#[derive(Debug, Clone, PartialEq, serde::Deserialize)]
pub(crate) struct FactRow {
    // kanon:ignore RUST/primitive-for-domain-id — mirrors server-side string IDs from the knowledge API
    pub id: String,
    #[serde(default)]
    pub nous_id: String,
    #[serde(default)]
    pub fact_type: String,
    #[serde(default)]
    pub content: String,
    #[serde(default)]
    pub scope: Option<String>,
    #[serde(default)]
    pub recorded_at: Option<String>,
    #[serde(default)]
    pub confidence: f64,
    #[serde(default)]
    pub tier: Option<String>,
}

const PANEL_STYLE: &str = "\
    display: flex; \
    flex-direction: column; \
    flex: 1; \
    min-height: 0; \
    overflow-y: auto;\
";

const BANNER_STYLE: &str = "\
    padding: var(--space-3) var(--space-4); \
    background: var(--bg-surface); \
    border: 1px solid var(--border); \
    border-radius: var(--radius-md); \
    color: var(--text-secondary); \
    font-size: var(--text-sm); \
    margin-bottom: var(--space-3);\
";

const FACT_CARD_STYLE: &str = "\
    background: var(--bg-surface); \
    border: 1px solid var(--border); \
    border-radius: var(--radius-md); \
    padding: var(--space-3); \
    margin-bottom: var(--space-2);\
";

const FACT_CONTENT_STYLE: &str = "\
    color: var(--text-primary); \
    font-size: var(--text-sm); \
    line-height: var(--leading-normal); \
    white-space: pre-wrap; \
    overflow-wrap: anywhere;\
";

const FACT_META_STYLE: &str = "\
    display: flex; \
    align-items: center; \
    gap: var(--space-3); \
    flex-wrap: wrap; \
    font-size: var(--text-xs); \
    color: var(--text-muted); \
    margin-top: var(--space-2);\
";

const NOUS_TAG_STYLE: &str = "\
    background: color-mix(in srgb, var(--accent) 14%, transparent); \
    color: var(--accent); \
    border-radius: var(--radius-full); \
    padding: 1px var(--space-2); \
    font-weight: var(--weight-medium);\
";

const FOOTER_STYLE: &str = "\
    padding: var(--space-2) 0; \
    font-size: var(--text-xs); \
    color: var(--text-muted); \
    text-align: center;\
";

const LOADING_STYLE: &str = "\
    display: flex; \
    align-items: center; \
    justify-content: center; \
    flex: 1; \
    color: var(--text-muted); \
    font-size: var(--text-base);\
";

/// Facts fallback panel: empty-state banner plus a recency-ordered fact list.
#[component]
pub(crate) fn FactsPanel(snapshot: FactsSnapshot, scope_label: Option<String>) -> Element {
    if !snapshot.loaded {
        return rsx! {
            div { style: "{LOADING_STYLE}", "Loading memory…" }
        };
    }

    let total = snapshot.total;
    let shown = snapshot.facts.len();
    let noun = if total == 1 { "fact" } else { "facts" };
    let banner = match (&scope_label, total) {
        (Some(name), 0) => format!("No entities for {name} yet — no facts recorded."),
        (Some(name), n) => format!(
            "No entities for {name} yet — {n} {noun} recorded; \
             entities appear as the knowledge graph links them."
        ),
        (None, 0) => {
            "No entities yet — no facts recorded. Facts appear as agents learn.".to_string()
        }
        (None, n) => format!(
            "No entities yet — {n} {noun} recorded; \
             entities appear as the knowledge graph links them."
        ),
    };

    rsx! {
        div {
            style: "{PANEL_STYLE}",
            div { style: "{BANNER_STYLE}", "{banner}" }
            for fact in snapshot.facts.iter() {
                {
                    let fact_id = fact.id.clone();
                    let content = fact.content.clone();
                    let nous = fact.nous_id.clone();
                    let fact_type = fact.fact_type.clone();
                    let scope = fact.scope.clone();
                    let tier = fact.tier.clone();
                    let confidence = fact.confidence;
                    let recorded_at = fact.recorded_at.clone();

                    rsx! {
                        div {
                            key: "{fact_id}",
                            style: "{FACT_CARD_STYLE}",
                            div { style: "{FACT_CONTENT_STYLE}", "{content}" }
                            div {
                                style: "{FACT_META_STYLE}",
                                if !nous.is_empty() {
                                    span { style: "{NOUS_TAG_STYLE}", "{nous}" }
                                }
                                if !fact_type.is_empty() {
                                    span { "{fact_type}" }
                                }
                                if let Some(ref s) = scope {
                                    span { "scope: {s}" }
                                }
                                if let Some(ref t) = tier {
                                    span { "{t}" }
                                }
                                if confidence > 0.0 {
                                    span { "{format_confidence(confidence)}" }
                                }
                                if let Some(ref ts) = recorded_at {
                                    span { title: "{ts}", "recorded {format_relative_time(ts)}" }
                                }
                            }
                        }
                    }
                }
            }
            if total > 0 {
                div { style: "{FOOTER_STYLE}", "{shown} of {total} {noun}" }
            }
        }
    }
}

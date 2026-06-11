//! Knowledge growth rate: entity/relationship trends, type distribution.

use dioxus::prelude::*;

use crate::state::meta::KnowledgeGrowthStore;
use crate::views::meta::{
    CARD_LABEL, CARD_STYLE, CARD_SUB, CARD_VALUE, ChartCard, ChartKind, GRID_STYLE,
};

const STACKED_CARD_STYLE: &str = "margin-top: var(--space-3);";

#[component]
pub(super) fn KnowledgeGrowthSection(store: KnowledgeGrowthStore) -> Element {
    let accel_arrow = store.acceleration.arrow();
    let accel_color = store.acceleration.color();

    rsx! {
        // NOTE: Growth rate summary cards.
        h3 { style: "font-size: var(--text-base); color: var(--text-secondary); margin: 0 0 var(--space-3) 0;", "Growth Rate" }
        div {
            style: "{GRID_STYLE}",
            div {
                style: "{CARD_STYLE} flex: 1; min-width: 140px;",
                div { style: "{CARD_VALUE}", "{store.current_entity_rate:.0}" }
                div { style: "{CARD_LABEL}", "Entities / period" }
            }
            div {
                style: "{CARD_STYLE} flex: 1; min-width: 140px;",
                div { style: "{CARD_VALUE}", "{store.current_relationship_rate:.0}" }
                div { style: "{CARD_LABEL}", "Relationships / period" }
            }
            div {
                style: "{CARD_STYLE} flex: 1; min-width: 140px;",
                div {
                    style: "{CARD_VALUE} color: {accel_color};",
                    "{accel_arrow}"
                }
                div { style: "{CARD_LABEL}", "Acceleration" }
                div { style: "{CARD_SUB}",
                    match store.acceleration {
                        crate::state::meta::TrendDirection::Up => "Growth speeding up",
                        crate::state::meta::TrendDirection::Down => "Growth slowing down",
                        crate::state::meta::TrendDirection::Flat => "Steady growth",
                    }
                }
            }
        }

        // NOTE: Cumulative entity growth.
        ChartCard {
            title: "Total Entities Over Time",
            kind: ChartKind::Line,
            data: store.total_entities.clone(),
            color: "var(--natural)",
            width: 960.0,
            height: 280.0,
            card_style: STACKED_CARD_STYLE,
        }

        // NOTE: New entities per period.
        ChartCard {
            title: "New Entities Per Period",
            kind: ChartKind::Bar,
            data: store.new_entities_per_period.clone(),
            color: "var(--status-success)",
            width: 960.0,
            height: 260.0,
            card_style: STACKED_CARD_STYLE,
        }

        // NOTE: Knowledge density.
        if !store.density_over_time.is_empty() {
            ChartCard {
                title: "Knowledge Density (Relationships / Entity)",
                kind: ChartKind::Line,
                data: store.density_over_time.clone(),
                color: "var(--status-warning)",
                width: 960.0,
                height: 220.0,
                card_style: STACKED_CARD_STYLE,
            }
        }

        // NOTE: Entity type distribution.
        if !store.entity_type_distribution.is_empty() {
            h3 {
                style: "font-size: var(--text-base); color: var(--text-secondary); margin: var(--space-4) 0 var(--space-3) 0;",
                "Entity Type Distribution"
            }
            EntityTypeBreakdown { slices: store.entity_type_distribution.clone() }
        }
    }
}

#[component]
fn EntityTypeBreakdown(slices: Vec<crate::state::meta::EntityTypeSlice>) -> Element {
    let total: u32 = slices.iter().map(|s| s.count).sum();
    let max_count = slices.iter().map(|s| s.count).max().unwrap_or(1).max(1);

    rsx! {
        div {
            style: "{CARD_STYLE}",
            for slice in &slices {
                {
                    let pct = if total > 0 {
                        (f64::from(slice.count) / f64::from(total)) * 100.0
                    } else {
                        0.0
                    };
                    let bar_pct = (f64::from(slice.count) / f64::from(max_count)) * 100.0;
                    let color = slice.color;
                    rsx! {
                        div {
                            style: "display: flex; align-items: center; gap: var(--space-2); margin-bottom: var(--space-2);",
                            div {
                                style: "width: 10px; height: 10px; border-radius: var(--radius-sm); \
                                        background: {color}; flex-shrink: 0;",
                            }
                            span {
                                style: "font-size: var(--text-xs); color: var(--text-secondary); width: 100px;",
                                "{slice.entity_type}"
                            }
                            div {
                                style: "flex: 1; height: 16px; background: var(--border); \
                                        border-radius: var(--radius-sm); overflow: hidden;",
                                div {
                                    style: "height: 100%; width: {bar_pct:.0}%; background: {color}; \
                                            border-radius: var(--radius-sm);",
                                }
                            }
                            span {
                                style: "font-size: var(--text-xs); color: var(--text-secondary); width: 70px; text-align: right;",
                                "{slice.count} ({pct:.0}%)"
                            }
                        }
                    }
                }
            }
        }
    }
}

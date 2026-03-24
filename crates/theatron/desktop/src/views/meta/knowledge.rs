//! Knowledge growth rate: entity/relationship trends, type distribution.

use dioxus::prelude::*;

use crate::state::meta::KnowledgeGrowthStore;
use crate::views::meta::{
    BarChart, LineChart, CARD_LABEL, CARD_STYLE, CARD_SUB, CARD_VALUE, GRID_STYLE,
};

#[component]
pub(super) fn KnowledgeGrowthSection(store: KnowledgeGrowthStore) -> Element {
    let accel_arrow = store.acceleration.arrow();
    let accel_color = store.acceleration.color();

    rsx! {
        // NOTE: Growth rate summary cards.
        h3 { style: "font-size: 14px; color: #aaa; margin: 0 0 12px 0;", "Growth Rate" }
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
        h3 {
            style: "font-size: 14px; color: #aaa; margin: 16px 0 12px 0;",
            "Total Entities Over Time"
        }
        div {
            style: "{CARD_STYLE}",
            LineChart {
                data: store.total_entities.clone(),
                width: 600.0,
                height: 200.0,
                color: "#4a9aff",
                show_labels: true,
            }
        }

        // NOTE: New entities per period.
        h3 {
            style: "font-size: 14px; color: #aaa; margin: 16px 0 12px 0;",
            "New Entities Per Period"
        }
        div {
            style: "{CARD_STYLE}",
            BarChart {
                data: store.new_entities_per_period.clone(),
                width: 600.0,
                height: 180.0,
                color: "#22c55e",
            }
        }

        // NOTE: Knowledge density.
        if !store.density_over_time.is_empty() {
            h3 {
                style: "font-size: 14px; color: #aaa; margin: 16px 0 12px 0;",
                "Knowledge Density (Relationships / Entity)"
            }
            div {
                style: "{CARD_STYLE}",
                LineChart {
                    data: store.density_over_time.clone(),
                    width: 600.0,
                    height: 150.0,
                    color: "#f59e0b",
                    show_labels: true,
                }
            }
        }

        // NOTE: Entity type distribution.
        if !store.entity_type_distribution.is_empty() {
            h3 {
                style: "font-size: 14px; color: #aaa; margin: 16px 0 12px 0;",
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
                            style: "display: flex; align-items: center; gap: 8px; margin-bottom: 6px;",
                            div {
                                style: "width: 10px; height: 10px; border-radius: 2px; \
                                        background: {color}; flex-shrink: 0;",
                            }
                            span {
                                style: "font-size: 12px; color: #aaa; width: 100px;",
                                "{slice.entity_type}"
                            }
                            div {
                                style: "flex: 1; height: 16px; background: #2a2a3a; \
                                        border-radius: 4px; overflow: hidden;",
                                div {
                                    style: "height: 100%; width: {bar_pct:.0}%; background: {color}; \
                                            border-radius: 4px;",
                                }
                            }
                            span {
                                style: "font-size: 11px; color: #888; width: 70px; text-align: right;",
                                "{slice.count} ({pct:.0}%)"
                            }
                        }
                    }
                }
            }
        }
    }
}

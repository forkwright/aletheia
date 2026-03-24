//! Data loading, pagination helpers, and node card construction.

use std::collections::BTreeMap;

use crate::app::App;
use crate::msg::ErrorToast;
use crate::state::memory::{
    DriftTab, FactDetail, GraphNodeCard, MemoryEntity, MemoryFact, MemoryRelationship, MemoryTab,
    NodeCardFact,
};

use super::graph_analysis;

pub(crate) fn handle_facts_loaded(app: &mut App, facts: Vec<MemoryFact>, total: usize) {
    app.layout.memory.fact_list.facts = facts;
    app.layout.memory.fact_list.total_facts = total;
    app.layout.memory.loading = false;
    app.layout.memory.fact_list.selected = 0;
    app.layout.memory.fact_list.scroll_offset = 0;
}

pub(crate) fn handle_detail_loaded(app: &mut App, detail: FactDetail) {
    app.layout.memory.fact_list.detail = Some(detail);
}

pub(crate) fn handle_action_result(app: &mut App, message: String) {
    app.viewport.error_toast = Some(ErrorToast::new(message));
}

pub(super) async fn load_facts(app: &mut App) {
    let client = app.client.clone();
    let sort = app.layout.memory.fact_list.sort.as_str();
    let order = if app.layout.memory.fact_list.sort_asc {
        "asc"
    } else {
        "desc"
    };

    match client.knowledge_facts(sort, order, 500).await {
        Ok(json) => {
            if let Ok(resp) = serde_json::from_value::<FactsListResponse>(json) {
                app.layout.memory.fact_list.facts = resp.facts;
                app.layout.memory.fact_list.total_facts = resp.total;
            }
            app.layout.memory.loading = false;
        }
        Err(e) => {
            tracing::debug!("failed to load facts: {e}");
            app.layout.memory.loading = false;
        }
    }
}

pub(super) async fn load_fact_detail(app: &mut App, fact_id: &str) {
    let client = app.client.clone();

    match client.knowledge_fact_detail(fact_id).await {
        Ok(json) => {
            if let Ok(detail) = serde_json::from_value::<FactDetail>(json) {
                app.layout.memory.fact_list.detail = Some(detail);
                return;
            }
        }
        Err(e) => {
            tracing::debug!("failed to load fact detail: {e}");
        }
    }
    // NOTE: fallback to local data when API detail fetch fails
    if let Some(fact) = app
        .layout
        .memory
        .fact_list
        .facts
        .iter()
        .find(|f| f.id == fact_id)
    {
        app.layout.memory.fact_list.detail = Some(FactDetail {
            fact: fact.clone(),
            relationships: Vec::new(),
            similar: Vec::new(),
        });
    }
}

pub(super) fn item_count(app: &App) -> usize {
    match app.layout.memory.tab {
        MemoryTab::Facts => app.layout.memory.fact_list.facts.len(),
        MemoryTab::Graph => app.layout.memory.graph.entity_stats.len(),
        MemoryTab::Drift => drift_item_count(app),
        MemoryTab::Timeline => app.layout.memory.graph.timeline_events.len(),
    }
}

fn drift_item_count(app: &App) -> usize {
    match app.layout.memory.graph.drift_tab {
        DriftTab::Suggestions => app.layout.memory.graph.drift_suggestions.len(),
        DriftTab::Orphans => app.layout.memory.graph.orphaned_entities.len(),
        DriftTab::Stale => app.layout.memory.graph.stale_entities.len(),
        DriftTab::Isolated => app.layout.memory.graph.isolated_clusters.len(),
    }
}

pub(super) fn visible_rows(app: &App) -> usize {
    usize::from(app.viewport.terminal_height.saturating_sub(8))
}

pub(super) fn adjust_scroll(app: &mut App) {
    let visible = visible_rows(app);
    if app.layout.memory.fact_list.selected < app.layout.memory.fact_list.scroll_offset {
        app.layout.memory.fact_list.scroll_offset = app.layout.memory.fact_list.selected;
    } else if app.layout.memory.fact_list.selected
        >= app.layout.memory.fact_list.scroll_offset + visible
    {
        app.layout.memory.fact_list.scroll_offset =
            app.layout.memory.fact_list.selected.saturating_sub(visible) + 1;
    }
}

#[derive(serde::Deserialize)]
struct FactsListResponse {
    facts: Vec<MemoryFact>,
    total: usize,
}

#[derive(serde::Deserialize)]
struct EntitiesResponse {
    #[serde(default)]
    entities: Vec<MemoryEntity>,
}

#[derive(serde::Deserialize)]
struct RelationshipsResponse {
    #[serde(default)]
    relationships: Vec<MemoryRelationship>,
}

#[derive(serde::Deserialize)]
struct TimelineResponse {
    #[serde(default)]
    events: Vec<crate::state::memory::MemoryTimelineEvent>,
}

pub(super) async fn load_graph_data(app: &mut App) {
    let client = app.client.clone();

    let mut entities: Vec<MemoryEntity> = Vec::new();
    let mut relationships: Vec<MemoryRelationship> = Vec::new();

    if let Ok(json) = client.knowledge_entities().await
        && let Ok(resp) = serde_json::from_value::<EntitiesResponse>(json)
    {
        entities = resp.entities;
    }

    // WHY: fetch all relationships by iterating entities; the API exposes per-entity endpoints
    let mut seen_rels = std::collections::HashSet::new();
    for entity in &entities {
        if let Ok(json) = client.knowledge_entity_relationships(&entity.id).await
            && let Ok(resp) = serde_json::from_value::<RelationshipsResponse>(json)
        {
            for rel in resp.relationships {
                let key = format!("{}:{}:{}", rel.src, rel.relation, rel.dst);
                if seen_rels.insert(key) {
                    relationships.push(rel);
                }
            }
        }
    }

    if let Ok(json) = client.knowledge_timeline().await
        && let Ok(resp) = serde_json::from_value::<TimelineResponse>(json)
    {
        app.layout.memory.graph.timeline_events = resp.events;
    }

    app.layout.memory.graph.entities = entities.clone();
    app.layout.memory.graph.relationships = relationships.clone();

    graph_analysis::compute_graph_stats(app);
    graph_analysis::compute_drift_analysis(app);
}

pub(super) fn build_node_card(app: &mut App, entity_id: &str) {
    let entity = app
        .layout
        .memory
        .graph
        .entities
        .iter()
        .find(|e| e.id == entity_id)
        .cloned();

    let entity = match entity {
        Some(e) => e,
        None => return,
    };

    let stat = app
        .layout
        .memory
        .graph
        .entity_stats
        .iter()
        .find(|s| s.entity.id == entity_id);

    let pagerank = stat.map(|s| s.pagerank).unwrap_or(0.0);
    let community_id = stat.and_then(|s| s.community_id);

    // WHY: group relationships by type for the node card display
    let mut grouped: BTreeMap<String, Vec<MemoryRelationship>> = BTreeMap::new();
    for rel in &app.layout.memory.graph.relationships {
        if rel.src == entity_id || rel.dst == entity_id {
            grouped
                .entry(rel.relation.clone())
                .or_default()
                .push(rel.clone());
        }
    }
    let relationships_grouped: Vec<(String, Vec<MemoryRelationship>)> =
        grouped.into_iter().collect();

    // WHY: search facts for content mentioning this entity name
    let related_facts: Vec<NodeCardFact> = app
        .layout
        .memory
        .fact_list
        .facts
        .iter()
        .filter(|f| {
            !f.lifecycle.is_forgotten
                && f.content
                    .to_lowercase()
                    .contains(&entity.name.to_lowercase())
        })
        .take(10)
        .map(|f| NodeCardFact {
            content: f.content.clone(),
            confidence: f.confidence,
            tier: f.tier.clone(),
        })
        .collect();

    app.layout.memory.graph.node_card = Some(GraphNodeCard {
        entity,
        pagerank,
        community_id,
        relationships_grouped,
        related_facts,
    });
}

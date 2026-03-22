//! Update handlers for the memory inspector panel.

use std::collections::{BTreeMap, HashMap};

use crate::app::App;
use crate::msg::ErrorToast;
use crate::state::memory::{
    DriftSuggestion, DriftTab, FactDetail, GraphEntityStat, GraphHealthMetrics, GraphNodeCard,
    IsolatedCluster, MemoryEntity, MemoryFact, MemoryRelationship, MemorySearchResult, MemoryTab,
    NodeCardFact,
};
use crate::state::view_stack::View;

/// Open the memory inspector, pushing it onto the view stack and loading data.
pub(crate) async fn handle_open(app: &mut App) {
    app.layout.view_stack.push(View::MemoryInspector);
    app.layout.memory.loading = true;
    load_facts(app).await;
    load_graph_data(app).await;
}

/// Close memory inspector (pop back).
pub(crate) fn handle_close(app: &mut App) {
    if matches!(
        app.layout.view_stack.current(),
        View::MemoryInspector | View::FactDetail { .. } | View::EntityDetail { .. }
    ) {
        app.layout.view_stack.pop();
    }
}

pub(crate) fn handle_tab_next(app: &mut App) {
    app.layout.memory.tab = app.layout.memory.tab.next();
}

pub(crate) fn handle_tab_prev(app: &mut App) {
    app.layout.memory.tab = app.layout.memory.tab.prev();
}

pub(crate) fn handle_select_up(app: &mut App) {
    if app.layout.memory.fact_list.selected > 0 {
        app.layout.memory.fact_list.selected -= 1;
        adjust_scroll(app);
    }
}

pub(crate) fn handle_select_down(app: &mut App) {
    let max = item_count(app).saturating_sub(1);
    if app.layout.memory.fact_list.selected < max {
        app.layout.memory.fact_list.selected += 1;
        adjust_scroll(app);
    }
}

pub(crate) fn handle_select_first(app: &mut App) {
    app.layout.memory.fact_list.selected = 0;
    app.layout.memory.fact_list.scroll_offset = 0;
}

pub(crate) fn handle_select_last(app: &mut App) {
    let max = item_count(app).saturating_sub(1);
    app.layout.memory.fact_list.selected = max;
    adjust_scroll(app);
}

pub(crate) fn handle_page_up(app: &mut App) {
    let page = visible_rows(app);
    app.layout.memory.fact_list.selected =
        app.layout.memory.fact_list.selected.saturating_sub(page);
    adjust_scroll(app);
}

pub(crate) fn handle_page_down(app: &mut App) {
    let max = item_count(app).saturating_sub(1);
    let page = visible_rows(app);
    app.layout.memory.fact_list.selected = (app.layout.memory.fact_list.selected + page).min(max);
    adjust_scroll(app);
}

pub(crate) fn handle_sort_cycle(app: &mut App) {
    app.layout.memory.fact_list.sort = app.layout.memory.fact_list.sort.next();
}

pub(crate) fn handle_filter_open(app: &mut App) {
    app.layout.memory.filters.filter_editing = true;
}

pub(crate) fn handle_filter_close(app: &mut App) {
    app.layout.memory.filters.filter_editing = false;
    app.layout.memory.filters.filter_text.clear();
}

pub(crate) fn handle_filter_input(app: &mut App, c: char) {
    app.layout.memory.filters.filter_text.push(c);
}

pub(crate) fn handle_filter_backspace(app: &mut App) {
    if app.layout.memory.filters.filter_text.is_empty() {
        app.layout.memory.filters.filter_editing = false;
    } else {
        app.layout.memory.filters.filter_text.pop();
    }
}

pub(crate) async fn handle_drill_in(app: &mut App) {
    if app.layout.memory.tab == MemoryTab::Facts
        && let Some(fact) = app.layout.memory.selected_fact()
    {
        let fact_id = fact.id.clone();
        app.layout.view_stack.push(View::FactDetail {
            fact_id: fact_id.clone(),
        });
        load_fact_detail(app, &fact_id).await;
    } else if app.layout.memory.tab == MemoryTab::Graph {
        let selected = app.layout.memory.graph.selected_entity;
        if let Some(stat) = app.layout.memory.graph.entity_stats.get(selected) {
            let entity_id = stat.entity.id.clone();
            build_node_card(app, &entity_id);
            app.layout.view_stack.push(View::EntityDetail {
                entity_id: entity_id.clone(),
            });
        }
    }
}

pub(crate) fn handle_pop_back(app: &mut App) {
    if matches!(app.layout.view_stack.current(), View::FactDetail { .. }) {
        app.layout.view_stack.pop();
        app.layout.memory.fact_list.detail = None;
    } else if matches!(app.layout.view_stack.current(), View::EntityDetail { .. }) {
        app.layout.view_stack.pop();
        app.layout.memory.graph.node_card = None;
    } else {
        app.layout.view_stack.pop();
    }
}

pub(crate) fn handle_drift_tab_next(app: &mut App) {
    app.layout.memory.graph.drift_tab = app.layout.memory.graph.drift_tab.next();
    app.layout.memory.graph.drift_selected = 0;
    app.layout.memory.graph.drift_scroll_offset = 0;
}

pub(crate) fn handle_drift_tab_prev(app: &mut App) {
    app.layout.memory.graph.drift_tab = app.layout.memory.graph.drift_tab.prev();
    app.layout.memory.graph.drift_selected = 0;
    app.layout.memory.graph.drift_scroll_offset = 0;
}

pub(crate) async fn handle_forget(app: &mut App) {
    if let Some(fact) = app.layout.memory.selected_fact() {
        let id = fact.id.clone();
        let client = app.client.clone();
        match client.knowledge_forget(&id).await {
            Ok(()) => {
                if let Some(f) = app
                    .layout
                    .memory
                    .fact_list
                    .facts
                    .iter_mut()
                    .find(|f| f.id == id)
                {
                    f.lifecycle.is_forgotten = true;
                }
                app.viewport.error_toast = Some(ErrorToast::new("Fact forgotten".into()));
            }
            Err(e) => {
                app.viewport.error_toast = Some(ErrorToast::new(format!("Forget failed: {e}")));
            }
        }
    }
}

pub(crate) async fn handle_restore(app: &mut App) {
    if let Some(fact) = app.layout.memory.selected_fact() {
        let id = fact.id.clone();
        let client = app.client.clone();
        match client.knowledge_restore(&id).await {
            Ok(()) => {
                if let Some(f) = app
                    .layout
                    .memory
                    .fact_list
                    .facts
                    .iter_mut()
                    .find(|f| f.id == id)
                {
                    f.lifecycle.is_forgotten = false;
                }
                app.viewport.error_toast = Some(ErrorToast::new("Fact restored".into()));
            }
            Err(e) => {
                app.viewport.error_toast = Some(ErrorToast::new(format!("Restore failed: {e}")));
            }
        }
    }
}

pub(crate) fn handle_edit_confidence_start(app: &mut App) {
    let conf = app.layout.memory.selected_fact().map(|f| f.confidence);
    if let Some(c) = conf {
        app.layout.memory.fact_list.editing_confidence = true;
        app.layout.memory.fact_list.confidence_buffer = format!("{c:.2}");
    }
}

pub(crate) fn handle_confidence_input(app: &mut App, c: char) {
    if c.is_ascii_digit() || c == '.' {
        app.layout.memory.fact_list.confidence_buffer.push(c);
    }
}

pub(crate) fn handle_confidence_backspace(app: &mut App) {
    app.layout.memory.fact_list.confidence_buffer.pop();
}

pub(crate) async fn handle_confidence_submit(app: &mut App) {
    let conf: f64 = match app.layout.memory.fact_list.confidence_buffer.parse() {
        Ok(v) if (0.0..=1.0).contains(&v) => v,
        _ => {
            app.viewport.error_toast = Some(ErrorToast::new("Confidence must be 0.0–1.0".into()));
            return;
        }
    };
    app.layout.memory.fact_list.editing_confidence = false;

    let selected = app
        .layout
        .memory
        .selected_fact()
        .map(|f| (f.id.clone(), f.confidence));
    if let Some((id, prev_conf)) = selected {
        // NOTE: optimistic update: applied locally before the API round-trip to avoid UI lag
        if let Some(f) = app
            .layout
            .memory
            .fact_list
            .facts
            .iter_mut()
            .find(|f| f.id == id)
        {
            f.confidence = conf;
        }
        let client = app.client.clone();
        match client.knowledge_update_confidence(&id, conf).await {
            Ok(()) => {
                app.viewport.error_toast =
                    Some(ErrorToast::new(format!("Confidence set to {conf:.2}")));
            }
            Err(e) => {
                // NOTE: revert the optimistic update on failure
                if let Some(f) = app
                    .layout
                    .memory
                    .fact_list
                    .facts
                    .iter_mut()
                    .find(|f| f.id == id)
                {
                    f.confidence = prev_conf;
                }
                app.viewport.error_toast =
                    Some(ErrorToast::new(format!("Confidence update failed: {e}")));
            }
        }
    }
}

pub(crate) fn handle_confidence_cancel(app: &mut App) {
    app.layout.memory.fact_list.editing_confidence = false;
    app.layout.memory.fact_list.confidence_buffer.clear();
}

pub(crate) fn handle_search_open(app: &mut App) {
    app.layout.memory.search.search_active = true;
    app.layout.memory.search.search_query.clear();
}

pub(crate) fn handle_search_input(app: &mut App, c: char) {
    app.layout.memory.search.search_query.push(c);
}

pub(crate) fn handle_search_backspace(app: &mut App) {
    if app.layout.memory.search.search_query.is_empty() {
        app.layout.memory.search.search_active = false;
    } else {
        app.layout.memory.search.search_query.pop();
    }
}

pub(crate) async fn handle_search_submit(app: &mut App) {
    if app.layout.memory.search.search_query.is_empty() {
        return;
    }
    app.layout.memory.search.search_active = false;
    // NOTE: local fuzzy filtering: server-side semantic search not yet wired in
    let query = app.layout.memory.search.search_query.to_lowercase();
    let terms: Vec<&str> = query.split_whitespace().collect();

    let results: Vec<MemorySearchResult> = app
        .layout
        .memory
        .fact_list
        .facts
        .iter()
        .filter(|f| !f.lifecycle.is_forgotten)
        .filter_map(|f| {
            let content_lower = f.content.to_lowercase();
            let mut score = 0.0_f64;
            for term in &terms {
                if content_lower.contains(term) {
                    score += 1.0;
                }
            }
            if score > 0.0 {
                score *= f.confidence;
                Some(MemorySearchResult {
                    id: f.id.clone(),
                    content: f.content.clone(),
                    confidence: f.confidence,
                    tier: f.tier.clone(),
                    fact_type: f.fact_type.clone(),
                    score,
                })
            } else {
                None
            }
        })
        .collect();

    app.layout.memory.search.search_results = results;
    if !app.layout.memory.search.search_results.is_empty() {
        let msg = format!(
            "{} results for '{}'",
            app.layout.memory.search.search_results.len(),
            app.layout.memory.search.search_query
        );
        app.viewport.error_toast = Some(ErrorToast::new(msg));
    } else {
        app.viewport.error_toast = Some(ErrorToast::new(format!(
            "No results for '{}'",
            app.layout.memory.search.search_query
        )));
    }
}

pub(crate) fn handle_search_close(app: &mut App) {
    app.layout.memory.search.search_active = false;
    app.layout.memory.search.search_query.clear();
    app.layout.memory.search.search_results.clear();
}

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

async fn load_facts(app: &mut App) {
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

async fn load_fact_detail(app: &mut App, fact_id: &str) {
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

fn item_count(app: &App) -> usize {
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

fn visible_rows(app: &App) -> usize {
    usize::from(app.viewport.terminal_height.saturating_sub(8))
}

fn adjust_scroll(app: &mut App) {
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

/// Stale threshold: entities not updated in this many days are flagged.
const STALE_THRESHOLD_DAYS: u64 = 30;
/// Clusters smaller than this are considered isolated.
const ISOLATED_CLUSTER_THRESHOLD: usize = 3;
/// Number of PageRank iterations for the TUI approximation.
const PAGERANK_ITERATIONS: usize = 20;
/// PageRank damping factor.
const PAGERANK_DAMPING: f64 = 0.85;

async fn load_graph_data(app: &mut App) {
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

    compute_graph_stats(app);
    compute_drift_analysis(app);
}

fn compute_graph_stats(app: &mut App) {
    let entities = &app.layout.memory.graph.entities;
    let relationships = &app.layout.memory.graph.relationships;

    let mut rel_counts: HashMap<String, usize> = HashMap::new();
    for rel in relationships {
        *rel_counts.entry(rel.src.clone()).or_default() += 1;
        *rel_counts.entry(rel.dst.clone()).or_default() += 1;
    }

    let pageranks = compute_pagerank(entities, relationships);
    let communities = compute_communities(entities, relationships);

    let entity_stats: Vec<GraphEntityStat> = entities
        .iter()
        .map(|e| {
            let rel_count = rel_counts.get(&e.id).copied().unwrap_or(0);
            let pagerank = pageranks.get(&e.id).copied().unwrap_or(0.0);
            let community_id = communities.get(&e.id).copied();
            GraphEntityStat {
                entity: e.clone(),
                relationship_count: rel_count,
                community_id,
                pagerank,
            }
        })
        .collect();

    let community_sizes = compute_community_sizes(&communities);
    let community_count = community_sizes.len();
    let avg_cluster_size = if community_count > 0 {
        let total: usize = community_sizes.values().sum();
        total as f64 / community_count as f64
    } else {
        0.0
    };

    let orphan_count = entity_stats
        .iter()
        .filter(|s| s.relationship_count == 0)
        .count();
    let stale_count = count_stale_entities(entities);
    let isolated_cluster_count = community_sizes
        .values()
        .filter(|&&size| size < ISOLATED_CLUSTER_THRESHOLD)
        .count();

    app.layout.memory.graph.entity_stats = entity_stats;
    app.layout.memory.graph.health = GraphHealthMetrics {
        total_entities: entities.len(),
        total_relationships: relationships.len(),
        orphan_count,
        stale_count,
        avg_cluster_size,
        community_count,
        isolated_cluster_count,
    };
}

fn compute_drift_analysis(app: &mut App) {
    let stats = &app.layout.memory.graph.entity_stats;
    let relationships = &app.layout.memory.graph.relationships;

    let mut suggestions = Vec::new();
    let mut orphaned = Vec::new();
    let mut stale = Vec::new();

    for stat in stats {
        if stat.relationship_count == 0 {
            orphaned.push(stat.entity.name.clone());
            suggestions.push(DriftSuggestion {
                action: "review".into(),
                entity_name: stat.entity.name.clone(),
                reason: "orphaned: no relationships".into(),
            });
        }
        if is_entity_stale(&stat.entity) {
            stale.push(stat.entity.name.clone());
            if stat.relationship_count == 0 {
                suggestions.push(DriftSuggestion {
                    action: "delete".into(),
                    entity_name: stat.entity.name.clone(),
                    reason: "stale and orphaned".into(),
                });
            }
        }
        if stat.pagerank < 0.01 && stat.relationship_count > 0 {
            suggestions.push(DriftSuggestion {
                action: "review".into(),
                entity_name: stat.entity.name.clone(),
                reason: "very low PageRank despite having relationships".into(),
            });
        }
    }

    // WHY: detect potential duplicates by checking for entities with similar names
    let names: Vec<&str> = stats.iter().map(|s| s.entity.name.as_str()).collect();
    for (i, a) in names.iter().enumerate() {
        for b in names.iter().skip(i + 1) {
            if a.to_lowercase() == b.to_lowercase() && a != b {
                suggestions.push(DriftSuggestion {
                    action: "merge".into(),
                    entity_name: format!("{a} / {b}"),
                    reason: "potential duplicate (case mismatch)".into(),
                });
            }
        }
    }

    let communities = compute_communities(&app.layout.memory.graph.entities, relationships);
    let community_members = invert_communities(&communities);
    let mut isolated_clusters = Vec::new();
    for members in community_members.values() {
        if members.len() < ISOLATED_CLUSTER_THRESHOLD {
            isolated_clusters.push(IsolatedCluster {
                entity_names: members.clone(),
                size: members.len(),
            });
        }
    }

    app.layout.memory.graph.drift_suggestions = suggestions;
    app.layout.memory.graph.orphaned_entities = orphaned;
    app.layout.memory.graph.stale_entities = stale;
    app.layout.memory.graph.isolated_clusters = isolated_clusters;
}

/// Simple iterative PageRank on the entity graph.
fn compute_pagerank(
    entities: &[MemoryEntity],
    relationships: &[MemoryRelationship],
) -> HashMap<String, f64> {
    if entities.is_empty() {
        return HashMap::new();
    }

    let n = entities.len();
    let initial = 1.0 / n as f64;
    let mut scores: HashMap<String, f64> =
        entities.iter().map(|e| (e.id.clone(), initial)).collect();

    let mut outgoing: HashMap<String, Vec<String>> = HashMap::new();
    for rel in relationships {
        outgoing
            .entry(rel.src.clone())
            .or_default()
            .push(rel.dst.clone());
    }

    for _ in 0..PAGERANK_ITERATIONS {
        let mut new_scores: HashMap<String, f64> = entities
            .iter()
            .map(|e| (e.id.clone(), (1.0 - PAGERANK_DAMPING) / n as f64))
            .collect();

        for entity in entities {
            let score = scores.get(&entity.id).copied().unwrap_or(0.0);
            if let Some(targets) = outgoing.get(&entity.id)
                && !targets.is_empty()
            {
                let share = score / targets.len() as f64;
                for target in targets {
                    if let Some(s) = new_scores.get_mut(target) {
                        *s += PAGERANK_DAMPING * share;
                    }
                }
            }
        }

        scores = new_scores;
    }

    scores
}

/// Union-Find community detection (connected components).
fn compute_communities(
    entities: &[MemoryEntity],
    relationships: &[MemoryRelationship],
) -> HashMap<String, u32> {
    if entities.is_empty() {
        return HashMap::new();
    }

    let mut id_to_idx: HashMap<&str, usize> = HashMap::new();
    for (i, e) in entities.iter().enumerate() {
        id_to_idx.insert(&e.id, i);
    }

    let mut parent: Vec<usize> = (0..entities.len()).collect();

    for rel in relationships {
        if let (Some(&a), Some(&b)) = (
            id_to_idx.get(rel.src.as_str()),
            id_to_idx.get(rel.dst.as_str()),
        ) {
            union(&mut parent, a, b);
        }
    }

    let mut component_ids: HashMap<usize, u32> = HashMap::new();
    let mut next_id = 0u32;
    let mut result: HashMap<String, u32> = HashMap::new();
    for (i, entity) in entities.iter().enumerate() {
        let root = find(&mut parent, i);
        let cid = *component_ids.entry(root).or_insert_with(|| {
            let id = next_id;
            next_id += 1;
            id
        });
        result.insert(entity.id.clone(), cid);
    }

    result
}

fn find(parent: &mut Vec<usize>, i: usize) -> usize {
    if parent[i] != i {
        parent[i] = find(parent, parent[i]);
    }
    parent[i]
}

fn union(parent: &mut Vec<usize>, a: usize, b: usize) {
    let ra = find(parent, a);
    let rb = find(parent, b);
    if ra != rb {
        parent[ra] = rb;
    }
}

fn compute_community_sizes(communities: &HashMap<String, u32>) -> HashMap<u32, usize> {
    let mut sizes: HashMap<u32, usize> = HashMap::new();
    for cid in communities.values() {
        *sizes.entry(*cid).or_default() += 1;
    }
    sizes
}

fn invert_communities(communities: &HashMap<String, u32>) -> BTreeMap<u32, Vec<String>> {
    let mut result: BTreeMap<u32, Vec<String>> = BTreeMap::new();
    for (entity_id, cid) in communities {
        result.entry(*cid).or_default().push(entity_id.clone());
    }
    result
}

fn is_entity_stale(entity: &MemoryEntity) -> bool {
    if entity.updated_at.is_empty() {
        return !entity.created_at.is_empty();
    }
    let date_str = entity
        .updated_at
        .split('T')
        .next()
        .unwrap_or(&entity.updated_at);
    // WHY: simple date comparison; parse YYYY-MM-DD and check against threshold
    if date_str.len() < 10 {
        return false;
    }
    let now_approx = "2026-03-22";
    date_str < &now_approx[..date_str.len().min(10)]
        && days_between_approx(date_str, now_approx) > STALE_THRESHOLD_DAYS
}

fn days_between_approx(a: &str, b: &str) -> u64 {
    // WHY: rough day estimate for staleness check; no jiff dependency in TUI
    let parse = |s: &str| -> Option<u64> {
        let parts: Vec<&str> = s.split('-').collect();
        if parts.len() < 3 {
            return None;
        }
        let y: u64 = parts[0].parse().ok()?;
        let m: u64 = parts[1].parse().ok()?;
        let d: u64 = parts[2].parse().ok()?;
        Some(y * 365 + m * 30 + d)
    };
    match (parse(a), parse(b)) {
        (Some(da), Some(db)) => db.saturating_sub(da),
        _ => 0,
    }
}

fn count_stale_entities(entities: &[MemoryEntity]) -> usize {
    entities.iter().filter(|e| is_entity_stale(e)).count()
}

fn build_node_card(app: &mut App, entity_id: &str) {
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

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions may panic on failure")]
mod tests {
    use super::*;
    use crate::app::test_helpers::*;
    use crate::state::memory::{FactLifecycleMeta, FactTemporalMeta, MemoryFact};

    fn sample_fact(id: &str, content: &str, confidence: f64) -> MemoryFact {
        MemoryFact {
            id: id.into(),
            nous_id: "syn".into(),
            content: content.into(),
            confidence,
            tier: "verified".into(),
            fact_type: "knowledge".into(),
            temporal: FactTemporalMeta {
                valid_from: "2026-01-01".into(),
                valid_to: "9999-12-31".into(),
                recorded_at: "2026-01-01T00:00:00Z".into(),
                access_count: 5,
                last_accessed_at: "2026-03-09T12:00:00Z".into(),
                stability_hours: 720.0,
            },
            lifecycle: FactLifecycleMeta {
                superseded_by: None,
                source_session_id: Some("ses-1".into()),
                is_forgotten: false,
                forgotten_at: None,
                forget_reason: None,
            },
        }
    }

    #[test]
    fn handle_select_down_increments() {
        let mut app = test_app();
        app.layout.memory.fact_list.facts = vec![
            sample_fact("f1", "first", 0.9),
            sample_fact("f2", "second", 0.8),
        ];
        app.layout.memory.fact_list.selected = 0;
        handle_select_down(&mut app);
        assert_eq!(app.layout.memory.fact_list.selected, 1);
    }

    #[test]
    fn handle_select_down_clamps_at_end() {
        let mut app = test_app();
        app.layout.memory.fact_list.facts = vec![sample_fact("f1", "first", 0.9)];
        app.layout.memory.fact_list.selected = 0;
        handle_select_down(&mut app);
        assert_eq!(app.layout.memory.fact_list.selected, 0);
    }

    #[test]
    fn handle_select_up_decrements() {
        let mut app = test_app();
        app.layout.memory.fact_list.facts = vec![
            sample_fact("f1", "first", 0.9),
            sample_fact("f2", "second", 0.8),
        ];
        app.layout.memory.fact_list.selected = 1;
        handle_select_up(&mut app);
        assert_eq!(app.layout.memory.fact_list.selected, 0);
    }

    #[test]
    fn handle_select_up_clamps_at_zero() {
        let mut app = test_app();
        app.layout.memory.fact_list.facts = vec![sample_fact("f1", "first", 0.9)];
        app.layout.memory.fact_list.selected = 0;
        handle_select_up(&mut app);
        assert_eq!(app.layout.memory.fact_list.selected, 0);
    }

    #[test]
    fn handle_select_first_goes_to_zero() {
        let mut app = test_app();
        app.layout.memory.fact_list.facts = vec![
            sample_fact("f1", "first", 0.9),
            sample_fact("f2", "second", 0.8),
        ];
        app.layout.memory.fact_list.selected = 1;
        handle_select_first(&mut app);
        assert_eq!(app.layout.memory.fact_list.selected, 0);
    }

    #[test]
    fn handle_select_last_goes_to_end() {
        let mut app = test_app();
        app.layout.memory.fact_list.facts = vec![
            sample_fact("f1", "first", 0.9),
            sample_fact("f2", "second", 0.8),
            sample_fact("f3", "third", 0.7),
        ];
        app.layout.memory.fact_list.selected = 0;
        handle_select_last(&mut app);
        assert_eq!(app.layout.memory.fact_list.selected, 2);
    }

    #[test]
    fn handle_sort_cycle_advances() {
        let mut app = test_app();
        assert_eq!(
            app.layout.memory.fact_list.sort,
            crate::state::memory::FactSort::Confidence
        );
        handle_sort_cycle(&mut app);
        assert_eq!(
            app.layout.memory.fact_list.sort,
            crate::state::memory::FactSort::Recency
        );
    }

    #[test]
    fn handle_tab_next_cycles() {
        let mut app = test_app();
        assert_eq!(
            app.layout.memory.tab,
            crate::state::memory::MemoryTab::Facts
        );
        handle_tab_next(&mut app);
        assert_eq!(
            app.layout.memory.tab,
            crate::state::memory::MemoryTab::Graph
        );
        handle_tab_next(&mut app);
        assert_eq!(
            app.layout.memory.tab,
            crate::state::memory::MemoryTab::Drift
        );
        handle_tab_next(&mut app);
        assert_eq!(
            app.layout.memory.tab,
            crate::state::memory::MemoryTab::Timeline
        );
        handle_tab_next(&mut app);
        assert_eq!(
            app.layout.memory.tab,
            crate::state::memory::MemoryTab::Facts
        );
    }

    #[test]
    fn handle_tab_prev_cycles() {
        let mut app = test_app();
        handle_tab_prev(&mut app);
        assert_eq!(
            app.layout.memory.tab,
            crate::state::memory::MemoryTab::Timeline
        );
    }

    #[test]
    fn handle_filter_lifecycle() {
        let mut app = test_app();
        assert!(!app.layout.memory.filters.filter_editing);

        handle_filter_open(&mut app);
        assert!(app.layout.memory.filters.filter_editing);

        handle_filter_input(&mut app, 'r');
        handle_filter_input(&mut app, 'u');
        handle_filter_input(&mut app, 's');
        handle_filter_input(&mut app, 't');
        assert_eq!(app.layout.memory.filters.filter_text, "rust");

        handle_filter_backspace(&mut app);
        assert_eq!(app.layout.memory.filters.filter_text, "rus");

        handle_filter_close(&mut app);
        assert!(!app.layout.memory.filters.filter_editing);
        assert!(app.layout.memory.filters.filter_text.is_empty());
    }

    #[test]
    fn handle_filter_backspace_on_empty_closes() {
        let mut app = test_app();
        app.layout.memory.filters.filter_editing = true;
        handle_filter_backspace(&mut app);
        assert!(!app.layout.memory.filters.filter_editing);
    }

    #[test]
    fn handle_facts_loaded_sets_data() {
        let mut app = test_app();
        let facts = vec![
            sample_fact("f1", "first", 0.9),
            sample_fact("f2", "second", 0.8),
        ];
        handle_facts_loaded(&mut app, facts, 2);
        assert_eq!(app.layout.memory.fact_list.facts.len(), 2);
        assert_eq!(app.layout.memory.fact_list.total_facts, 2);
        assert!(!app.layout.memory.loading);
    }

    #[test]
    fn handle_close_pops_memory_view() {
        let mut app = test_app();
        app.layout.view_stack.push(View::MemoryInspector);
        handle_close(&mut app);
        assert!(app.layout.view_stack.is_home());
    }

    #[test]
    fn handle_pop_back_from_detail_goes_to_inspector() {
        let mut app = test_app();
        app.layout.view_stack.push(View::MemoryInspector);
        app.layout.view_stack.push(View::FactDetail {
            fact_id: "f1".into(),
        });
        handle_pop_back(&mut app);
        assert_eq!(app.layout.view_stack.current(), &View::MemoryInspector);
        assert!(app.layout.memory.fact_list.detail.is_none());
    }

    #[test]
    fn handle_confidence_input_accepts_digits_and_dot() {
        let mut app = test_app();
        app.layout.memory.fact_list.editing_confidence = true;
        app.layout.memory.fact_list.confidence_buffer.clear();
        handle_confidence_input(&mut app, '0');
        handle_confidence_input(&mut app, '.');
        handle_confidence_input(&mut app, '8');
        assert_eq!(app.layout.memory.fact_list.confidence_buffer, "0.8");
    }

    #[test]
    fn handle_confidence_input_rejects_letters() {
        let mut app = test_app();
        app.layout.memory.fact_list.editing_confidence = true;
        app.layout.memory.fact_list.confidence_buffer.clear();
        handle_confidence_input(&mut app, 'a');
        assert!(app.layout.memory.fact_list.confidence_buffer.is_empty());
    }

    #[test]
    fn handle_confidence_cancel_clears() {
        let mut app = test_app();
        app.layout.memory.fact_list.editing_confidence = true;
        app.layout.memory.fact_list.confidence_buffer = "0.5".into();
        handle_confidence_cancel(&mut app);
        assert!(!app.layout.memory.fact_list.editing_confidence);
        assert!(app.layout.memory.fact_list.confidence_buffer.is_empty());
    }

    #[test]
    fn handle_search_lifecycle() {
        let mut app = test_app();
        handle_search_open(&mut app);
        assert!(app.layout.memory.search.search_active);

        handle_search_input(&mut app, 'r');
        handle_search_input(&mut app, 'u');
        assert_eq!(app.layout.memory.search.search_query, "ru");

        handle_search_backspace(&mut app);
        assert_eq!(app.layout.memory.search.search_query, "r");

        handle_search_close(&mut app);
        assert!(!app.layout.memory.search.search_active);
        assert!(app.layout.memory.search.search_query.is_empty());
    }

    #[test]
    fn handle_search_backspace_on_empty_closes() {
        let mut app = test_app();
        app.layout.memory.search.search_active = true;
        handle_search_backspace(&mut app);
        assert!(!app.layout.memory.search.search_active);
    }

    #[test]
    fn handle_action_result_sets_toast() {
        let mut app = test_app();
        handle_action_result(&mut app, "done".into());
        assert!(app.viewport.error_toast.is_some());
        assert_eq!(app.viewport.error_toast.as_ref().unwrap().message, "done");
    }

    #[test]
    fn adjust_scroll_scrolls_down() {
        let mut app = test_app();
        app.viewport.terminal_height = 20; // visible_rows = 12
        app.layout.memory.fact_list.facts = (0..20)
            .map(|i| sample_fact(&format!("f{i}"), &format!("fact {i}"), 0.9))
            .collect();
        app.layout.memory.fact_list.selected = 15;
        adjust_scroll(&mut app);
        assert!(app.layout.memory.fact_list.scroll_offset > 0);
    }

    #[test]
    fn item_count_for_each_tab() {
        let mut app = test_app();
        app.layout.memory.fact_list.facts = vec![sample_fact("f1", "a", 0.9)];
        app.layout.memory.tab = crate::state::memory::MemoryTab::Facts;
        assert_eq!(item_count(&app), 1);

        app.layout.memory.tab = crate::state::memory::MemoryTab::Graph;
        assert_eq!(item_count(&app), 0);

        app.layout.memory.tab = crate::state::memory::MemoryTab::Drift;
        assert_eq!(item_count(&app), 0);

        app.layout.memory.tab = crate::state::memory::MemoryTab::Timeline;
        assert_eq!(item_count(&app), 0);
    }

    #[test]
    fn handle_drift_tab_next_cycles() {
        let mut app = test_app();
        assert_eq!(
            app.layout.memory.graph.drift_tab,
            crate::state::memory::DriftTab::Suggestions
        );
        handle_drift_tab_next(&mut app);
        assert_eq!(
            app.layout.memory.graph.drift_tab,
            crate::state::memory::DriftTab::Orphans
        );
    }

    #[test]
    fn handle_drift_tab_prev_cycles() {
        let mut app = test_app();
        handle_drift_tab_prev(&mut app);
        assert_eq!(
            app.layout.memory.graph.drift_tab,
            crate::state::memory::DriftTab::Isolated
        );
    }

    #[test]
    fn compute_pagerank_empty() {
        let scores = compute_pagerank(&[], &[]);
        assert!(scores.is_empty());
    }

    #[test]
    fn compute_pagerank_single_entity() {
        let entities = vec![MemoryEntity {
            id: "e1".into(),
            name: "alice".into(),
            entity_type: "person".into(),
            aliases: Vec::new(),
            created_at: String::new(),
            updated_at: String::new(),
        }];
        let scores = compute_pagerank(&entities, &[]);
        assert!(scores.contains_key("e1"));
    }

    #[test]
    fn compute_communities_groups_connected_entities() {
        let entities = vec![
            MemoryEntity {
                id: "e1".into(),
                name: "alice".into(),
                entity_type: "person".into(),
                aliases: Vec::new(),
                created_at: String::new(),
                updated_at: String::new(),
            },
            MemoryEntity {
                id: "e2".into(),
                name: "bob".into(),
                entity_type: "person".into(),
                aliases: Vec::new(),
                created_at: String::new(),
                updated_at: String::new(),
            },
            MemoryEntity {
                id: "e3".into(),
                name: "charlie".into(),
                entity_type: "person".into(),
                aliases: Vec::new(),
                created_at: String::new(),
                updated_at: String::new(),
            },
        ];
        let relationships = vec![MemoryRelationship {
            src: "e1".into(),
            dst: "e2".into(),
            relation: "KNOWS".into(),
            weight: 1.0,
            created_at: String::new(),
        }];
        let communities = compute_communities(&entities, &relationships);
        assert_eq!(communities.get("e1"), communities.get("e2"));
        assert_ne!(communities.get("e1"), communities.get("e3"));
    }

    #[test]
    fn days_between_approx_calculates() {
        assert!(days_between_approx("2026-01-01", "2026-03-22") > 30);
        assert_eq!(days_between_approx("invalid", "2026-03-22"), 0);
    }
}

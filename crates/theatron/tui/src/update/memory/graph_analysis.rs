//! Graph statistics, PageRank, community detection, and drift analysis.

use std::collections::{BTreeMap, HashMap};

use crate::app::App;
use crate::state::memory::{
    DriftSuggestion, GraphEntityStat, GraphHealthMetrics, IsolatedCluster, MemoryEntity,
    MemoryRelationship,
};

/// Stale threshold: entities not updated in this many days are flagged.
const STALE_THRESHOLD_DAYS: u64 = 30;
/// Clusters smaller than this are considered isolated.
const ISOLATED_CLUSTER_THRESHOLD: usize = 3;
/// Number of PageRank iterations for the TUI approximation.
const PAGERANK_ITERATIONS: usize = 20;
/// PageRank damping factor.
const PAGERANK_DAMPING: f64 = 0.85;

pub(super) fn compute_graph_stats(app: &mut App) {
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

pub(super) fn compute_drift_analysis(app: &mut App) {
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
pub(super) fn compute_pagerank(
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

pub(super) fn days_between_approx(a: &str, b: &str) -> u64 {
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

//! Serendipity engine: cross-domain discovery and unexpected connection finding.
//!
//! Implements four core capabilities:
//! - **Random walk exploration**: traverse the knowledge graph from a starting entity
//! - **Surprise scoring**: identify facts the agent hasn't accessed recently
//! - **Path exploration**: find paths between seemingly unrelated entities
//! - **Serendipity injection**: select "did you know?" facts for context injection
#![cfg_attr(
    all(not(test), not(feature = "mneme-engine")),
    expect(
        dead_code,
        reason = "serendipity engine helpers are used by gated callers"
    )
)]

use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};

use crate::graph_intelligence::GraphContext;
use crate::id::EntityId;
#[cfg(feature = "mneme-engine")]
use crate::knowledge_store::{KnowledgeStore, SerendipityDiscoveryReport};
#[cfg(feature = "mneme-engine")]
use std::collections::BTreeMap;
#[cfg(feature = "mneme-engine")]
use std::collections::hash_map::DefaultHasher;
#[cfg(feature = "mneme-engine")]
use std::hash::{Hash, Hasher};

/// Configuration for the serendipity engine.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct SerendipityConfig {
    /// Maximum steps in a random walk. Default: 6
    pub max_walk_length: u32,
    /// Number of random walks per exploration. Default: 10
    pub walk_count: u32,
    /// Minimum surprise score to surface a discovery. Default: 0.3
    pub surprise_threshold: f64,
    /// Weight for novelty vs relevance in serendipity scoring. Default: 0.5
    /// 0.0 = pure relevance, 1.0 = pure novelty.
    pub novelty_weight: f64,
    /// Maximum graph distance for path exploration. Default: 4
    pub max_path_depth: u32,
    /// Maximum number of discovery results to return. Default: 10
    pub max_results: usize,
}

impl Default for SerendipityConfig {
    fn default() -> Self {
        Self {
            max_walk_length: 6,
            walk_count: 10,
            surprise_threshold: 0.3,
            novelty_weight: 0.5,
            max_path_depth: 4,
            max_results: 10,
        }
    }
}

/// A node in the knowledge graph for serendipity exploration.
#[derive(Debug, Clone)]
pub(crate) struct GraphNode {
    /// Entity identifier.
    pub entity_id: EntityId,
    /// Display name.
    pub name: String,
    /// `PageRank` importance score [0.0, 1.0].
    pub pagerank: f64,
    /// Louvain community/cluster assignment.
    pub community: i64,
}

/// A scored discovery from the serendipity engine.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct Discovery {
    /// The discovered entity.
    pub entity_id: EntityId,
    /// Display name.
    pub name: String,
    /// Combined serendipity score [0.0, 1.0].
    pub serendipity_score: f64,
    /// Relevance component [0.0, 1.0] (inverse graph distance).
    pub relevance: f64,
    /// Novelty component [0.0, 1.0] (cross-community + obscurity).
    pub novelty: f64,
    /// Surprise component [0.0, 1.0] (recency-weighted unexpectedness).
    pub surprise: f64,
    /// Graph distance from the query context.
    pub graph_distance: Option<u32>,
    /// Community/cluster ID.
    pub community: i64,
}

/// A path between two entities in the knowledge graph.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct ExploredPath {
    /// Ordered list of entity IDs along the path.
    pub nodes: Vec<EntityId>,
    /// Relationship labels along each edge.
    pub edge_labels: Vec<String>,
    /// Path length (number of edges).
    pub length: u32,
    /// Number of distinct communities traversed.
    pub communities_traversed: u32,
    /// Interest score combining distance and cross-community traversal.
    pub interest_score: f64,
}

/// A fact selected for serendipity injection into context.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct SerendipityInjection {
    /// The fact content to inject.
    pub content: String,
    /// Source fact ID.
    pub fact_id: String,
    /// Surprise score that triggered the injection.
    pub surprise_score: f64,
    /// Brief explanation of why this is interesting.
    pub connection_reason: String,
}

/// Snapshot of the knowledge graph for serendipity operations.
///
/// Loaded once per exploration session. All graph traversal operates
/// on this in-memory snapshot rather than hitting the store repeatedly.
#[derive(Debug, Clone, Default)]
pub(crate) struct GraphSnapshot {
    /// All nodes keyed by entity ID string.
    pub nodes: HashMap<String, GraphNode>,
    /// Adjacency list keyed by entity ID.
    pub adjacency: HashMap<String, HashSet<String>>,
    /// Edge labels: `(src, dst)` → relationship type.
    pub edge_labels: HashMap<(String, String), String>,
    /// Maximum `PageRank` score across all nodes (for normalization).
    pub max_pagerank: f64,
}

impl GraphSnapshot {
    /// Add a node to the snapshot.
    pub(crate) fn add_node(&mut self, node: GraphNode) {
        let id = node.entity_id.as_str().to_owned();
        if node.pagerank > self.max_pagerank {
            self.max_pagerank = node.pagerank;
        }
        self.adjacency.entry(id.clone()).or_default();
        self.nodes.insert(id, node);
    }

    /// Add an edge to the snapshot.
    pub(crate) fn add_edge(&mut self, src: &str, dst: &str, relation: &str) {
        self.adjacency
            .entry(src.to_owned())
            .or_default()
            .insert(dst.to_owned());
        self.adjacency
            .entry(dst.to_owned())
            .or_default()
            .insert(src.to_owned());
        self.edge_labels
            .insert((src.to_owned(), dst.to_owned()), relation.to_owned());
        self.edge_labels
            .insert((dst.to_owned(), src.to_owned()), relation.to_owned());
    }

    /// Build a snapshot from graph context plus explicit node names and labeled edges.
    #[must_use]
    pub(crate) fn from_graph_context<N, E>(ctx: &GraphContext, nodes: N, edges: E) -> Self
    where
        N: IntoIterator<Item = (String, String)>,
        E: IntoIterator<Item = (String, String, String)>,
    {
        let mut snapshot = GraphSnapshot::default();

        for (entity_id, name) in nodes {
            if let Ok(entity_id) = EntityId::new(entity_id.as_str()) {
                snapshot.add_node(GraphNode {
                    pagerank: ctx.importance(entity_id.as_str()),
                    community: ctx.clusters.get(entity_id.as_str()).copied().unwrap_or(-1),
                    entity_id,
                    name,
                });
            }
        }

        let ensure_node = |snapshot: &mut GraphSnapshot, entity_id: &str| {
            if snapshot.nodes.contains_key(entity_id) {
                return;
            }
            if let Ok(entity_id) = EntityId::new(entity_id) {
                let entity_id_str = entity_id.as_str().to_owned();
                snapshot.add_node(GraphNode {
                    pagerank: ctx.importance(entity_id.as_str()),
                    community: ctx.clusters.get(entity_id.as_str()).copied().unwrap_or(-1),
                    name: entity_id_str,
                    entity_id,
                });
            }
        };

        for (src, dst, relation) in edges {
            ensure_node(&mut snapshot, &src);
            ensure_node(&mut snapshot, &dst);
            snapshot.add_edge(&src, &dst, &relation);
        }

        snapshot
    }

    /// Get neighbors of an entity.
    #[must_use]
    pub(crate) fn neighbors(&self, entity_id: &str) -> Vec<&str> {
        let mut neighbors: Vec<&str> = self
            .adjacency
            .get(entity_id)
            .map(|set| set.iter().map(String::as_str).collect())
            .unwrap_or_default();
        neighbors.sort_unstable();
        neighbors
    }

    /// Total number of nodes.
    #[must_use]
    #[cfg(test)]
    pub(crate) fn node_count(&self) -> usize {
        self.nodes.len()
    }

    /// Normalized `PageRank` denominator (avoids division by zero).
    fn max_pagerank_or_one(&self) -> f64 {
        if self.max_pagerank > 0.0 {
            self.max_pagerank
        } else {
            1.0
        }
    }
}

/// Perform a random walk exploration from seed entities.
///
/// Executes multiple random walks from each seed, collecting visited nodes.
/// Returns visit frequencies that indicate how "reachable" each entity is
/// from the seeds through random traversal.
#[must_use]
pub(crate) fn random_walk(
    graph: &GraphSnapshot,
    seeds: &[String],
    config: &SerendipityConfig,
    rng_seed: u64,
) -> HashMap<String, u32> {
    use rand::SeedableRng;
    use rand::prelude::IndexedRandom;
    use rand::rngs::SmallRng;

    let mut rng = SmallRng::seed_from_u64(rng_seed);
    let mut visit_counts: HashMap<String, u32> = HashMap::new();

    for seed in seeds {
        if !graph.adjacency.contains_key(seed) {
            continue;
        }

        for _ in 0..config.walk_count {
            let mut current = seed.clone();
            for _ in 0..config.max_walk_length {
                let neighbors = graph.neighbors(&current);
                if neighbors.is_empty() {
                    break;
                }
                let next = match neighbors.choose(&mut rng) {
                    Some(n) => (*n).to_owned(),
                    None => break,
                };
                *visit_counts.entry(next.clone()).or_insert(0) += 1;
                current = next;
            }
        }
    }

    // WHY: Remove seeds from results to focus on discoveries
    for seed in seeds {
        visit_counts.remove(seed);
    }

    visit_counts
}

/// Compute surprise scores for entities based on access recency.
///
/// Surprise is higher for entities that:
/// 1. Haven't been accessed recently (high recency surprise)
/// 2. Are connected to the current context (relevance floor)
/// 3. Are in a different community from the query context (cross-community bonus)
#[must_use]
pub(crate) fn surprise_scores(
    graph: &GraphSnapshot,
    walk_visits: &HashMap<String, u32>,
    home_communities: &HashSet<i64>,
    last_access_hours: &HashMap<String, f64>,
) -> Vec<(String, f64)> {
    let max_visits = walk_visits.values().copied().max().unwrap_or(1);
    let max_pr = graph.max_pagerank_or_one();

    let mut scores: Vec<(String, f64)> = walk_visits
        .iter()
        .filter_map(|(entity_id, &visits)| {
            let node = graph.nodes.get(entity_id)?;

            let reachability = f64::from(visits) / f64::from(max_visits);
            let access_hours = last_access_hours.get(entity_id).copied().unwrap_or(10000.0);
            let recency_surprise = 1.0 - (-0.01 * access_hours).exp();
            let cross_community = community_novelty(node.community, home_communities);
            let obscurity = 1.0 - (node.pagerank / max_pr);

            let surprise = 0.3 * reachability
                + 0.3 * recency_surprise
                + 0.2 * cross_community
                + 0.2 * obscurity;

            Some((entity_id.clone(), surprise))
        })
        .collect();

    scores.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    scores
}

/// Score entities for serendipity: balance relevance vs novelty.
///
/// Serendipity = `(1 - novelty_weight) × relevance + novelty_weight × novelty`
///
/// Relevance: inverse graph distance from seed entities.
/// Novelty: cross-community score + obscurity (low `PageRank`).
#[must_use]
pub(crate) fn score_discoveries(
    graph: &GraphSnapshot,
    seeds: &[String],
    distances: &HashMap<String, u32>,
    config: &SerendipityConfig,
) -> Vec<Discovery> {
    let seed_set: HashSet<&str> = seeds.iter().map(String::as_str).collect();

    let home_communities: HashSet<i64> = seeds
        .iter()
        .filter_map(|s| graph.nodes.get(s).map(|n| n.community))
        .filter(|c| *c >= 0)
        .collect();

    let max_pr = graph.max_pagerank_or_one();

    let mut discoveries: Vec<Discovery> = graph
        .nodes
        .iter()
        .filter(|(id, _)| !seed_set.contains(id.as_str()))
        .filter_map(|(id, node)| {
            let dist = distances.get(id).copied();
            let relevance = dist.map_or(0.0, |d| 1.0 / (1.0 + f64::from(d)));

            if relevance <= 0.0 {
                return None;
            }

            let cross_community = community_novelty(node.community, &home_communities);
            let obscurity = 1.0 - (node.pagerank / max_pr);
            let novelty = 0.6 * cross_community + 0.4 * obscurity;

            let relevance_weight = 1.0 - config.novelty_weight;
            let serendipity = relevance_weight * relevance + config.novelty_weight * novelty;

            if serendipity <= 0.1 {
                return None;
            }

            Some(Discovery {
                entity_id: node.entity_id.clone(),
                name: node.name.clone(),
                serendipity_score: serendipity,
                relevance,
                novelty,
                surprise: 0.0,
                graph_distance: dist,
                community: node.community,
            })
        })
        .collect();

    discoveries.sort_by(|a, b| {
        b.serendipity_score
            .partial_cmp(&a.serendipity_score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    discoveries.truncate(config.max_results);
    discoveries
}

/// Find shortest path between two entities using BFS.
///
/// Returns `None` if no path exists within `max_depth` hops.
#[must_use]
pub(crate) fn find_path(
    graph: &GraphSnapshot,
    source: &str,
    target: &str,
    max_depth: u32,
) -> Option<ExploredPath> {
    if source == target {
        return None;
    }
    if !graph.adjacency.contains_key(source) || !graph.adjacency.contains_key(target) {
        return None;
    }

    let mut visited: HashSet<String> = HashSet::new();
    let mut parent: HashMap<String, String> = HashMap::new();
    let mut queue: std::collections::VecDeque<(String, u32)> = std::collections::VecDeque::new();

    visited.insert(source.to_owned());
    queue.push_back((source.to_owned(), 0));

    while let Some((current, depth)) = queue.pop_front() {
        if depth >= max_depth {
            continue;
        }
        for neighbor in graph.neighbors(&current) {
            if visited.contains(neighbor) {
                continue;
            }
            visited.insert(neighbor.to_owned());
            parent.insert(neighbor.to_owned(), current.clone());

            if neighbor == target {
                return Some(reconstruct_path(graph, source, target, &parent));
            }
            queue.push_back((neighbor.to_owned(), depth + 1));
        }
    }

    None
}

/// Explore novel entities reachable from source within the configured depth.
///
/// Returns paths to the most "interesting" reachable entities, ranked
/// by cross-community traversal and distance.
#[must_use]
pub(crate) fn explore_from(
    graph: &GraphSnapshot,
    source: &str,
    config: &SerendipityConfig,
) -> Vec<ExploredPath> {
    if !graph.adjacency.contains_key(source) {
        return Vec::new();
    }

    let source_community = graph.nodes.get(source).map_or(-1, |n| n.community);
    let distances = bfs_distances(graph, source, config.max_path_depth);

    let mut scored: Vec<(String, f64, u32)> = distances
        .iter()
        .filter(|(id, _)| id.as_str() != source)
        .filter_map(|(id, &dist)| {
            let node = graph.nodes.get(id)?;
            let cross = if node.community != source_community
                && node.community >= 0
                && source_community >= 0
            {
                1.0
            } else {
                0.3
            };
            let interest = cross * f64::from(dist);
            Some((id.clone(), interest, dist))
        })
        .collect();

    scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    scored.truncate(config.max_results);

    scored
        .into_iter()
        .filter_map(|(target, interest, _)| {
            let mut path = find_path(graph, source, &target, config.max_path_depth)?;
            path.interest_score = interest;
            Some(path)
        })
        .collect()
}

/// Select facts for serendipity injection into agent context.
///
/// Picks the most surprising fact from the scored discoveries that
/// exceeds the surprise threshold.
#[must_use]
pub(crate) fn select_injection<S: ::std::hash::BuildHasher>(
    discoveries: &[Discovery],
    fact_contents: &HashMap<String, (String, String), S>,
    config: &SerendipityConfig,
) -> Option<SerendipityInjection> {
    discoveries
        .iter()
        .filter(|d| d.surprise >= config.surprise_threshold || d.serendipity_score >= 0.5)
        .find_map(|d| {
            let entity_key = d.entity_id.as_str();
            let (fact_id, content) = fact_contents.get(entity_key)?;
            Some(SerendipityInjection {
                content: content.clone(),
                fact_id: fact_id.clone(),
                surprise_score: d.surprise.max(d.serendipity_score),
                connection_reason: format!(
                    "connected to {} (distance: {}, community: {})",
                    d.name,
                    d.graph_distance
                        .map_or_else(|| "unknown".to_owned(), |dist| dist.to_string()),
                    d.community
                ),
            })
        })
}

/// Community novelty score: 1.0 if in a different community, 0.3 if same.
fn community_novelty(community: i64, home_communities: &HashSet<i64>) -> f64 {
    if community >= 0 && !home_communities.contains(&community) {
        1.0
    } else {
        0.3
    }
}

/// BFS distance computation from a source node.
fn bfs_distances(graph: &GraphSnapshot, source: &str, max_depth: u32) -> HashMap<String, u32> {
    let mut distances: HashMap<String, u32> = HashMap::new();
    let mut queue: std::collections::VecDeque<(String, u32)> = std::collections::VecDeque::new();

    distances.insert(source.to_owned(), 0);
    queue.push_back((source.to_owned(), 0));

    while let Some((current, depth)) = queue.pop_front() {
        if depth >= max_depth {
            continue;
        }
        for neighbor in graph.neighbors(&current) {
            if distances.contains_key(neighbor) {
                continue;
            }
            let next_depth = depth + 1;
            distances.insert(neighbor.to_owned(), next_depth);
            queue.push_back((neighbor.to_owned(), next_depth));
        }
    }

    distances
}

/// Reconstruct a path from BFS parent map.
fn reconstruct_path(
    graph: &GraphSnapshot,
    source: &str,
    target: &str,
    parent: &HashMap<String, String>,
) -> ExploredPath {
    let mut path_nodes = vec![target.to_owned()];
    let mut current = target.to_owned();
    while current != source {
        let prev = match parent.get(&current) {
            Some(p) => p.clone(),
            None => break,
        };
        path_nodes.push(prev.clone());
        current = prev;
    }
    path_nodes.reverse();

    let edge_labels: Vec<String> = path_nodes
        .windows(2)
        .filter_map(|pair| {
            let src = pair.first()?;
            let dst = pair.get(1)?;
            Some(
                graph
                    .edge_labels
                    .get(&(src.clone(), dst.clone()))
                    .cloned()
                    .unwrap_or_else(|| "connected".to_owned()),
            )
        })
        .collect();

    let mut communities: HashSet<i64> = HashSet::new();
    for node_id in &path_nodes {
        if let Some(node) = graph.nodes.get(node_id)
            && node.community >= 0
        {
            communities.insert(node.community);
        }
    }

    let length = u32::try_from(path_nodes.len().saturating_sub(1)).unwrap_or(u32::MAX);
    let communities_traversed = u32::try_from(communities.len()).unwrap_or(u32::MAX);

    let node_ids: Vec<EntityId> = path_nodes
        .into_iter()
        .filter_map(|id| EntityId::new(id).ok())
        .collect();

    ExploredPath {
        nodes: node_ids,
        edge_labels,
        length,
        communities_traversed,
        interest_score: 0.0,
    }
}

#[cfg(feature = "mneme-engine")]
#[derive(Debug, Clone, Copy)]
struct EntityActivity {
    last_access_hours: f64,
    access_count: u32,
    recorded_at: jiff::Timestamp,
}

#[cfg(feature = "mneme-engine")]
impl EntityActivity {
    fn new(last_access_hours: f64, access_count: u32, recorded_at: jiff::Timestamp) -> Self {
        Self {
            last_access_hours,
            access_count,
            recorded_at,
        }
    }

    fn update(&mut self, last_access_hours: f64, access_count: u32, recorded_at: jiff::Timestamp) {
        self.last_access_hours = self.last_access_hours.min(last_access_hours);
        self.access_count = self.access_count.max(access_count);
        if recorded_at > self.recorded_at {
            self.recorded_at = recorded_at;
        }
    }
}

#[cfg(feature = "mneme-engine")]
#[expect(
    clippy::too_many_lines,
    reason = "orchestrates the full serendipity maintenance flow"
)]
/// Run the serendipity engine against real store data and return a report.
pub(crate) fn discover_serendipitous_facts(
    store: &KnowledgeStore,
    nous_id: &str,
) -> crate::error::Result<SerendipityDiscoveryReport> {
    let now = jiff::Timestamp::now();
    let now_str = crate::knowledge::format_timestamp(&now);
    let facts = store.query_facts(nous_id, &now_str, 200)?;
    let config = SerendipityConfig::default();

    let mut entity_activity: HashMap<String, EntityActivity> = HashMap::new();
    let mut fact_contents: HashMap<String, (String, String)> = HashMap::new();
    let mut entity_links = 0u64;

    for fact in &facts {
        let access_time = fact
            .access
            .last_accessed_at
            .unwrap_or(fact.temporal.recorded_at);
        let age_hours = now.duration_since(access_time).as_secs_f64() / 3600.0;
        let entity_ids = fact_entity_ids(store, fact.id.as_str())?;

        for entity_id in entity_ids {
            entity_links = entity_links.saturating_add(1);
            fact_contents
                .entry(entity_id.clone())
                .or_insert_with(|| (fact.id.as_str().to_owned(), fact.content.clone()));
            entity_activity
                .entry(entity_id)
                .and_modify(|activity| {
                    activity.update(
                        age_hours,
                        fact.access.access_count,
                        fact.temporal.recorded_at,
                    );
                })
                .or_insert_with(|| {
                    EntityActivity::new(
                        age_hours,
                        fact.access.access_count,
                        fact.temporal.recorded_at,
                    )
                });
        }
    }

    if entity_activity.is_empty() {
        let detail = format!(
            "Serendipity discovery: scanned {} facts, found no recently active entities",
            facts.len()
        );
        tracing::info!(%detail, "maintenance: serendipity discovery complete");
        return Ok(SerendipityDiscoveryReport {
            items_processed: u64::try_from(facts.len()).unwrap_or(u64::MAX),
            items_modified: 0,
            discovery_count: 0,
            detail: Some(detail),
            ..Default::default()
        });
    }

    let last_access_hours: HashMap<String, f64> = entity_activity
        .iter()
        .map(|(entity_id, activity)| (entity_id.clone(), activity.last_access_hours))
        .collect();

    let mut ranked_entities: Vec<(String, EntityActivity)> = entity_activity.into_iter().collect();
    ranked_entities.sort_by(|(left_id, left), (right_id, right)| {
        left.last_access_hours
            .partial_cmp(&right.last_access_hours)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| right.access_count.cmp(&left.access_count))
            .then_with(|| right.recorded_at.cmp(&left.recorded_at))
            .then_with(|| left_id.cmp(right_id))
    });

    let seed_limit = ranked_entities.len().min(5);
    let seed_entity_ids: Vec<String> = ranked_entities
        .into_iter()
        .take(seed_limit)
        .map(|(entity_id, _)| entity_id)
        .collect();

    let snapshot = store.build_serendipity_snapshot(&seed_entity_ids)?;
    let mut hasher = DefaultHasher::new();
    nous_id.hash(&mut hasher);
    seed_entity_ids.hash(&mut hasher);
    let walk_visits = random_walk(&snapshot, &seed_entity_ids, &config, hasher.finish());

    let home_communities: HashSet<i64> = seed_entity_ids
        .iter()
        .filter_map(|seed| snapshot.nodes.get(seed).map(|node| node.community))
        .filter(|community| *community >= 0)
        .collect();

    let surprise_scores = surprise_scores(
        &snapshot,
        &walk_visits,
        &home_communities,
        &last_access_hours,
    );
    let surprise_lookup: HashMap<String, f64> = surprise_scores.into_iter().collect();

    let mut distances: HashMap<String, u32> = HashMap::new();
    for seed in &seed_entity_ids {
        for (entity_id, distance) in bfs_distances(&snapshot, seed, config.max_path_depth) {
            distances
                .entry(entity_id)
                .and_modify(|current| {
                    if distance < *current {
                        *current = distance;
                    }
                })
                .or_insert(distance);
        }
    }

    let mut discoveries = score_discoveries(&snapshot, &seed_entity_ids, &distances, &config);
    for discovery in &mut discoveries {
        discovery.surprise = surprise_lookup
            .get(discovery.entity_id.as_str())
            .copied()
            .unwrap_or(0.0);
    }

    let selected_injection = select_injection(&discoveries, &fact_contents, &config);
    let path_summary = seed_entity_ids
        .first()
        .and_then(|seed| explore_from(&snapshot, seed, &config).into_iter().next());

    let discovery_count = u64::try_from(discoveries.len()).unwrap_or(u64::MAX);
    let items_processed = u64::try_from(facts.len()).unwrap_or(u64::MAX);
    let selected_preview = selected_injection.as_ref().map(|injection| {
        let mut preview = injection.content.chars().take(160).collect::<String>();
        if injection.content.chars().count() > 160 {
            preview.push_str("...");
        }
        preview
    });

    let top_discoveries = discoveries
        .iter()
        .take(3)
        .map(|discovery| {
            format!(
                "{} ({:.2}, dist {})",
                discovery.name,
                discovery.serendipity_score,
                discovery
                    .graph_distance
                    .map_or_else(|| "n/a".to_owned(), |distance| distance.to_string())
            )
        })
        .collect::<Vec<_>>();

    let mut detail_parts = vec![
        format!("facts scanned {}", facts.len()),
        format!("entity links {}", entity_links),
        format!("seeds {}", seed_entity_ids.join(", ")),
        format!("discoveries surfaced {}", discoveries.len()),
    ];
    if !top_discoveries.is_empty() {
        detail_parts.push(format!("top {}", top_discoveries.join(" | ")));
    }
    if let Some(injection) = &selected_injection {
        detail_parts.push(format!(
            "selected fact {} ({:.2})",
            injection.fact_id, injection.surprise_score
        ));
        detail_parts.push(format!("reason {}", injection.connection_reason));
        if let Some(preview) = selected_preview {
            detail_parts.push(format!("content {preview}"));
        }
    } else {
        detail_parts.push("selected fact none".to_owned());
    }
    if let Some(path) = path_summary {
        detail_parts.push(format!(
            "path {} hops across {} communities",
            path.length, path.communities_traversed
        ));
    }

    let detail = format!("Serendipity discovery: {}", detail_parts.join("; "));
    tracing::info!(%detail, "maintenance: serendipity discovery complete");

    Ok(SerendipityDiscoveryReport {
        items_processed,
        items_modified: discovery_count,
        discovery_count,
        selected_fact_id: selected_injection
            .as_ref()
            .map(|injection| injection.fact_id.clone()),
        selected_connection_reason: selected_injection
            .as_ref()
            .map(|injection| injection.connection_reason.clone()),
        selected_surprise_score: selected_injection
            .as_ref()
            .map(|injection| injection.surprise_score),
        detail: Some(detail),
    })
}

#[cfg(feature = "mneme-engine")]
fn fact_entity_ids(store: &KnowledgeStore, fact_id: &str) -> crate::error::Result<Vec<String>> {
    let mut params = BTreeMap::new();
    params.insert(
        "fid".to_owned(),
        crate::engine::DataValue::Str(fact_id.to_owned().into()),
    );
    let rows = store.run_query(
        "?[entity_id] := *fact_entities{fact_id: $fid, entity_id}",
        params,
    )?;
    let mut entity_ids = Vec::with_capacity(rows.row_count());
    for row_idx in 0..rows.row_count() {
        if let Some(entity_id) = rows.get_string(row_idx, "entity_id") {
            entity_ids.push(entity_id);
        }
    }
    Ok(entity_ids)
}

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test assertions")]
#[expect(
    clippy::indexing_slicing,
    reason = "test code with known-valid indices"
)]
mod tests;

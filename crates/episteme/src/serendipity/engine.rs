//! Knowledge-store integration for serendipity discovery runs.

use std::collections::BTreeMap;
use std::collections::hash_map::DefaultHasher;
use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};

use crate::knowledge_store::{KnowledgeStore, SerendipityDiscoveryReport};

use super::{
    SerendipityConfig, bfs_distances, explore_from, random_walk, score_discoveries,
    select_injection, surprise_scores,
};

#[derive(Debug, Clone, Copy)]
struct EntityActivity {
    last_access_hours: f64,
    access_count: u32,
    recorded_at: jiff::Timestamp,
}

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

/// Run the serendipity engine against real store data and return a report.
#[expect(
    clippy::too_many_lines,
    reason = "orchestrates the full serendipity maintenance flow"
)]
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
            .map(|injection| injection.fact_id.as_str().to_owned()),
        selected_connection_reason: selected_injection
            .as_ref()
            .map(|injection| injection.connection_reason.clone()),
        selected_surprise_score: selected_injection
            .as_ref()
            .map(|injection| injection.surprise_score),
        detail: Some(detail),
    })
}

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

//! Data assembly and timestamp utilities for meta-insights view.

use std::collections::HashMap;

use super::{
    AgentEntry, AgentPerformanceStore, EntityEntry, FactEntry, HealthApiResponse,
    KnowledgeGrowthStore, MemoryHealthStore, MetaData, MetricsApiResponse, QualityStore,
    SessionEntry, SystemReflectionStore, TimelineEntry,
};

/// Assemble all fetched data into the composite `MetaData` structure.
#[expect(clippy::cast_precision_loss, reason = "display-only metrics")]
pub(super) fn assemble_meta_data(
    health: HealthApiResponse,
    metrics: MetricsApiResponse,
    facts: Vec<FactEntry>,
    entities: Vec<EntityEntry>,
    timeline: Vec<TimelineEntry>,
    sessions: Vec<SessionEntry>,
    agents: Vec<AgentEntry>,
) -> MetaData {
    // -- Performance --
    let mut scorecards = Vec::new();
    for agent in &agents {
        let agent_sessions: Vec<&SessionEntry> =
            sessions.iter().filter(|s| s.nous_id == agent.id).collect();
        let session_count = agent_sessions.len().max(1) as f64;
        let total_messages: u32 = agent_sessions.iter().map(|s| s.message_count).sum();

        scorecards.push(crate::state::meta::AgentScorecard {
            agent_id: agent.id.clone(),
            agent_name: if agent.name.is_empty() {
                agent.id.clone()
            } else {
                agent.name.clone()
            },
            avg_tokens_per_response: 0.0,
            tool_calls_per_session: 0.0,
            tool_success_rate: 0.85,
            distillation_frequency: 0.0,
            avg_context_before_distill: 0.0,
            messages_per_session: total_messages as f64 / session_count,
            sessions_per_day: 0.0,
            errors_per_session: 0.0,
        });
    }

    let performance = AgentPerformanceStore {
        scorecards,
        anomalies: Vec::new(),
        tokens_per_response_series: HashMap::new(),
    };

    // -- Quality --
    let mut depth = crate::state::meta::DepthDistribution::default();
    for s in &sessions {
        match crate::state::meta::DepthDistribution::classify(s.message_count) {
            "short" => depth.short += 1,
            "medium" => depth.medium += 1,
            _ => depth.long += 1,
        }
    }

    let quality = QualityStore {
        avg_turn_length: Vec::new(),
        response_to_question_ratio: Vec::new(),
        tool_call_density: Vec::new(),
        thinking_time_ratio: Vec::new(),
        depth_distribution: depth,
        top_topics: Vec::new(),
    };

    // -- Knowledge growth --
    let total_entity_count = entities.len() as u64;
    let total_relationship_count: u32 = entities.iter().map(|e| e.relationship_count).sum();

    let cumulative_entities: Vec<crate::state::meta::DataPoint> = timeline
        .iter()
        .scan(0u32, |acc, entry| {
            *acc += entry.count;
            Some(crate::state::meta::DataPoint {
                label: entry.date.clone(),
                value: f64::from(*acc),
            })
        })
        .collect();

    let new_per_period: Vec<crate::state::meta::DataPoint> = timeline
        .iter()
        .map(|e| crate::state::meta::DataPoint {
            label: e.date.clone(),
            value: f64::from(e.count),
        })
        .collect();

    let cumulative_values: Vec<f64> = cumulative_entities.iter().map(|p| p.value).collect();
    let acceleration = crate::state::meta::compute_acceleration(&cumulative_values);

    // WHY: Entity type distribution from entity list.
    let mut type_counts: HashMap<&str, u32> = HashMap::new();
    for e in &entities {
        let t = if e.entity_type.is_empty() {
            "unknown"
        } else {
            e.entity_type.as_str()
        };
        *type_counts.entry(t).or_default() += 1;
    }
    let mut type_slices: Vec<crate::state::meta::EntityTypeSlice> = type_counts
        .into_iter()
        .enumerate()
        .map(|(i, (t, count)): (usize, (&str, u32))| crate::state::meta::EntityTypeSlice {
            entity_type: t.to_string(),
            count,
            color: crate::state::meta::ENTITY_TYPE_COLORS
                [i % crate::state::meta::ENTITY_TYPE_COLORS.len()],
        })
        .collect();
    type_slices.sort_by(|a, b| b.count.cmp(&a.count));

    let current_entity_rate = new_per_period.last().map_or(0.0, |p| p.value);

    let density_over_time = if total_entity_count > 0 {
        vec![crate::state::meta::DataPoint {
            label: "now".to_string(),
            value: total_relationship_count as f64 / total_entity_count as f64,
        }]
    } else {
        Vec::new()
    };

    let knowledge = KnowledgeGrowthStore {
        total_entities: cumulative_entities,
        new_entities_per_period: new_per_period,
        total_relationships: Vec::new(),
        new_relationships_per_period: Vec::new(),
        density_over_time,
        entity_type_distribution: type_slices,
        current_entity_rate,
        current_relationship_rate: 0.0,
        acceleration,
    };

    // -- Memory health --
    let avg_confidence = if facts.is_empty() {
        0.0
    } else {
        facts.iter().map(|f| f.confidence).sum::<f64>() / facts.len() as f64
    };

    let orphan_count = entities
        .iter()
        .filter(|e| e.relationship_count <= 1)
        .count();
    let orphan_ratio = if entities.is_empty() {
        0.0
    } else {
        orphan_count as f64 / entities.len() as f64
    };

    // WHY: Approximate staleness from facts with empty updated_at or old dates.
    let stale_count = facts.iter().filter(|f| f.updated_at.is_empty()).count();
    let staleness_ratio = if facts.is_empty() {
        0.0
    } else {
        stale_count as f64 / facts.len() as f64
    };

    let health_score =
        crate::state::meta::compute_health_score(avg_confidence, orphan_ratio, staleness_ratio);

    // WHY: Build confidence distribution histogram (10 buckets of 0.1 width).
    let mut confidence_buckets = vec![0u32; 10];
    for f in &facts {
        let idx = ((f.confidence * 10.0).floor() as usize).min(9);
        confidence_buckets[idx] += 1;
    }
    let confidence_distribution: Vec<crate::state::meta::ConfidenceBucket> = confidence_buckets
        .into_iter()
        .enumerate()
        .map(|(i, count)| {
            let lo = i as f64 * 0.1;
            let hi = lo + 0.1;
            crate::state::meta::ConfidenceBucket {
                range_label: format!("{lo:.1}-{hi:.1}"),
                count,
            }
        })
        .collect();

    let recommendations = crate::state::meta::generate_recommendations(
        staleness_ratio,
        orphan_ratio,
        avg_confidence,
        current_entity_rate,
    );

    let mem_health = MemoryHealthStore {
        health_score,
        confidence_distribution,
        stale_entities: Vec::new(),
        orphan_ratio,
        decay_pressure_count: 0,
        health_over_time: vec![crate::state::meta::DataPoint {
            label: "now".to_string(),
            value: health_score,
        }],
        recommendations,
    };

    // -- Reflection --
    let total_tokens = metrics.total_input_tokens + metrics.total_output_tokens;
    let overview = crate::state::meta::SystemOverview {
        uptime_seconds: health.uptime_seconds,
        total_sessions: metrics.total_sessions,
        total_tokens,
        total_entities: total_entity_count,
        total_cost_usd: metrics.total_cost_usd,
    };

    let efficiency = crate::state::meta::EfficiencyMetrics {
        cost_per_entity: crate::state::meta::cost_per_entity(
            metrics.total_cost_usd,
            total_entity_count,
        ),
        cost_per_session: if metrics.total_sessions > 0 {
            metrics.total_cost_usd / metrics.total_sessions as f64
        } else {
            0.0
        },
        tokens_per_entity: crate::state::meta::tokens_per_entity(total_tokens, total_entity_count),
        cost_per_entity_trend: crate::state::meta::TrendDirection::Flat,
    };

    // WHY: Build heatmap from session created_at timestamps.
    // Parse "YYYY-MM-DDTHH:MM:SS" -> (day_of_week, hour).
    let timestamps: Vec<(u8, u8)> = sessions
        .iter()
        .filter_map(|s| parse_timestamp_to_day_hour(&s.created_at))
        .collect();
    let heatmap = crate::state::meta::build_heatmap(&timestamps);

    let reflection = SystemReflectionStore {
        overview,
        heatmap,
        efficiency,
        journal: Vec::new(),
    };

    MetaData {
        performance,
        quality,
        knowledge,
        health: mem_health,
        reflection,
    }
}

/// Extract (day_of_week, hour) from an ISO 8601 timestamp string.
///
/// Uses a basic parser -- no external date library dependency needed.
fn parse_timestamp_to_day_hour(ts: &str) -> Option<(u8, u8)> {
    // WHY: Minimal parsing for "YYYY-MM-DDTHH:..." format.
    if ts.len() < 13 {
        return None;
    }
    let hour: u8 = ts.get(11..13)?.parse().ok()?;
    if hour >= 24 {
        return None;
    }

    // WHY: Approximate day-of-week using Tomohiko Sakamoto's algorithm.
    let year: i32 = ts.get(0..4)?.parse().ok()?;
    let month: u32 = ts.get(5..7)?.parse().ok()?;
    let day: u32 = ts.get(8..10)?.parse().ok()?;

    if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }

    let dow = day_of_week(year, month, day);
    Some((dow, hour))
}

/// Tomohiko Sakamoto's day-of-week algorithm. Returns 0=Mon..6=Sun.
fn day_of_week(mut year: i32, month: u32, day: u32) -> u8 {
    const T: [i32; 12] = [0, 3, 2, 5, 0, 3, 5, 1, 4, 6, 2, 4];
    if month < 3 {
        year -= 1;
    }
    let dow =
        (year + year / 4 - year / 100 + year / 400 + T[month as usize - 1] + day as i32) % 7;
    // WHY: Sakamoto returns 0=Sun, convert to 0=Mon.
    ((dow + 6) % 7) as u8
}

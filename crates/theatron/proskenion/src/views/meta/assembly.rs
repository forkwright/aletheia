//! Data assembly and timestamp utilities for meta-insights view.

use std::collections::HashMap;

use super::{
    AgentEntry, AgentPerformanceApiResponse, AgentPerformanceStore, CostMetricsApiResponse,
    EntityEntry, FactEntry, HealthApiResponse, JournalEventEntry, KnowledgeGrowthStore,
    META_SERIES_COLORS, MemoryHealthStore, MetaData, QualityMetricsApiResponse, QualityStore,
    SessionEntry, SystemReflectionStore, TimeSeriesPointEntry, TimelineEntry,
    TokenMetricsApiResponse,
};
use crate::state::meta::MetricSource;

/// Assemble all fetched data into the composite `MetaData` structure.
#[expect(
    clippy::too_many_arguments,
    reason = "data assembly requires all fetched API sources"
)]
pub(super) fn assemble_meta_data(
    health: HealthApiResponse,
    tokens: TokenMetricsApiResponse,
    costs: CostMetricsApiResponse,
    facts: Vec<FactEntry>,
    entities: Vec<EntityEntry>,
    timeline: Vec<TimelineEntry>,
    sessions: Vec<SessionEntry>,
    agents: Vec<AgentEntry>,
    perf: AgentPerformanceApiResponse,
    quality: QualityMetricsApiResponse,
    journal: Vec<JournalEventEntry>,
) -> MetaData {
    // -- Performance --
    let mut scorecards = Vec::new();
    let mut anomalies = Vec::new();
    let mut tokens_per_response_series: HashMap<String, Vec<crate::state::meta::DataPoint>> =
        HashMap::new();

    if perf.agents.is_empty() {
        // Fallback: compute from sessions/agents when endpoint returns empty.
        for agent in &agents {
            let agent_sessions: Vec<&SessionEntry> = sessions
                .iter()
                .filter(|s| s.nous_id.as_ref() == agent.id.as_str())
                .collect();
            let session_count = count_to_f64(agent_sessions.len().max(1));
            let total_messages: f64 = agent_sessions
                .iter()
                .map(|s| count_to_f64(usize::try_from(s.message_count).unwrap_or_default()))
                .sum();

            scorecards.push(crate::state::meta::AgentScorecard {
                agent_id: agent.id.clone(),
                agent_name: if agent.name.is_empty() {
                    agent.id.clone()
                } else {
                    agent.name.clone()
                },
                avg_tokens_per_response: 0.0,
                tool_calls_per_session: 0.0,
                tool_success_rate: 0.0,
                distillation_frequency: 0.0,
                avg_context_before_distill: 0.0,
                messages_per_session: total_messages / session_count,
                sessions_per_day: compute_sessions_per_day(&agent_sessions),
                errors_per_session: 0.0,
            });
        }
    } else {
        for entry in &perf.agents {
            scorecards.push(crate::state::meta::AgentScorecard {
                agent_id: entry.agent_id.clone(),
                agent_name: if entry.agent_name.is_empty() {
                    entry.agent_id.clone()
                } else {
                    entry.agent_name.clone()
                },
                avg_tokens_per_response: entry.avg_tokens_per_response,
                tool_calls_per_session: entry.tool_calls_per_session,
                tool_success_rate: entry.tool_success_rate,
                distillation_frequency: entry.distillation_frequency,
                avg_context_before_distill: entry.avg_context_before_distill,
                messages_per_session: entry.messages_per_session,
                sessions_per_day: entry.sessions_per_day,
                errors_per_session: entry.errors_per_session,
            });

            if !entry.tokens_per_response_series.is_empty() {
                let series: Vec<crate::state::meta::DataPoint> = entry
                    .tokens_per_response_series
                    .iter()
                    .map(|p| crate::state::meta::DataPoint {
                        label: p.date.clone(),
                        value: p.value,
                    })
                    .collect();
                tokens_per_response_series.insert(entry.agent_id.clone(), series);
            }
        }

        for entry in &perf.anomalies {
            let direction = match entry.direction.as_str() {
                "up" => crate::state::meta::TrendDirection::Up,
                "down" => crate::state::meta::TrendDirection::Down,
                _ => crate::state::meta::TrendDirection::Flat,
            };
            anomalies.push(crate::state::meta::Anomaly {
                agent_name: entry.agent_name.clone(),
                metric_name: entry.metric_name.clone(),
                current_value: entry.current_value,
                baseline_mean: entry.baseline_mean,
                deviation_pct: entry.deviation_pct,
                direction,
            });
        }
    }

    let performance = AgentPerformanceStore {
        scorecards,
        anomalies,
        tokens_per_response_series,
        endpoint_available: true,
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

    let avg_turn_length = convert_series(&quality.series.avg_turn_length);
    let response_to_question_ratio = convert_series(&quality.series.response_to_question_ratio);
    let tool_call_density = convert_series(&quality.series.tool_call_density);
    let thinking_time_ratio = convert_series(&quality.series.thinking_time_ratio);

    let quality = QualityStore {
        avg_turn_length,
        response_to_question_ratio,
        tool_call_density,
        thinking_time_ratio,
        depth_distribution: depth,
        top_topics: Vec::new(),
        charts_endpoint_available: true,
    };

    // -- Knowledge growth --
    let total_entity_count = usize_to_u64(entities.len());
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
        .map(
            |(i, (t, count)): (usize, (&str, u32))| crate::state::meta::EntityTypeSlice {
                entity_type: t.to_string(),
                count,
                color: META_SERIES_COLORS[i % META_SERIES_COLORS.len()],
            },
        )
        .collect();
    type_slices.sort_by_key(|b| std::cmp::Reverse(b.count));

    let current_entity_rate = new_per_period.last().map_or(0.0, |p| p.value);

    let density_over_time = if total_entity_count > 0 {
        vec![crate::state::meta::DataPoint {
            label: "now".to_string(),
            value: f64::from(total_relationship_count) / count_to_f64(entities.len()),
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
        facts.iter().map(|f| f.confidence).sum::<f64>() / count_to_f64(facts.len())
    };

    // WHY: an orphan has zero relationships, not one; a single relationship is
    // still connected to the graph.
    let orphan_count = entities
        .iter()
        .filter(|e| e.relationship_count == 0)
        .count();
    let orphan_ratio = if entities.is_empty() {
        0.0
    } else {
        count_to_f64(orphan_count) / count_to_f64(entities.len())
    };

    // WHY: staleness is computed from fact recorded_at age, not an empty string.
    let active_facts: Vec<&FactEntry> = facts.iter().filter(|f| !f.is_forgotten).collect();
    let stale_count = active_facts.iter().filter(|f| fact_is_stale(f)).count();
    let staleness_ratio = if active_facts.is_empty() {
        0.0
    } else {
        count_to_f64(stale_count) / count_to_f64(active_facts.len())
    };

    let health_score =
        crate::state::meta::compute_health_score(avg_confidence, orphan_ratio, staleness_ratio);

    // WHY: Build confidence distribution histogram (10 buckets of 0.1 width).
    let mut confidence_buckets = vec![0u32; 10];
    for f in &facts {
        let idx = confidence_bucket_index(f.confidence);
        if let Some(bucket) = confidence_buckets.get_mut(idx) {
            *bucket += 1;
        }
    }
    let confidence_distribution: Vec<crate::state::meta::ConfidenceBucket> = confidence_buckets
        .into_iter()
        .enumerate()
        .map(|(i, count)| {
            let lo = count_to_f64(i) * 0.1;
            let hi = lo + 0.1;
            crate::state::meta::ConfidenceBucket {
                range_label: format!("{lo:.1}-{hi:.1}"),
                count,
            }
        })
        .collect();

    // WHY: decay pressure is computed client-side from backend stability and
    // last-access timestamps. It is not a server-reported metric, so the UI
    // labels it "computed" rather than presenting a hardcoded zero.
    let decay_pressure_count: u32 = active_facts
        .iter()
        .filter(|f| {
            let stability = f.stability_hours;
            if stability <= 0.0 {
                return false;
            }
            let last_access_age = if f.last_accessed_at.is_empty() {
                crate::state::memory::age_in_days(&f.recorded_at).map(|d| d as f64 * 24.0)
            } else {
                crate::state::memory::age_in_days(&f.last_accessed_at).map(|d| d as f64 * 24.0)
            }
            .unwrap_or(0.0);
            last_access_age >= stability
        })
        .count()
        .try_into()
        .unwrap_or(u32::MAX);
    let decay_pressure_source = if active_facts.iter().any(|f| f.stability_hours > 0.0) {
        MetricSource::Computed
    } else {
        MetricSource::Unavailable
    };

    let stale_entities: Vec<crate::state::meta::StaleEntity> = entities
        .iter()
        .filter(|e| {
            crate::state::memory::age_in_days(&e.updated_at).is_some_and(|d| d > 30)
                && !e.name.is_empty()
        })
        .filter_map(|e| {
            let days = crate::state::memory::age_in_days(&e.updated_at)?;
            Some(crate::state::meta::StaleEntity {
                name: e.name.clone(),
                last_updated: e.updated_at.clone(),
                days_stale: days.min(u32::MAX as u64) as u32,
            })
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
        stale_entities,
        orphan_ratio,
        orphan_ratio_source: if entities.is_empty() {
            MetricSource::Unavailable
        } else {
            MetricSource::Computed
        },
        decay_pressure_count,
        decay_pressure_source,
        health_over_time: vec![crate::state::meta::DataPoint {
            label: "now".to_string(),
            value: health_score,
        }],
        recommendations,
    };

    // ── Reflection ──
    // WHY: aggregates are DERIVED from the time-series + per-agent data
    // (deterministic metrics) rather than read from flat total fields.
    let total_input_tokens: u64 = tokens.series.iter().map(|b| b.input_tokens).sum();
    let total_output_tokens: u64 = tokens.series.iter().map(|b| b.output_tokens).sum();
    let total_tokens = total_input_tokens + total_output_tokens;
    let total_sessions: u64 = tokens.agents.iter().map(|a| a.session_count).sum();
    let total_cost_usd: f64 = costs.series.iter().map(|b| b.cost_usd).sum();
    let overview = crate::state::meta::SystemOverview {
        uptime_seconds: health.uptime_seconds,
        total_sessions,
        total_tokens,
        total_entities: total_entity_count,
        total_cost_usd,
    };

    let efficiency = crate::state::meta::EfficiencyMetrics {
        cost_per_entity: crate::state::meta::cost_per_entity(total_cost_usd, total_entity_count),
        cost_per_session: if total_sessions > 0 {
            total_cost_usd / u64_to_f64(total_sessions)
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
        .filter_map(|s| parse_timestamp_to_day_hour(session_activity_timestamp(s)))
        .collect();
    let heatmap = crate::state::meta::build_heatmap(&timestamps);

    let journal_events: Vec<crate::state::meta::JournalEvent> = journal
        .into_iter()
        .map(|e| crate::state::meta::JournalEvent {
            timestamp: e.timestamp,
            event_type: parse_journal_event_type(&e.event_type),
            message: e.message,
        })
        .collect();

    let reflection = SystemReflectionStore {
        overview,
        heatmap,
        efficiency,
        journal: journal_events,
        journal_endpoint_available: true,
    };

    MetaData {
        performance,
        quality,
        knowledge,
        health: mem_health,
        reflection,
    }
}

fn fact_is_stale(fact: &FactEntry) -> bool {
    crate::state::memory::age_in_days(&fact.recorded_at).is_some_and(|d| d > 30)
        || timestamp_is_past(&fact.valid_to)
}

fn timestamp_is_past(timestamp: &str) -> bool {
    let Some(ts) = crate::state::sessions::parse_iso_to_unix(timestamp) else {
        return false;
    };
    if ts == 0 {
        return false;
    }

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    ts <= now
}

fn session_activity_timestamp(session: &SessionEntry) -> &str {
    if session.created_at.is_empty() {
        &session.updated_at
    } else {
        &session.created_at
    }
}

/// Convert API time-series entries to internal `DataPoint` values.
fn convert_series(entries: &[TimeSeriesPointEntry]) -> Vec<crate::state::meta::DataPoint> {
    entries
        .iter()
        .map(|e| crate::state::meta::DataPoint {
            label: e.date.clone(),
            value: e.value,
        })
        .collect()
}

/// Parse a journal event type string into the internal enum.
fn parse_journal_event_type(s: &str) -> crate::state::meta::JournalEventType {
    match s {
        "error" => crate::state::meta::JournalEventType::Error,
        "distillation" => crate::state::meta::JournalEventType::Distillation,
        "config" => crate::state::meta::JournalEventType::ConfigChange,
        "memory" => crate::state::meta::JournalEventType::MemoryMerge,
        _ => crate::state::meta::JournalEventType::Error,
    }
}

/// Compute average sessions per active day from session timestamps.
fn compute_sessions_per_day(sessions: &[&SessionEntry]) -> f64 {
    let count = count_to_f64(sessions.len());
    if count < 1.0 {
        return 0.0;
    }
    let mut unique_dates = std::collections::HashSet::new();
    for s in sessions {
        if let Some(date) = s.created_at.get(..10) {
            unique_dates.insert(date.to_string());
        }
    }
    if unique_dates.is_empty() {
        return 0.0;
    }
    count / count_to_f64(unique_dates.len())
}

fn confidence_bucket_index(confidence: f64) -> usize {
    if !confidence.is_finite() || confidence <= 0.0 {
        return 0;
    }
    if confidence >= 1.0 {
        return 9;
    }

    for bucket in 0..10 {
        let upper_bound = count_to_f64(bucket + 1) * 0.1;
        if confidence < upper_bound {
            return bucket;
        }
    }

    9
}

fn count_to_f64(count: usize) -> f64 {
    match count.to_string().parse::<f64>() {
        Ok(value) => value,
        Err(error) => {
            tracing::warn!(?error, count, "failed to convert count to display metric");
            0.0
        }
    }
}

fn u64_to_f64(count: u64) -> f64 {
    match count.to_string().parse::<f64>() {
        Ok(value) => value,
        Err(error) => {
            tracing::warn!(?error, count, "failed to convert count to display metric");
            0.0
        }
    }
}

fn usize_to_u64(count: usize) -> u64 {
    match u64::try_from(count) {
        Ok(value) => value,
        Err(error) => {
            tracing::warn!(?error, count, "failed to convert count to overview metric");
            u64::MAX
        }
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
    let dow = {
        #[expect(
            clippy::as_conversions,
            reason = "month/day to index/i32 for day-of-week algorithm"
        )]
        let dow_raw =
            (year + year / 4 - year / 100 + year / 400 + T[month as usize - 1] + day as i32) % 7;
        dow_raw
    };
    // WHY: Sakamoto returns 0=Sun, convert to 0=Mon.
    {
        #[expect(clippy::as_conversions, reason = "day-of-week 0–6 fits u8")]
        let result = ((dow + 6) % 7) as u8;
        result
    }
}

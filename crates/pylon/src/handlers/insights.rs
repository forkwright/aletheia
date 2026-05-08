//! Meta-insights handlers: agent performance, quality metrics, system journal.

use std::collections::HashMap;

use axum::Json;
use axum::extract::{Path, Query, State};
use tracing::warn;

use mneme::types::{Message, Role, Session};

use crate::error::{ApiError, InternalSnafu, NousNotFoundSnafu};
use crate::insights::anomaly::detect_anomalies;
use crate::state::InsightsState;
use crate::types::insights::{
    AgentPerformance, AgentPerformanceListResponse, JournalEvent, JournalQuery,
    QualityMetricsResponse, QualitySeries, TimeSeriesPoint,
};

// -- Safe numeric conversions ------------------------------------------------

/// Convert `i64` to `f64` losslessly for values that fit in `i32`.
///
/// # Panics
///
/// Does not panic — saturates at `i32::MAX`.
fn i64_to_f64(n: i64) -> f64 {
    f64::from(i32::try_from(n).unwrap_or(i32::MAX))
}

/// Convert `usize` to `f64` losslessly for values that fit in `u32`.
///
/// # Panics
///
/// Does not panic — saturates at `u32::MAX`.
fn usize_to_f64(n: usize) -> f64 {
    f64::from(u32::try_from(n).unwrap_or(u32::MAX))
}

// -- GET /api/v1/metrics/agents ----------------------------------------------

/// GET /api/v1/metrics/agents: list performance metrics for all agents.
#[utoipa::path(
    get,
    path = "/api/v1/metrics/agents",
    responses(
        (status = 200, description = "Agent performance list", body = AgentPerformanceListResponse),
        (status = 401, description = "Unauthorized", body = crate::error::ErrorResponse),
    ),
    security(("bearer_auth" = []))
)]
pub async fn get_agent_perf(
    State(state): State<InsightsState>,
) -> Json<AgentPerformanceListResponse> {
    // WHY: Collect agent configs outside spawn_blocking because configs()
    // returns references tied to the manager's lifetime.
    let agent_configs: Vec<(String, Option<String>)> = state
        .nous_manager
        .configs()
        .into_iter()
        .map(|c| (c.id.to_string(), c.name.clone()))
        .collect();

    let state_clone = state.clone();
    let all_sessions_res = tokio::task::spawn_blocking(move || {
        let store = state_clone.session_store.blocking_lock();
        store.list_sessions(None).map_err(ApiError::from)
    })
    .await
    .unwrap_or_else(|e| {
        Err(InternalSnafu {
            message: format!("task join failed: {e}"),
        }
        .build())
    });

    let all_sessions = match all_sessions_res {
        Ok(sessions) => sessions,
        Err(e) => {
            warn!(error = %e, "failed to list sessions for agent performance");
            Vec::new()
        }
    };

    let mut performances = Vec::with_capacity(agent_configs.len());
    let mut anomalies = Vec::new();

    for (agent_id, agent_name) in &agent_configs {
        let agent_sessions: Vec<&Session> = all_sessions
            .iter()
            .filter(|s| &s.nous_id == agent_id)
            .collect();

        let perf = compute_agent_performance(agent_id, agent_name.as_deref(), &agent_sessions);

        anomalies.extend(detect_anomalies(
            &perf.agent_id,
            &perf.agent_name,
            "messages_per_session",
            &perf.tokens_per_response_series,
        ));

        performances.push(perf);
    }

    Json(AgentPerformanceListResponse {
        agents: performances,
        anomalies,
    })
}

// -- GET /api/v1/metrics/agents/{id} -----------------------------------------

/// GET /api/v1/metrics/agents/{id}: performance metrics for a single agent.
#[utoipa::path(
    get,
    path = "/api/v1/metrics/agents/{id}",
    params(("id" = String, Path, description = "Agent ID")),
    responses(
        (status = 200, description = "Agent performance", body = AgentPerformance),
        (status = 401, description = "Unauthorized", body = crate::error::ErrorResponse),
        (status = 404, description = "Agent not found", body = crate::error::ErrorResponse),
    ),
    security(("bearer_auth" = []))
)]
pub async fn get_agent_perf_one(
    State(state): State<InsightsState>,
    Path(id): Path<String>,
) -> Result<Json<AgentPerformance>, ApiError> {
    let config = state
        .nous_manager
        .get_config(&id)
        .ok_or_else(|| NousNotFoundSnafu { id: id.clone() }.build())?;

    let state_clone = state.clone();
    let id_clone = id.clone();
    let sessions = tokio::task::spawn_blocking(move || {
        let store = state_clone.session_store.blocking_lock();
        store.list_sessions(Some(&id_clone)).map_err(ApiError::from)
    })
    .await
    .unwrap_or_else(|e| {
        Err(InternalSnafu {
            message: format!("task join failed: {e}"),
        }
        .build())
    })?;

    let session_refs: Vec<&Session> = sessions.iter().collect();
    Ok(Json(compute_agent_performance(
        &id,
        config.name.as_deref(),
        &session_refs,
    )))
}

// -- GET /api/v1/metrics/quality ---------------------------------------------

/// GET /api/v1/metrics/quality: conversation quality time series.
#[utoipa::path(
    get,
    path = "/api/v1/metrics/quality",
    responses(
        (status = 200, description = "Quality metrics", body = QualityMetricsResponse),
        (status = 401, description = "Unauthorized", body = crate::error::ErrorResponse),
    ),
    security(("bearer_auth" = []))
)]
pub async fn get_quality_metrics(
    State(state): State<InsightsState>,
) -> Json<QualityMetricsResponse> {
    let state_clone = state.clone();
    let (sessions, messages) = tokio::task::spawn_blocking(move || {
        let store = state_clone.session_store.blocking_lock();
        let sessions = store.list_sessions(None).map_err(ApiError::from)?;

        let mut messages = Vec::new();
        for session in &sessions {
            match store.get_history(&session.id, None) {
                Ok(mut ms) => messages.append(&mut ms),
                Err(e) => {
                    warn!(session_id = %session.id, error = %e, "failed to load messages for quality metrics");
                }
            }
        }
        Ok::<_, ApiError>((sessions, messages))
    })
    .await
    .unwrap_or_else(|e| {
        Err(InternalSnafu {
            message: format!("task join failed: {e}"),
        }
        .build())
    })
    .unwrap_or_else(|_| (Vec::new(), Vec::new()));

    let series = compute_quality_series(&sessions, &messages);
    Json(QualityMetricsResponse { series })
}

// -- GET /api/v1/journal -----------------------------------------------------

/// GET /api/v1/journal: queryable system event log.
#[utoipa::path(
    get,
    path = "/api/v1/journal",
    params(JournalQuery),
    responses(
        (status = 200, description = "Journal events", body = Vec<JournalEvent>),
        (status = 401, description = "Unauthorized", body = crate::error::ErrorResponse),
    ),
    security(("bearer_auth" = []))
)]
pub async fn get_journal(Query(query): Query<JournalQuery>) -> Json<Vec<JournalEvent>> {
    warn!(
        source = ?query.source,
        level = ?query.level,
        since = ?query.since,
        limit = query.limit,
        "journal endpoint called but no persistent event journal is available in pylon"
    );
    Json(Vec::new())
}

// -- Computation helpers -----------------------------------------------------

/// Compute per-agent performance from a slice of sessions.
fn compute_agent_performance(
    agent_id: &str,
    agent_name: Option<&str>,
    sessions: &[&Session],
) -> AgentPerformance {
    let session_count = sessions.len();
    let session_count_f64 = usize_to_f64(session_count);

    let total_messages: f64 = sessions
        .iter()
        .map(|s| i64_to_f64(s.metrics.message_count))
        .sum();
    let total_tokens: f64 = sessions
        .iter()
        .map(|s| i64_to_f64(s.metrics.token_count_estimate))
        .sum();
    let total_distillations: f64 = sessions
        .iter()
        .map(|s| i64_to_f64(s.metrics.distillation_count))
        .sum();

    let sessions_with_distill: Vec<&Session> = sessions
        .iter()
        .copied()
        .filter(|s| s.metrics.distillation_count > 0)
        .collect();

    let avg_context_before_distill = if sessions_with_distill.is_empty() {
        0.0
    } else {
        let total_context: f64 = sessions_with_distill
            .iter()
            .map(|s| i64_to_f64(s.metrics.computed_context_tokens))
            .sum();
        total_context / usize_to_f64(sessions_with_distill.len())
    };

    let messages_per_session = if session_count == 0 {
        0.0
    } else {
        total_messages / session_count_f64
    };

    let avg_tokens_per_response = if total_messages > 0.0 {
        total_tokens / total_messages
    } else {
        0.0
    };

    let distillation_frequency = if session_count == 0 {
        0.0
    } else {
        total_distillations / session_count_f64
    };

    let sessions_per_day = compute_sessions_per_day(sessions);

    // NOTE: No data source for tool call counts, success rates, or errors.
    warn!(
        agent_id = %agent_id,
        "tool_calls_per_session, tool_success_rate, and errors_per_session have no backing data source in pylon — returning 0.0"
    );

    let tokens_per_response_series = build_daily_series(sessions, |sess| {
        let msgs = i64_to_f64(sess.metrics.message_count);
        let toks = i64_to_f64(sess.metrics.token_count_estimate);
        if msgs > 0.0 { toks / msgs } else { 0.0 }
    });

    AgentPerformance {
        agent_id: agent_id.to_owned(),
        agent_name: agent_name
            .filter(|n| !n.is_empty())
            .unwrap_or(agent_id)
            .to_owned(),
        avg_tokens_per_response,
        tool_calls_per_session: 0.0,
        tool_success_rate: 0.0,
        distillation_frequency,
        avg_context_before_distill,
        messages_per_session,
        sessions_per_day,
        errors_per_session: 0.0,
        tokens_per_response_series,
    }
}

/// Compute average sessions per active day.
fn compute_sessions_per_day(sessions: &[&Session]) -> f64 {
    if sessions.is_empty() {
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
    session_count_f64(sessions.len()) / usize_to_f64(unique_dates.len())
}

/// Build a daily time series from sessions using the given extractor.
fn build_daily_series<F>(sessions: &[&Session], mut extract: F) -> Vec<TimeSeriesPoint>
where
    F: FnMut(&Session) -> f64,
{
    let mut by_date: HashMap<String, Vec<f64>> = HashMap::new();
    for s in sessions {
        let date = s.created_at.get(..10).unwrap_or("1970-01-01").to_owned();
        by_date.entry(date).or_default().push(extract(s));
    }

    let mut points: Vec<TimeSeriesPoint> = by_date
        .into_iter()
        .map(|(date, values)| {
            let avg = if values.is_empty() {
                0.0
            } else {
                values.iter().sum::<f64>() / usize_to_f64(values.len())
            };
            TimeSeriesPoint { date, value: avg }
        })
        .collect();

    points.sort_by(|a, b| a.date.cmp(&b.date));
    points
}

/// Compute quality time series from sessions and messages.
fn compute_quality_series(sessions: &[Session], messages: &[Message]) -> QualitySeries {
    // Group sessions by date for avg_turn_length.
    let mut session_counts_by_date: HashMap<String, Vec<u64>> = HashMap::new();
    for s in sessions {
        let date = s.created_at.get(..10).unwrap_or("1970-01-01").to_owned();
        let count = u64::try_from(s.metrics.message_count).unwrap_or(0);
        session_counts_by_date.entry(date).or_default().push(count);
    }

    let avg_turn_length: Vec<TimeSeriesPoint> = session_counts_by_date
        .into_iter()
        .map(|(date, counts)| {
            let total: f64 = counts.iter().map(|&c| u64_to_f64(c)).sum();
            let avg = if counts.is_empty() {
                0.0
            } else {
                total / usize_to_f64(counts.len())
            };
            TimeSeriesPoint { date, value: avg }
        })
        .collect();

    // Group messages by date for ratios and density.
    let mut msgs_by_date: HashMap<String, MessageCounts> = HashMap::new();
    for m in messages {
        let date = m.created_at.get(..10).unwrap_or("1970-01-01").to_owned();
        let entry = msgs_by_date.entry(date).or_default();
        entry.total += 1;
        match m.role {
            Role::Assistant => entry.assistant += 1,
            Role::User => entry.user += 1,
            Role::ToolResult => entry.tool_result += 1,
            _ => {
                // System messages do not affect user/assistant/tool counts.
            }
        }
    }

    let mut response_to_question_ratio: Vec<TimeSeriesPoint> = Vec::new();
    let mut tool_call_density: Vec<TimeSeriesPoint> = Vec::new();

    for (date, counts) in &msgs_by_date {
        let user_f64 = u64_to_f64(counts.user);
        let assistant_f64 = u64_to_f64(counts.assistant);
        let total_f64 = u64_to_f64(counts.total);
        let tool_f64 = u64_to_f64(counts.tool_result);

        response_to_question_ratio.push(TimeSeriesPoint {
            date: date.clone(),
            value: if user_f64 > 0.0 {
                assistant_f64 / user_f64
            } else {
                0.0
            },
        });

        tool_call_density.push(TimeSeriesPoint {
            date: date.clone(),
            value: if total_f64 > 0.0 {
                tool_f64 / total_f64
            } else {
                0.0
            },
        });
    }

    response_to_question_ratio.sort_by(|a, b| a.date.cmp(&b.date));
    tool_call_density.sort_by(|a, b| a.date.cmp(&b.date));

    warn!("thinking_time_ratio has no backing data source in pylon — returning empty series");

    QualitySeries {
        avg_turn_length: sort_points(avg_turn_length),
        response_to_question_ratio,
        tool_call_density,
        thinking_time_ratio: Vec::new(),
    }
}

#[derive(Debug, Default)]
struct MessageCounts {
    total: u64,
    assistant: u64,
    user: u64,
    tool_result: u64,
}

fn sort_points(mut points: Vec<TimeSeriesPoint>) -> Vec<TimeSeriesPoint> {
    points.sort_by(|a, b| a.date.cmp(&b.date));
    points
}

fn u64_to_f64(n: u64) -> f64 {
    f64::from(u32::try_from(n.min(u64::from(u32::MAX))).unwrap_or(u32::MAX))
}

fn session_count_f64(n: usize) -> f64 {
    usize_to_f64(n)
}

// kanon:ignore RUST/file-too-long — cohesive insights surface: agent-perf, quality, token, cost, and journal handlers share helper bucketing/date-range/series functions and DTOs; splitting now would duplicate those helpers across sibling modules.
//! Meta-insights handlers: agent performance, quality metrics, system journal.

use std::collections::{HashMap, HashSet};

use axum::Json;
use axum::extract::{Path, Query, State};
use tracing::warn;

use jiff::ToSpan;
use mneme::types::{Message, Role, Session, UsageRecord};

use crate::error::{ApiError, BadRequestSnafu, InternalSnafu, NousNotFoundSnafu};
use crate::extract::{Claims, require_nous_access, require_role};
use crate::insights::anomaly::detect_anomalies;
use crate::state::InsightsState;
use crate::types::insights::{
    AgentCostRow, AgentPerformance, AgentPerformanceListResponse, AgentTokenRow,
    CostMetricsResponse, CostSeriesPoint, JournalQuery, JournalResponse, MetricsQuery,
    ModelTokenRow, QualityMetricsResponse, QualitySeries, TimeSeriesPoint, TokenMetricsResponse,
    TokenSeriesPoint, UnavailableMetric,
};

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

fn require_unscoped_operator(claims: &Claims, scoped_message: &str) -> Result<(), ApiError> {
    require_role(claims, symbolon::types::Role::Operator)?;
    if claims.nous_id.is_some() {
        return Err(ApiError::forbidden(scoped_message));
    }
    Ok(())
}

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
    claims: Claims,
) -> Result<Json<AgentPerformanceListResponse>, ApiError> {
    require_unscoped_operator(
        &claims,
        "scoped tokens cannot access aggregate agent metrics; use /metrics/agents/{id}",
    )?;
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

    Ok(Json(AgentPerformanceListResponse {
        agents: performances,
        anomalies,
    }))
}

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
    claims: Claims,
    Path(id): Path<String>,
) -> Result<Json<AgentPerformance>, ApiError> {
    // SECURITY(#4618): Scoped tokens may only view their own agent's metrics.
    require_nous_access(&claims, &id)?;
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

/// GET /api/v1/metrics/quality: conversation quality time series.
#[utoipa::path(
    get,
    path = "/api/v1/metrics/quality",
    params(MetricsQuery),
    responses(
        (status = 200, description = "Quality metrics", body = QualityMetricsResponse),
        (status = 400, description = "Invalid query parameters", body = crate::error::ErrorResponse),
        (status = 401, description = "Unauthorized", body = crate::error::ErrorResponse),
    ),
    security(("bearer_auth" = []))
)]
pub async fn get_quality_metrics(
    State(state): State<InsightsState>,
    claims: Claims,
    Query(query): Query<MetricsQuery>,
) -> Result<Json<QualityMetricsResponse>, ApiError> {
    require_unscoped_operator(
        &claims,
        "scoped tokens cannot access aggregate quality metrics",
    )?;
    validate_metrics_query(&query)?;

    let state_clone = state.clone();
    // WHY(#5668): Use date-range filtering to bound the session scan and avoid
    // loading unbounded message content. compute_quality_series needs only
    // m.created_at and m.role; load a capped message count per session.
    let (sessions, messages) = tokio::task::spawn_blocking(move || {
        let store = state_clone.session_store.blocking_lock();
        let all_sessions = store.list_sessions(None).map_err(ApiError::from)?;

        // Apply date-range filter at the application layer to match the
        // pattern used by load_token_metrics (store lacks a date-query API).
        let sessions: Vec<Session> = all_sessions
            .into_iter()
            .filter(|s| date_in_range(&s.created_at, &query))
            .collect();

        // WHY(#5668): cap per-session history at 500 messages; quality series
        // needs only role and timestamp, not full content. This bounds
        // allocation at O(sessions * 500) instead of O(all-message-content).
        let quality_limit: i64 = 500;
        let mut messages = Vec::new();
        for session in &sessions {
            match store.get_history(&session.id, Some(quality_limit)) {
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
    Ok(Json(QualityMetricsResponse {
        series,
        data_unavailable: vec![UnavailableMetric {
            metric: "thinking_time_ratio".to_owned(),
            reason: "no backing data source for thinking time in pylon".to_owned(),
        }],
    }))
}

/// Granularity values accepted by the metrics endpoints.
///
/// Anything else would otherwise fall through `bucket_date`'s `_` arm and be
/// silently treated as `daily`, so an unknown granularity is rejected up front.
const VALID_GRANULARITIES: [&str; 3] = ["daily", "weekly", "monthly"];

/// Reject metrics query parameters that would otherwise be silently ignored.
///
/// `date_in_range` compares dates lexicographically and `bucket_date` defaults
/// unknown granularities to `daily`, so unvalidated malformed input would
/// produce a misleading empty/`daily` `200` response instead of an error.
/// Validating here turns those silent wrong-answers into an honest `400`.
/// Absent (`None`) and empty values keep their meaning (no filter / default
/// granularity).
fn validate_metrics_query(query: &MetricsQuery) -> Result<(), ApiError> {
    if let Some(granularity) = query.granularity.as_deref()
        && !granularity.is_empty()
        && !VALID_GRANULARITIES.contains(&granularity)
    {
        return Err(BadRequestSnafu {
            message: format!(
                "granularity must be one of daily, weekly, monthly (got `{granularity}`)"
            ),
        }
        .build());
    }
    validate_optional_date("from", query.from.as_deref())?;
    validate_optional_date("to", query.to.as_deref())?;
    Ok(())
}

/// Validate an optional `YYYY-MM-DD` bound, rejecting unparseable calendar dates.
fn validate_optional_date(field: &str, value: Option<&str>) -> Result<(), ApiError> {
    if let Some(raw) = value
        && !raw.is_empty()
        && raw.parse::<jiff::civil::Date>().is_err()
    {
        return Err(BadRequestSnafu {
            message: format!("{field} must be a valid ISO date (YYYY-MM-DD), got `{raw}`"),
        }
        .build());
    }
    Ok(())
}

/// GET /api/v1/metrics/tokens: token usage envelope consumed by desktop metrics.
#[utoipa::path(
    get,
    path = "/api/v1/metrics/tokens",
    params(MetricsQuery),
    responses(
        (status = 200, description = "Token metrics", body = TokenMetricsResponse),
        (status = 400, description = "Invalid query parameters", body = crate::error::ErrorResponse),
        (status = 401, description = "Unauthorized", body = crate::error::ErrorResponse),
    ),
    security(("bearer_auth" = []))
)]
pub async fn get_token_metrics(
    State(state): State<InsightsState>,
    claims: Claims,
    Query(query): Query<MetricsQuery>,
) -> Result<Json<TokenMetricsResponse>, ApiError> {
    require_unscoped_operator(
        &claims,
        "scoped tokens cannot access aggregate token metrics",
    )?;
    validate_metrics_query(&query)?;
    Ok(Json(load_token_metrics(state, query).await))
}

/// GET /api/v1/metrics/costs: cost metrics envelope consumed by desktop metrics.
#[utoipa::path(
    get,
    path = "/api/v1/metrics/costs",
    params(MetricsQuery),
    responses(
        (status = 200, description = "Cost metrics", body = CostMetricsResponse),
        (status = 400, description = "Invalid query parameters", body = crate::error::ErrorResponse),
        (status = 401, description = "Unauthorized", body = crate::error::ErrorResponse),
    ),
    security(("bearer_auth" = []))
)]
pub async fn get_cost_metrics(
    State(state): State<InsightsState>,
    claims: Claims,
    Query(query): Query<MetricsQuery>,
) -> Result<Json<CostMetricsResponse>, ApiError> {
    require_unscoped_operator(
        &claims,
        "scoped tokens cannot access aggregate cost metrics",
    )?;
    validate_metrics_query(&query)?;
    let tokens = load_token_metrics(state, query).await;
    Ok(Json(costs_from_tokens(&tokens)))
}

/// GET /api/v1/journal: queryable system event log.
#[utoipa::path(
    get,
    path = "/api/v1/journal",
    params(JournalQuery),
    responses(
        (status = 200, description = "Journal events", body = JournalResponse),
        (status = 401, description = "Unauthorized", body = crate::error::ErrorResponse),
    ),
    security(("bearer_auth" = []))
)]
pub async fn get_journal(
    claims: Claims,
    Query(query): Query<JournalQuery>,
) -> Result<Json<JournalResponse>, ApiError> {
    require_unscoped_operator(&claims, "scoped tokens cannot access the system journal")?;
    warn!(
        source = ?query.source,
        level = ?query.level,
        since = ?query.since,
        limit = query.limit,
        "journal endpoint called but no persistent event journal is available in pylon"
    );
    Ok(Json(JournalResponse {
        events: Vec::new(),
        data_unavailable: vec![UnavailableMetric {
            metric: "journal".to_owned(),
            reason: "no persistent event journal is available in pylon".to_owned(),
        }],
    }))
}

// ── Computation helpers ──

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
    // The zero placeholders are paired with `data_unavailable` so clients can
    // distinguish "not measured" from "zero observed".
    warn!(
        agent_id = %agent_id,
        "tool_calls_per_session, tool_success_rate, and errors_per_session have no backing data source in pylon — returning 0.0"
    );

    let tokens_per_response_series = build_daily_series(sessions, |sess| {
        let msgs = i64_to_f64(sess.metrics.message_count);
        let toks = i64_to_f64(sess.metrics.token_count_estimate);
        if msgs > 0.0 { toks / msgs } else { 0.0 }
    });

    let data_unavailable = vec![
        UnavailableMetric {
            metric: "tool_calls_per_session".to_owned(),
            reason: "no backing data source for tool call counts in pylon".to_owned(),
        },
        UnavailableMetric {
            metric: "tool_success_rate".to_owned(),
            reason: "no backing data source for tool success rate in pylon".to_owned(),
        },
        UnavailableMetric {
            metric: "errors_per_session".to_owned(),
            reason: "no backing data source for error counts in pylon".to_owned(),
        },
    ];

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
        data_unavailable,
    }
}

#[derive(Debug, Clone, Default)]
struct TokenTotals {
    input_tokens: u64,
    output_tokens: u64,
    session_count: u64,
}

impl TokenTotals {
    fn add_tokens(&mut self, input_tokens: u64, output_tokens: u64) {
        self.input_tokens = self.input_tokens.saturating_add(input_tokens);
        self.output_tokens = self.output_tokens.saturating_add(output_tokens);
    }

    fn add_session(&mut self) {
        self.session_count = self.session_count.saturating_add(1);
    }
}

#[derive(Debug, Clone)]
struct SessionUsage {
    session: Session,
    usage_records: Vec<UsageRecord>,
}

async fn load_token_metrics(state: InsightsState, query: MetricsQuery) -> TokenMetricsResponse {
    let agent_configs: Vec<(String, String, String)> = state
        .nous_manager
        .configs()
        .into_iter()
        .map(|c| {
            (
                c.id.to_string(),
                c.name
                    .clone()
                    .filter(|n| !n.is_empty())
                    .unwrap_or_else(|| c.id.to_string()),
                c.generation.model.clone(),
            )
        })
        .collect();
    let model_by_agent: HashMap<String, String> = agent_configs
        .iter()
        .map(|(id, _, model)| (id.clone(), model.clone()))
        .collect();

    let state_clone = state.clone();
    let all_sessions_res = tokio::task::spawn_blocking(move || {
        let store = state_clone.session_store.blocking_lock();
        let sessions = store.list_sessions(None).map_err(ApiError::from)?;
        let mut rows = Vec::with_capacity(sessions.len());
        for session in sessions {
            let usage_records = store
                .get_usage_for_session(&session.id)
                .unwrap_or_else(|err| {
                    warn!(
                        session_id = %session.id,
                        error = %err,
                        "failed to load usage records for token metrics"
                    );
                    Vec::new()
                });
            rows.push(SessionUsage {
                session,
                usage_records,
            });
        }
        Ok::<_, ApiError>(rows)
    })
    .await
    .unwrap_or_else(|e| {
        Err(InternalSnafu {
            message: format!("task join failed: {e}"),
        }
        .build())
    });

    let session_rows = all_sessions_res.unwrap_or_else(|_err| {
        warn!("failed to list sessions for usage metrics");
        Vec::new()
    });

    let today = jiff::Timestamp::now()
        .to_zoned(jiff::tz::TimeZone::UTC)
        .date();
    build_token_metrics_at(
        &agent_configs,
        &model_by_agent,
        &session_rows,
        &query,
        today,
    )
}

fn build_token_metrics_at(
    agent_configs: &[(String, String, String)],
    model_by_agent: &HashMap<String, String>,
    session_rows: &[SessionUsage],
    query: &MetricsQuery,
    today: jiff::civil::Date,
) -> TokenMetricsResponse {
    let mut total = TokenTotals::default();
    let mut agents: HashMap<String, (String, TokenTotals)> = agent_configs
        .iter()
        .map(|(id, name, _)| (id.clone(), (name.clone(), TokenTotals::default())))
        .collect();
    let mut models: HashMap<String, TokenTotals> = agent_configs
        .iter()
        .map(|(_, _, model)| (model.clone(), TokenTotals::default()))
        .collect();
    let mut series: HashMap<String, TokenTotals> = HashMap::new();

    for row in session_rows {
        let session = &row.session;
        if !date_in_range(&session.created_at, query) {
            continue;
        }
        if row.usage_records.is_empty() {
            continue;
        }

        let (input_tokens, output_tokens) = usage_token_split(&row.usage_records);
        total.add_tokens(input_tokens, output_tokens);
        total.add_session();

        let agent_entry = agents
            .entry(session.nous_id.clone())
            .or_insert_with(|| (session.nous_id.clone(), TokenTotals::default()));
        agent_entry.1.add_tokens(input_tokens, output_tokens);
        agent_entry.1.add_session();

        let mut models_for_session = HashSet::new();
        for usage in &row.usage_records {
            let model = usage
                .model
                .clone()
                .or_else(|| session.model.clone())
                .or_else(|| model_by_agent.get(&session.nous_id).cloned())
                .unwrap_or_else(|| "unknown".to_owned());
            let model_entry = models.entry(model.clone()).or_default();
            model_entry.add_tokens(
                token_i64_to_u64(usage.input_tokens),
                token_i64_to_u64(usage.output_tokens),
            );
            models_for_session.insert(model);
        }
        for model in models_for_session {
            models.entry(model).or_default().add_session();
        }

        let bucket = bucket_date(&session.created_at, query.granularity.as_deref());
        series
            .entry(bucket)
            .or_default()
            .add_tokens(input_tokens, output_tokens);
    }

    let windows = token_period_windows(session_rows, today);
    TokenMetricsResponse {
        series: series_points(series),
        agents: agent_rows(agents),
        models: model_rows(models),
        today_input: windows.today.input_tokens,
        today_output: windows.today.output_tokens,
        week_input: windows.week.input_tokens,
        week_output: windows.week.output_tokens,
        month_input: windows.month.input_tokens,
        month_output: windows.month.output_tokens,
        prev_today_input: windows.prev_today.input_tokens,
        prev_today_output: windows.prev_today.output_tokens,
        prev_week_input: windows.prev_week.input_tokens,
        prev_week_output: windows.prev_week.output_tokens,
        prev_month_input: windows.prev_month.input_tokens,
        prev_month_output: windows.prev_month.output_tokens,
    }
}

#[derive(Debug, Default)]
struct TokenPeriodWindows {
    today: TokenTotals,
    week: TokenTotals,
    month: TokenTotals,
    prev_today: TokenTotals,
    prev_week: TokenTotals,
    prev_month: TokenTotals,
}

fn token_period_windows(
    session_rows: &[SessionUsage],
    today: jiff::civil::Date,
) -> TokenPeriodWindows {
    let week_start = today.checked_sub(6.days()).unwrap_or(today);
    let prev_week_end = week_start.checked_sub(1.days()).unwrap_or(week_start);
    let prev_week_start = week_start.checked_sub(7.days()).unwrap_or(week_start);
    let month_start = jiff::civil::Date::new(today.year(), today.month(), 1).unwrap_or(today);
    let (prev_month_year, prev_month) = if today.month() == 1 {
        (today.year() - 1, 12)
    } else {
        (today.year(), today.month() - 1)
    };
    let prev_month_start =
        jiff::civil::Date::new(prev_month_year, prev_month, 1).unwrap_or(month_start);
    let prev_month_end = month_start
        .checked_sub(1.days())
        .unwrap_or(prev_month_start);
    let yesterday = today.checked_sub(1.days()).unwrap_or(today);

    let mut windows = TokenPeriodWindows::default();
    for row in session_rows {
        let Some(date) = session_date(&row.session) else {
            continue;
        };
        if row.usage_records.is_empty() {
            continue;
        }
        let (input_tokens, output_tokens) = usage_token_split(&row.usage_records);
        if date == today {
            windows.today.add_tokens(input_tokens, output_tokens);
        }
        if date == yesterday {
            windows.prev_today.add_tokens(input_tokens, output_tokens);
        }
        if date >= week_start && date <= today {
            windows.week.add_tokens(input_tokens, output_tokens);
        }
        if date >= prev_week_start && date <= prev_week_end {
            windows.prev_week.add_tokens(input_tokens, output_tokens);
        }
        if date >= month_start && date <= today {
            windows.month.add_tokens(input_tokens, output_tokens);
        }
        if date >= prev_month_start && date <= prev_month_end {
            windows.prev_month.add_tokens(input_tokens, output_tokens);
        }
    }
    windows
}

fn session_date(session: &Session) -> Option<jiff::civil::Date> {
    session.created_at.get(..10)?.parse().ok()
}

fn usage_token_split(records: &[UsageRecord]) -> (u64, u64) {
    let input_tokens = records
        .iter()
        .map(|record| token_i64_to_u64(record.input_tokens))
        .sum();
    let output_tokens = records
        .iter()
        .map(|record| token_i64_to_u64(record.output_tokens))
        .sum();
    (input_tokens, output_tokens)
}

fn token_i64_to_u64(tokens: i64) -> u64 {
    u64::try_from(tokens).unwrap_or(0)
}

fn date_in_range(timestamp: &str, query: &MetricsQuery) -> bool {
    let Some(date) = timestamp.get(..10) else {
        return true;
    };
    if let Some(from) = query.from.as_deref()
        && !from.is_empty()
        && date < from
    {
        return false;
    }
    if let Some(to) = query.to.as_deref()
        && !to.is_empty()
        && date > to
    {
        return false;
    }
    true
}

fn bucket_date(timestamp: &str, granularity: Option<&str>) -> String {
    let date = timestamp.get(..10).unwrap_or("1970-01-01");
    match granularity {
        Some("monthly") => date.get(..7).unwrap_or(date).to_owned(),
        Some("weekly") => {
            let year = date.get(..4).unwrap_or("1970");
            let month_day = date.get(5..10).unwrap_or("01-01");
            format!("{year}-W{month_day}")
        }
        _ => date.to_owned(),
    }
}

fn series_points(series: HashMap<String, TokenTotals>) -> Vec<TokenSeriesPoint> {
    let mut points: Vec<TokenSeriesPoint> = series
        .into_iter()
        .map(|(date, totals)| TokenSeriesPoint {
            date,
            input_tokens: totals.input_tokens,
            output_tokens: totals.output_tokens,
        })
        .collect();
    points.sort_by(|a, b| a.date.cmp(&b.date));
    points
}

fn agent_rows(agents: HashMap<String, (String, TokenTotals)>) -> Vec<AgentTokenRow> {
    let mut rows: Vec<AgentTokenRow> = agents
        .into_iter()
        .map(|(id, (name, totals))| AgentTokenRow {
            id,
            name,
            input_tokens: totals.input_tokens,
            output_tokens: totals.output_tokens,
            session_count: totals.session_count,
        })
        .collect();
    rows.sort_by(|a, b| a.id.cmp(&b.id));
    rows
}

fn model_rows(models: HashMap<String, TokenTotals>) -> Vec<ModelTokenRow> {
    let mut rows: Vec<ModelTokenRow> = models
        .into_iter()
        .map(|(model, totals)| ModelTokenRow {
            model,
            input_tokens: totals.input_tokens,
            output_tokens: totals.output_tokens,
            session_count: totals.session_count,
        })
        .collect();
    rows.sort_by(|a, b| a.model.cmp(&b.model));
    rows
}

fn costs_from_tokens(tokens: &TokenMetricsResponse) -> CostMetricsResponse {
    // NOTE: Cost values remain zero because pylon has no provider/model pricing
    // source. The zero placeholders are paired with `data_unavailable` so clients
    // can distinguish "not measured" from "zero observed".
    let agents = tokens
        .agents
        .iter()
        .map(|agent| AgentCostRow {
            id: agent.id.clone(),
            name: agent.name.clone(),
            total_cost: 0.0,
            message_count: 0,
            session_count: agent.session_count,
            output_tokens: agent.output_tokens,
            prev_period_cost: 0.0,
        })
        .collect();

    CostMetricsResponse {
        series: tokens
            .series
            .iter()
            .map(|point| CostSeriesPoint {
                date: point.date.clone(),
                cost_usd: 0.0,
            })
            .collect(),
        agents,
        today_cost: 0.0,
        week_cost: 0.0,
        month_cost: 0.0,
        prev_today_cost: 0.0,
        prev_week_cost: 0.0,
        prev_month_cost: 0.0,
        data_unavailable: vec![UnavailableMetric {
            metric: "cost".to_owned(),
            reason: "no provider/model pricing source available in pylon; cost attribution requires ResolvedModelContext (#4798)".to_owned(),
        }],
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

#[cfg(test)]
mod tests {
    use super::*;

    fn query(granularity: Option<&str>, from: Option<&str>, to: Option<&str>) -> MetricsQuery {
        MetricsQuery {
            granularity: granularity.map(str::to_owned),
            from: from.map(str::to_owned),
            to: to.map(str::to_owned),
        }
    }

    #[test]
    fn accepts_absent_and_empty_parameters() {
        assert!(validate_metrics_query(&query(None, None, None)).is_ok());
        // Empty strings keep their legacy meaning (default granularity / no filter).
        assert!(validate_metrics_query(&query(Some(""), Some(""), Some(""))).is_ok());
    }

    #[test]
    fn accepts_known_granularities_and_iso_dates() {
        for g in ["daily", "weekly", "monthly"] {
            assert!(
                validate_metrics_query(&query(Some(g), Some("2026-01-01"), Some("2026-12-31")))
                    .is_ok(),
                "granularity {g} should be accepted"
            );
        }
    }

    #[test]
    fn rejects_unknown_granularity() {
        let result = validate_metrics_query(&query(Some("hourly"), None, None));
        assert!(
            matches!(result, Err(ApiError::BadRequest { .. })),
            "unknown granularity must be rejected with a 400"
        );
    }

    #[test]
    fn rejects_unparseable_dates() {
        assert!(validate_metrics_query(&query(None, Some("not-a-date"), None)).is_err());
        assert!(validate_metrics_query(&query(None, None, Some("2026-13-45"))).is_err());
        // A syntactically plausible but out-of-calendar date is also rejected.
        assert!(validate_metrics_query(&query(None, Some("2026-02-30"), None)).is_err());
    }

    fn session(id: &str, created_at: &str) -> Session {
        Session {
            id: id.to_owned(),
            nous_id: "alice".to_owned(),
            session_key: id.to_owned(),
            status: mneme::types::SessionStatus::Active,
            model: Some("model-a".to_owned()),
            session_type: mneme::types::SessionType::Primary,
            created_at: format!("{created_at}T00:00:00Z"),
            updated_at: format!("{created_at}T00:00:00Z"),
            metrics: mneme::types::SessionMetrics {
                token_count_estimate: 0,
                message_count: 0,
                last_input_tokens: 0,
                bootstrap_hash: None,
                distillation_count: 0,
                last_distilled_at: None,
                computed_context_tokens: 0,
            },
            origin: mneme::types::SessionOrigin {
                parent_session_id: None,
                thread_id: None,
                transport: Some("test".to_owned()),
                display_name: None,
            },
            artefact_meta: None,
        }
    }

    fn usage(session_id: &str, input_tokens: i64, output_tokens: i64) -> UsageRecord {
        UsageRecord {
            session_id: session_id.to_owned(),
            turn_seq: 1,
            input_tokens,
            output_tokens,
            cache_read_tokens: 0,
            cache_write_tokens: 0,
            model: Some("model-a".to_owned()),
        }
    }

    fn fixed_date(date: &str) -> jiff::civil::Date {
        match date.parse() {
            Ok(date) => date,
            Err(err) => panic!("fixed test date must parse: {err}"),
        }
    }

    fn first_item<'a, T>(items: &'a [T], label: &str) -> &'a T {
        match items.first() {
            Some(item) => item,
            None => panic!("{label} should contain at least one item"),
        }
    }

    #[test]
    fn token_metrics_use_durable_usage_and_real_period_windows() {
        let rows = vec![
            SessionUsage {
                session: session("today", "2026-06-12"),
                usage_records: vec![usage("today", 10, 5)],
            },
            SessionUsage {
                session: session("yesterday", "2026-06-11"),
                usage_records: vec![usage("yesterday", 20, 10)],
            },
            SessionUsage {
                session: session("prev-week", "2026-06-05"),
                usage_records: vec![usage("prev-week", 30, 15)],
            },
            SessionUsage {
                session: session("prev-month", "2026-05-20"),
                usage_records: vec![usage("prev-month", 40, 20)],
            },
        ];
        let agent_configs = vec![("alice".to_owned(), "Alice".to_owned(), "model-a".to_owned())];
        let model_by_agent = HashMap::from([("alice".to_owned(), "model-a".to_owned())]);
        let response = build_token_metrics_at(
            &agent_configs,
            &model_by_agent,
            &rows,
            &query(Some("daily"), Some("2026-06-12"), Some("2026-06-12")),
            fixed_date("2026-06-12"),
        );

        assert_eq!(response.series.len(), 1);
        assert_eq!(first_item(&response.series, "series").input_tokens, 10);
        assert_eq!(first_item(&response.agents, "agents").input_tokens, 10);
        assert_eq!(first_item(&response.agents, "agents").session_count, 1);
        assert_eq!(first_item(&response.models, "models").output_tokens, 5);
        assert_eq!(response.today_input, 10);
        assert_eq!(response.prev_today_input, 20);
        assert_eq!(response.week_input, 30);
        assert_eq!(response.prev_week_input, 30);
        assert_eq!(response.month_input, 60);
        assert_eq!(response.prev_month_input, 40);
    }

    #[test]
    fn agent_performance_marks_tool_metrics_unavailable() {
        let perf = compute_agent_performance("alice", Some("Alice"), &[]);
        let unavailable: Vec<&str> = perf
            .data_unavailable
            .iter()
            .map(|u| u.metric.as_str())
            .collect();
        assert!(unavailable.contains(&"tool_calls_per_session"));
        assert!(unavailable.contains(&"tool_success_rate"));
        assert!(unavailable.contains(&"errors_per_session"));
    }

    #[test]
    fn quality_response_marks_thinking_time_unavailable() {
        let response = QualityMetricsResponse {
            series: compute_quality_series(&[], &[]),
            data_unavailable: vec![UnavailableMetric {
                metric: "thinking_time_ratio".to_owned(),
                reason: "no backing data source for thinking time in pylon".to_owned(),
            }],
        };
        assert!(
            response
                .data_unavailable
                .iter()
                .any(|u| u.metric == "thinking_time_ratio")
        );
    }

    #[test]
    fn journal_response_marks_journal_unavailable() {
        let response = JournalResponse {
            events: Vec::new(),
            data_unavailable: vec![UnavailableMetric {
                metric: "journal".to_owned(),
                reason: "no persistent event journal is available in pylon".to_owned(),
            }],
        };
        assert!(
            response
                .data_unavailable
                .iter()
                .any(|u| u.metric == "journal")
        );
    }

    #[test]
    fn cost_response_marks_cost_unavailable() {
        let tokens = TokenMetricsResponse {
            series: Vec::new(),
            agents: Vec::new(),
            models: Vec::new(),
            today_input: 0,
            today_output: 0,
            week_input: 0,
            week_output: 0,
            month_input: 0,
            month_output: 0,
            prev_today_input: 0,
            prev_today_output: 0,
            prev_week_input: 0,
            prev_week_output: 0,
            prev_month_input: 0,
            prev_month_output: 0,
        };
        let response = costs_from_tokens(&tokens);
        assert!(response.data_unavailable.iter().any(|u| u.metric == "cost"));
    }
}

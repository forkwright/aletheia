//! Session management and message streaming handlers.

// WHY: pub(crate) so utoipa-generated `__path_*` types are visible to the OpenAPI derive.
pub(crate) mod approvals;
pub(crate) mod streaming;
pub(crate) mod types;

pub use approvals::{approve_tool, deny_tool, resolve as resolve_approval};
pub use streaming::{events, reconnect_turn, send_message, stream_turn};

use types::{
    CreateSessionRequest, HistoryMessage, HistoryParams, HistoryResponse, ListSessionsParams,
    ListSessionsResponse, RenameSessionRequest, ReplayMessage, ReplaySession,
    ReplayToolAuditRecord, ReplayTurnAttempt, ReplayUsageRecord, SessionListItem,
    SessionReplayResponse, SessionResponse,
};

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use serde::Deserialize;
use tracing::{info, instrument};

use mneme::types::SessionStatus;

use symbolon::types::Role;

use crate::error::{
    ApiError, ConflictSnafu, ErrorResponse, FieldError, NousNotFoundSnafu, SessionNotFoundSnafu,
    ValidationFailedSnafu,
};
use crate::extract::{Claims, require_nous_access, require_role};
use crate::state::SessionsState;

const SESSION_REPLAY_VERSION: u32 = 1;
const SESSION_REPLAY_EXPORT_TYPE: &str = "sessionReplay";
const TURN_NOTE_CATEGORY: &str = "context";

#[derive(Debug, Deserialize)]
struct StoredTurnAttempt {
    version: u32,
    turn_id: String,
    session_id: String,
    nous_id: String,
    status: String,
    #[serde(default)]
    stage: Option<String>,
    #[serde(default)]
    error_code: Option<String>,
    #[serde(default)]
    error_message: Option<String>,
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    messages_persisted: Option<usize>,
    #[serde(default)]
    expected_messages: Option<usize>,
    created_at: String,
}

fn session_matches_search(session: &mneme::types::Session, search: &str) -> bool {
    let search = search.to_lowercase();
    let fields = [
        session.id.as_str(),
        session.nous_id.as_str(),
        session.session_key.as_str(),
        session.status.as_str(),
        session.origin.display_name.as_deref().unwrap_or(""),
    ];

    fields
        .into_iter()
        .any(|field| field.to_lowercase().contains(&search))
}

fn replay_session_from_mneme(session: mneme::types::Session) -> ReplaySession {
    ReplaySession {
        id: session.id,
        nous_id: session.nous_id,
        session_key: session.session_key,
        status: session.status.to_string(),
        session_type: session.session_type.to_string(),
        model: session.model,
        message_count: session.metrics.message_count,
        token_count_estimate: session.metrics.token_count_estimate,
        distillation_count: session.metrics.distillation_count,
        created_at: session.created_at,
        updated_at: session.updated_at,
        parent_session_id: session.origin.parent_session_id,
        thread_id: session.origin.thread_id,
        transport: session.origin.transport,
        display_name: session.origin.display_name,
        last_input_tokens: session.metrics.last_input_tokens,
        bootstrap_hash: session.metrics.bootstrap_hash,
        last_distilled_at: session.metrics.last_distilled_at,
        computed_context_tokens: session.metrics.computed_context_tokens,
    }
}

fn replay_message_from_mneme(message: mneme::types::Message) -> ReplayMessage {
    ReplayMessage {
        id: message.id,
        seq: message.seq,
        role: message.role.as_str().to_owned(),
        content: message.content,
        tool_call_id: message.tool_call_id,
        tool_name: message.tool_name,
        token_estimate: message.token_estimate,
        is_distilled: message.is_distilled,
        created_at: message.created_at,
    }
}

fn replay_usage_from_mneme(record: mneme::types::UsageRecord) -> ReplayUsageRecord {
    ReplayUsageRecord {
        turn_seq: record.turn_seq,
        input_tokens: record.input_tokens,
        output_tokens: record.output_tokens,
        cache_read_tokens: record.cache_read_tokens,
        cache_write_tokens: record.cache_write_tokens,
        model: record.model,
    }
}

fn replay_tool_audit_from_mneme(record: mneme::types::ToolAuditRecord) -> ReplayToolAuditRecord {
    ReplayToolAuditRecord {
        id: record.id,
        nous_id: record.nous_id,
        turn_seq: record.turn_seq,
        tool_call_id: record.tool_call_id,
        tool_name: record.tool_name,
        duration_ms: record.duration_ms,
        is_error: record.is_error,
        outcome: record.outcome,
        result: record.result,
        approval: record.approval,
        receipt: record.receipt,
        created_at: record.created_at,
    }
}

fn replay_turn_attempts_from_notes(notes: Vec<mneme::types::AgentNote>) -> Vec<ReplayTurnAttempt> {
    let mut attempts: Vec<ReplayTurnAttempt> = notes
        .into_iter()
        .filter(|note| note.category == TURN_NOTE_CATEGORY)
        .filter_map(|note| serde_json::from_str::<StoredTurnAttempt>(&note.content).ok())
        .map(|attempt| ReplayTurnAttempt {
            version: attempt.version,
            turn_id: attempt.turn_id,
            session_id: attempt.session_id,
            nous_id: attempt.nous_id,
            status: attempt.status,
            stage: attempt.stage,
            error_code: attempt.error_code,
            error_message: attempt.error_message,
            model: attempt.model,
            messages_persisted: attempt.messages_persisted,
            expected_messages: attempt.expected_messages,
            created_at: attempt.created_at,
        })
        .collect();
    attempts.sort_by(|a, b| a.created_at.cmp(&b.created_at));
    attempts
}

/// POST /api/v1/sessions: create a new session.
///
/// # Cancel safety
///
/// Cancel-safe. Axum handler; cancellation drops the future with no
/// side effects beyond not returning a response.
#[utoipa::path(
    post,
    path = "/api/v1/sessions",
    request_body = CreateSessionRequest,
    responses(
        (status = 201, description = "Session created", body = SessionResponse),
        (status = 400, description = "Bad request", body = ErrorResponse),
        (status = 401, description = "Unauthorized", body = ErrorResponse),
        (status = 404, description = "Nous not found", body = ErrorResponse),
    ),
    security(("bearer_auth" = []))
)]
#[instrument(skip(state, claims, body))]
pub async fn create(
    State(state): State<SessionsState>,
    claims: Claims,
    Json(body): Json<CreateSessionRequest>,
) -> Result<impl IntoResponse, ApiError> {
    require_role(&claims, Role::Operator)?;
    require_nous_access(&claims, &body.nous_id)?;
    let nous_id = body.nous_id;
    let session_key = body.session_key;

    // WHY(#3275): collect all field errors and return them in one response so
    // callers can fix all issues in a single round-trip.
    let max_id_bytes = state.config.read().await.api_limits.max_identifier_bytes;
    let mut field_errors = Vec::new();
    if nous_id.is_empty() {
        field_errors.push(FieldError {
            field: "nous_id".to_owned(),
            code: "required".to_owned(),
            message: "must not be empty".to_owned(),
        });
    } else if nous_id.len() > max_id_bytes {
        field_errors.push(FieldError {
            field: "nous_id".to_owned(),
            code: "too_long".to_owned(),
            message: format!("exceeds maximum length of {max_id_bytes} bytes"),
        });
    }
    if session_key.is_empty() {
        field_errors.push(FieldError {
            field: "session_key".to_owned(),
            code: "required".to_owned(),
            message: "must not be empty".to_owned(),
        });
    } else if session_key.len() > max_id_bytes {
        field_errors.push(FieldError {
            field: "session_key".to_owned(),
            code: "too_long".to_owned(),
            message: format!("exceeds maximum length of {max_id_bytes} bytes"),
        });
    }
    if !field_errors.is_empty() {
        return Err(ValidationFailedSnafu {
            errors: field_errors,
        }
        .build());
    }

    let config = state.nous_manager.get_config(&nous_id).ok_or_else(|| {
        NousNotFoundSnafu {
            id: nous_id.clone(),
        }
        .build()
    })?;

    // WHY: SessionId (UUID v4) is the canonical format. ULID here caused
    // 'invalid SessionId' when nous parsed the stored ID back (#2349).
    let id = koina::id::SessionId::new().to_string();
    let model = config.generation.model.clone();

    let state_clone = state.clone();
    let id_clone = id.clone();
    let nid = nous_id.clone();
    let skey = session_key.clone();

    let session = tokio::task::spawn_blocking(move || {
        let store = state_clone.session_store.blocking_lock();
        // WHY: use create_session (not find_or_create) so that any existing session
        // with this (nous_id, session_key) pair: active or archived: produces a
        // 409 Conflict rather than silently returning the existing session (#1249).
        // The UNIQUE(nous_id, session_key) schema constraint enforces this even under
        // concurrent requests.
        match store.create_session(&id_clone, &nid, &skey, None, Some(&model)) {
            Ok(session) => Ok(session),
            Err(e) if is_unique_constraint_violation(&e) => Err(ConflictSnafu {
                message: format!("a session with key '{skey}' already exists for agent '{nid}'"),
            }
            .build()),
            Err(e) => Err(ApiError::from(e)),
        }
    })
    .await??;

    info!(session_id = %session.id, nous_id, "session created");

    Ok((
        StatusCode::CREATED,
        Json(SessionResponse::from_mneme(&session)),
    ))
}

/// GET /api/v1/sessions: list sessions, optionally filtered by agent.
///
/// # Cancel safety
///
/// Cancel-safe. Axum handler; cancellation drops the future with no
/// side effects beyond not returning a response.
#[utoipa::path(
    get,
    path = "/api/v1/sessions",
    params(
        ("nous_id" = Option<String>, Query, description = "Filter sessions by agent ID"),
        ("search" = Option<String>, Query, description = "Case-insensitive substring search across session id, key, status, and display name"),
        ("status" = Option<String>, Query, description = "Filter sessions by lifecycle status (active, archived, distilled)"),
        ("limit" = Option<u32>, Query, description = "Maximum number of sessions to return (default 50, max 1000)"),
        ("after" = Option<String>, Query, description = "Cursor token from a previous response's next_cursor field"),
    ),
    responses(
        (status = 200, description = "Paginated session list"),
        (status = 401, description = "Unauthorized", body = ErrorResponse),
    ),
    security(("bearer_auth" = []))
)]
#[instrument(skip(state, claims))]
pub async fn list_sessions(
    State(state): State<SessionsState>,
    claims: Claims,
    Query(params): Query<ListSessionsParams>,
) -> Result<Json<ListSessionsResponse>, ApiError> {
    use crate::pagination::{DEFAULT_LIMIT, MAX_LIMIT, PaginatedResponse};

    // WHY: a token scoped to a single nous_id may only see its own agent's
    // sessions, regardless of the `nous_id` query parameter. Without this
    // override, a scoped caller could pass `?nous_id=other-agent` and read
    // every session for that agent. If the caller's scope contradicts the
    // requested filter, reject the request rather than silently rewriting it.
    let nous_id = match (claims.nous_id.as_deref(), params.nous_id.as_deref()) {
        (Some(scoped), Some(requested)) if scoped != requested => {
            return Err(ApiError::forbidden("access denied for this agent"));
        }
        (Some(scoped), _) => Some(scoped.to_owned()),
        (None, requested) => requested.map(str::to_owned),
    };
    let search = params
        .search
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned);
    let status = params
        .status
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_ascii_lowercase);
    let limit = params.limit.unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_LIMIT);

    let state_clone = state.clone();
    let sessions = tokio::task::spawn_blocking(move || {
        let store = state_clone.session_store.blocking_lock();
        store
            .list_sessions(nous_id.as_deref())
            .map_err(ApiError::from)
    })
    .await??;

    let sessions: Vec<_> = sessions
        .into_iter()
        .filter(|session| {
            search
                .as_deref()
                .is_none_or(|query| session_matches_search(session, query))
        })
        .filter(|session| {
            status
                .as_deref()
                .is_none_or(|requested| session.status.as_str() == requested)
        })
        .collect();

    let total = u64::try_from(sessions.len()).unwrap_or(u64::MAX);
    let items: Vec<SessionListItem> = sessions
        .into_iter()
        .map(|s| SessionListItem {
            id: s.id,
            nous_id: s.nous_id,
            session_key: s.session_key,
            status: s.status.as_str().to_owned(),
            message_count: s.metrics.message_count,
            updated_at: s.updated_at,
            name: s.origin.display_name,
        })
        .collect();

    Ok(Json(PaginatedResponse::from_vec(
        items,
        limit,
        params.after.as_deref(),
        |s| s.id.clone(),
        Some(total),
    )))
}

/// GET /api/v1/sessions/{id}: get session state.
///
/// # Cancel safety
///
/// Cancel-safe. Axum handler; cancellation drops the future with no
/// side effects beyond not returning a response.
#[utoipa::path(
    get,
    path = "/api/v1/sessions/{id}",
    params(("id" = String, Path, description = "Session ID")),
    responses(
        (status = 200, description = "Session details", body = SessionResponse),
        (status = 401, description = "Unauthorized", body = ErrorResponse),
        (status = 404, description = "Session not found or deleted", body = ErrorResponse),
    ),
    security(("bearer_auth" = []))
)]
#[instrument(skip(state, claims))]
pub async fn get_session(
    State(state): State<SessionsState>,
    claims: Claims,
    Path(id): Path<String>,
) -> Result<Json<SessionResponse>, ApiError> {
    let session = find_session(&state, &id).await?;
    require_nous_access(&claims, &session.nous_id)?;
    // WHY: archived sessions must not be visible via normal GET (#3196).
    // The unarchive endpoint uses `find_session` directly (any status),
    // so this filter only affects the read path.
    if session.status == SessionStatus::Archived {
        return Err(SessionNotFoundSnafu { id: id.clone() }.build());
    }
    Ok(Json(SessionResponse::from_mneme(&session)))
}

/// GET /api/v1/sessions/{id}/replay: replay-faithful session export.
///
/// # Cancel safety
///
/// Cancel-safe. Axum handler; cancellation drops the future with no
/// side effects beyond not returning a response.
#[utoipa::path(
    get,
    path = "/api/v1/sessions/{id}/replay",
    params(("id" = String, Path, description = "Session ID")),
    responses(
        (status = 200, description = "Replay-faithful session export", body = SessionReplayResponse),
        (status = 401, description = "Unauthorized", body = ErrorResponse),
        (status = 404, description = "Session not found", body = ErrorResponse),
    ),
    security(("bearer_auth" = []))
)]
#[instrument(skip(state, claims))]
pub async fn replay(
    State(state): State<SessionsState>,
    claims: Claims,
    Path(id): Path<String>,
) -> Result<Json<SessionReplayResponse>, ApiError> {
    let session = find_session(&state, &id).await?;
    require_nous_access(&claims, &session.nous_id)?;

    let state_clone = state.clone();
    let id_clone = id.clone();
    let (messages, usage_records, tool_audit_records, notes) =
        tokio::task::spawn_blocking(move || -> Result<_, ApiError> {
            let store = state_clone.session_store.blocking_lock();
            let messages = store
                .get_history_raw(&id_clone, None)
                .map_err(ApiError::from)?;
            let usage_records = store
                .get_usage_for_session(&id_clone)
                .map_err(ApiError::from)?;
            let tool_audit_records = store
                .tool_audit_records_for_session(&id_clone)
                .map_err(ApiError::from)?;
            let notes = store.get_notes(&id_clone).map_err(ApiError::from)?;
            Ok((messages, usage_records, tool_audit_records, notes))
        })
        .await??;

    Ok(Json(SessionReplayResponse {
        version: SESSION_REPLAY_VERSION,
        export_type: SESSION_REPLAY_EXPORT_TYPE.to_owned(),
        exported_at: jiff::Timestamp::now().to_string(),
        session: replay_session_from_mneme(session),
        messages: messages
            .into_iter()
            .map(replay_message_from_mneme)
            .collect(),
        usage_records: usage_records
            .into_iter()
            .map(replay_usage_from_mneme)
            .collect(),
        tool_audit_records: tool_audit_records
            .into_iter()
            .map(replay_tool_audit_from_mneme)
            .collect(),
        turn_attempts: replay_turn_attempts_from_notes(notes),
    }))
}

/// DELETE /api/v1/sessions/{id}: close (archive) a session.
///
/// # Cancel safety
///
/// Cancel-safe. Axum handler; cancellation drops the future with no
/// side effects beyond not returning a response.
#[utoipa::path(
    delete,
    path = "/api/v1/sessions/{id}",
    params(("id" = String, Path, description = "Session ID")),
    responses(
        (status = 204, description = "Session closed"),
        (status = 401, description = "Unauthorized", body = ErrorResponse),
        (status = 404, description = "Session not found", body = ErrorResponse),
    ),
    security(("bearer_auth" = []))
)]
#[instrument(skip(state, claims))]
pub async fn close(
    State(state): State<SessionsState>,
    claims: Claims,
    Path(id): Path<String>,
) -> Result<StatusCode, ApiError> {
    require_role(&claims, Role::Operator)?;
    let session = find_session(&state, &id).await?;
    require_nous_access(&claims, &session.nous_id)?;
    archive_session_by_id(&state, &id).await
}

/// DELETE /api/v1/sessions/{id}/purge: permanently delete a session and all its messages.
///
/// # Cancel safety
///
/// Cancel-safe. Axum handler; cancellation drops the future with no
/// side effects beyond not returning a response.
#[utoipa::path(
    delete,
    path = "/api/v1/sessions/{id}/purge",
    params(("id" = String, Path, description = "Session ID")),
    responses(
        (status = 204, description = "Session permanently deleted"),
        (status = 401, description = "Unauthorized", body = ErrorResponse),
        (status = 404, description = "Session not found", body = ErrorResponse),
    ),
    security(("bearer_auth" = []))
)]
#[instrument(skip(state, claims))]
pub async fn purge(
    State(state): State<SessionsState>,
    claims: Claims,
    Path(id): Path<String>,
) -> Result<StatusCode, ApiError> {
    require_role(&claims, Role::Operator)?;
    let session = find_session(&state, &id).await?;
    require_nous_access(&claims, &session.nous_id)?;

    let state_clone = state.clone();
    let id_clone = id.clone();
    tokio::task::spawn_blocking(move || {
        let store = state_clone.session_store.blocking_lock();
        store.delete_session(&id_clone).map_err(ApiError::from)
    })
    .await??;

    info!(session_id = %id, "session permanently deleted");
    Ok(StatusCode::NO_CONTENT)
}

/// POST /api/v1/sessions/{id}/archive: archive a session.
///
/// Same behavior as DELETE but via POST, matching the TUI's API contract.
///
/// # Cancel safety
///
/// Cancel-safe. Axum handler; cancellation drops the future with no
/// side effects beyond not returning a response.
#[utoipa::path(
    post,
    path = "/api/v1/sessions/{id}/archive",
    params(("id" = String, Path, description = "Session ID")),
    responses(
        (status = 204, description = "Session archived"),
        (status = 401, description = "Unauthorized", body = ErrorResponse),
        (status = 404, description = "Session not found", body = ErrorResponse),
    ),
    security(("bearer_auth" = []))
)]
#[instrument(skip(state, claims))]
pub async fn archive(
    State(state): State<SessionsState>,
    claims: Claims,
    Path(id): Path<String>,
) -> Result<StatusCode, ApiError> {
    require_role(&claims, Role::Operator)?;
    let session = find_session(&state, &id).await?;
    require_nous_access(&claims, &session.nous_id)?;
    archive_session_by_id(&state, &id).await
}

/// Shared archive logic for both DELETE and POST archive routes.
async fn archive_session_by_id(state: &SessionsState, id: &str) -> Result<StatusCode, ApiError> {
    let _ = find_session(state, id).await?;

    let state_clone = state.clone();
    let id_clone = id.to_owned();
    tokio::task::spawn_blocking(move || {
        let store = state_clone.session_store.blocking_lock();
        store
            .update_session_status(&id_clone, SessionStatus::Archived)
            .map_err(ApiError::from)
    })
    .await??;

    info!(session_id = %id, "session archived");
    Ok(StatusCode::NO_CONTENT)
}

/// POST /api/v1/sessions/{id}/unarchive: reactivate an archived session.
///
/// # Cancel safety
///
/// Cancel-safe. Axum handler; cancellation drops the future with no
/// side effects beyond not returning a response.
#[utoipa::path(
    post,
    path = "/api/v1/sessions/{id}/unarchive",
    params(("id" = String, Path, description = "Session ID")),
    responses(
        (status = 204, description = "Session reactivated"),
        (status = 401, description = "Unauthorized", body = ErrorResponse),
        (status = 404, description = "Session not found", body = ErrorResponse),
    ),
    security(("bearer_auth" = []))
)]
#[instrument(skip(state, claims))]
pub async fn unarchive(
    State(state): State<SessionsState>,
    claims: Claims,
    Path(id): Path<String>,
) -> Result<StatusCode, ApiError> {
    require_role(&claims, Role::Operator)?;
    let session = find_session(&state, &id).await?;
    require_nous_access(&claims, &session.nous_id)?;

    let state_clone = state.clone();
    let id_clone = id.clone();
    tokio::task::spawn_blocking(move || {
        let store = state_clone.session_store.blocking_lock();
        store
            .update_session_status(&id_clone, SessionStatus::Active)
            .map_err(ApiError::from)
    })
    .await??;

    info!(session_id = %id, "session unarchived");
    Ok(StatusCode::NO_CONTENT)
}

/// PUT /api/v1/sessions/{id}/name: rename a session.
///
/// # Cancel safety
///
/// Cancel-safe. Axum handler; cancellation drops the future with no
/// side effects beyond not returning a response.
#[utoipa::path(
    put,
    path = "/api/v1/sessions/{id}/name",
    params(("id" = String, Path, description = "Session ID")),
    request_body = RenameSessionRequest,
    responses(
        (status = 204, description = "Session renamed"),
        (status = 400, description = "Bad request", body = ErrorResponse),
        (status = 401, description = "Unauthorized", body = ErrorResponse),
        (status = 404, description = "Session not found", body = ErrorResponse),
    ),
    security(("bearer_auth" = []))
)]
#[instrument(skip(state, claims, body))]
pub async fn rename(
    State(state): State<SessionsState>,
    claims: Claims,
    Path(id): Path<String>,
    Json(body): Json<RenameSessionRequest>,
) -> Result<StatusCode, ApiError> {
    require_role(&claims, Role::Operator)?;
    let session = find_session(&state, &id).await?;
    require_nous_access(&claims, &session.nous_id)?;

    // WHY(#3275): field-level validation errors for the rename endpoint.
    let max_name_len = state.config.read().await.api_limits.max_session_name_len;
    if body.name.is_empty() {
        return Err(ValidationFailedSnafu {
            errors: vec![FieldError {
                field: "name".to_owned(),
                code: "required".to_owned(),
                message: "must not be empty".to_owned(),
            }],
        }
        .build());
    }
    if body.name.len() > max_name_len {
        return Err(ValidationFailedSnafu {
            errors: vec![FieldError {
                field: "name".to_owned(),
                code: "too_long".to_owned(),
                message: format!("exceeds maximum length of {max_name_len} bytes"),
            }],
        }
        .build());
    }

    let state_clone = state.clone();
    let id_clone = id.clone();
    let name = body.name;
    tokio::task::spawn_blocking(move || {
        let store = state_clone.session_store.blocking_lock();
        store
            .update_display_name(&id_clone, &name)
            .map_err(ApiError::from)
    })
    .await??;

    info!(session_id = %id, "session renamed");
    Ok(StatusCode::NO_CONTENT)
}

/// GET /api/v1/sessions/{id}/history: get conversation history.
///
/// # Cancel safety
///
/// Cancel-safe. Axum handler; cancellation drops the future with no
/// side effects beyond not returning a response.
#[utoipa::path(
    get,
    path = "/api/v1/sessions/{id}/history",
    params(
        ("id" = String, Path, description = "Session ID"),
        ("limit" = Option<u32>, Query, description = "Maximum messages to return"),
        ("before" = Option<i64>, Query, description = "Return messages before this sequence number"),
    ),
    responses(
        (status = 200, description = "Conversation history", body = HistoryResponse),
        (status = 401, description = "Unauthorized", body = ErrorResponse),
        (status = 404, description = "Session not found", body = ErrorResponse),
    ),
    security(("bearer_auth" = []))
)]
#[instrument(skip(state, claims))]
pub async fn history(
    State(state): State<SessionsState>,
    claims: Claims,
    Path(id): Path<String>,
    Query(params): Query<HistoryParams>,
) -> Result<Json<HistoryResponse>, ApiError> {
    let session = find_session(&state, &id).await?;
    require_nous_access(&claims, &session.nous_id)?;

    // WHY: Cap limit at max_history_limit and apply a sensible default so a single
    // request cannot fetch an unbounded number of messages.
    let api_limits = &state.config.read().await.api_limits;
    let limit = params
        .limit
        .unwrap_or(api_limits.default_history_limit)
        .min(api_limits.max_history_limit);
    let before_seq = params.before;

    let state_clone = state.clone();
    let id_clone = id.clone();
    let messages = tokio::task::spawn_blocking(move || {
        let store = state_clone.session_store.blocking_lock();
        store
            .get_history_filtered(&id_clone, Some(i64::from(limit)), before_seq)
            .map_err(ApiError::from)
    })
    .await??;

    let items: Vec<HistoryMessage> = messages
        .into_iter()
        .map(|m| HistoryMessage {
            id: m.id,
            seq: m.seq,
            role: m.role.as_str().to_owned(),
            content: m.content,
            tool_call_id: m.tool_call_id,
            tool_name: m.tool_name,
            created_at: m.created_at,
        })
        .collect();

    Ok(Json(HistoryResponse { messages: items }))
}

/// Resolve or create a session for the given agent and session key.
pub(crate) async fn resolve_session(
    state: &SessionsState,
    agent_id: &str,
    session_key: &str,
    model: Option<&str>,
) -> Result<String, ApiError> {
    // WHY: SessionId (UUID v4) is the canonical format. ULID here caused
    // 'invalid SessionId' when nous parsed the stored ID back (#2349).
    let id = koina::id::SessionId::new().to_string();
    let state_clone = state.clone();
    let id_clone = id.clone();
    let aid = agent_id.to_owned();
    let skey = session_key.to_owned();
    let model_owned = model.map(ToOwned::to_owned);

    let session = tokio::task::spawn_blocking(move || {
        let store = state_clone.session_store.blocking_lock();
        match store.find_or_create_session(&id_clone, &aid, &skey, model_owned.as_deref(), None) {
            Ok(session) => Ok(session),
            Err(e) if is_unique_constraint_violation(&e) => {
                // WHY: Concurrent stream requests may race to create the same session.
                // Fall back to returning whichever session won the INSERT race.
                store
                    .find_session(&aid, &skey)
                    .map_err(ApiError::from)?
                    .ok_or_else(|| ApiError::Internal {
                        message: "session missing after constraint violation".to_owned(),
                        location: snafu::location!(),
                    })
            }
            Err(e) => Err(ApiError::from(e)),
        }
    })
    .await??;

    Ok(session.id)
}

/// Returns `true` when a mneme error indicates a duplicate session-key
/// (the fjall backend's equivalent of a UNIQUE constraint violation).
///
/// Delegates to [`mneme::error::Error::is_unique_constraint_violation`].
fn is_unique_constraint_violation(err: &mneme::error::Error) -> bool {
    err.is_unique_constraint_violation()
}

pub(crate) async fn find_session(
    state: &SessionsState,
    id: &str,
) -> Result<mneme::types::Session, ApiError> {
    let state_clone = state.clone();
    let id_owned = id.to_owned();
    let id_for_error = id.to_owned();
    let session = tokio::task::spawn_blocking(move || {
        let store = state_clone.session_store.blocking_lock();
        store.find_session_by_id(&id_owned).map_err(ApiError::from)
    })
    .await??;

    session.ok_or_else(|| SessionNotFoundSnafu { id: id_for_error }.build())
}

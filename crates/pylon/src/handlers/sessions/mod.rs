//! Session management and message streaming handlers.

// WHY: pub(crate) so utoipa-generated `__path_*` types are visible to the OpenAPI derive.
pub(crate) mod streaming;
mod types;

pub use streaming::{events, send_message, stream_turn};
pub use types::*;

use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use tracing::{info, instrument};

use aletheia_mneme::types::SessionStatus;

use crate::error::{
    ApiError, BadRequestSnafu, ConflictSnafu, ErrorResponse, NousNotFoundSnafu,
    SessionNotFoundSnafu,
};
use crate::extract::Claims;
use crate::state::AppState;

/// POST /api/v1/sessions: create a new session.
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
#[instrument(skip(state, _claims, body))]
pub async fn create(
    State(state): State<Arc<AppState>>,
    _claims: Claims,
    Json(body): Json<CreateSessionRequest>,
) -> Result<impl IntoResponse, ApiError> {
    let nous_id = body.nous_id;
    let session_key = body.session_key;

    if nous_id.is_empty() {
        return Err(BadRequestSnafu {
            message: "nous_id must not be empty",
        }
        .build());
    }
    if session_key.is_empty() {
        return Err(BadRequestSnafu {
            message: "session_key must not be empty",
        }
        .build());
    }

    let config = state.nous_manager.get_config(&nous_id).ok_or_else(|| {
        NousNotFoundSnafu {
            id: nous_id.clone(),
        }
        .build()
    })?;

    let id = ulid::Ulid::new().to_string();
    let model = config.model.clone();

    let state_clone = Arc::clone(&state);
    let id_clone = id.clone();
    let nid = nous_id.clone();
    let skey = session_key.clone();

    let session = tokio::task::spawn_blocking(move || {
        let store = state_clone.session_store.blocking_lock();
        // WHY: use create_session (not find_or_create) so that any existing session
        // with this (nous_id, session_key) pair (active or archived) produces a
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
#[utoipa::path(
    get,
    path = "/api/v1/sessions",
    params(
        ("nous_id" = Option<String>, Query, description = "Filter sessions by agent ID"),
        ("limit" = Option<u32>, Query, description = "Maximum number of sessions to return"),
    ),
    responses(
        (status = 200, description = "Session list", body = ListSessionsResponse),
        (status = 401, description = "Unauthorized", body = ErrorResponse),
    ),
    security(("bearer_auth" = []))
)]
#[instrument(skip(state, _claims))]
pub async fn list_sessions(
    State(state): State<Arc<AppState>>,
    _claims: Claims,
    Query(params): Query<ListSessionsParams>,
) -> Result<Json<ListSessionsResponse>, ApiError> {
    let nous_id = params.nous_id;
    let limit = params.limit;

    let state_clone = Arc::clone(&state);
    let sessions = tokio::task::spawn_blocking(move || {
        let store = state_clone.session_store.blocking_lock();
        store
            .list_sessions(nous_id.as_deref())
            .map_err(ApiError::from)
    })
    .await??;

    // WHY: the store does not accept a LIMIT parameter; apply the cap in-process
    // after retrieval. Datasets are small enough that this is not a bottleneck (#1254).
    let mut sessions = sessions;
    if let Some(n) = limit {
        sessions.truncate(usize::try_from(n).unwrap_or(usize::MAX));
    }

    let items = sessions
        .into_iter()
        .map(|s| SessionListItem {
            id: s.id,
            nous_id: s.nous_id,
            session_key: s.session_key,
            status: s.status.as_str().to_owned(),
            message_count: s.message_count,
            updated_at: s.updated_at,
            display_name: s.display_name,
        })
        .collect();

    Ok(Json(ListSessionsResponse { sessions: items }))
}

/// GET /api/v1/sessions/{id}: get session state.
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
#[instrument(skip(state, _claims))]
pub async fn get_session(
    State(state): State<Arc<AppState>>,
    _claims: Claims,
    Path(id): Path<String>,
) -> Result<Json<SessionResponse>, ApiError> {
    let session = find_session(&state, &id).await?;
    // WHY: DELETE archives the session. Archived sessions are non-retrievable via GET
    // so that DELETE has the expected "resource gone" semantics (#1251).
    if session.status != SessionStatus::Active {
        return Err(SessionNotFoundSnafu { id }.build());
    }
    Ok(Json(SessionResponse::from_mneme(&session)))
}

/// DELETE /api/v1/sessions/{id}: close (archive) a session.
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
#[instrument(skip(state, _claims))]
pub async fn close(
    State(state): State<Arc<AppState>>,
    _claims: Claims,
    Path(id): Path<String>,
) -> Result<StatusCode, ApiError> {
    archive_session_by_id(&state, &id).await
}

/// POST /api/v1/sessions/{id}/archive: archive a session.
///
/// Same behavior as DELETE but via POST, matching the TUI's API contract.
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
#[instrument(skip(state, _claims))]
pub async fn archive(
    State(state): State<Arc<AppState>>,
    _claims: Claims,
    Path(id): Path<String>,
) -> Result<StatusCode, ApiError> {
    archive_session_by_id(&state, &id).await
}

/// Shared archive logic for both DELETE and POST archive routes.
async fn archive_session_by_id(state: &Arc<AppState>, id: &str) -> Result<StatusCode, ApiError> {
    let _ = find_session(state, id).await?;

    let state_clone = Arc::clone(state);
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
#[instrument(skip(state, _claims))]
pub async fn unarchive(
    State(state): State<Arc<AppState>>,
    _claims: Claims,
    Path(id): Path<String>,
) -> Result<StatusCode, ApiError> {
    let _ = find_session(&state, &id).await?;

    let state_clone = Arc::clone(&state);
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
#[instrument(skip(state, _claims, body))]
pub async fn rename(
    State(state): State<Arc<AppState>>,
    _claims: Claims,
    Path(id): Path<String>,
    Json(body): Json<RenameSessionRequest>,
) -> Result<StatusCode, ApiError> {
    let _ = find_session(&state, &id).await?;

    if body.name.is_empty() {
        return Err(BadRequestSnafu {
            message: "name must not be empty",
        }
        .build());
    }

    let state_clone = Arc::clone(&state);
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

/// Maximum number of messages returnable per history request.
const MAX_HISTORY_LIMIT: u32 = 1000;
/// Default number of messages when no limit is supplied.
const DEFAULT_HISTORY_LIMIT: u32 = 50;

/// GET /api/v1/sessions/{id}/history: get conversation history.
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
#[instrument(skip(state, _claims))]
pub async fn history(
    State(state): State<Arc<AppState>>,
    _claims: Claims,
    Path(id): Path<String>,
    Query(params): Query<HistoryParams>,
) -> Result<Json<HistoryResponse>, ApiError> {
    let _ = find_session(&state, &id).await?;

    // Cap limit at MAX_HISTORY_LIMIT and apply a sensible default so a single
    // request cannot fetch an unbounded number of messages.
    let limit = params
        .limit
        .unwrap_or(DEFAULT_HISTORY_LIMIT)
        .min(MAX_HISTORY_LIMIT);
    let before_seq = params.before;

    let state_clone = Arc::clone(&state);
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
    state: &Arc<AppState>,
    agent_id: &str,
    session_key: &str,
    model: Option<&str>,
) -> Result<String, ApiError> {
    let id = ulid::Ulid::new().to_string();
    let state_clone = Arc::clone(state);
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
                        location: snafu::Location::default(),
                    })
            }
            Err(e) => Err(ApiError::from(e)),
        }
    })
    .await??;

    Ok(session.id)
}

/// Returns `true` when a mneme error is a `SQLite` UNIQUE constraint violation.
///
/// `SQLite` always includes "UNIQUE constraint failed" in the error message for
/// constraint violations. We match on the string because pylon does not take a
/// direct rusqlite dependency: the type lives inside mneme's `Database` variant.
fn is_unique_constraint_violation(err: &aletheia_mneme::error::Error) -> bool {
    err.to_string().contains("UNIQUE constraint failed")
}

pub(crate) async fn find_session(
    state: &Arc<AppState>,
    id: &str,
) -> Result<aletheia_mneme::types::Session, ApiError> {
    let state_clone = Arc::clone(state);
    let id_owned = id.to_owned();
    let id_for_error = id.to_owned();
    let session = tokio::task::spawn_blocking(move || {
        let store = state_clone.session_store.blocking_lock();
        store.find_session_by_id(&id_owned).map_err(ApiError::from)
    })
    .await??;

    session.ok_or_else(|| SessionNotFoundSnafu { id: id_for_error }.build())
}

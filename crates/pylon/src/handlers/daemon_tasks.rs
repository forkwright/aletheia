//! Daemon-task admin endpoints: list, enable, disable, retry a registered
//! daemon task. (#7206)
//!
//! Every registered daemon task (`oikonomos::maintenance::registry`, plus
//! per-agent prosoche/self-prompt tasks) is background, non-serving-path
//! work -- see `crate::handlers::health`'s `subsystem_daemon_runtime` for how
//! a disabled task now degrades `system/status` rather than failing it.
//! Before these routes existed, the only way to inspect or recover a
//! disabled task was to hand-edit its persisted
//! `oikonomos::state::TaskStateStore` record (via the `aletheia maintenance
//! reset` CLI, or by hand) and wait for -- or force -- a daemon restart.
//!
//! These handlers write directly to the same `TaskStateStore` handle the
//! live `TaskRunner` reads through (`AppState::daemon_task_states`, #5142):
//! the store is the only channel that crosses the process boundary between
//! this API and the daemon's live scheduler loop. The running
//! `TaskRunner::sync_external_state` (`oikonomos::runner::persistence`)
//! picks up a write made here within its sync interval, without a restart.

use axum::Json;
use axum::extract::{Path, State};

use oikonomos::state::{DisableCause, TASK_STATE_SCHEMA_VERSION, TaskState, TaskStateStore};
use symbolon::types::Role;

use crate::error::{ApiError, NotFoundSnafu};
use crate::extract::{Claims, require_role};
use crate::state::HealthState;

#[path = "daemon_tasks_dto.rs"]
mod daemon_tasks_dto;
pub use daemon_tasks_dto::{DaemonTaskEntry, DaemonTaskListResponse, DisableDaemonTaskRequest};

/// `GET /api/v1/system/daemon/tasks`: list every daemon task with persisted
/// execution history, across every attached runner.
///
/// # Cancel safety
///
/// Cancel-safe. Axum handler; cancellation drops the future with no side
/// effects beyond not returning a response.
#[utoipa::path(
    get,
    path = "/api/v1/system/daemon/tasks",
    responses(
        (status = 200, description = "Daemon task list", body = DaemonTaskListResponse),
        (status = 401, description = "Unauthorized", body = crate::error::ErrorResponse),
        (status = 403, description = "Forbidden", body = crate::error::ErrorResponse),
    ),
    security(("bearer_auth" = []))
)]
pub async fn list_tasks(
    State(state): State<HealthState>,
    claims: Claims,
) -> Result<Json<DaemonTaskListResponse>, ApiError> {
    require_role(&claims, Role::Operator)?;

    let mut tasks = Vec::new();
    for (runner, store) in state.daemon_task_states.iter() {
        let states = store.load_all().map_err(|e| {
            crate::error::InternalSnafu {
                message: format!("failed to read daemon task state for runner '{runner}': {e}"),
            }
            .build()
        })?;
        tasks.extend(
            states
                .into_iter()
                .map(|saved| entry_from_state(runner.clone(), saved)),
        );
    }
    // WHY: stable order for a human-facing listing and deterministic tests --
    // `load_all` iterates fjall key order per store, but stores themselves
    // are visited in `daemon_task_states`' construction order.
    tasks.sort_by(|a, b| (&a.runner, &a.task_id).cmp(&(&b.runner, &b.task_id)));

    Ok(Json(DaemonTaskListResponse { tasks }))
}

/// `POST /api/v1/system/daemon/tasks/{runner}/{task_id}/enable`: fully
/// re-enable a task, resetting its failure history.
///
/// Equivalent to the `aletheia maintenance reset` CLI command, exposed over
/// HTTP so agents and humans share one capability through one route (#7206).
///
/// # Cancel safety
///
/// Cancel-safe. Axum handler; cancellation drops the future with no side
/// effects beyond not returning a response. `TaskStateStore::save` either
/// completes its fjall commit+fsync or returns an error -- it never leaves a
/// half-written record.
#[utoipa::path(
    post,
    path = "/api/v1/system/daemon/tasks/{runner}/{task_id}/enable",
    params(
        ("runner" = String, Path, description = "Daemon runner: \"system\" or an agent id"),
        ("task_id" = String, Path, description = "Task identifier"),
    ),
    responses(
        (status = 200, description = "Task enabled", body = DaemonTaskEntry),
        (status = 401, description = "Unauthorized", body = crate::error::ErrorResponse),
        (status = 403, description = "Forbidden", body = crate::error::ErrorResponse),
        (status = 404, description = "Unknown runner or task", body = crate::error::ErrorResponse),
    ),
    security(("bearer_auth" = []))
)]
pub async fn enable_task(
    State(state): State<HealthState>,
    claims: Claims,
    Path((runner, task_id)): Path<(String, String)>,
) -> Result<Json<DaemonTaskEntry>, ApiError> {
    require_role(&claims, Role::Operator)?;
    let store = find_store(&state, &runner)?;
    let mut saved = load_or_default(store, &runner, &task_id)?;

    saved.enabled = Some(true);
    saved.disable_cause = None;
    saved.consecutive_failures = 0;
    saved.backoff_until_ts = None;
    saved.last_error = None;
    saved.schema_version = TASK_STATE_SCHEMA_VERSION;
    persist(store, &saved)?;

    Ok(Json(entry_from_state(runner, saved)))
}

/// `POST /api/v1/system/daemon/tasks/{runner}/{task_id}/disable`: explicitly
/// disable a task.
///
/// Persists `disable_cause: Operator` (#7206), which -- unlike an
/// auto-disable -- is never re-armed on hydration; only another call to this
/// route family's `enable` (or `retry`) re-enables it. This is the intended
/// replacement for hand-editing persisted state to take a noisy or
/// misbehaving task out of rotation on purpose.
///
/// # Cancel safety
///
/// Cancel-safe. Axum handler; cancellation drops the future with no side
/// effects beyond not returning a response.
#[utoipa::path(
    post,
    path = "/api/v1/system/daemon/tasks/{runner}/{task_id}/disable",
    params(
        ("runner" = String, Path, description = "Daemon runner: \"system\" or an agent id"),
        ("task_id" = String, Path, description = "Task identifier"),
    ),
    request_body = DisableDaemonTaskRequest,
    responses(
        (status = 200, description = "Task disabled", body = DaemonTaskEntry),
        (status = 401, description = "Unauthorized", body = crate::error::ErrorResponse),
        (status = 403, description = "Forbidden", body = crate::error::ErrorResponse),
        (status = 404, description = "Unknown runner or task", body = crate::error::ErrorResponse),
    ),
    security(("bearer_auth" = []))
)]
pub async fn disable_task(
    State(state): State<HealthState>,
    claims: Claims,
    Path((runner, task_id)): Path<(String, String)>,
    Json(body): Json<DisableDaemonTaskRequest>,
) -> Result<Json<DaemonTaskEntry>, ApiError> {
    require_role(&claims, Role::Operator)?;
    let store = find_store(&state, &runner)?;
    let mut saved = load_or_default(store, &runner, &task_id)?;

    saved.enabled = Some(false);
    saved.disable_cause = Some(DisableCause::Operator);
    if let Some(reason) = body.reason {
        saved.last_error = Some(reason);
    }
    saved.schema_version = TASK_STATE_SCHEMA_VERSION;
    persist(store, &saved)?;

    Ok(Json(entry_from_state(runner, saved)))
}

/// `POST /api/v1/system/daemon/tasks/{runner}/{task_id}/retry`: give a
/// disabled task exactly one more attempt now, without resetting its
/// failure history.
///
/// Unlike `enable`, `consecutive_failures` is left untouched: if the retry
/// also fails, `record_task_failure`'s existing "3 consecutive failures"
/// check trips on the very next failure (since the count is already at or
/// above 3), so a still-broken task re-disables after one visible attempt
/// instead of getting three fresh strikes. This mirrors the automatic
/// hydration retry `oikonomos::runner::persistence::apply_saved_state`
/// performs for an `auto_failure`-caused disable on every daemon restart --
/// this route is that same retry, available on demand instead of waiting for
/// one.
///
/// # Cancel safety
///
/// Cancel-safe. Axum handler; cancellation drops the future with no side
/// effects beyond not returning a response.
#[utoipa::path(
    post,
    path = "/api/v1/system/daemon/tasks/{runner}/{task_id}/retry",
    params(
        ("runner" = String, Path, description = "Daemon runner: \"system\" or an agent id"),
        ("task_id" = String, Path, description = "Task identifier"),
    ),
    responses(
        (status = 200, description = "Task re-armed for one retry", body = DaemonTaskEntry),
        (status = 401, description = "Unauthorized", body = crate::error::ErrorResponse),
        (status = 403, description = "Forbidden", body = crate::error::ErrorResponse),
        (status = 404, description = "Unknown runner or task", body = crate::error::ErrorResponse),
    ),
    security(("bearer_auth" = []))
)]
pub async fn retry_task(
    State(state): State<HealthState>,
    claims: Claims,
    Path((runner, task_id)): Path<(String, String)>,
) -> Result<Json<DaemonTaskEntry>, ApiError> {
    require_role(&claims, Role::Operator)?;
    let store = find_store(&state, &runner)?;
    let mut saved = load_or_default(store, &runner, &task_id)?;

    saved.enabled = Some(true);
    saved.disable_cause = None;
    saved.backoff_until_ts = None;
    saved.schema_version = TASK_STATE_SCHEMA_VERSION;
    persist(store, &saved)?;

    Ok(Json(entry_from_state(runner, saved)))
}

/// Find the attached [`TaskStateStore`] for `runner`.
///
/// # Errors
///
/// Returns [`ApiError::NotFound`] when `runner` is not one of this
/// instance's attached daemon runners (`"system"` or a configured agent id).
fn find_store<'a>(state: &'a HealthState, runner: &str) -> Result<&'a TaskStateStore, ApiError> {
    state
        .daemon_task_states
        .iter()
        .find(|(component, _)| component == runner)
        .map(|(_, store)| store)
        .ok_or_else(|| {
            NotFoundSnafu {
                path: format!("daemon runner '{runner}'"),
            }
            .build()
        })
}

/// Load a task's persisted state, or -- for a `"system"`-runner task known to
/// the maintenance registry that has simply never run yet -- a fresh enabled
/// default.
///
/// WHY scoped to `runner == "system"`: the maintenance registry
/// (`oikonomos::maintenance::registry`) only enumerates the shared scheduler's
/// tasks. A non-`"system"` runner's tasks (per-agent prosoche/self-prompt)
/// have no static registry to validate against, so admitting an unknown id
/// there would let a typo silently create a phantom record in the wrong
/// runner's store; requiring an existing persisted record is the safe
/// default until such a task has actually run once.
///
/// # Errors
///
/// Returns [`ApiError::Internal`] if the store cannot be read, or
/// [`ApiError::NotFound`] when `task_id` is neither persisted nor (for
/// `"system"`) a known registry task.
fn load_or_default(
    store: &TaskStateStore,
    runner: &str,
    task_id: &str,
) -> Result<TaskState, ApiError> {
    let states = store.load_all().map_err(|e| {
        crate::error::InternalSnafu {
            message: format!("failed to read daemon task state for runner '{runner}': {e}"),
        }
        .build()
    })?;
    if let Some(existing) = states.into_iter().find(|s| s.task_id == task_id) {
        return Ok(existing);
    }
    if runner == "system" && oikonomos::maintenance::maintenance_task_by_id(task_id).is_some() {
        return Ok(TaskState {
            task_id: task_id.to_owned(),
            enabled: Some(true),
            schema_version: TASK_STATE_SCHEMA_VERSION,
            ..TaskState::default()
        });
    }
    Err(NotFoundSnafu {
        path: format!("daemon task '{task_id}' on runner '{runner}'"),
    }
    .build())
}

/// Persist `saved`, mapping a store write failure to [`ApiError::Internal`].
fn persist(store: &TaskStateStore, saved: &TaskState) -> Result<(), ApiError> {
    store.save(saved).map_err(|e| {
        crate::error::InternalSnafu {
            message: format!(
                "failed to persist daemon task state for '{}': {e}",
                saved.task_id
            ),
        }
        .build()
    })
}

/// Build a wire [`DaemonTaskEntry`] from a persisted [`TaskState`].
fn entry_from_state(runner: String, saved: TaskState) -> DaemonTaskEntry {
    let enabled = saved.enabled.unwrap_or(true);
    // WHY(#7206): a legacy disabled record predates `disable_cause` and was,
    // by construction, always an auto-disable -- matching the same
    // ambiguity-resolution `oikonomos::runner::persistence::apply_saved_state`
    // and `crate::handlers::health::summarize_daemon_task_states` apply.
    let cause = (!enabled).then(|| {
        crate::handlers::health::disable_cause_label(
            saved.disable_cause.unwrap_or(DisableCause::AutoFailure),
        )
        .to_owned()
    });
    let name = oikonomos::maintenance::maintenance_task_by_id(&saved.task_id)
        .map_or_else(|| saved.task_id.clone(), |def| def.name().to_owned());

    DaemonTaskEntry {
        runner,
        task_id: saved.task_id,
        name,
        enabled,
        cause,
        consecutive_failures: saved.consecutive_failures,
        last_error: saved.last_error,
        last_outcome: saved.last_outcome,
        last_run: saved.last_run_ts,
        backoff_until: saved.backoff_until_ts,
    }
}

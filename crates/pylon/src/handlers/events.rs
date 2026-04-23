//! Domain event subscription handlers.

use std::convert::Infallible;
use std::time::Duration;

use axum::Json;
use axum::extract::{Query, State};
use axum::response::IntoResponse;
use axum::response::sse::{Event, KeepAlive, Sse};
use serde::Deserialize;
use tokio_stream::StreamExt;
use tokio_stream::wrappers::BroadcastStream;
use tokio_stream::wrappers::errors::BroadcastStreamRecvError;
use tracing::{instrument, warn};

use crate::error::ApiError;
use crate::event_bus::DomainEvent;
use crate::extract::Claims;
use crate::state::EventBusState;

/// Query parameters for the event subscription endpoint.
#[derive(Debug, Deserialize)]
pub struct SubscribeParams {
    /// Comma-separated list of topics to subscribe to (e.g. `fact.created,turn.complete`).
    pub topics: String,
}

/// GET /api/v1/events/subscribe
///
/// Opens an SSE stream filtered to the requested topics.
/// Each event is delivered as `event: <topic>\ndata: <json>\n\n`.
/// Periodic heartbeat comments keep the connection alive.
#[utoipa::path(
    get,
    path = "/api/v1/events/subscribe",
    params(
        ("topics" = String, Query, description = "Comma-separated topic filter"),
    ),
    responses(
        (status = 200, description = "Filtered SSE event stream", content_type = "text/event-stream"),
        (status = 401, description = "Unauthorized", body = crate::error::ErrorResponse),
    ),
    security(("bearer_auth" = []))
)]
/// # Cancel safety
///
/// Cancel-safe. Axum handler; cancellation drops the future with no
/// side effects beyond not returning a response.
#[instrument(skip(state, _claims))]
pub async fn subscribe(
    State(state): State<EventBusState>,
    _claims: Claims,
    Query(params): Query<SubscribeParams>,
) -> Result<Sse<impl tokio_stream::Stream<Item = Result<Event, Infallible>>>, ApiError> {
    let topics: Vec<String> = params
        .topics
        .split(',')
        .map(|s| s.trim().to_owned())
        .filter(|s| !s.is_empty())
        .collect();

    if topics.is_empty() {
        return Err(ApiError::BadRequest {
            message: "topics query parameter must contain at least one topic".to_owned(),
            location: snafu::location!(),
        });
    }

    let heartbeat_secs = state
        .config
        .read()
        .await
        .gateway
        .sse_heartbeat_interval_secs;
    let rx = state.event_bus.subscribe();
    let subscriber_id = koina::ulid::Ulid::new().to_string();

    let stream = BroadcastStream::new(rx).filter_map(move |result| match result {
        Ok(DomainEvent { topic, payload, .. }) if topics.contains(&topic) => {
            match serde_json::to_string(&payload) {
                Ok(data) => Some(Ok(Event::default().event(&topic).data(data))),
                Err(e) => {
                    warn!(error = %e, topic, "failed to serialize domain event payload");
                    Some(Ok(Event::default()
                        .event(&topic)
                        .data(r#"{"error":"serialization failed"}"#)))
                }
            }
        }
        Ok(_) => None,
        Err(BroadcastStreamRecvError::Lagged(n)) => {
            warn!(subscriber_id = %subscriber_id, lagged_by = n, "event subscriber lagged");
            None
        }
    });

    Ok(Sse::new(stream).keep_alive(
        KeepAlive::new()
            .interval(Duration::from_secs(heartbeat_secs))
            .text("heartbeat"),
    ))
}

/// GET /api/v1/events/discovery
///
/// Returns the list of available event topics.  This is a static discovery
/// endpoint; dynamic topic registration is not yet supported.
#[utoipa::path(
    get,
    path = "/api/v1/events/discovery",
    responses(
        (status = 200, description = "Available event topics"),
        (status = 401, description = "Unauthorized", body = crate::error::ErrorResponse),
    ),
    security(("bearer_auth" = []))
)]
#[instrument(skip(_claims))]
pub async fn discovery(_claims: Claims) -> impl IntoResponse {
    let topics = vec![
        "fact.created",
        "turn.complete",
        "session.started",
        "session.ended",
    ];
    Json(topics)
}

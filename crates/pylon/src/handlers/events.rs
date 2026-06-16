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
use crate::event_bus::{DISCOVERABLE_TOPICS, DomainEvent};
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
#[instrument(skip(state, claims))]
pub async fn subscribe(
    State(state): State<EventBusState>,
    claims: Claims,
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

    // SECURITY(#5341, #4994, #4617): Scoped tokens may only subscribe to
    // events for their own nous_id. Unscoped Operator/Admin tokens retain
    // full firehose access. This makes the event bus respect the same
    // least-privilege boundary as direct agent/session APIs.
    let scoped_nous_id: Option<String> = claims.nous_id.clone();

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
            // SECURITY(#5341, #4994, #4617): For scoped tokens, filter events
            // to those whose payload carries a matching nous_id. Events without
            // a nous_id field are cross-agent events; withhold from scoped tokens.
            if let Some(ref scoped) = scoped_nous_id {
                let event_nous_id = payload.get("nous_id").and_then(|v| v.as_str());
                if event_nous_id != Some(scoped.as_str()) {
                    return None;
                }
            }
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
            // WHY: Silently swallowing lag would let clients believe they saw
            // every event. Emit a typed control event carrying the dropped
            // count so the loss is observable and recoverable upstream.
            warn!(
                subscriber_id = %subscriber_id,
                dropped = n,
                "event subscriber lagged; surfacing loss to client"
            );
            let data = serde_json::json!({"dropped": n});
            let data =
                serde_json::to_string(&data).unwrap_or_else(|_| "{\"dropped\":0}".to_owned());
            Some(Ok(Event::default().event("stream_lagged").data(data)))
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
    // WHY: Discovery must only advertise topics that have a current pylon
    // publisher; the canonical list lives in `event_bus_dto` so it is shared
    // with tests and cannot drift from the handlers that emit events.
    Json(DISCOVERABLE_TOPICS)
}

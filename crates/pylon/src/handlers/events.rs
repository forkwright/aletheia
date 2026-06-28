//! Domain event subscription handlers.

use std::convert::Infallible;
use std::time::Duration;

use axum::Json;
use axum::extract::{Query, State};
use axum::http::HeaderMap;
use axum::response::IntoResponse;
use axum::response::sse::{Event, KeepAlive, Sse};
use futures::stream::{self, StreamExt};
use serde::Deserialize;
use tokio_stream::wrappers::BroadcastStream;
use tokio_stream::wrappers::errors::BroadcastStreamRecvError;
use tracing::{instrument, warn};

use crate::error::ApiError;
use crate::event_bus::{DomainEvent, EventDiscoveryResponse, discoverable_topic_descriptors};
use crate::extract::Claims;
use crate::state::EventBusState;

/// Query parameters for the event subscription endpoint.
#[derive(Debug, Deserialize)]
pub struct SubscribeParams {
    /// Comma-separated list of topics to subscribe to (e.g. `fact.created,turn.complete`).
    pub topics: String,
    /// Optional SSE `Last-Event-ID` value supplied as a query parameter.
    ///
    /// WHY(#4910): SSE clients send `Last-Event-ID` automatically on reconnect.
    /// Some test/proxy setups cannot set headers on an `EventSource`, so the
    /// query parameter is accepted as a fallback. The header wins when both are
    /// present.
    pub last_event_id: Option<String>,
}

/// SSE event name for unrecoverable reconnect gaps.
const STREAM_GAP_EVENT: &str = "stream_gap";

/// SSE event name for live subscriber lag.
const STREAM_LAGGED_EVENT: &str = "stream_lagged";

/// Parse a `Last-Event-ID` value from the `Last-Event-ID` header or the
/// `last_event_id` query parameter.
fn parse_last_event_id(headers: &HeaderMap, query: &SubscribeParams) -> Option<u64> {
    if let Some(value) = headers.get("Last-Event-ID").and_then(|v| v.to_str().ok()) {
        return value.parse().ok();
    }
    query.last_event_id.as_ref()?.parse().ok()
}

/// Filter and serialize a domain event into an SSE [`Event`].
///
/// Returns `None` when the event does not match the requested topics or scoped
/// visibility.
fn domain_event_to_sse(
    event: &DomainEvent,
    topics: &[String],
    scoped_nous_id: Option<&str>,
) -> Option<Result<Event, Infallible>> {
    if !topics.contains(&event.topic) {
        return None;
    }

    // SECURITY(#5341, #4994, #4617): For scoped tokens, filter events to those
    // whose payload carries a matching nous_id. Events without a nous_id field
    // are cross-agent events; withhold from scoped tokens.
    if let Some(scoped) = scoped_nous_id {
        let event_nous_id = event.payload.get("nous_id").and_then(|v| v.as_str());
        if event_nous_id != Some(scoped) {
            return None;
        }
    }

    let id = event.id.to_string();
    match serde_json::to_string(event) {
        Ok(data) => Some(Ok(Event::default().event(&event.topic).id(id).data(data))),
        Err(e) => {
            warn!(error = %e, topic = %event.topic, "failed to serialize domain event");
            Some(Ok(Event::default()
                .event(&event.topic)
                .id(id)
                .data(r#"{"error":"serialization failed"}"#)))
        }
    }
}

/// Build a gap SSE event carrying the range of missed ids.
#[expect(
    clippy::unnecessary_wraps,
    reason = "return type must match stream item type Result<Event, Infallible>"
)]
fn gap_event(first_missed: u64, last_missed: u64, scoped: bool) -> Result<Event, Infallible> {
    // SECURITY(#5341, #4994, #4617): The missed-id range spans every event that
    // fell out of the journal, including cross-agent events a scoped token must
    // never observe. Leaking the raw `(first, last)` range to a scoped token
    // discloses the existence and volume of other agents' activity. Scoped
    // tokens therefore receive an empty gap object `{}` — the loss is still
    // signalled, but the cross-agent id range is withheld. Only unscoped
    // Operator/Admin tokens (which already see all events) receive the range.
    let data = if scoped {
        serde_json::json!({})
    } else {
        serde_json::json!({
            "first_missed_id": first_missed,
            "last_missed_id": last_missed,
        })
    };
    let data = serde_json::to_string(&data).unwrap_or_else(|_| "{}".to_owned());
    Ok(Event::default().event(STREAM_GAP_EVENT).data(data))
}

/// GET /api/v1/events/subscribe
///
/// Opens an SSE stream filtered to the requested topics.
/// Each event is delivered as `event: <topic>\nid: <id>\ndata: <json>\n\n`.
/// Periodic heartbeat comments keep the connection alive.
///
/// WHY(#4910): Reconnects with `Last-Event-ID` replay retained events from the
/// in-memory journal. If the requested id has fallen out of the journal, a
/// typed `stream_gap` event is emitted so the client knows it missed events.
#[utoipa::path(
    get,
    path = "/api/v1/events/subscribe",
    params(
        ("topics" = String, Query, description = "Comma-separated topic filter"),
        ("last_event_id" = Option<String>, Query, description = "Optional Last-Event-ID for reconnect replay"),
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
#[instrument(skip(state, claims, headers))]
pub async fn subscribe(
    State(state): State<EventBusState>,
    claims: Claims,
    Query(params): Query<SubscribeParams>,
    headers: HeaderMap,
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
    let subscriber_id = koina::ulid::Ulid::new().to_string();

    let last_event_id = parse_last_event_id(&headers, &params).unwrap_or(0);
    let (replay, has_gap, gap_first, gap_last, rx) = if last_event_id > 0 {
        let (snapshot, rx) = state.event_bus.subscribe_from(last_event_id).await;
        if snapshot.has_gap {
            warn!(
                subscriber_id = %subscriber_id,
                first_missed = snapshot.gap_first_missed_id,
                last_missed = snapshot.gap_last_missed_id,
                "event subscriber reconnect missed events that fell out of journal"
            );
        }
        (
            snapshot.replay,
            snapshot.has_gap,
            snapshot.gap_first_missed_id,
            snapshot.gap_last_missed_id,
            rx,
        )
    } else {
        (Vec::new(), false, 0, 0, state.event_bus.subscribe())
    };

    // WHY(#4910): Pre-materialize the replay iterator so each closure below
    // owns its copy of the filter state. This avoids borrowing local variables
    // that would outlive the handler future.
    let max_replayed_id = replay.iter().map(|e| e.id).max().unwrap_or(last_event_id);
    let topics_for_replay = topics.clone();
    let scoped_for_replay = scoped_nous_id.clone();
    let replay_stream = stream::iter(replay.into_iter().filter_map(move |event| {
        domain_event_to_sse(&event, &topics_for_replay, scoped_for_replay.as_deref())
    }));

    let gap_stream = if has_gap {
        Some(stream::iter(std::iter::once(gap_event(
            gap_first,
            gap_last,
            scoped_nous_id.is_some(),
        ))))
    } else {
        None
    };

    // WHY(#4910): Skip live events whose id is not greater than the newest
    // replayed id. The broadcast receiver starts at the channel's current tail,
    // which may overlap with the journal replay; de-duplicating by id prevents
    // emitting the same event twice.
    let live_stream = BroadcastStream::new(rx).filter_map(move |result| {
        let item = match result {
            Ok(event) if event.id > max_replayed_id => {
                domain_event_to_sse(&event, &topics, scoped_nous_id.as_deref())
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
                Some(Ok(Event::default().event(STREAM_LAGGED_EVENT).data(data)))
            }
        };
        std::future::ready(item)
    });

    // WHY(#4910): Stream order is: gap event (if any), replay events, live
    // events. This lets a reconnecting client learn about unrecoverable loss
    // before receiving the retained tail of the stream.
    let stream: futures::stream::BoxStream<'static, Result<Event, Infallible>> =
        if let Some(gap) = gap_stream {
            gap.chain(replay_stream).chain(live_stream).boxed()
        } else {
            replay_stream.chain(live_stream).boxed()
        };

    Ok(Sse::new(stream).keep_alive(
        KeepAlive::new()
            .interval(Duration::from_secs(heartbeat_secs))
            .text("heartbeat"),
    ))
}

/// GET /api/v1/events/discovery
///
/// Returns available event topics and payload contracts.  This is a static
/// discovery endpoint; dynamic topic registration is not yet supported.
#[utoipa::path(
    get,
    path = "/api/v1/events/discovery",
    responses(
        (status = 200, description = "Available event topics", body = EventDiscoveryResponse),
        (status = 401, description = "Unauthorized", body = crate::error::ErrorResponse),
    ),
    security(("bearer_auth" = []))
)]
// kanon:ignore API/unused-auth-param — extractor enforces authentication; no per-claim RBAC needed for topic discovery
#[instrument(skip(_claims))]
pub async fn discovery(_claims: Claims) -> impl IntoResponse {
    // WHY: Discovery must only advertise topics that have a current pylon
    // publisher; the canonical list lives in `event_bus_dto` so it is shared
    // with tests and cannot drift from the handlers that emit events.
    Json(discoverable_topic_descriptors())
}

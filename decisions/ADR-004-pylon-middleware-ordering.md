# ADR-004: pylon middleware ordering

## Status

Proposed

## Context

Pylon is aletheia's Axum HTTP gateway. It combines versioned routes, session and knowledge handlers, SSE streams, OpenAPI, metrics, auth extraction, rate limiting, request ids, tracing, compression, error shaping, CORS, and security headers. Middleware order is architectural because it decides which requests consume quota, which failures receive request ids, what spans observe, and which responses carry security and rate-limit headers.

The canary issue summarizes the policy as auth, rate-limit, trace, handler. The implementation is more precise: pylon's global Axum stack is documented outermost-first as security headers, CORS, request id, trace, compression, metrics, error enrichment, body limit, CSRF, per-IP rate limit, per-user rate limit, then JWT claims and handler execution. Knowledge routes also have a route-layer bearer-auth wrapper. The ADR therefore records the effective code contract and its gotchas rather than implying a single `tower::ServiceBuilder` sequence.

The router file states that route and middleware assembly stays together because order is part of correctness:

```text
crates/pylon/src/router.rs:42
// NOTE(#940): 130+ lines: route and middleware layer assembly where ordering matters.
// Extraction would obscure the middleware stack ordering that is critical for correctness.
```

The handler reference describes the current global stack:

```text
crates/pylon/docs/handlers.md:12
Every request traverses the following stack, outermost first. Later sections note any
per-handler exceptions.
```

Extra routes are merged before global layers so the same protections apply to external routers:

```text
crates/pylon/src/router.rs:170
// WHY: Extra routes are merged BEFORE middleware layers so they benefit from
// the same global protections (rate limiting, CSRF, compression, tracing,
// metrics, error enrichment) as pylon's own routes (#3226).
```

The route tree applies subtree auth for knowledge routes and handler-level auth for protected session and API handlers. The reusable auth middleware validates bearer tokens and stores claims so handlers do not re-validate:

```text
crates/pylon/src/router.rs:91
.route_layer(axum::middleware::from_fn_with_state(
    Arc::clone(&state),
    require_bearer_auth,
));
```

```text
crates/pylon/src/middleware/mod.rs:27
/// Middleware that validates bearer auth for an entire router subtree.
///
/// The validated claims are cached in request extensions so handlers that also
/// extract [`Claims`] do not re-validate the same token.
```

Handlers that require authorization also extract claims directly:

```text
crates/pylon/src/handlers/sessions/mod.rs:50
pub async fn create(
    State(state): State<SessionsState>,
    claims: Claims,
    Json(body): Json<CreateSessionRequest>,
```

Rate limiting is applied globally after route construction and before the later response-shaping/observability layers in the builder. The per-user limiter is installed before the per-IP limiter in code, and Axum/Tower layering means later layers wrap earlier layers on the request path. The important policy is that rate limiting happens before expensive handlers and before handler-level business side effects:

```text
crates/pylon/src/router.rs:177
if security.rate_limit.per_user.enabled {
    let user_limiter = Arc::new(UserRateLimiter::new(security.rate_limit.per_user.clone()));
    spawn_stale_cleanup(Arc::clone(&user_limiter), shutdown.clone());
    router = router
        .layer(axum::middleware::from_fn(per_user_rate_limit))
```

```text
crates/pylon/src/router.rs:185
if security.rate_limit.enabled {
    let limiter = Arc::new(
        RateLimiter::new(security.rate_limit.requests_per_minute)
            .with_trust_proxy(security.rate_limit.trust_proxy),
    );
    router = router
        .layer(axum::middleware::from_fn(rate_limit))
```

The rate-limit middleware itself short-circuits with `429` before running the next service:

```text
crates/pylon/src/middleware/rate_limiter.rs:170
let client = extract_client_key(&request, limiter.trust_proxy);
if let Some(retry_after_secs) = limiter.check(&client) {
```

Tracing and request-id propagation are deliberately adjacent. Request IDs must be injected before the trace layer observes the request so the span includes the correlation id:

```text
crates/pylon/src/router.rs:228
router = router.layer(
    TraceLayer::new_for_http()
        .make_span_with(|request: &axum::http::Request<_>| {
```

```text
crates/pylon/src/router.rs:256
// WARNING: Must be before trace layer so the span includes the ID.
router = router.layer(axum::middleware::from_fn(inject_request_id));
```

## Decision

**Pylon preserves the documented Axum middleware order: outer security/CORS/request-id/trace/response layers, then body/CSRF/rate-limit guards, then bearer claims and handler execution, with route-layer auth allowed for protected subtrees such as knowledge routes.**

In practical Axum terms, this is not a single `ServiceBuilder` list; it is a route tree plus route-layer auth, handler claim extractors, conditional rate-limit layers, request-id injection, trace spans, and outer response decorators. The decision is about the policy order that must be preserved when editing `build_router_with`:

1. Request ids and trace spans wrap the guarded work so both successful handlers and middleware-generated failures are observable.
2. Body limits, CSRF, and rate limiting must short-circuit before expensive handlers and side effects. Per-user and per-IP checks remain in middleware, and successful responses should receive quota headers.
3. Bearer authentication must happen before protected handler work. Most protected routes use the `Claims` extractor in the handler signature; selected subtrees can use `require_bearer_auth` as a route layer.
4. Handlers should run only after the applicable quota, request identity, trace context, and auth decisions are in place.

Outer layers have their own invariants. Security headers must wrap every response. CORS must apply broadly. Error enrichment must see uncompressed bodies and should add request ids to error envelopes. Compression and ETag behavior must not hide error bodies from the enrichment layer. These concerns are intentionally documented in the router comments because a visually harmless reorder can change client-visible behavior.

## Consequences

**Positive:**

- **Protected work is guarded.** Unauthorized protected routes fail through the auth extractor or auth route layer before handler logic performs session, knowledge, or config mutations.
- **Quota enforcement is central.** Global and per-user rate limiting do not have to be repeated in handlers, and `429` responses can be emitted consistently with retry and rate-limit headers.
- **Traceability is consistent.** Request ids are available to spans and error envelopes, including failures emitted by CSRF and rate-limit middleware, making logs, client errors, and metrics easier to correlate.
- **External routes inherit the same policy.** Routers merged through `extra` are wrapped by the same global protections instead of becoming an unprotected side entrance.

**Negative:**

- **Axum layer order is non-obvious.** The order in code is not the same as the intuitive request path, because later layers wrap earlier services. Future edits need tests or careful review.
- **Auth has two mechanisms and usually sits inside rate limiting.** Route-layer auth and handler-level `Claims` extraction both exist. That is intentional, but reviewers must verify each protected route uses one of them and applies role/scope checks where needed.
- **Rate limiting before verified claims limits identity precision.** The per-user limiter hashes the bearer token rather than relying on fully verified `Claims`, because claims are normally extracted deeper in handlers. This avoids trusting unsigned payloads, but it means quota keys are token-derived rather than subject-derived at that layer.
- **Response decorators depend on placement.** Moving compression, error enrichment, request ids, or security headers can silently drop request ids from errors, compress bodies before they are normalized, or omit headers from short-circuit responses.

## References

- [forkwright/aletheia#4039](https://github.com/forkwright/aletheia/issues/4039) - ADR canary issue requesting ADR-003.
- `crates/pylon/src/router.rs:42` and `crates/pylon/docs/handlers.md:12` - router construction and documented outermost-first stack.
- `crates/pylon/src/router.rs:170`, `crates/pylon/src/router.rs:177`, `crates/pylon/src/router.rs:185`, `crates/pylon/src/router.rs:228`, `crates/pylon/src/router.rs:256` - route merge, rate-limit, trace, and request-id layer anchors.
- `crates/pylon/src/middleware/mod.rs:27` and `crates/pylon/src/extract.rs:29` - bearer auth route layer and handler claim extraction.
- `crates/pylon/src/middleware/rate_limiter.rs:170` and `crates/pylon/src/middleware/user_rate_limiter.rs:406` - rate-limit short-circuit policy and per-IP ceiling.
- Michael Nygard, "Documenting Architecture Decisions" - lightweight decision record practice used by this ADR.

//! Exhaustive OpenAPI route-role floor walker (#7228).
//!
//! Follow-up to #7200/#7227. Those PRs added a `Role::Agent` read floor to
//! session/knowledge/nous routes with hand-maintained `[&str; N]` route
//! lists per domain (`tests/{knowledge,session,nous}.rs`): a future handler
//! that adds a new read route and forgets `require_role`/`require_read_role`
//! would not appear in any of those lists, and so would not be caught.
//!
//! This module is the single declaration site #7200's fix text asked for:
//! [`route_role_floor_table`] states the role floor every route this server
//! serves must enforce, above whatever `require_bearer_auth`/`Claims`
//! already requires (a present, valid Bearer token -- or the synthetic
//! `auth.mode = "none"` identity). Three checks hold it honest:
//!
//! 1. **Completeness** (`walker_table_covers_every_openapi_route`): every
//!    path+method the generated `OpenAPI` document actually serves has an
//!    entry here. A route with no entry fails loudly instead of silently
//!    passing by omission.
//! 2. **Declared** (`walker_declared_responses_document_403_above_baseline`):
//!    every route whose floor is above the baseline (bearer-only) floor
//!    documents a `403` response in its `OpenAPI` `responses()`.
//! 3. **Enforced** (`walker_enforces_role_floor_below_and_at`): an actual
//!    `oneshot` request one role-tier below the declared floor gets `403`;
//!    the same request at the declared floor does not.
//!
//! A route with no floor beyond the baseline (or, for `/api/health`, no
//! floor at all) still needs an explicit entry -- [`RoleFloor::NoFloor`] or
//! [`RoleFloor::Public`] -- so absence from the table is never read as "no
//! floor"; it is read as "not walked yet", which check 1 rejects.

use axum::http::{Method, StatusCode};
use tower::ServiceExt;

use koina::http::{BEARER_PREFIX, CONTENT_TYPE_JSON};
use symbolon::types::Role;

use super::helpers::*;

/// The minimum role a caller must hold to use a route, beyond whatever the
/// baseline (a present, valid Bearer token, or the synthetic `auth.mode =
/// "none"` identity) already requires.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RoleFloor {
    /// No auth at all. Only `GET /api/health` (and its deprecated alias
    /// `GET /health`) qualify -- every other route requires at least a
    /// present Bearer token via `Claims` or `require_bearer_auth`.
    Public,
    /// The baseline applies (a valid Bearer token, or `auth.mode = "none"`)
    /// but no role above that is checked -- any role passes.
    NoFloor,
    /// The baseline applies AND the caller's role must be at least this.
    Min(Role),
}

/// One route this server registers, and the role floor a contributor
/// asserts for it.
#[derive(Debug, Clone)]
struct RouteSpec {
    method: Method,
    /// Canonical `OpenAPI`/router path template (e.g. `/api/v1/nous/{id}`),
    /// never a concrete path -- lookups against the `OpenAPI` document use
    /// this verbatim; [`concrete_path`] substitutes real path segments only
    /// when building a live request.
    path: &'static str,
    floor: RoleFloor,
    /// Appended verbatim after the concrete path when building a live
    /// request, e.g. `"?q=floor-walker"`. Empty when the route has no
    /// required query parameters.
    query: &'static str,
    /// Raw JSON body sent with `Content-Type: application/json` when
    /// building a live request. `None` for routes with no request body.
    /// Present only when the route's extractor needs a body that parses
    /// (a mandatory `Json<T>`/`Query<T>` field) to ever reach the
    /// `require_role`/`require_read_role` call inside the handler --
    /// otherwise a malformed or missing body would 400 before the role
    /// check runs, and the enforced check could never observe a 403.
    body: Option<&'static str>,
}

impl RouteSpec {
    fn new(method: Method, path: &'static str, floor: RoleFloor) -> Self {
        Self {
            method,
            path,
            floor,
            query: "",
            body: None,
        }
    }

    fn with_query(mut self, query: &'static str) -> Self {
        self.query = query;
        self
    }

    fn with_body(mut self, body: &'static str) -> Self {
        self.body = Some(body);
        self
    }
}

/// Replace every `{param}` path segment with a fixed placeholder, or with
/// `overrides`' value for a param named in it.
///
/// Every handler in this crate resolves `require_role`/`require_read_role`
/// before it does anything with a path parameter's value (see e.g. the
/// pre-existing `knowledge_read_routes_reject_readonly_role` test, which
/// already relies on this for fact/entity ids) -- so which placeholder is
/// used does not matter for most routes, and the default "rwt" is enough.
/// `overrides` exists for the two routes where that is not true: a route
/// whose *extractor* (not its handler body) validates a path segment before
/// `require_role` ever runs (`PUT /api/v1/config/{section}` needs a real
/// section name), and a route whose handler looks up a resource by path
/// param *before* `require_role` (`GET .../sessions/{session_id}/turns/...`
/// needs a session that actually exists) -- see `route_path_overrides`.
fn concrete_path(path: &str, overrides: &[(&str, &str)]) -> String {
    let mut out = String::with_capacity(path.len());
    let mut chars = path.chars();
    while let Some(c) = chars.next() {
        if c == '{' {
            let mut name = String::new();
            for c2 in chars.by_ref() {
                if c2 == '}' {
                    break;
                }
                name.push(c2);
            }
            let value = overrides
                .iter()
                .find(|(k, _)| *k == name)
                .map_or("rwt", |(_, v)| *v);
            out.push_str(value);
        } else {
            out.push(c);
        }
    }
    out
}

/// Path-parameter overrides for the two routes [`concrete_path`]'s doc
/// comment explains -- everything else uses the default placeholder.
fn route_path_overrides<'a>(path: &str, real_session_id: &'a str) -> Vec<(&'static str, &'a str)> {
    match path {
        "/api/v1/config/{section}" => vec![("section", "data")],
        "/api/v1/sessions/{session_id}/turns/{turn_id}/events" => {
            vec![("session_id", real_session_id)]
        }
        _ => Vec::new(),
    }
}

/// The role one tier below `role`, for the "just under the floor" probe.
///
/// `Role` is `#[non_exhaustive]` (defined in `symbolon`, a different
/// crate), so a match here must carry a wildcard even though only four
/// variants exist today.
fn one_role_below(role: Role) -> Role {
    match role {
        Role::Admin => Role::Operator,
        Role::Operator => Role::Agent,
        Role::Agent => Role::Readonly,
        _ => panic!(
            "route_role_floor_table declares Min({role:?}); there is no \
             role below it to probe with -- Min(Role::Readonly) is never a \
             meaningful floor (everyone is Readonly-or-above already)"
        ),
    }
}

/// The single declaration site: every route this server registers, and the
/// role floor a contributor asserts for it.
///
/// `walker_table_covers_every_openapi_route` is this table's own
/// completeness check -- it fails if the generated `OpenAPI` document
/// serves a path+method with no entry here, so a new route cannot silently
/// ship without a declared floor.
fn route_role_floor_table() -> Vec<RouteSpec> {
    use Method as M;
    use Role::{Agent, Operator};
    use RoleFloor::{Min, NoFloor, Public};

    vec![
        RouteSpec::new(M::GET, "/api/health", Public),
        RouteSpec::new(M::GET, "/health", Public),
        // WHY: `/metrics` is gated by `taxis::config::MetricsMode`
        // (disabled/local_only/bearer/public), not by `Role` -- there is no
        // role floor to declare for it here.
        RouteSpec::new(M::GET, "/metrics", NoFloor),
        // WHY: bearer-gated by `require_bearer_auth` at the router layer;
        // the handler itself checks no role.
        RouteSpec::new(M::GET, "/api/docs/openapi.json", NoFloor),
        RouteSpec::new(M::GET, "/api/tool-stats", Min(Operator)),
        RouteSpec::new(M::GET, "/api/v1/sessions", Min(Agent)),
        RouteSpec::new(M::POST, "/api/v1/sessions", Min(Operator))
            .with_body(r#"{"nous_id":"floor-walker","session_key":"floor-walker"}"#),
        RouteSpec::new(M::POST, "/api/v1/sessions/stream", Min(Operator))
            .with_body(r#"{"nous_id":"floor-walker","message":"hi"}"#),
        RouteSpec::new(M::GET, "/api/v1/sessions/{id}", Min(Agent)),
        RouteSpec::new(M::DELETE, "/api/v1/sessions/{id}", Min(Operator)),
        RouteSpec::new(M::GET, "/api/v1/sessions/{id}/replay", Min(Agent)),
        RouteSpec::new(M::POST, "/api/v1/sessions/{id}/archive", Min(Operator)),
        RouteSpec::new(M::POST, "/api/v1/sessions/{id}/unarchive", Min(Operator)),
        RouteSpec::new(M::DELETE, "/api/v1/sessions/{id}/purge", Min(Operator)),
        RouteSpec::new(M::PUT, "/api/v1/sessions/{id}/name", Min(Operator))
            .with_body(r#"{"name":"floor-walker"}"#),
        RouteSpec::new(M::POST, "/api/v1/sessions/{id}/messages", Min(Operator))
            .with_body(r#"{"content":"hi"}"#),
        RouteSpec::new(
            M::GET,
            "/api/v1/sessions/{session_id}/approvals",
            Min(Agent),
        ),
        RouteSpec::new(
            M::POST,
            "/api/v1/sessions/{session_id}/approvals",
            Min(Operator),
        )
        .with_body(r#"{"turn_id":"floor-walker","tool_id":"floor-walker","decision":"approved"}"#),
        RouteSpec::new(M::GET, "/api/v1/approvals", Min(Agent)),
        RouteSpec::new(
            M::POST,
            "/api/v1/turns/{turn_id}/tools/{tool_id}/approve",
            Min(Operator),
        ),
        RouteSpec::new(
            M::POST,
            "/api/v1/turns/{turn_id}/tools/{tool_id}/deny",
            Min(Operator),
        ),
        RouteSpec::new(
            M::GET,
            "/api/v1/sessions/{session_id}/turns/{turn_id}/events",
            Min(Operator),
        ),
        RouteSpec::new(M::GET, "/api/v1/sessions/{id}/history", Min(Agent)),
        RouteSpec::new(M::GET, "/api/v1/events", NoFloor),
        RouteSpec::new(M::GET, "/api/v1/events/subscribe", NoFloor),
        RouteSpec::new(M::GET, "/api/v1/events/discovery", NoFloor),
        RouteSpec::new(M::GET, "/api/v1/ops/tools", Min(Operator)),
        RouteSpec::new(M::GET, "/api/v1/system/credentials", Min(Operator)),
        RouteSpec::new(M::POST, "/api/v1/system/credentials", Min(Operator))
            .with_body(r#"{"provider":"anthropic","key":"sk-test-floor-walker","role":"primary"}"#),
        RouteSpec::new(M::GET, "/api/v1/system/health", Min(Operator)),
        RouteSpec::new(M::GET, "/api/v1/system/status", Min(Operator)),
        RouteSpec::new(M::GET, "/api/v1/system/daemon/tasks", Min(Operator)),
        RouteSpec::new(
            M::POST,
            "/api/v1/system/daemon/tasks/{runner}/{task_id}/enable",
            Min(Operator),
        ),
        RouteSpec::new(
            M::POST,
            "/api/v1/system/daemon/tasks/{runner}/{task_id}/disable",
            Min(Operator),
        )
        .with_body("{}"),
        RouteSpec::new(
            M::POST,
            "/api/v1/system/daemon/tasks/{runner}/{task_id}/retry",
            Min(Operator),
        ),
        RouteSpec::new(M::POST, "/api/v1/system/credentials/rotate", Min(Operator))
            .with_query("?provider=anthropic"),
        RouteSpec::new(M::DELETE, "/api/v1/system/credentials/{id}", Min(Operator)),
        RouteSpec::new(
            M::POST,
            "/api/v1/system/credentials/{id}/validate",
            Min(Operator),
        ),
        RouteSpec::new(M::GET, "/api/v1/nous", Min(Agent)),
        RouteSpec::new(M::POST, "/api/v1/nous", Min(Operator))
            .with_body(r#"{"id":"floor-walker-agent"}"#),
        RouteSpec::new(M::GET, "/api/v1/nous/{id}", Min(Agent)),
        RouteSpec::new(M::PATCH, "/api/v1/nous/{id}", Min(Operator))
            .with_body(r#"{"enabled":true}"#),
        RouteSpec::new(M::GET, "/api/v1/nous/{id}/tools", Min(Agent)),
        RouteSpec::new(M::PATCH, "/api/v1/nous/{id}/tools", Min(Operator))
            .with_body(r#"{"tool":"floor-walker","enabled":true}"#),
        RouteSpec::new(M::POST, "/api/v1/nous/{id}/recover", Min(Operator)),
        RouteSpec::new(M::GET, "/api/v1/config", Min(Operator)),
        RouteSpec::new(M::POST, "/api/v1/config/reload", Min(Operator)),
        RouteSpec::new(M::GET, "/api/v1/config/{section}", Min(Operator)),
        RouteSpec::new(M::PUT, "/api/v1/config/{section}", Min(Operator)).with_body("{}"),
        RouteSpec::new(M::GET, "/api/v1/workspace/files", Min(Agent)),
        RouteSpec::new(M::GET, "/api/v1/workspace/git-status", Min(Agent)),
        RouteSpec::new(M::GET, "/api/v1/workspace/files/content", Min(Agent))
            .with_query("?path=floor-walker.txt"),
        RouteSpec::new(M::PUT, "/api/v1/workspace/files/content", Min(Operator))
            .with_body(r#"{"path":"floor-walker.txt","content":"hi"}"#),
        RouteSpec::new(M::POST, "/api/v1/workspace/open", Min(Operator))
            .with_body(r#"{"path":"floor-walker.txt"}"#),
        RouteSpec::new(M::GET, "/api/v1/workspace/diff", Min(Agent))
            .with_query("?path=floor-walker.txt"),
        RouteSpec::new(M::GET, "/api/v1/workspace/search", Min(Agent))
            .with_query("?q=floor-walker"),
        RouteSpec::new(M::GET, "/api/v1/knowledge/facts", Min(Agent)),
        RouteSpec::new(M::POST, "/api/v1/knowledge/facts/import", Min(Operator)).with_body("{}"),
        RouteSpec::new(M::POST, "/api/v1/knowledge/ingest", Min(Operator))
            .with_body(r#"{"content":"floor-walker"}"#),
        RouteSpec::new(M::POST, "/api/v1/knowledge/ingest/webhook", Min(Operator))
            .with_body(r#"{"nous_id":"floor-walker","facts":[]}"#),
        RouteSpec::new(M::GET, "/api/v1/knowledge/facts/{id}", Min(Agent)),
        RouteSpec::new(
            M::POST,
            "/api/v1/knowledge/facts/{id}/forget",
            Min(Operator),
        )
        .with_body("{}"),
        RouteSpec::new(
            M::POST,
            "/api/v1/knowledge/facts/{id}/restore",
            Min(Operator),
        ),
        RouteSpec::new(
            M::PUT,
            "/api/v1/knowledge/facts/{id}/confidence",
            Min(Operator),
        )
        .with_body(r#"{"confidence":0.5}"#),
        RouteSpec::new(
            M::PUT,
            "/api/v1/knowledge/facts/{id}/sensitivity",
            Min(Operator),
        )
        .with_body(r#"{"sensitivity":"internal"}"#),
        RouteSpec::new(M::GET, "/api/v1/knowledge/entities", Min(Agent)),
        RouteSpec::new(M::POST, "/api/v1/knowledge/entities/merge", Min(Operator))
            .with_body(r#"{"canonical_id":"a","merged_id":"b"}"#),
        RouteSpec::new(M::GET, "/api/v1/knowledge/entities/{id}", Min(Agent)),
        RouteSpec::new(M::DELETE, "/api/v1/knowledge/entities/{id}", Min(Operator)),
        RouteSpec::new(
            M::GET,
            "/api/v1/knowledge/entities/{id}/memories",
            Min(Agent),
        ),
        RouteSpec::new(
            M::GET,
            "/api/v1/knowledge/entities/{id}/relationships",
            Min(Agent),
        ),
        RouteSpec::new(
            M::POST,
            "/api/v1/knowledge/entities/{id}/flag",
            Min(Operator),
        )
        .with_body(r#"{"reason":"floor-walker","severity":"low"}"#),
        RouteSpec::new(M::GET, "/api/v1/knowledge/search/explain", Min(Agent))
            .with_query("?q=floor-walker"),
        RouteSpec::new(M::GET, "/api/v1/knowledge/search", Min(Agent))
            .with_query("?q=floor-walker"),
        RouteSpec::new(M::GET, "/api/v1/knowledge/timeline", Min(Agent)),
        RouteSpec::new(M::GET, "/api/v1/knowledge/check", Min(Operator)),
        RouteSpec::new(M::GET, "/api/v1/knowledge/health", Min(Operator)),
        RouteSpec::new(M::GET, "/api/v1/metrics/agents", Min(Operator)),
        RouteSpec::new(M::GET, "/api/v1/metrics/agents/{id}", Min(Agent)),
        RouteSpec::new(M::GET, "/api/v1/metrics/quality", Min(Operator)),
        RouteSpec::new(M::GET, "/api/v1/metrics/tokens", Min(Operator)),
        RouteSpec::new(M::GET, "/api/v1/metrics/costs", Min(Operator)),
        RouteSpec::new(M::GET, "/api/v1/journal", Min(Operator)),
        RouteSpec::new(
            M::GET,
            "/api/v1/planning/projects/{project_id}/verification",
            Min(Operator),
        ),
        RouteSpec::new(
            M::POST,
            "/api/v1/planning/projects/{project_id}/verification/refresh",
            Min(Operator),
        )
        .with_body("{}"),
        RouteSpec::new(M::GET, "/api/v1/providers", Min(Operator)),
        RouteSpec::new(M::GET, "/api/v1/providers/route", Min(Operator))
            .with_query("?model=floor-walker"),
    ]
}

/// Every path+method the generated `OpenAPI` document actually serves.
///
/// WHY: excludes `/api/docs/openapi.json` itself -- the spec cannot list
/// its own serving route as one of its own `paths()` entries -- which is
/// exactly why the completeness direction runs `OpenAPI ⊆ table`, not the
/// other way around: the table is allowed to know about routes the
/// generated document cannot self-describe.
fn openapi_routes() -> Vec<(Method, String)> {
    let spec = crate::openapi::openapi_value_for_auth_mode("token");
    let paths = spec
        .get("paths")
        .and_then(serde_json::Value::as_object)
        .expect("OpenAPI spec must contain a paths object");

    let mut routes = Vec::new();
    for (path, operations) in paths {
        let Some(operations) = operations.as_object() else {
            continue;
        };
        for method_key in operations.keys() {
            let Some(method) = method_from_openapi_key(method_key) else {
                continue;
            };
            routes.push((method, path.clone()));
        }
    }
    routes
}

fn method_from_openapi_key(key: &str) -> Option<Method> {
    match key {
        "get" => Some(Method::GET),
        "post" => Some(Method::POST),
        "put" => Some(Method::PUT),
        "patch" => Some(Method::PATCH),
        "delete" => Some(Method::DELETE),
        _ => None,
    }
}

/// Completeness: every route the generated `OpenAPI` document serves has
/// an entry in [`route_role_floor_table`]. A route that reaches this test
/// with no entry fails it -- absence is never silently read as "no floor".
#[test]
fn walker_table_covers_every_openapi_route() {
    let table = route_role_floor_table();
    let mut missing = Vec::new();

    for (method, path) in openapi_routes() {
        let covered = table
            .iter()
            .any(|spec| spec.method == method && spec.path == path);
        if !covered {
            missing.push(format!("{method} {path}"));
        }
    }

    assert!(
        missing.is_empty(),
        "route_role_floor_table is missing an entry for these OpenAPI \
         routes -- add one (Public/NoFloor/Min(role)) so the walker \
         actually covers them:\n{}",
        missing.join("\n")
    );
}

/// Declared: every route whose floor is above the baseline (bearer-only)
/// floor documents a `403` response in its `OpenAPI` `responses()`.
#[test]
fn walker_declared_responses_document_403_above_baseline() {
    let spec = crate::openapi::openapi_value_for_auth_mode("token");
    let paths = spec
        .get("paths")
        .and_then(serde_json::Value::as_object)
        .expect("OpenAPI spec must contain a paths object");

    let mut undocumented = Vec::new();
    for route in route_role_floor_table() {
        let RoleFloor::Min(role) = route.floor else {
            continue;
        };
        let method_key = route.method.as_str().to_ascii_lowercase();
        let has_403 = paths
            .get(route.path)
            .and_then(|path_item| path_item.get(&method_key))
            .and_then(|op| op.get("responses"))
            .and_then(|responses| responses.get("403"))
            .is_some();
        if !has_403 {
            undocumented.push(format!("{} {} (floor: {role:?})", route.method, route.path));
        }
    }

    assert!(
        undocumented.is_empty(),
        "these routes declare a role floor above baseline but do not \
         document a 403 response in their #[utoipa::path] responses():\n{}",
        undocumented.join("\n")
    );
}

/// Enforced: for every `Min(role)` floor, a request one role-tier below
/// gets `403 Forbidden`; the same request at the declared floor does not.
#[tokio::test]
async fn walker_enforces_role_floor_below_and_at() {
    let (app, _dir) = app().await;

    // WHY: `reconnect_turn` (see `route_path_overrides`) looks up the
    // session by path param *before* `require_role` runs, so it needs a
    // session that actually exists to ever reach the role check at all.
    let seeded_session = create_test_session(&app).await;
    let real_session_id = seeded_session["id"]
        .as_str()
        .expect("created session has a string id")
        .to_owned();

    let mut failures = Vec::new();
    for route in route_role_floor_table() {
        let RoleFloor::Min(role) = route.floor else {
            continue;
        };
        let below = one_role_below(role);
        let overrides = route_path_overrides(route.path, &real_session_id);

        let below_req = build_request(&route, below, &overrides);
        let below_resp = app.clone().oneshot(below_req).await.expect("oneshot");
        if below_resp.status() != StatusCode::FORBIDDEN {
            failures.push(format!(
                "{} {}: role {below:?} (one below floor {role:?}) got {}, want 403",
                route.method,
                route.path,
                below_resp.status()
            ));
        }

        let at_req = build_request(&route, role, &overrides);
        let at_resp = app.clone().oneshot(at_req).await.expect("oneshot");
        if at_resp.status() == StatusCode::FORBIDDEN {
            failures.push(format!(
                "{} {}: role {role:?} (at the declared floor) got 403, want anything else",
                route.method, route.path
            ));
        }
    }

    assert!(
        failures.is_empty(),
        "route role floor enforcement failed:\n{}",
        failures.join("\n")
    );
}

/// Sanity check on the other direction of completeness: a route declared
/// [`RoleFloor::NoFloor`] must not reject the lowest role, `Readonly`, with
/// `403`. This guards against a route silently growing a role check that
/// the table was never updated to reflect.
#[tokio::test]
async fn walker_no_floor_routes_admit_readonly_role() {
    let (app, _dir) = app().await;

    let mut failures = Vec::new();
    for route in route_role_floor_table() {
        if route.floor != RoleFloor::NoFloor {
            continue;
        }
        let req = build_request(&route, Role::Readonly, &[]);
        let resp = app.clone().oneshot(req).await.expect("oneshot");
        if resp.status() == StatusCode::FORBIDDEN {
            failures.push(format!(
                "{} {}: declared NoFloor but Role::Readonly got 403",
                route.method, route.path
            ));
        }
    }

    assert!(
        failures.is_empty(),
        "these routes are declared NoFloor but reject Role::Readonly; \
         update route_role_floor_table to Min(role) to match reality:\n{}",
        failures.join("\n")
    );
}

/// Sanity check for [`RoleFloor::Public`] routes: no Bearer token at all
/// must not be rejected.
#[tokio::test]
async fn walker_public_routes_admit_no_token() {
    let (app, _dir) = app().await;

    for route in route_role_floor_table() {
        if route.floor != RoleFloor::Public {
            continue;
        }
        let uri = format!("{}{}", concrete_path(route.path, &[]), route.query);
        let req = axum::http::Request::builder()
            .method(route.method.clone())
            .uri(uri)
            .body(axum::body::Body::empty())
            .expect("build request");
        let resp = app.clone().oneshot(req).await.expect("oneshot");
        assert_eq!(
            resp.status(),
            StatusCode::OK,
            "{} {}: declared Public but no-token request got {}",
            route.method,
            route.path,
            resp.status()
        );
    }
}

/// Build a live request for `route`, authenticated as `role`, from its
/// declared query suffix and body, substituting `overrides` (see
/// [`route_path_overrides`]) into its path template.
///
/// When `role` is `Agent`, the token is scoped to the same placeholder
/// [`concrete_path`] substitutes into the route's own path params. This is
/// not merely cosmetic: some Agent-floor routes (the knowledge domain's
/// reads) additionally require an Agent-role caller to be scoped to a
/// nous_id -- `KnowledgeReadPolicy::from_single_nous`/`from_claims` reject
/// an *unscoped* Agent token outright, a real, separate business rule this
/// walker must not misread as a broken role floor. Scoping to "rwt" also
/// satisfies any `require_nous_access` check a route runs after
/// `require_role` (scoped == target), so it is safe for every route, not
/// only the ones that need it.
fn build_request(
    route: &RouteSpec,
    role: Role,
    overrides: &[(&str, &str)],
) -> axum::http::Request<axum::body::Body> {
    let uri = format!("{}{}", concrete_path(route.path, overrides), route.query);
    let body = route.body.map(|raw| {
        serde_json::from_str::<serde_json::Value>(raw)
            .unwrap_or_else(|e| panic!("invalid JSON literal in route table for {uri}: {e}"))
    });
    let token = if role == Role::Agent {
        token_scoped_to(role, "rwt")
    } else {
        token_for_role(role)
    };
    request_with_bearer_token(route.method.as_str(), &uri, body, &token)
}

/// Build a request carrying an explicit, already-issued bearer token --
/// the same shape `helpers::authed_request_as` builds from a role, but
/// this walker sometimes needs a *scoped* token that helper cannot express.
fn request_with_bearer_token(
    method: &str,
    uri: &str,
    body: Option<serde_json::Value>,
    token: &str,
) -> axum::http::Request<axum::body::Body> {
    let builder = axum::http::Request::builder()
        .method(method)
        .uri(uri)
        .header("content-type", CONTENT_TYPE_JSON)
        .header("authorization", format!("{BEARER_PREFIX}{token}"));
    match body {
        Some(b) => builder
            .body(axum::body::Body::from(
                serde_json::to_vec(&b).expect("serialize route table body"),
            ))
            .expect("build request"),
        None => builder
            .body(axum::body::Body::empty())
            .expect("build request"),
    }
}

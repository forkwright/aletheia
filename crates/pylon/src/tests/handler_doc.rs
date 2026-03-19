//! Sync test: verifies that `crates/pylon/docs/handlers.md` documents
//! every route registered in `src/router.rs`.
//!
//! When adding a new route, update both `router.rs` and `handlers.md`.
//! Run with: `cargo test -p aletheia-pylon -- handler_doc`

const HANDLERS_MD: &str = include_str!("../../docs/handlers.md");

/// Routes registered in `router.rs`. Add new routes here when they are added
/// to `build_router()`.
const ROUTES: &[&str] = &[
    "/api/health",
    "/api/docs/openapi.json",
    "/metrics",
    // sessions
    "/api/v1/sessions",
    "/api/v1/sessions/stream",
    "/api/v1/sessions/{id}",
    "/api/v1/sessions/{id}/archive",
    "/api/v1/sessions/{id}/unarchive",
    "/api/v1/sessions/{id}/purge",
    "/api/v1/sessions/{id}/name",
    "/api/v1/sessions/{id}/messages",
    "/api/v1/sessions/{id}/history",
    "/api/v1/events",
    // nous
    "/api/v1/nous",
    "/api/v1/nous/{id}",
    "/api/v1/nous/{id}/tools",
    // config
    "/api/v1/config",
    "/api/v1/config/reload",
    "/api/v1/config/{section}",
    // knowledge
    "/api/v1/knowledge/facts",
    "/api/v1/knowledge/facts/{id}",
    "/api/v1/knowledge/facts/{id}/forget",
    "/api/v1/knowledge/facts/{id}/restore",
    "/api/v1/knowledge/facts/{id}/confidence",
    "/api/v1/knowledge/entities",
    "/api/v1/knowledge/entities/{id}/relationships",
    "/api/v1/knowledge/search",
    "/api/v1/knowledge/timeline",
];

#[test]
fn handler_doc_covers_all_routes() {
    let mut missing = Vec::new();
    for route in ROUTES {
        if !HANDLERS_MD.contains(route) {
            missing.push(*route);
        }
    }
    assert!(
        missing.is_empty(),
        "handlers.md is missing documentation for these routes:\n{}\n\
         Update crates/pylon/docs/handlers.md to add the missing entries.",
        missing
            .iter()
            .map(|r| format!("  {r}"))
            .collect::<Vec<_>>()
            .join("\n")
    );
}

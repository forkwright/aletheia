//! Canonical route builders for first-party Aletheia API clients.
//!
//! Every builder encodes path segments exactly once with
//! [`keryx::url::encode_path_segment`], so callers pass raw identifier
//! values and receive safe, correctly-encoded paths. Callers must not
//! encode identifiers before passing them in; doing so would produce
//! double-encoded paths (e.g. `%252F` for a literal `/`).

/// A first-party client route expected to be registered by pylon.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClientRouteContract {
    /// HTTP method used by the client.
    pub method: &'static str,
    /// Canonical server route template, without query parameters.
    pub path_template: &'static str,
}

/// Routes exercised by the skene client surface.
///
/// The pylon route-contract test consumes this list and fails when a client
/// route has no registered server handler.
pub const SKENE_CLIENT_ROUTE_CONTRACTS: &[ClientRouteContract] = &[
    ClientRouteContract {
        method: "GET",
        path_template: "/api/health",
    },
    ClientRouteContract {
        method: "GET",
        path_template: "/api/v1/nous",
    },
    ClientRouteContract {
        method: "GET",
        path_template: "/api/v1/nous/{id}/tools",
    },
    ClientRouteContract {
        method: "GET",
        path_template: "/api/v1/sessions",
    },
    ClientRouteContract {
        method: "POST",
        path_template: "/api/v1/sessions",
    },
    ClientRouteContract {
        method: "POST",
        path_template: "/api/v1/sessions/stream",
    },
    ClientRouteContract {
        method: "GET",
        path_template: "/api/v1/sessions/{id}/history",
    },
    ClientRouteContract {
        method: "POST",
        path_template: "/api/v1/sessions/{id}/archive",
    },
    ClientRouteContract {
        method: "POST",
        path_template: "/api/v1/sessions/{id}/unarchive",
    },
    ClientRouteContract {
        method: "PUT",
        path_template: "/api/v1/sessions/{id}/name",
    },
    ClientRouteContract {
        method: "POST",
        path_template: "/api/v1/turns/{turn_id}/tools/{tool_id}/approve",
    },
    ClientRouteContract {
        method: "POST",
        path_template: "/api/v1/turns/{turn_id}/tools/{tool_id}/deny",
    },
    ClientRouteContract {
        method: "GET",
        path_template: "/api/v1/events",
    },
    ClientRouteContract {
        method: "GET",
        path_template: "/api/v1/events/subscribe",
    },
    ClientRouteContract {
        method: "GET",
        path_template: "/api/v1/config",
    },
    ClientRouteContract {
        method: "PUT",
        path_template: "/api/v1/config/{section}",
    },
    ClientRouteContract {
        method: "GET",
        path_template: "/api/v1/knowledge/facts",
    },
    ClientRouteContract {
        method: "GET",
        path_template: "/api/v1/knowledge/facts/{id}",
    },
    ClientRouteContract {
        method: "POST",
        path_template: "/api/v1/knowledge/facts/{id}/forget",
    },
    ClientRouteContract {
        method: "POST",
        path_template: "/api/v1/knowledge/facts/{id}/restore",
    },
    ClientRouteContract {
        method: "PUT",
        path_template: "/api/v1/knowledge/facts/{id}/confidence",
    },
    ClientRouteContract {
        method: "PUT",
        path_template: "/api/v1/knowledge/facts/{id}/sensitivity",
    },
    ClientRouteContract {
        method: "GET",
        path_template: "/api/v1/knowledge/entities",
    },
    ClientRouteContract {
        method: "GET",
        path_template: "/api/v1/knowledge/entities/{id}/relationships",
    },
    ClientRouteContract {
        method: "GET",
        path_template: "/api/v1/knowledge/timeline",
    },
];

/// Planning API routes.
pub mod planning {
    /// Template for `GET` project verification.
    ///
    /// `{project_id}` is a placeholder — do not interpolate directly.
    /// Use [`project_verification_path`] to build an encoded path.
    pub const PROJECT_VERIFICATION_TEMPLATE: &str =
        "/api/v1/planning/projects/{project_id}/verification";

    /// Template for `POST` project verification refresh.
    ///
    /// `{project_id}` is a placeholder — do not interpolate directly.
    /// Use [`project_verification_refresh_path`] to build an encoded path.
    pub const PROJECT_VERIFICATION_REFRESH_TEMPLATE: &str =
        "/api/v1/planning/projects/{project_id}/verification/refresh";

    /// Build the path for `GET` project verification.
    ///
    /// `project_id` is encoded exactly once with
    /// [`keryx::url::encode_path_segment`]. Identifiers containing `/`,
    /// `?`, `#`, spaces, or `:` are safe to pass in raw form.
    #[must_use]
    pub fn project_verification_path(project_id: &str) -> String {
        let encoded = keryx::url::encode_path_segment(project_id);
        format!("/api/v1/planning/projects/{encoded}/verification")
    }

    /// Build the absolute URL for `GET` project verification.
    #[must_use]
    pub fn project_verification_url(base_url: &str, project_id: &str) -> String {
        keryx::url::join_base_path(base_url, &project_verification_path(project_id))
    }

    /// Build the path for `POST` project verification refresh.
    ///
    /// `project_id` is encoded exactly once with
    /// [`keryx::url::encode_path_segment`]. Identifiers containing `/`,
    /// `?`, `#`, spaces, or `:` are safe to pass in raw form.
    #[must_use]
    pub fn project_verification_refresh_path(project_id: &str) -> String {
        let encoded = keryx::url::encode_path_segment(project_id);
        format!("/api/v1/planning/projects/{encoded}/verification/refresh")
    }

    /// Build the absolute URL for `POST` project verification refresh.
    #[must_use]
    pub fn project_verification_refresh_url(base_url: &str, project_id: &str) -> String {
        keryx::url::join_base_path(base_url, &project_verification_refresh_path(project_id))
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::planning::*;
    use super::{ClientRouteContract, SKENE_CLIENT_ROUTE_CONTRACTS};

    fn quoted_strings(source: &str) -> Vec<String> {
        let mut strings = Vec::new();
        let mut chars = source.chars();
        while let Some(ch) = chars.next() {
            if ch != '"' {
                continue;
            }
            let mut value = String::new();
            let mut escaped = false;
            for inner in chars.by_ref() {
                if escaped {
                    value.push(inner);
                    escaped = false;
                    continue;
                }
                match inner {
                    '\\' => escaped = true,
                    '"' => break,
                    _ => value.push(inner),
                }
            }
            strings.push(value);
        }
        strings
    }

    fn normalize_route(path: &str) -> String {
        let path = path.split_once('?').map_or(path, |(path, _query)| path);
        let mut normalized = String::with_capacity(path.len());
        let mut chars = path.chars().peekable();
        while let Some(ch) = chars.next() {
            if ch != '{' {
                normalized.push(ch);
                continue;
            }
            normalized.push_str("{}");
            for inner in chars.by_ref() {
                if inner == '}' {
                    break;
                }
            }
        }
        normalized
    }

    fn contract_paths(contracts: &[ClientRouteContract]) -> BTreeSet<String> {
        contracts
            .iter()
            .map(|contract| normalize_route(contract.path_template))
            .collect()
    }

    #[test]
    fn api_client_route_literals_have_contracts() {
        let contracts = contract_paths(SKENE_CLIENT_ROUTE_CONTRACTS);
        let source = include_str!("client.rs");

        for route in quoted_strings(source)
            .into_iter()
            .filter(|literal| literal.starts_with("/api/"))
            .map(|literal| normalize_route(&literal))
        {
            assert!(
                contracts.contains(&route),
                "ApiClient route literal has no route contract: {route}"
            );
        }
    }

    // ── normal IDs pass through unmodified ─────────────────────────────────

    #[test]
    fn alphanumeric_id_unmodified() {
        let path = project_verification_path("proj-abc123");
        assert_eq!(path, "/api/v1/planning/projects/proj-abc123/verification");
    }

    #[test]
    fn refresh_alphanumeric_id_unmodified() {
        let path = project_verification_refresh_path("proj-abc123");
        assert_eq!(
            path,
            "/api/v1/planning/projects/proj-abc123/verification/refresh"
        );
    }

    // ── traversal / injection cases ────────────────────────────────────────

    #[test]
    fn slash_in_id_is_encoded() {
        // A raw `/` would break path routing and enable traversal.
        // Dots are RFC 3986 unreserved chars and pass through unchanged;
        // slashes in the ID are encoded as %2F, which prevents path-segment
        // traversal — the server never treats %2F as a path separator.
        let path = project_verification_path("../../etc/passwd");
        assert!(
            path.contains("%2F"),
            "slash must be percent-encoded as %2F: {path}"
        );
        // Whole path must stay within the expected prefix.
        assert!(
            path.starts_with("/api/v1/planning/projects/"),
            "path prefix must be intact: {path}"
        );
        // The encoded segment must appear between the static prefix and suffix.
        assert!(
            path.ends_with("/verification"),
            "path suffix must be intact: {path}"
        );
    }

    #[test]
    fn question_mark_in_id_is_encoded() {
        let path = project_verification_path("proj?inject=true");
        assert!(path.contains("%3F"), "? must be encoded as %3F: {path}");
        assert!(!path.contains('?'), "raw ? must not appear: {path}");
    }

    #[test]
    fn hash_in_id_is_encoded() {
        let path = project_verification_path("proj#fragment");
        assert!(path.contains("%23"), "# must be encoded as %23: {path}");
        assert!(!path.contains('#'), "raw # must not appear: {path}");
    }

    #[test]
    fn space_in_id_is_encoded() {
        let path = project_verification_path("my project");
        assert!(path.contains("%20"), "space must be encoded as %20: {path}");
        assert!(!path.contains(' '), "raw space must not appear: {path}");
    }

    #[test]
    fn colon_in_id_is_encoded() {
        let path = project_verification_path("ns:proj-id");
        assert!(path.contains("%3A"), ": must be encoded as %3A: {path}");
        assert!(!path.contains(':'), "raw colon must not appear: {path}");
    }

    #[test]
    fn unicode_id_is_encoded() {
        let path = project_verification_path("プロジェクト");
        // Each non-ASCII byte must be percent-encoded.
        assert!(
            path.contains('%'),
            "unicode must be percent-encoded: {path}"
        );
        assert!(
            path.starts_with("/api/v1/planning/projects/"),
            "path prefix must be intact: {path}"
        );
    }

    #[test]
    fn percent_in_id_is_encoded_not_double_encoded() {
        // An ID that already looks encoded — e.g. "proj%2Fid" — must be
        // encoded as-is; the builder must not double-encode the `%`.
        let path = project_verification_path("proj%2Fid");
        assert!(
            path.contains("%25"),
            "% must be encoded as %25 to avoid double-encoding: {path}"
        );
    }

    // ── URL builder delegates to path builder ─────────────────────────────

    #[test]
    fn url_builder_prepends_base() {
        let url = project_verification_url("https://api.example.com", "p1");
        assert_eq!(
            url,
            "https://api.example.com/api/v1/planning/projects/p1/verification"
        );
    }

    #[test]
    fn url_builder_strips_trailing_slash_from_base() {
        let url = project_verification_url("https://api.example.com/", "p1");
        assert_eq!(
            url,
            "https://api.example.com/api/v1/planning/projects/p1/verification"
        );
    }

    #[test]
    fn refresh_url_builder_prepends_base() {
        let url = project_verification_refresh_url("https://api.example.com", "p1");
        assert_eq!(
            url,
            "https://api.example.com/api/v1/planning/projects/p1/verification/refresh"
        );
    }
}

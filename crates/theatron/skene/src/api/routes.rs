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
        path_template: "/api/v1/client/contract",
    },
    ClientRouteContract {
        method: "GET",
        path_template: "/api/v1/nous",
    },
    ClientRouteContract {
        method: "GET",
        path_template: "/api/v1/nous/{id}",
    },
    ClientRouteContract {
        method: "PATCH",
        path_template: "/api/v1/nous/{id}",
    },
    ClientRouteContract {
        method: "GET",
        path_template: "/api/v1/nous/{id}/tools",
    },
    ClientRouteContract {
        method: "PATCH",
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
        path_template: "/api/v1/system/credentials",
    },
    ClientRouteContract {
        method: "POST",
        path_template: "/api/v1/system/credentials",
    },
    ClientRouteContract {
        method: "POST",
        path_template: "/api/v1/system/credentials/{id}/validate",
    },
    ClientRouteContract {
        method: "POST",
        path_template: "/api/v1/system/credentials/rotate",
    },
    ClientRouteContract {
        method: "DELETE",
        path_template: "/api/v1/system/credentials/{id}",
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
    ClientRouteContract {
        method: "GET",
        path_template: "/api/v1/planning/projects/{project_id}/verification",
    },
    ClientRouteContract {
        method: "POST",
        path_template: "/api/v1/planning/projects/{project_id}/verification/refresh",
    },
];

/// Encoding helpers for route builders.
///
/// Path segments and query values have different escaping rules. Exposing
/// separate function names makes the call-site context explicit and prevents
/// reusing a path-segment encoder for query strings.
pub mod encoding {
    /// Percent-encode a raw identifier for use as one URL path segment.
    ///
    /// This is path-segment encoding only. Query values must use
    /// [`query_value`] instead.
    #[must_use]
    pub fn path_segment(segment: &str) -> String {
        keryx::url::encode_path_segment(segment)
    }

    /// Percent-encode a value for use in a URL query string.
    ///
    /// This follows `application/x-www-form-urlencoded` query encoding,
    /// including `+` for spaces. Path segments must use [`path_segment`]
    /// instead.
    #[must_use]
    pub fn query_value(value: &str) -> String {
        form_urlencoded::byte_serialize(value.as_bytes()).collect()
    }
}

fn query_pair(name: &str, value: &str) -> String {
    format!(
        "{}={}",
        encoding::query_value(name),
        encoding::query_value(value)
    )
}

/// Session API routes.
pub mod sessions {
    use super::{encoding, query_pair};

    /// Template for session collection routes.
    pub const SESSIONS_TEMPLATE: &str = "/api/v1/sessions";

    /// Template for one session history route.
    ///
    /// `{id}` is a placeholder - do not interpolate directly. Use
    /// [`session_history_path`] to build an encoded path.
    pub const SESSION_HISTORY_TEMPLATE: &str = "/api/v1/sessions/{id}/history";

    /// Template for archiving one session.
    ///
    /// `{id}` is a placeholder - do not interpolate directly. Use
    /// [`session_archive_path`] to build an encoded path.
    pub const SESSION_ARCHIVE_TEMPLATE: &str = "/api/v1/sessions/{id}/archive";

    /// Template for unarchiving one session.
    ///
    /// `{id}` is a placeholder - do not interpolate directly. Use
    /// [`session_unarchive_path`] to build an encoded path.
    pub const SESSION_UNARCHIVE_TEMPLATE: &str = "/api/v1/sessions/{id}/unarchive";

    /// Template for renaming one session.
    ///
    /// `{id}` is a placeholder - do not interpolate directly. Use
    /// [`session_name_path`] to build an encoded path.
    pub const SESSION_NAME_TEMPLATE: &str = "/api/v1/sessions/{id}/name";

    /// Build the path for listing or creating sessions.
    #[must_use]
    pub fn sessions_path() -> &'static str {
        SESSIONS_TEMPLATE
    }

    /// Build the path for listing sessions for one agent.
    #[must_use]
    pub fn sessions_for_agent_path(nous_id: &str) -> String {
        format!("{SESSIONS_TEMPLATE}?{}", query_pair("nous_id", nous_id))
    }

    /// Build the path for one session's history.
    #[must_use]
    pub fn session_history_path(id: &str) -> String {
        let encoded = encoding::path_segment(id);
        format!("{SESSIONS_TEMPLATE}/{encoded}/history")
    }

    /// Build the path for archiving one session.
    #[must_use]
    pub fn session_archive_path(id: &str) -> String {
        let encoded = encoding::path_segment(id);
        format!("{SESSIONS_TEMPLATE}/{encoded}/archive")
    }

    /// Build the path for unarchiving one session.
    #[must_use]
    pub fn session_unarchive_path(id: &str) -> String {
        let encoded = encoding::path_segment(id);
        format!("{SESSIONS_TEMPLATE}/{encoded}/unarchive")
    }

    /// Build the path for renaming one session.
    #[must_use]
    pub fn session_name_path(id: &str) -> String {
        let encoded = encoding::path_segment(id);
        format!("{SESSIONS_TEMPLATE}/{encoded}/name")
    }
}

/// System API routes.
pub mod system {
    use super::{encoding, query_pair};

    /// Template for credential collection routes.
    pub const CREDENTIALS_TEMPLATE: &str = "/api/v1/system/credentials";

    /// Template for single-credential routes.
    ///
    /// `{id}` is a placeholder - do not interpolate directly. Use
    /// [`credential_path`] to build an encoded path.
    pub const CREDENTIAL_TEMPLATE: &str = "/api/v1/system/credentials/{id}";

    /// Template for credential validation routes.
    ///
    /// `{id}` is a placeholder - do not interpolate directly. Use
    /// [`credential_validate_path`] to build an encoded path.
    pub const CREDENTIAL_VALIDATE_TEMPLATE: &str = "/api/v1/system/credentials/{id}/validate";

    /// Template for credential rotation.
    pub const CREDENTIAL_ROTATE_TEMPLATE: &str = "/api/v1/system/credentials/rotate";

    /// Build the path for listing or creating credentials.
    #[must_use]
    pub fn credentials_path() -> &'static str {
        CREDENTIALS_TEMPLATE
    }

    /// Build the absolute URL for listing or creating credentials.
    #[must_use]
    pub fn credentials_url(base_url: &str) -> String {
        keryx::url::join_base_path(base_url, credentials_path())
    }

    /// Build the path for one credential.
    #[must_use]
    pub fn credential_path(id: &str) -> String {
        let encoded = encoding::path_segment(id);
        format!("{CREDENTIALS_TEMPLATE}/{encoded}")
    }

    /// Build the absolute URL for one credential.
    #[must_use]
    pub fn credential_url(base_url: &str, id: &str) -> String {
        keryx::url::join_base_path(base_url, &credential_path(id))
    }

    /// Build the path for credential validation.
    #[must_use]
    pub fn credential_validate_path(id: &str) -> String {
        let encoded = encoding::path_segment(id);
        format!("{CREDENTIALS_TEMPLATE}/{encoded}/validate")
    }

    /// Build the absolute URL for credential validation.
    #[must_use]
    pub fn credential_validate_url(base_url: &str, id: &str) -> String {
        keryx::url::join_base_path(base_url, &credential_validate_path(id))
    }

    /// Build the path for rotating credentials by provider.
    #[must_use]
    pub fn credential_rotate_path(provider: &str) -> String {
        format!(
            "{CREDENTIAL_ROTATE_TEMPLATE}?{}",
            query_pair("provider", provider)
        )
    }

    /// Build the absolute URL for rotating credentials by provider.
    #[must_use]
    pub fn credential_rotate_url(base_url: &str, provider: &str) -> String {
        keryx::url::join_base_path(base_url, &credential_rotate_path(provider))
    }
}

/// Agent API routes.
pub mod nous {
    use super::encoding;

    /// Template for one agent route.
    ///
    /// `{id}` is a placeholder - do not interpolate directly. Use
    /// [`agent_path`] to build an encoded path.
    pub const AGENT_TEMPLATE: &str = "/api/v1/nous/{id}";

    /// Template for one agent's tool route.
    ///
    /// `{id}` is a placeholder - do not interpolate directly. Use
    /// [`agent_tools_path`] to build an encoded path.
    pub const AGENT_TOOLS_TEMPLATE: &str = "/api/v1/nous/{id}/tools";

    /// Build the path for reading or toggling one agent.
    #[must_use]
    pub fn agent_path(id: &str) -> String {
        let encoded = encoding::path_segment(id);
        format!("/api/v1/nous/{encoded}")
    }

    /// Build the absolute URL for reading or toggling one agent.
    #[must_use]
    pub fn agent_url(base_url: &str, id: &str) -> String {
        keryx::url::join_base_path(base_url, &agent_path(id))
    }

    /// Build the path for reading or toggling tools on one agent.
    #[must_use]
    pub fn agent_tools_path(id: &str) -> String {
        let encoded = encoding::path_segment(id);
        format!("/api/v1/nous/{encoded}/tools")
    }

    /// Build the absolute URL for reading or toggling tools on one agent.
    #[must_use]
    pub fn agent_tools_url(base_url: &str, id: &str) -> String {
        keryx::url::join_base_path(base_url, &agent_tools_path(id))
    }
}

/// Configuration API routes.
pub mod config {
    use super::encoding;

    /// Template for one config section.
    ///
    /// `{section}` is a placeholder - do not interpolate directly. Use
    /// [`section_path`] to build an encoded path.
    pub const SECTION_TEMPLATE: &str = "/api/v1/config/{section}";

    /// Config section name for feature flags.
    pub const FEATURE_FLAGS_SECTION: &str = "feature_flags";

    /// Build the path for one config section.
    #[must_use]
    pub fn section_path(section: &str) -> String {
        let encoded = encoding::path_segment(section);
        format!("/api/v1/config/{encoded}")
    }

    /// Build the absolute URL for one config section.
    #[must_use]
    pub fn section_url(base_url: &str, section: &str) -> String {
        keryx::url::join_base_path(base_url, &section_path(section))
    }

    /// Build the path for the feature flags config section.
    #[must_use]
    pub fn feature_flags_path() -> String {
        section_path(FEATURE_FLAGS_SECTION)
    }

    /// Build the absolute URL for the feature flags config section.
    #[must_use]
    pub fn feature_flags_url(base_url: &str) -> String {
        keryx::url::join_base_path(base_url, &feature_flags_path())
    }
}

/// Planning API routes.
pub mod planning {
    use super::encoding;

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

    /// Template for updating one requirement.
    ///
    /// `{project_id}` and `{requirement_id}` are placeholders - do not
    /// interpolate directly. Use [`project_requirement_path`] to build an
    /// encoded path.
    pub const PROJECT_REQUIREMENT_TEMPLATE: &str =
        "/api/v1/planning/projects/{project_id}/requirements/{requirement_id}";

    /// Template for checkpoint actions.
    ///
    /// `{project_id}` and `{checkpoint_id}` are placeholders - do not
    /// interpolate directly. Use [`project_checkpoint_action_path`] to build
    /// an encoded path.
    pub const PROJECT_CHECKPOINT_ACTION_TEMPLATE: &str =
        "/api/v1/planning/projects/{project_id}/checkpoints/{checkpoint_id}/action";

    /// Template for category proposal actions.
    ///
    /// `{project_id}` and `{proposal_id}` are placeholders - do not
    /// interpolate directly. Use [`project_proposal_path`] to build an encoded
    /// path.
    pub const PROJECT_PROPOSAL_TEMPLATE: &str =
        "/api/v1/planning/projects/{project_id}/proposals/{proposal_id}";

    /// Template for answering a planning discussion.
    ///
    /// `{project_id}` and `{discussion_id}` are placeholders - do not
    /// interpolate directly. Use [`project_discussion_answer_path`] to build
    /// an encoded path.
    pub const PROJECT_DISCUSSION_ANSWER_TEMPLATE: &str =
        "/api/v1/planning/projects/{project_id}/discussions/{discussion_id}/answer";

    /// Template for reopening a planning discussion.
    ///
    /// `{project_id}` and `{discussion_id}` are placeholders - do not
    /// interpolate directly. Use [`project_discussion_reopen_path`] to build
    /// an encoded path.
    pub const PROJECT_DISCUSSION_REOPEN_TEMPLATE: &str =
        "/api/v1/planning/projects/{project_id}/discussions/{discussion_id}/reopen";

    /// Build the path for `GET` project verification.
    ///
    /// `project_id` is encoded exactly once with
    /// [`keryx::url::encode_path_segment`]. Identifiers containing `/`,
    /// `?`, `#`, spaces, or `:` are safe to pass in raw form.
    #[must_use]
    pub fn project_verification_path(project_id: &str) -> String {
        let encoded = encoding::path_segment(project_id);
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
        let encoded = encoding::path_segment(project_id);
        format!("/api/v1/planning/projects/{encoded}/verification/refresh")
    }

    /// Build the absolute URL for `POST` project verification refresh.
    #[must_use]
    pub fn project_verification_refresh_url(base_url: &str, project_id: &str) -> String {
        keryx::url::join_base_path(base_url, &project_verification_refresh_path(project_id))
    }

    /// Build the path for updating one requirement.
    #[must_use]
    pub fn project_requirement_path(project_id: &str, requirement_id: &str) -> String {
        let project = encoding::path_segment(project_id);
        let requirement = encoding::path_segment(requirement_id);
        format!("/api/v1/planning/projects/{project}/requirements/{requirement}")
    }

    /// Build the absolute URL for updating one requirement.
    #[must_use]
    pub fn project_requirement_url(
        base_url: &str,
        project_id: &str,
        requirement_id: &str,
    ) -> String {
        keryx::url::join_base_path(
            base_url,
            &project_requirement_path(project_id, requirement_id),
        )
    }

    /// Build the path for a checkpoint action.
    #[must_use]
    pub fn project_checkpoint_action_path(project_id: &str, checkpoint_id: &str) -> String {
        let project = encoding::path_segment(project_id);
        let checkpoint = encoding::path_segment(checkpoint_id);
        format!("/api/v1/planning/projects/{project}/checkpoints/{checkpoint}/action")
    }

    /// Build the absolute URL for a checkpoint action.
    #[must_use]
    pub fn project_checkpoint_action_url(
        base_url: &str,
        project_id: &str,
        checkpoint_id: &str,
    ) -> String {
        keryx::url::join_base_path(
            base_url,
            &project_checkpoint_action_path(project_id, checkpoint_id),
        )
    }

    /// Build the path for a category proposal action.
    #[must_use]
    pub fn project_proposal_path(project_id: &str, proposal_id: &str) -> String {
        let project = encoding::path_segment(project_id);
        let proposal = encoding::path_segment(proposal_id);
        format!("/api/v1/planning/projects/{project}/proposals/{proposal}")
    }

    /// Build the absolute URL for a category proposal action.
    #[must_use]
    pub fn project_proposal_url(base_url: &str, project_id: &str, proposal_id: &str) -> String {
        keryx::url::join_base_path(base_url, &project_proposal_path(project_id, proposal_id))
    }

    /// Build the path for answering a planning discussion.
    #[must_use]
    pub fn project_discussion_answer_path(project_id: &str, discussion_id: &str) -> String {
        let project = encoding::path_segment(project_id);
        let discussion = encoding::path_segment(discussion_id);
        format!("/api/v1/planning/projects/{project}/discussions/{discussion}/answer")
    }

    /// Build the absolute URL for answering a planning discussion.
    #[must_use]
    pub fn project_discussion_answer_url(
        base_url: &str,
        project_id: &str,
        discussion_id: &str,
    ) -> String {
        keryx::url::join_base_path(
            base_url,
            &project_discussion_answer_path(project_id, discussion_id),
        )
    }

    /// Build the path for reopening a planning discussion.
    #[must_use]
    pub fn project_discussion_reopen_path(project_id: &str, discussion_id: &str) -> String {
        let project = encoding::path_segment(project_id);
        let discussion = encoding::path_segment(discussion_id);
        format!("/api/v1/planning/projects/{project}/discussions/{discussion}/reopen")
    }

    /// Build the absolute URL for reopening a planning discussion.
    #[must_use]
    pub fn project_discussion_reopen_url(
        base_url: &str,
        project_id: &str,
        discussion_id: &str,
    ) -> String {
        keryx::url::join_base_path(
            base_url,
            &project_discussion_reopen_path(project_id, discussion_id),
        )
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::planning::*;
    use super::{ClientRouteContract, SKENE_CLIENT_ROUTE_CONTRACTS};
    use super::{config, encoding, nous, sessions, system};

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

    #[test]
    fn query_value_uses_query_context_not_path_context() {
        assert_eq!(encoding::path_segment("open ai"), "open%20ai");
        assert_eq!(encoding::query_value("open ai"), "open+ai");
        assert_eq!(
            encoding::query_value("open ai/a?b#c:100%"),
            "open+ai%2Fa%3Fb%23c%3A100%25"
        );
    }

    #[test]
    fn session_routes_encode_path_and_query_inputs() {
        assert_eq!(
            sessions::sessions_for_agent_path("agent/a?b#c:100%"),
            "/api/v1/sessions?nous_id=agent%2Fa%3Fb%23c%3A100%25"
        );
        assert_eq!(
            sessions::session_history_path("session/a?b#c"),
            "/api/v1/sessions/session%2Fa%3Fb%23c/history"
        );
        assert_eq!(
            sessions::session_archive_path("session:one"),
            "/api/v1/sessions/session%3Aone/archive"
        );
        assert_eq!(
            sessions::session_unarchive_path("session%2Fone"),
            "/api/v1/sessions/session%252Fone/unarchive"
        );
        assert_eq!(
            sessions::session_name_path("session one"),
            "/api/v1/sessions/session%20one/name"
        );
    }

    #[test]
    fn credential_routes_encode_path_and_query_inputs() {
        assert_eq!(
            system::credential_path("anthropic:primary"),
            "/api/v1/system/credentials/anthropic%3Aprimary"
        );
        assert_eq!(
            system::credential_validate_path("provider/id?x#y"),
            "/api/v1/system/credentials/provider%2Fid%3Fx%23y/validate"
        );
        assert_eq!(
            system::credential_rotate_path("open ai/a?b#c:100%"),
            "/api/v1/system/credentials/rotate?provider=open+ai%2Fa%3Fb%23c%3A100%25"
        );
    }

    #[test]
    fn agent_and_config_routes_encode_path_segments() {
        assert_eq!(
            nous::agent_path("agent/alpha?beta#gamma"),
            "/api/v1/nous/agent%2Falpha%3Fbeta%23gamma"
        );
        assert_eq!(
            nous::agent_tools_path("agent:tools"),
            "/api/v1/nous/agent%3Atools/tools"
        );
        assert_eq!(
            config::section_path("feature/flags"),
            "/api/v1/config/feature%2Fflags"
        );
    }

    #[test]
    fn planning_action_routes_encode_each_identifier_segment() {
        assert_eq!(
            project_requirement_path("proj/a?b", "req#one two"),
            "/api/v1/planning/projects/proj%2Fa%3Fb/requirements/req%23one%20two"
        );
        assert_eq!(
            project_checkpoint_action_path("proj:1", "check%2Fpoint"),
            "/api/v1/planning/projects/proj%3A1/checkpoints/check%252Fpoint/action"
        );
        assert_eq!(
            project_proposal_path("プロジェクト", "proposal/1"),
            "/api/v1/planning/projects/%E3%83%97%E3%83%AD%E3%82%B8%E3%82%A7%E3%82%AF%E3%83%88/proposals/proposal%2F1"
        );
        assert_eq!(
            project_discussion_answer_path("project", "discussion?1"),
            "/api/v1/planning/projects/project/discussions/discussion%3F1/answer"
        );
        assert_eq!(
            project_discussion_reopen_path("project", "discussion#1"),
            "/api/v1/planning/projects/project/discussions/discussion%231/reopen"
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

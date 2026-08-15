//! Versioned manifest format for scenarios that don't need Rust code.
//!
//! Two scenario shapes:
//!
//! - [`ManifestScenarioDef`]: a single GET request whose response body is
//!   checked against an optional substring/pattern assertion. The
//!   narrowest shape; proves the round trip end to end.
//! - [`SessionScenarioDef`]: creates a session, optionally seeds memory,
//!   sends a message, and asserts on the response text plus which tools
//!   were (or were not) called. Covers memory/context seed state and tool
//!   discipline -- two of the three gaps the narrowest shape left open.
//!   Tool *availability/permission policy* (restricting which tools an
//!   agent may call in the first place, as opposed to observing which it
//!   did call) is not covered: the eval HTTP API has no per-request knob
//!   for it today -- it's configured at the agent/`NousConfig` level, not
//!   per session or message -- so a manifest field for it would not
//!   actually control anything. That's a real API-surface gap, not an
//!   effort-based scope cut.
//!
//! Both shapes construct a real [`Scenario`] and run it against a live
//! instance through the same harness every hand-written scenario uses
//! (`validate_response` is the exact function `ScenarioMeta::expected_contains`
//! / `expected_pattern` are validated against everywhere else).

use serde::{Deserialize, Serialize};
use snafu::ResultExt;

use crate::client::EvalClient;
use crate::error;
use crate::scenario::{
    Scenario, ScenarioClassification, ScenarioFuture, ScenarioMeta, assert_eval, validate_response,
};

/// Schema version for [`ScenarioManifest`]. Bump on a breaking field change;
/// [`parse_manifest`] does not (yet) reject unknown versions, so a bump is
/// a documentation commitment until a loader-side check is added.
pub const MANIFEST_SCHEMA_VERSION: u32 = 1;

/// A file of manifest-defined scenarios.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScenarioManifest {
    /// Schema version this manifest was written against.
    pub schema_version: u32,
    /// The GET-based scenarios this manifest defines.
    #[serde(default)]
    pub scenarios: Vec<ManifestScenarioDef>,
    /// The session-based scenarios this manifest defines.
    #[serde(default)]
    pub session_scenarios: Vec<SessionScenarioDef>,
}

/// One manifest-defined scenario: a single GET plus an optional
/// substring/regex assertion against the response body.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManifestScenarioDef {
    /// Unique identifier (e.g., "health-returns-ok-manifest").
    pub id: String,
    /// Human-readable description.
    pub description: String,
    /// Category for grouping in output (e.g., "health").
    pub category: String,
    /// Whether this scenario requires an auth token.
    #[serde(default)]
    pub requires_auth: bool,
    /// Whether this scenario requires at least one configured nous.
    #[serde(default)]
    pub requires_nous: bool,
    /// Classification of the scenario's intent. Defaults to `Assertive`.
    #[serde(default)]
    pub classification: ScenarioClassification,
    /// Relative path requested via `EvalClient::raw_get`.
    pub path: String,
    /// Substring the response body must contain, if set.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected_contains: Option<String>,
    /// Regex the response body must match, if set.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected_pattern: Option<String>,
}

/// Parse a [`ScenarioManifest`] from a JSON document.
///
/// # Errors
///
/// Returns an error if the document is not valid JSON or does not match
/// the manifest schema.
pub fn parse_manifest(json_source: &str) -> Result<ScenarioManifest, serde_json::Error> {
    serde_json::from_str(json_source)
}

/// A manifest-defined scenario, constructed once at load time.
///
/// WHY the leaked strings: [`ScenarioMeta`]'s fields are `&'static str` --
/// every hand-written scenario satisfies this trivially with string
/// literals baked into the binary. A manifest-loaded scenario has no
/// literal to borrow from, but a manifest is loaded once at process start
/// and lives for the process's lifetime -- the same lifetime a literal
/// would have -- so leaking is a deliberate, bounded trade: one small
/// allocation per manifest scenario, never repeated, in exchange for
/// reusing `ScenarioMeta` and `validate_response` unchanged rather than
/// widening every existing scenario's field types to accommodate one new
/// producer.
pub struct ManifestScenario {
    id: &'static str,
    description: &'static str,
    category: &'static str,
    requires_auth: bool,
    requires_nous: bool,
    classification: ScenarioClassification,
    path: String,
    expected_contains: Option<&'static str>,
    expected_pattern: Option<&'static str>,
}

impl ManifestScenario {
    /// Construct a runnable scenario from a parsed manifest definition.
    #[must_use]
    pub fn from_def(def: ManifestScenarioDef) -> Self {
        Self {
            id: leak(def.id),
            description: leak(def.description),
            category: leak(def.category),
            requires_auth: def.requires_auth,
            requires_nous: def.requires_nous,
            classification: def.classification,
            path: def.path,
            expected_contains: def.expected_contains.map(leak),
            expected_pattern: def.expected_pattern.map(leak),
        }
    }
}

fn leak(s: String) -> &'static str {
    Box::leak(s.into_boxed_str())
}

impl Scenario for ManifestScenario {
    fn meta(&self) -> ScenarioMeta {
        ScenarioMeta {
            id: self.id,
            description: self.description,
            category: self.category,
            requires_auth: self.requires_auth,
            requires_nous: self.requires_nous,
            expected_contains: self.expected_contains,
            expected_pattern: self.expected_pattern,
            classification: self.classification,
        }
    }

    fn run<'a>(&'a self, client: &'a EvalClient) -> ScenarioFuture<'a> {
        Box::pin(async move {
            let meta = self.meta();
            let result: crate::error::Result<()> = async {
                let response = client.raw_get(&self.path).await?;
                let body = response.text().await.context(error::HttpSnafu)?;
                validate_response(&meta, &body)
            }
            .await;
            result.into()
        })
    }
}

/// One manifest-defined session scenario: create a session, optionally
/// seed memory, send a message, and assert on the response text plus tool
/// discipline. See the module docs for what this does and does not cover.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionScenarioDef {
    /// Unique identifier (e.g., "recall-seeded-fact-manifest").
    pub id: String,
    /// Human-readable description.
    pub description: String,
    /// Category for grouping in output (e.g., "session").
    pub category: String,
    /// Whether this scenario requires an auth token.
    #[serde(default)]
    pub requires_auth: bool,
    /// Classification of the scenario's intent. Defaults to `Assertive`.
    #[serde(default)]
    pub classification: ScenarioClassification,
    /// Nous agent id to create the session under.
    pub nous_id: String,
    /// Content to ingest into the nous's knowledge store before sending
    /// `message` (memory/context seed state), via
    /// `EvalClient::ingest_transcript`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub memory_seed: Option<String>,
    /// User message to send.
    pub message: String,
    /// Substring the response text must contain, if set.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected_contains: Option<String>,
    /// Regex the response text must match, if set.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected_pattern: Option<String>,
    /// Tool names that must appear among the response's `tool_use` events.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub required_tools: Vec<String>,
    /// Tool names that must NOT appear among the response's `tool_use`
    /// events.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub forbidden_tools: Vec<String>,
}

/// A manifest-defined session scenario, constructed once at load time. See
/// [`ManifestScenario`]'s docs for why its string fields are leaked.
pub struct SessionScenario {
    id: &'static str,
    description: &'static str,
    category: &'static str,
    requires_auth: bool,
    classification: ScenarioClassification,
    nous_id: String,
    memory_seed: Option<String>,
    message: String,
    expected_contains: Option<&'static str>,
    expected_pattern: Option<&'static str>,
    required_tools: Vec<String>,
    forbidden_tools: Vec<String>,
}

impl SessionScenario {
    /// Construct a runnable scenario from a parsed manifest definition.
    #[must_use]
    pub fn from_def(def: SessionScenarioDef) -> Self {
        Self {
            id: leak(def.id),
            description: leak(def.description),
            category: leak(def.category),
            requires_auth: def.requires_auth,
            classification: def.classification,
            nous_id: def.nous_id,
            memory_seed: def.memory_seed,
            message: def.message,
            expected_contains: def.expected_contains.map(leak),
            expected_pattern: def.expected_pattern.map(leak),
            required_tools: def.required_tools,
            forbidden_tools: def.forbidden_tools,
        }
    }
}

impl Scenario for SessionScenario {
    fn meta(&self) -> ScenarioMeta {
        ScenarioMeta {
            id: self.id,
            description: self.description,
            category: self.category,
            requires_auth: self.requires_auth,
            // WHY always true: this scenario kind creates a session under
            // nous_id as its first step, so it cannot run without one.
            requires_nous: true,
            expected_contains: self.expected_contains,
            expected_pattern: self.expected_pattern,
            classification: self.classification,
        }
    }

    fn run<'a>(&'a self, client: &'a EvalClient) -> ScenarioFuture<'a> {
        Box::pin(async move {
            let meta = self.meta();
            let result: crate::error::Result<()> = async {
                if let Some(seed) = &self.memory_seed {
                    client.ingest_transcript(&self.nous_id, seed).await?;
                }
                let session = client.create_session(&self.nous_id, self.id).await?;
                let events = client.send_message(&session.id, &self.message).await?;

                let text = crate::sse::extract_text(&events);
                validate_response(&meta, &text)?;

                let used = crate::sse::tool_names_used(&events);
                for required in &self.required_tools {
                    assert_eval(
                        used.iter().any(|name| name == required),
                        format!("required tool {required:?} was not called; tools used: {used:?}"),
                    )?;
                }
                for forbidden in &self.forbidden_tools {
                    assert_eval(
                        !used.iter().any(|name| name == forbidden),
                        format!("forbidden tool {forbidden:?} was called; tools used: {used:?}"),
                    )?;
                }
                Ok(())
            }
            .await;
            result.into()
        })
    }
}

/// Construct runnable [`Scenario`] trait objects from every scenario in a
/// parsed manifest.
#[must_use]
pub fn scenarios_from_manifest(manifest: ScenarioManifest) -> Vec<Box<dyn Scenario>> {
    let mut result: Vec<Box<dyn Scenario>> = manifest
        .scenarios
        .into_iter()
        .map(|def| -> Box<dyn Scenario> { Box::new(ManifestScenario::from_def(def)) })
        .collect();
    result.extend(
        manifest
            .session_scenarios
            .into_iter()
            .map(|def| -> Box<dyn Scenario> { Box::new(SessionScenario::from_def(def)) }),
    );
    result
}

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test assertions")]
mod tests {
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    use super::*;

    fn init_crypto() {
        if rustls::crypto::ring::default_provider()
            .install_default()
            .is_err()
        {
            // Already installed by another test in this process.
        }
    }

    const HEALTH_MANIFEST_JSON: &str = r#"{
        "schema_version": 1,
        "scenarios": [
            {
                "id": "health-ok-manifest",
                "description": "manifest-driven health check",
                "category": "health",
                "path": "/api/health",
                "expected_contains": "healthy"
            }
        ]
    }"#;

    #[test]
    fn parse_manifest_round_trips_scenario_def() {
        let manifest = parse_manifest(HEALTH_MANIFEST_JSON).expect("valid manifest JSON");
        assert_eq!(manifest.schema_version, MANIFEST_SCHEMA_VERSION);
        assert_eq!(manifest.scenarios.len(), 1);
        let def = manifest
            .scenarios
            .first()
            .expect("HEALTH_MANIFEST_JSON declares exactly one scenario");
        assert_eq!(def.id, "health-ok-manifest");
        assert_eq!(def.path, "/api/health");
        assert_eq!(def.expected_contains.as_deref(), Some("healthy"));
        assert_eq!(def.classification, ScenarioClassification::Assertive);
    }

    #[test]
    fn scenarios_from_manifest_meta_matches_definition() {
        let manifest = parse_manifest(HEALTH_MANIFEST_JSON).expect("valid manifest JSON");
        let scenarios = scenarios_from_manifest(manifest);
        assert_eq!(scenarios.len(), 1);
        let meta = scenarios
            .first()
            .expect("HEALTH_MANIFEST_JSON declares exactly one scenario")
            .meta();
        assert_eq!(meta.id, "health-ok-manifest");
        assert_eq!(meta.category, "health");
        assert_eq!(meta.expected_contains, Some("healthy"));
        assert!(!meta.requires_auth);
        assert!(!meta.requires_nous);
    }

    #[tokio::test]
    async fn manifest_scenario_passes_when_response_matches() {
        init_crypto();
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api/health"))
            .respond_with(ResponseTemplate::new(200).set_body_string("{\"status\":\"healthy\"}"))
            .mount(&server)
            .await;

        let manifest = parse_manifest(HEALTH_MANIFEST_JSON).expect("valid manifest JSON");
        let scenarios = scenarios_from_manifest(manifest);
        let client = EvalClient::new(server.uri(), None);

        let outcome = scenarios
            .first()
            .expect("HEALTH_MANIFEST_JSON declares exactly one scenario")
            .run(&client)
            .await;
        assert!(
            outcome.result.is_ok(),
            "manifest scenario should pass when the response contains the expected substring: {:?}",
            outcome.result
        );
    }

    #[tokio::test]
    async fn manifest_scenario_fails_when_response_does_not_match() {
        init_crypto();
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api/health"))
            .respond_with(ResponseTemplate::new(200).set_body_string("{\"status\":\"degraded\"}"))
            .mount(&server)
            .await;

        let manifest = parse_manifest(HEALTH_MANIFEST_JSON).expect("valid manifest JSON");
        let scenarios = scenarios_from_manifest(manifest);
        let client = EvalClient::new(server.uri(), None);

        let outcome = scenarios
            .first()
            .expect("HEALTH_MANIFEST_JSON declares exactly one scenario")
            .run(&client)
            .await;
        assert!(
            outcome.result.is_err(),
            "manifest scenario must fail a response that lacks the expected substring"
        );
    }

    const SESSION_RESPONSE_BODY: &str = r#"{
        "id": "ses-manifest-1",
        "nous_id": "alice",
        "session_key": "manifest-session-scenario",
        "status": "active",
        "model": "test-model",
        "message_count": 0,
        "token_count_estimate": 0,
        "created_at": "2026-01-01T00:00:00Z",
        "updated_at": "2026-01-01T00:00:00Z"
    }"#;

    /// Mount the session-creation and message-send endpoints a
    /// `SessionScenario` run needs. `sse_body` is the raw SSE text
    /// `send_message` returns; the caller supplies `tool_use` events (or
    /// none) to drive the tool-discipline assertions under test.
    async fn mount_session_and_message(server: &MockServer, sse_body: &str) {
        Mock::given(method("POST"))
            .and(path("/api/v1/sessions"))
            .respond_with(ResponseTemplate::new(201).set_body_string(SESSION_RESPONSE_BODY))
            .mount(server)
            .await;
        Mock::given(method("POST"))
            .and(path("/api/v1/sessions/ses-manifest-1/messages"))
            .respond_with(ResponseTemplate::new(200).set_body_string(sse_body.to_owned()))
            .mount(server)
            .await;
    }

    fn session_manifest_json(extra_fields: &str) -> String {
        format!(
            r#"{{
                "schema_version": 1,
                "session_scenarios": [
                    {{
                        "id": "recall-manifest",
                        "description": "manifest-driven session scenario",
                        "category": "session",
                        "nous_id": "alice",
                        "message": "what tools did you use?",
                        {extra_fields}
                    }}
                ]
            }}"#
        )
    }

    #[test]
    fn parse_manifest_round_trips_session_scenario_def() {
        let json = session_manifest_json(
            r#""expected_contains": "ok", "required_tools": ["search"], "forbidden_tools": ["shell"]"#,
        );
        let manifest = parse_manifest(&json).expect("valid manifest JSON");
        assert_eq!(manifest.session_scenarios.len(), 1);
        let def = manifest
            .session_scenarios
            .first()
            .expect("session_manifest_json declares exactly one session scenario");
        assert_eq!(def.id, "recall-manifest");
        assert_eq!(def.nous_id, "alice");
        assert_eq!(def.required_tools, vec!["search".to_owned()]);
        assert_eq!(def.forbidden_tools, vec!["shell".to_owned()]);
    }

    #[test]
    fn session_scenario_meta_requires_nous_unconditionally() {
        let json = session_manifest_json(r#""expected_contains": "ok""#);
        let manifest = parse_manifest(&json).expect("valid manifest JSON");
        let scenarios = scenarios_from_manifest(manifest);
        let meta = scenarios
            .first()
            .expect("session_manifest_json declares exactly one session scenario")
            .meta();
        assert!(
            meta.requires_nous,
            "a session scenario cannot run without the nous it creates a session under"
        );
    }

    #[tokio::test]
    async fn session_scenario_passes_when_response_and_tools_match() {
        init_crypto();
        let server = MockServer::start().await;
        mount_session_and_message(
            &server,
            "event: text_delta\ndata: {\"text\":\"used search\"}\n\n\
             event: tool_use\ndata: {\"id\":\"1\",\"name\":\"search\",\"input\":{}}\n\n",
        )
        .await;

        let json = session_manifest_json(
            r#""expected_contains": "used search", "required_tools": ["search"], "forbidden_tools": ["shell"]"#,
        );
        let manifest = parse_manifest(&json).expect("valid manifest JSON");
        let scenarios = scenarios_from_manifest(manifest);
        let client = EvalClient::new(server.uri(), None);

        let outcome = scenarios
            .first()
            .expect("session_manifest_json declares exactly one session scenario")
            .run(&client)
            .await;
        assert!(
            outcome.result.is_ok(),
            "session scenario should pass when text and tool discipline both match: {:?}",
            outcome.result
        );
    }

    #[tokio::test]
    async fn session_scenario_fails_when_required_tool_missing() {
        init_crypto();
        let server = MockServer::start().await;
        mount_session_and_message(
            &server,
            "event: text_delta\ndata: {\"text\":\"used search\"}\n\n",
        )
        .await;

        let json = session_manifest_json(
            r#""expected_contains": "used search", "required_tools": ["search"]"#,
        );
        let manifest = parse_manifest(&json).expect("valid manifest JSON");
        let scenarios = scenarios_from_manifest(manifest);
        let client = EvalClient::new(server.uri(), None);

        let outcome = scenarios
            .first()
            .expect("session_manifest_json declares exactly one session scenario")
            .run(&client)
            .await;
        assert!(
            outcome.result.is_err(),
            "session scenario must fail when a required tool was never called"
        );
    }

    #[tokio::test]
    async fn session_scenario_fails_when_forbidden_tool_called() {
        init_crypto();
        let server = MockServer::start().await;
        mount_session_and_message(
            &server,
            "event: text_delta\ndata: {\"text\":\"ok\"}\n\n\
             event: tool_use\ndata: {\"id\":\"1\",\"name\":\"shell\",\"input\":{}}\n\n",
        )
        .await;

        let json =
            session_manifest_json(r#""expected_contains": "ok", "forbidden_tools": ["shell"]"#);
        let manifest = parse_manifest(&json).expect("valid manifest JSON");
        let scenarios = scenarios_from_manifest(manifest);
        let client = EvalClient::new(server.uri(), None);

        let outcome = scenarios
            .first()
            .expect("session_manifest_json declares exactly one session scenario")
            .run(&client)
            .await;
        assert!(
            outcome.result.is_err(),
            "session scenario must fail when a forbidden tool was called"
        );
    }

    #[tokio::test]
    async fn session_scenario_ingests_memory_seed_before_sending_message() {
        init_crypto();
        let server = MockServer::start().await;
        mount_session_and_message(&server, "event: text_delta\ndata: {\"text\":\"ok\"}\n\n").await;
        Mock::given(method("POST"))
            .and(path("/api/v1/knowledge/ingest"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_string(r#"{"inserted":1,"skipped":0,"errors":[]}"#),
            )
            .mount(&server)
            .await;

        let json = session_manifest_json(
            r#""expected_contains": "ok", "memory_seed": "Alice prefers dark mode""#,
        );
        let manifest = parse_manifest(&json).expect("valid manifest JSON");
        let scenarios = scenarios_from_manifest(manifest);
        let client = EvalClient::new(server.uri(), None);

        let outcome = scenarios
            .first()
            .expect("session_manifest_json declares exactly one session scenario")
            .run(&client)
            .await;
        assert!(
            outcome.result.is_ok(),
            "session scenario with a memory_seed should still pass: {:?}",
            outcome.result
        );

        let requests = server
            .received_requests()
            .await
            .expect("mock server should record requests");
        assert!(
            requests
                .iter()
                .any(|request| request.url.path() == "/api/v1/knowledge/ingest"),
            "memory_seed must actually trigger EvalClient::ingest_transcript, \
             not be silently ignored"
        );
    }
}

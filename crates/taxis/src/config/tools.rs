// kanon:ignore API/mixed-casing -- WHY: config fields use camelCase while protocol enum wire values use lowercase or snake_case.
//! Typed configuration for runtime-bridged external tools.
//!
//! The `[tools]` section is owned by `taxis` so it participates in the config
//! cascade (defaults → TOML → environment), secret interpolation/decryption,
//! unknown-field rejection, and validation.

use std::collections::HashMap;
use std::path::PathBuf;

use koina::secret::SecretString;
use serde::{Deserialize, Serialize};

/// Authentication configuration for external HTTP-based tools and MCP servers.
///
/// WHY: authenticated streamable HTTP MCP servers need first-class header or
/// bearer configuration without embedding secrets in URLs or relying on
/// machine-specific wrappers (#4633).
///
/// WHY `#[serde(tag = "type")]`: validation (`validate_tool_auth`) and callers
/// expect internal tagging — `{"type":"bearer","token":"…"}`. Without this
/// attr serde uses external tagging (`{"bearer":{"token":"…"}}`), so a config
/// that validates always fails to deserialize (feature non-functional, #4633).
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
pub enum ExternalToolAuth {
    /// Static bearer token sent as `Authorization: Bearer <token>`.
    Bearer {
        /// Bearer token value. Serializes as `[REDACTED]` to prevent secret leakage.
        #[serde(default)]
        token: SecretString,
    },
    /// Static custom header name and value.
    Header {
        /// HTTP header name.
        #[serde(skip_serializing_if = "String::is_empty", default)]
        name: String,
        /// HTTP header value. Serializes as `[REDACTED]` to prevent secret leakage.
        #[serde(default)]
        value: SecretString,
    },
    /// Read a token from an environment variable at connection time.
    EnvToken {
        /// HTTP header name that will carry the token.
        #[serde(skip_serializing_if = "String::is_empty", default)]
        header_name: String,
        /// Environment variable to read.
        #[serde(skip_serializing_if = "String::is_empty", default)]
        env_var: String,
    },
}

impl std::fmt::Debug for ExternalToolAuth {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Bearer { .. } => f
                .debug_struct("Bearer")
                .field("token", &"<redacted>")
                .finish(),
            Self::Header { name, .. } => f
                .debug_struct("Header")
                .field("name", name)
                .field("value", &"<redacted>")
                .finish(),
            Self::EnvToken {
                header_name,
                env_var,
            } => f
                .debug_struct("EnvToken")
                .field("header_name", header_name)
                .field("env_var", env_var)
                .finish(),
        }
    }
}

impl ExternalToolAuth {
    /// Validate that the auth variant has all required fields populated.
    #[must_use]
    pub fn validate(&self) -> bool {
        match self {
            Self::Bearer { token } => !token.expose_secret().is_empty(),
            Self::Header { name, value } => !name.is_empty() && !value.expose_secret().is_empty(),
            Self::EnvToken {
                header_name,
                env_var,
            } => !header_name.is_empty() && !env_var.is_empty(),
        }
    }
}

/// Root configuration for the `[tools]` section.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(default)]
#[serde(deny_unknown_fields)]
pub struct ExternalToolsConfig {
    /// Startup policy when a required external tool cannot be registered.
    pub required_failure_mode: ExternalToolRequiredFailureMode,
    /// Tools that every deployment must have. Unavailability follows `required_failure_mode`.
    pub required: HashMap<String, ExternalToolEntry>,
    /// Tools that are deployment-specific. Registered if available.
    pub optional: HashMap<String, ExternalToolEntry>,
}

/// Startup policy for unavailable `[tools.required]` declarations.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum ExternalToolRequiredFailureMode {
    /// Abort startup when a required tool cannot be registered.
    #[default]
    FailStartup,
    /// Continue startup in an explicit degraded state.
    Degraded,
}

/// A single external tool declaration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(deny_unknown_fields)]
pub struct ExternalToolEntry {
    /// Transport type for this tool.
    #[serde(rename = "type")]
    pub kind: ExternalToolKind,
    /// HTTP endpoint URL for `mcp` and `http` tool types.
    pub endpoint: Option<String>,
    /// Stdio MCP server command. For `type = "mcp"`, defaults to the config key.
    pub command: Option<String>,
    /// Stdio MCP server command arguments.
    #[serde(default)]
    pub args: Vec<String>,
    /// Stdio MCP server working directory.
    pub cwd: Option<PathBuf>,
    /// Extra environment for stdio MCP server processes.
    #[serde(default)]
    pub env: HashMap<String, String>,
    /// Human-readable description sent to the LLM.
    pub description: Option<String>,
    /// HTTP method for `http` tools. Defaults to POST.
    #[serde(default)]
    pub method: ExternalToolMethod,
    /// Tool groups that gate access to this HTTP tool.
    #[serde(default)]
    pub groups: Option<Vec<ExternalToolGroupId>>,
    /// Reversibility classification used to derive approval requirements.
    #[serde(default)]
    pub reversibility: Option<ExternalToolReversibility>,
    /// Optional authentication configuration for HTTP-based tools and MCP servers.
    #[serde(default)]
    pub auth: Option<ExternalToolAuth>,
}

impl Default for ExternalToolEntry {
    fn default() -> Self {
        Self {
            kind: ExternalToolKind::Http,
            endpoint: None,
            command: None,
            args: Vec::new(),
            cwd: None,
            env: HashMap::new(),
            description: None,
            method: ExternalToolMethod::default(),
            groups: None,
            reversibility: None,
            auth: None,
        }
    }
}

/// Transport/execution strategy for an external tool.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[non_exhaustive]
pub enum ExternalToolKind {
    /// Model Context Protocol server.
    Mcp,
    /// Generic HTTP endpoint.
    #[default]
    Http,
    /// Reference to a built-in tool (documentation only, not re-registered).
    Builtin,
}

/// HTTP method for an external HTTP tool.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
#[non_exhaustive]
pub enum ExternalToolMethod {
    /// Retrieve a resource.
    Get,
    /// Submit data to be processed.
    #[default]
    Post,
    /// Replace a resource.
    Put,
    /// Delete a resource.
    Delete,
    /// Apply a partial modification.
    Patch,
}

/// Tool-group labels accepted by external HTTP tool config.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum ExternalToolGroupId {
    /// File/code reading tools.
    Read,
    /// File, memory, or session-state mutation tools.
    Edit,
    /// Shell/cargo execution and local commands.
    Command,
    /// MCP tool invocation and external API calls.
    Mcp,
    /// Spawning sub-agents.
    SpawnSubtask,
    /// Planning and design tools.
    Plan,
    /// Tests, lints, checks, validation.
    Verify,
}

/// Reversibility labels accepted by external HTTP tool config.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum ExternalToolReversibility {
    /// Read-only operations with no side effects.
    FullyReversible,
    /// Effects can be undone.
    Reversible,
    /// Partial undo may be possible.
    PartiallyReversible,
    /// Effects cannot be undone.
    Irreversible,
}

#[cfg(test)]
mod tests {
    #![expect(clippy::expect_used, reason = "test assertions")]

    use super::*;

    #[test]
    fn required_tools_fail_startup_by_default() {
        assert_eq!(
            ExternalToolsConfig::default().required_failure_mode,
            ExternalToolRequiredFailureMode::FailStartup
        );
    }

    #[test]
    fn required_tool_failure_mode_deserializes_degraded() {
        let json = r#"{"requiredFailureMode":"degraded"}"#;
        let config: ExternalToolsConfig =
            serde_json::from_str(json).expect("requiredFailureMode must deserialize");
        assert_eq!(
            config.required_failure_mode,
            ExternalToolRequiredFailureMode::Degraded
        );
    }

    #[test]
    fn bearer_auth_debug_redacts_token() {
        let auth = ExternalToolAuth::Bearer {
            token: SecretString::from("super-secret"),
        };
        let debug = format!("{auth:?}");
        assert!(
            debug.contains("<redacted>"),
            "debug output must redact bearer token: {debug}"
        );
        assert!(
            !debug.contains("super-secret"),
            "debug output must not contain bearer token: {debug}"
        );
    }

    #[test]
    fn header_auth_debug_redacts_value() {
        let auth = ExternalToolAuth::Header {
            name: "X-Api-Key".to_owned(),
            value: SecretString::from("super-secret"),
        };
        let debug = format!("{auth:?}");
        assert!(
            debug.contains("X-Api-Key"),
            "debug output should keep header name: {debug}"
        );
        assert!(
            debug.contains("<redacted>"),
            "debug output must redact header value: {debug}"
        );
        assert!(
            !debug.contains("super-secret"),
            "debug output must not contain header value: {debug}"
        );
    }

    #[test]
    fn env_token_auth_validation_requires_both_fields() {
        assert!(
            ExternalToolAuth::EnvToken {
                header_name: "X-Token".to_owned(),
                env_var: "TOKEN".to_owned(),
            }
            .validate()
        );
        assert!(
            !ExternalToolAuth::EnvToken {
                header_name: "X-Token".to_owned(),
                env_var: String::new(),
            }
            .validate()
        );
    }

    #[test]
    fn bearer_auth_validation_requires_non_empty_token() {
        assert!(
            ExternalToolAuth::Bearer {
                token: SecretString::from("token"),
            }
            .validate()
        );
        assert!(
            !ExternalToolAuth::Bearer {
                token: SecretString::default(),
            }
            .validate()
        );
    }

    /// WHY: prove that the JSON format validation accepts (internal-tagged
    /// `{"type":"bearer","token":"…"}`) is exactly what serde deserializes.
    /// Before the `#[serde(tag = "type")]` fix, validation accepted this
    /// format but deserialization silently failed — the feature was
    /// non-functional (#4633).
    #[test]
    fn serde_round_trip_matches_validator_bearer() {
        let json = r#"{"type":"bearer","token":"my-secret-token"}"#;
        let auth: ExternalToolAuth =
            serde_json::from_str(json).expect("JSON accepted by validator must deserialize");
        assert!(
            auth.validate(),
            "deserialized auth must pass validate() — validator and deserializer must agree"
        );
        // Serialize must not leak the secret.
        let serialized =
            serde_json::to_string(&auth).expect("ExternalToolAuth must be serializable");
        assert!(
            !serialized.contains("my-secret-token"),
            "serialized output must not contain bearer secret: {serialized}"
        );
    }

    #[test]
    fn serde_round_trip_matches_validator_header() {
        let json = r#"{"type":"header","name":"X-Api-Key","value":"my-api-secret"}"#;
        let auth: ExternalToolAuth =
            serde_json::from_str(json).expect("JSON accepted by validator must deserialize");
        assert!(
            auth.validate(),
            "deserialized header auth must pass validate()"
        );
        let serialized =
            serde_json::to_string(&auth).expect("ExternalToolAuth must be serializable");
        assert!(
            !serialized.contains("my-api-secret"),
            "serialized output must not contain header secret: {serialized}"
        );
    }

    #[test]
    fn serde_round_trip_matches_validator_env_token() {
        let json = r#"{"type":"env_token","header_name":"X-Token","env_var":"MY_TOKEN_VAR"}"#;
        let auth: ExternalToolAuth =
            serde_json::from_str(json).expect("JSON accepted by validator must deserialize");
        assert!(
            auth.validate(),
            "deserialized env_token auth must pass validate()"
        );
    }

    #[test]
    fn serde_rejects_external_tagged_format() {
        // WHY: external tagging was the broken pre-fix format; deserialization
        // must reject it so misconfigured configs fail fast.
        let external_tagged = r#"{"bearer":{"token":"secret"}}"#;
        let result: Result<ExternalToolAuth, _> = serde_json::from_str(external_tagged);
        assert!(
            result.is_err(),
            "external-tagged format must be rejected — only internal tagging is valid"
        );
    }

    #[test]
    fn bearer_serialize_redacts_token() {
        let auth = ExternalToolAuth::Bearer {
            token: SecretString::from("actual-secret"),
        };
        let json = serde_json::to_string(&auth).expect("serialization must not fail");
        assert!(
            !json.contains("actual-secret"),
            "serialized bearer must not expose token: {json}"
        );
    }

    #[test]
    fn header_serialize_redacts_value() {
        let auth = ExternalToolAuth::Header {
            name: "Authorization".to_owned(),
            value: SecretString::from("actual-secret"),
        };
        let json = serde_json::to_string(&auth).expect("serialization must not fail");
        assert!(
            !json.contains("actual-secret"),
            "serialized header must not expose value: {json}"
        );
    }
}

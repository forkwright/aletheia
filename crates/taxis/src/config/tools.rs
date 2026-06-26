//! Typed configuration for runtime-bridged external tools.
//!
//! The `[tools]` section is owned by `taxis` so it participates in the config
//! cascade (defaults → TOML → environment), secret interpolation/decryption,
//! unknown-field rejection, and validation.

use std::collections::HashMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// Authentication configuration for external HTTP-based tools and MCP servers.
///
/// WHY: authenticated streamable HTTP MCP servers need first-class header or
/// bearer configuration without embedding secrets in URLs or relying on
/// machine-specific wrappers (#4633).
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
pub enum ExternalToolAuth {
    /// Static bearer token sent as `Authorization: Bearer <token>`.
    Bearer {
        /// Bearer token value.
        #[serde(skip_serializing_if = "String::is_empty", default)]
        token: String,
    },
    /// Static custom header name and value.
    Header {
        /// HTTP header name.
        #[serde(skip_serializing_if = "String::is_empty", default)]
        name: String,
        /// HTTP header value.
        #[serde(skip_serializing_if = "String::is_empty", default)]
        value: String,
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
            Self::Bearer { token } => !token.is_empty(),
            Self::Header { name, value } => !name.is_empty() && !value.is_empty(),
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
    /// Tools that every deployment must have. Startup warns if unavailable.
    pub required: HashMap<String, ExternalToolEntry>,
    /// Tools that are deployment-specific. Registered if available.
    pub optional: HashMap<String, ExternalToolEntry>,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bearer_auth_debug_redacts_token() {
        let auth = ExternalToolAuth::Bearer {
            token: "super-secret".to_owned(),
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
            value: "super-secret".to_owned(),
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
                token: "token".to_owned(),
            }
            .validate()
        );
        assert!(
            !ExternalToolAuth::Bearer {
                token: String::new(),
            }
            .validate()
        );
    }
}

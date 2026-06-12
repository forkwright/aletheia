//! Typed configuration for runtime-bridged external tools.
//!
//! The `[tools]` section is owned by `taxis` so it participates in the config
//! cascade (defaults → TOML → environment), secret interpolation/decryption,
//! unknown-field rejection, and validation.

use std::collections::HashMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

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

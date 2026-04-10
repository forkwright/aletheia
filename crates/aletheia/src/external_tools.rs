//! Pluggable external tool registry: config-driven tool discovery and execution.
//!
//! Reads the `[tools]` section from `aletheia.toml` and registers external tool
//! executors into the [`ToolRegistry`]. Each external tool is a thin HTTP proxy
//! that forwards tool calls to the configured endpoint.
//!
//! WHY: Different deployments have different MCP servers available.  A menos
//! instance has Semantic Scholar + `PubMed` + kanon-mcp.  A verda instance has
//! none.  This module lets operators declare available tools per-deployment
//! without hardcoded tool assumptions.

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use tracing::{info, warn};

use koina::id::ToolName;
use organon::registry::{ToolExecutor, ToolRegistry};
use organon::types::{
    InputSchema, PropertyDef, PropertyType, Reversibility, ToolCategory, ToolContext, ToolDef,
    ToolInput, ToolResult,
};
use taxis::oikos::Oikos;

// ── Config types ──────────���─────────────────────────────────────────────────

/// Root config for the `[tools]` section of `aletheia.toml`.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct ExternalToolsConfig {
    /// Tools that every deployment must have. Startup warns if unavailable.
    pub required: HashMap<String, ExternalToolEntry>,
    /// Tools that are deployment-specific. Registered if available.
    pub optional: HashMap<String, ExternalToolEntry>,
}

/// A single external tool declaration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct ExternalToolEntry {
    /// Transport type for this tool.
    #[serde(rename = "type")]
    pub kind: ExternalToolKind,
    /// HTTP endpoint URL for `mcp` and `http` tool types.
    #[serde(default)]
    pub endpoint: Option<String>,
    /// Human-readable description sent to the LLM.
    #[serde(default)]
    pub description: Option<String>,
}

/// Transport/execution strategy for an external tool.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum ExternalToolKind {
    /// Model Context Protocol server.
    Mcp,
    /// Generic HTTP POST endpoint.
    Http,
    /// Reference to a built-in tool (documentation only, not re-registered).
    Builtin,
}

// ── Tool manifest ───────────────────────────────────────────────────────────

/// Deployment-scoped summary of available tools.
///
/// Built at startup and available for agent bootstrap inspection.
/// Agents use the manifest to adapt behaviour based on what tools are
/// available rather than what tools are hardcoded.
#[derive(Debug, Clone, Serialize)]
pub(crate) struct ToolManifest {
    /// Tools declared as required in config.
    pub required: Vec<ToolManifestEntry>,
    /// Tools declared as optional in config.
    pub optional: Vec<ToolManifestEntry>,
}

impl ToolManifest {
    /// Count of tools that were successfully registered.
    #[must_use]
    pub(crate) fn available_count(&self) -> usize {
        self.required
            .iter()
            .chain(self.optional.iter())
            .filter(|e| e.available)
            .count()
    }

    /// Count of required tools that failed registration.
    #[must_use]
    pub(crate) fn missing_required_count(&self) -> usize {
        self.required.iter().filter(|e| !e.available).count()
    }
}

/// Per-tool availability information in the manifest.
#[derive(Debug, Clone, Serialize)]
pub(crate) struct ToolManifestEntry {
    /// Tool name as registered in the registry.
    pub name: String,
    /// Transport type.
    pub kind: ExternalToolKind,
    /// Whether the tool was successfully registered.
    pub available: bool,
    /// Description sent to the LLM.
    pub description: String,
}

// ── Config loader ─────────────���─────────────────────────────────────────────

/// Load the `[tools]` section from `aletheia.toml`.
///
/// Returns [`ExternalToolsConfig::default()`] if the file does not exist or
/// has no `[tools]` section. This makes external tools purely opt-in.
#[must_use]
pub(crate) fn load_tools_config(oikos: &Oikos) -> ExternalToolsConfig {
    let toml_path = oikos.config().join("aletheia.toml");
    let Ok(content) = std::fs::read_to_string(&toml_path) else {
        return ExternalToolsConfig::default();
    };

    // WHY: parse the full TOML and extract only the [tools] table rather than
    // deserializing the entire config.  This avoids coupling to AletheiaConfig
    // and keeps the external tools config self-contained in the binary crate.
    let table: toml::Value = match toml::from_str(&content) {
        Ok(v) => v,
        Err(e) => {
            warn!(error = %e, "failed to parse aletheia.toml for [tools] section");
            return ExternalToolsConfig::default();
        }
    };

    match table.get("tools") {
        Some(tools_value) => {
            // NOTE: clone is cheap here -- the tools section is small
            match tools_value.clone().try_into::<ExternalToolsConfig>() {
                Ok(config) => config,
                Err(e) => {
                    warn!(error = %e, "failed to deserialize [tools] config section");
                    ExternalToolsConfig::default()
                }
            }
        }
        None => ExternalToolsConfig::default(),
    }
}

// ── Registration ────────────────────────────────────────────────────────────

/// Register external tools into the [`ToolRegistry`] and build a manifest.
///
/// For each declared tool:
/// - `builtin` entries are skipped (documentation-only markers).
/// - `mcp` and `http` entries with an endpoint get an [`ExternalToolExecutor`]
///   registered into the tool registry.
/// - Missing endpoints produce a warning and mark the tool unavailable.
pub(crate) fn register_external_tools(
    config: &ExternalToolsConfig,
    registry: &mut ToolRegistry,
    http_client: &reqwest::Client,
) -> ToolManifest {
    let mut manifest = ToolManifest {
        required: Vec::new(),
        optional: Vec::new(),
    };

    // WHY: process required tools first so startup warnings about missing
    // required tools appear before optional tool info.
    for (name, entry) in &config.required {
        let result = register_single_tool(name, entry, registry, http_client);
        manifest.required.push(result);
    }

    for (name, entry) in &config.optional {
        let result = register_single_tool(name, entry, registry, http_client);
        manifest.optional.push(result);
    }

    manifest
}

/// Attempt to register a single external tool.
fn register_single_tool(
    name: &str,
    entry: &ExternalToolEntry,
    registry: &mut ToolRegistry,
    http_client: &reqwest::Client,
) -> ToolManifestEntry {
    let description = entry
        .description
        .clone()
        .unwrap_or_else(|| format!("External tool: {name}"));

    // NOTE: builtin entries are markers -- the tool is already registered by
    // organon::builtins. We record them in the manifest for completeness.
    if entry.kind == ExternalToolKind::Builtin {
        let available = ToolName::new(name)
            .ok()
            .and_then(|tn| registry.get_def(&tn))
            .is_some();
        return ToolManifestEntry {
            name: name.to_owned(),
            kind: entry.kind,
            available,
            description,
        };
    }

    let Some(endpoint) = entry.endpoint.clone() else {
        warn!(tool = name, "external tool has no endpoint configured");
        return ToolManifestEntry {
            name: name.to_owned(),
            kind: entry.kind,
            available: false,
            description,
        };
    };

    let tool_name = match ToolName::new(name) {
        Ok(tn) => tn,
        Err(e) => {
            warn!(tool = name, error = %e, "invalid external tool name");
            return ToolManifestEntry {
                name: name.to_owned(),
                kind: entry.kind,
                available: false,
                description,
            };
        }
    };

    let tool_def = ToolDef {
        name: tool_name.clone(),
        description: description.clone(),
        extended_description: None,
        input_schema: external_tool_schema(),
        category: ToolCategory::Research,
        reversibility: Reversibility::FullyReversible,
        auto_activate: false,
    };

    let executor = ExternalToolExecutor {
        name: name.to_owned(),
        endpoint,
        client: http_client.clone(),
        kind: entry.kind,
    };

    match registry.register(tool_def, Box::new(executor)) {
        Ok(()) => {
            info!(tool = name, kind = ?entry.kind, "external tool registered");
            ToolManifestEntry {
                name: name.to_owned(),
                kind: entry.kind,
                available: true,
                description,
            }
        }
        Err(e) => {
            warn!(tool = name, error = %e, "failed to register external tool");
            ToolManifestEntry {
                name: name.to_owned(),
                kind: entry.kind,
                available: false,
                description,
            }
        }
    }
}

/// Input schema for external tools.
///
/// Accepts a freeform `query` string and an optional JSON `params` object.
fn external_tool_schema() -> InputSchema {
    let mut properties = IndexMap::new();
    properties.insert(
        "query".to_owned(),
        PropertyDef {
            property_type: PropertyType::String,
            description: "Query or command to send to the external tool".to_owned(),
            enum_values: None,
            default: None,
        },
    );
    properties.insert(
        "params".to_owned(),
        PropertyDef {
            property_type: PropertyType::Object,
            description: "Additional parameters as a JSON object (tool-specific)".to_owned(),
            enum_values: None,
            default: None,
        },
    );
    InputSchema {
        properties,
        required: vec!["query".to_owned()],
    }
}

// ── Executor ──────────────���─────────────────────────────────────────────────

/// Thin HTTP proxy executor for external tools.
///
/// Forwards tool calls to the configured endpoint as JSON POST requests.
/// The response body is returned as the tool result text.
struct ExternalToolExecutor {
    name: String,
    endpoint: String,
    client: reqwest::Client,
    kind: ExternalToolKind,
}

impl ToolExecutor for ExternalToolExecutor {
    fn execute<'a>(
        &'a self,
        input: &'a ToolInput,
        _ctx: &'a ToolContext,
    ) -> Pin<Box<dyn Future<Output = organon::error::Result<ToolResult>> + Send + 'a>>
    {
        Box::pin(async move {
            let payload = serde_json::json!({
                "tool": self.name,
                "kind": format!("{:?}", self.kind),
                "arguments": input.arguments,
            });

            let response = self
                .client
                .post(&self.endpoint)
                .json(&payload)
                .timeout(std::time::Duration::from_secs(30))
                .send()
                .await;

            match response {
                Ok(resp) => {
                    let status = resp.status();
                    let body = resp.text().await.unwrap_or_default();
                    if status.is_success() {
                        Ok(ToolResult::text(body))
                    } else {
                        Ok(ToolResult::error(format!(
                            "External tool '{name}' returned HTTP {status}: {body}",
                            name = self.name
                        )))
                    }
                }
                Err(e) => Ok(ToolResult::error(format!(
                    "External tool '{name}' request failed: {e}",
                    name = self.name
                ))),
            }
        })
    }
}

// ── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test assertions")]
mod tests {
    use super::*;

    /// WHY: reqwest requires a rustls `CryptoProvider` to be installed. In
    /// production this happens in `main()`. Tests must install it themselves.
    fn ensure_crypto_provider() {
        let _ = rustls::crypto::ring::default_provider().install_default();
    }

    #[test]
    fn deserialize_tools_config() {
        let toml_str = r#"
[required]
file_ops = { type = "builtin" }
shell = { type = "builtin" }

[optional]
semantic_scholar = { type = "mcp", endpoint = "http://localhost:3100", description = "Search academic papers via Semantic Scholar" }
pubmed = { type = "http", endpoint = "http://localhost:3101", description = "Search PubMed articles" }
"#;
        let config: ExternalToolsConfig =
            toml::from_str(toml_str).expect("should deserialize tools config");

        assert_eq!(config.required.len(), 2);
        assert_eq!(config.optional.len(), 2);

        let file_ops = config.required.get("file_ops").expect("file_ops");
        assert_eq!(file_ops.kind, ExternalToolKind::Builtin);
        assert!(file_ops.endpoint.is_none());

        let scholar = config.optional.get("semantic_scholar").expect("scholar");
        assert_eq!(scholar.kind, ExternalToolKind::Mcp);
        assert_eq!(scholar.endpoint.as_deref(), Some("http://localhost:3100"));
        assert_eq!(
            scholar.description.as_deref(),
            Some("Search academic papers via Semantic Scholar")
        );
    }

    #[test]
    fn empty_config_is_default() {
        let config = ExternalToolsConfig::default();
        assert!(config.required.is_empty());
        assert!(config.optional.is_empty());
    }

    #[test]
    fn register_builtin_tool_checks_registry() {
        ensure_crypto_provider();
        let mut registry = ToolRegistry::new();
        let entry = ExternalToolEntry {
            kind: ExternalToolKind::Builtin,
            endpoint: None,
            description: Some("built-in file ops".to_owned()),
        };
        let result = register_single_tool(
            "nonexistent_builtin",
            &entry,
            &mut registry,
            &reqwest::Client::new(),
        );
        // NOTE: builtin entry for a tool that doesn't exist in the registry
        assert!(!result.available);
        assert_eq!(result.kind, ExternalToolKind::Builtin);
    }

    #[test]
    fn register_mcp_tool_without_endpoint() {
        ensure_crypto_provider();
        let mut registry = ToolRegistry::new();
        let entry = ExternalToolEntry {
            kind: ExternalToolKind::Mcp,
            endpoint: None,
            description: Some("missing endpoint".to_owned()),
        };
        let result =
            register_single_tool("bad_tool", &entry, &mut registry, &reqwest::Client::new());
        assert!(!result.available);
    }

    #[test]
    fn register_mcp_tool_with_endpoint() {
        ensure_crypto_provider();
        let mut registry = ToolRegistry::new();
        let entry = ExternalToolEntry {
            kind: ExternalToolKind::Mcp,
            endpoint: Some("http://localhost:3100".to_owned()),
            description: Some("Semantic Scholar".to_owned()),
        };
        let result = register_single_tool(
            "semantic_scholar",
            &entry,
            &mut registry,
            &reqwest::Client::new(),
        );
        assert!(result.available);
        assert_eq!(result.name, "semantic_scholar");

        // NOTE: verify the tool is actually in the registry
        let tool_name = ToolName::new("semantic_scholar").expect("valid name");
        assert!(registry.get_def(&tool_name).is_some());
    }

    #[test]
    fn manifest_counts() {
        let manifest = ToolManifest {
            required: vec![
                ToolManifestEntry {
                    name: "shell".to_owned(),
                    kind: ExternalToolKind::Builtin,
                    available: true,
                    description: "shell".to_owned(),
                },
                ToolManifestEntry {
                    name: "missing".to_owned(),
                    kind: ExternalToolKind::Mcp,
                    available: false,
                    description: "missing".to_owned(),
                },
            ],
            optional: vec![ToolManifestEntry {
                name: "scholar".to_owned(),
                kind: ExternalToolKind::Mcp,
                available: true,
                description: "scholar".to_owned(),
            }],
        };
        assert_eq!(manifest.available_count(), 2);
        assert_eq!(manifest.missing_required_count(), 1);
    }

    #[test]
    fn full_registration_flow() {
        ensure_crypto_provider();
        let toml_str = r#"
[required]
file_ops = { type = "builtin" }

[optional]
test_tool = { type = "http", endpoint = "http://192.168.1.100:8080", description = "Test HTTP tool" }
no_endpoint = { type = "mcp" }
"#;
        let config: ExternalToolsConfig = toml::from_str(toml_str).expect("valid config");
        let mut registry = ToolRegistry::new();
        let client = reqwest::Client::new();

        let manifest = register_external_tools(&config, &mut registry, &client);

        // NOTE: file_ops is builtin but not registered in empty registry → unavailable
        assert_eq!(manifest.required.len(), 1);
        assert!(!manifest.required.iter().any(|e| e.available));

        // NOTE: test_tool should register, no_endpoint should not
        let test_entry = manifest.optional.iter().find(|e| e.name == "test_tool");
        assert!(test_entry.is_some_and(|e| e.available));

        let no_ep = manifest.optional.iter().find(|e| e.name == "no_endpoint");
        assert!(no_ep.is_some_and(|e| !e.available));
    }

    #[test]
    fn external_tool_schema_has_query() {
        let schema = external_tool_schema();
        assert!(schema.properties.contains_key("query"));
        assert!(schema.properties.contains_key("params"));
        assert!(schema.required.contains(&"query".to_owned()));
    }

    #[test]
    fn load_from_nonexistent_path_returns_default() {
        let oikos = taxis::oikos::Oikos::from_root("/nonexistent/path");
        let config = load_tools_config(&oikos);
        assert!(config.required.is_empty());
        assert!(config.optional.is_empty());
    }
}

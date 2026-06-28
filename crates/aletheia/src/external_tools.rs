// kanon:ignore RUST/file-too-long — cohesive tool registry: MCP client, HTTP proxy, and schema conversion are interdependent
//! Pluggable external tool registry: config-driven tool discovery and execution.
//!
//! The `[tools]` config is owned by `taxis` and consumed from
//! [`AletheiaConfig::tools`](taxis::config::AletheiaConfig). This module
//! registers external tool executors into the [`ToolRegistry`]. HTTP tools are
//! thin proxies using the configured method. MCP entries are server
//! registrations: Aletheia connects to the external MCP server, discovers tools
//! via `tools/list`, registers each discovered tool in organon, and routes
//! execution back through `tools/call`.
//!
//! WHY: Different deployments have different MCP servers available. One
//! deployment might expose Semantic Scholar + `PubMed` + an internal MCP server,
//! while another exposes none. This module lets operators declare available
//! tools per-deployment without hardcoded tool assumptions.

use std::future::Future;
use std::pin::Pin;

#[cfg(feature = "mcp")]
use diaporeia::client::{ExternalMcpClient, StdioMcpServerConfig};
use indexmap::IndexMap;
use serde::Serialize;
use tracing::{info, warn};

use koina::id::ToolName;
use organon::registry::{ToolExecutor, ToolRegistry};
#[cfg(feature = "mcp")]
use organon::types::ToolOrigin;
use organon::types::{
    ApprovalRequirement, InputSchema, PropertyDef, PropertyType, Reversibility, ToolCategory,
    ToolContext, ToolDef, ToolGroupId, ToolInput, ToolResult, ToolTag,
};
#[cfg(feature = "mcp")]
use taxis::config::ExternalToolAuth;
pub(crate) use taxis::config::{
    ExternalToolEntry, ExternalToolGroupId, ExternalToolKind, ExternalToolMethod,
    ExternalToolReversibility, ExternalToolsConfig,
};

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
    /// MCP server that exposes this tool, if any.
    pub server_name: Option<String>,
    /// Remote tool name on the MCP server, if any.
    pub remote_name: Option<String>,
}

// ── Registration ────────────────────────────────────────────────────────────

/// Register external tools into the [`ToolRegistry`] and build a manifest.
///
/// For each declared tool:
/// - `builtin` entries are skipped (documentation-only markers).
/// - `mcp` and `http` entries with an endpoint get an [`ExternalToolExecutor`]
///   registered into the tool registry.
/// - Missing endpoints produce a warning and mark the tool unavailable.
pub(crate) async fn register_external_tools(
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
        let result = register_single_tool(name, entry, registry, http_client).await;
        manifest.required.extend(result);
    }

    for (name, entry) in &config.optional {
        let result = register_single_tool(name, entry, registry, http_client).await;
        manifest.optional.extend(result);
    }

    manifest
}

/// Attempt to register a single external tool.
async fn register_single_tool(
    name: &str,
    entry: &ExternalToolEntry,
    registry: &mut ToolRegistry,
    http_client: &reqwest::Client,
) -> Vec<ToolManifestEntry> {
    #[cfg(not(feature = "mcp"))]
    std::future::ready(()).await;

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
        return vec![ToolManifestEntry {
            name: name.to_owned(),
            kind: entry.kind,
            available,
            description,
            server_name: None,
            remote_name: None,
        }];
    }

    if entry.kind == ExternalToolKind::Mcp {
        #[cfg(feature = "mcp")]
        return register_mcp_server(name, entry, registry, description).await;
        #[cfg(not(feature = "mcp"))]
        return register_mcp_server(name, entry, registry, description);
    }

    let Some(endpoint) = entry.endpoint.clone() else {
        warn!(tool = name, "external tool has no endpoint configured");
        return vec![ToolManifestEntry {
            name: name.to_owned(),
            kind: entry.kind,
            available: false,
            description,
            server_name: None,
            remote_name: None,
        }];
    };

    register_http_tool(name, entry, registry, http_client, description, endpoint)
}

fn register_http_tool(
    name: &str,
    entry: &ExternalToolEntry,
    registry: &mut ToolRegistry,
    http_client: &reqwest::Client,
    description: String,
    endpoint: String,
) -> Vec<ToolManifestEntry> {
    let tool_name = match ToolName::new(name) {
        Ok(tn) => tn,
        Err(e) => {
            warn!(tool = name, error = %e, "invalid external tool name");
            return vec![ToolManifestEntry {
                name: name.to_owned(),
                kind: entry.kind,
                available: false,
                description,
                server_name: None,
                remote_name: None,
            }];
        }
    };

    let groups = effective_http_groups(entry);
    let reversibility = effective_http_reversibility(entry);
    let approval = ApprovalRequirement::from(reversibility);
    let tool_def = ToolDef {
        name: tool_name.clone(),
        description: description.clone(),
        extended_description: None,
        input_schema: external_tool_schema(),
        category: ToolCategory::Research,
        reversibility,
        auto_activate: false,
        groups: groups.clone(),
        tags: tags_from_http_policy(reversibility),
    };

    let executor = ExternalToolExecutor {
        name: name.to_owned(),
        endpoint,
        client: http_client.clone(),
        kind: entry.kind,
        method: entry.method,
    };

    match registry.register(tool_def, Box::new(executor)) {
        Ok(()) => {
            info!(
                tool = name,
                kind = ?entry.kind,
                groups = %format_tool_groups(&groups),
                groups_source = if entry.groups.is_some() { "explicit" } else { "default" },
                reversibility = %reversibility,
                reversibility_source = if entry.reversibility.is_some() { "explicit" } else { "default" },
                approval = %approval,
                "external HTTP tool registered"
            );
            vec![ToolManifestEntry {
                name: name.to_owned(),
                kind: entry.kind,
                available: true,
                description,
                server_name: None,
                remote_name: None,
            }]
        }
        Err(e) => {
            warn!(tool = name, error = %e, "failed to register external tool");
            vec![ToolManifestEntry {
                name: name.to_owned(),
                kind: entry.kind,
                available: false,
                description,
                server_name: None,
                remote_name: None,
            }]
        }
    }
}

#[cfg(feature = "mcp")]
#[expect(
    clippy::too_many_lines,
    reason = "MCP registration iterates tool list with multi-branch error handling; cohesive logic"
)]
async fn register_mcp_server(
    server_name: &str,
    entry: &ExternalToolEntry,
    registry: &mut ToolRegistry,
    description: String,
) -> Vec<ToolManifestEntry> {
    let client = match connect_mcp_server(server_name, entry).await {
        Ok(client) => client,
        Err(e) => {
            warn!(server = server_name, error = %e, "failed to connect MCP server");
            return vec![ToolManifestEntry {
                name: server_name.to_owned(),
                kind: ExternalToolKind::Mcp,
                available: false,
                description,
                server_name: Some(server_name.to_owned()),
                remote_name: None,
            }];
        }
    };

    let remote_tools = match client.list_tools().await {
        Ok(tools) => tools,
        Err(e) => {
            warn!(server = server_name, error = %e, "failed to list MCP server tools");
            let _close_result = client.close().await;
            return vec![ToolManifestEntry {
                name: server_name.to_owned(),
                kind: ExternalToolKind::Mcp,
                available: false,
                description,
                server_name: Some(server_name.to_owned()),
                remote_name: None,
            }];
        }
    };

    if remote_tools.is_empty() {
        warn!(server = server_name, "MCP server exposed no tools");
        let _close_result = client.close().await;
        return vec![ToolManifestEntry {
            name: server_name.to_owned(),
            kind: ExternalToolKind::Mcp,
            available: false,
            description,
            server_name: Some(server_name.to_owned()),
            remote_name: None,
        }];
    }

    let mut entries = Vec::new();
    for remote_tool in remote_tools {
        let remote_name = remote_tool.name.to_string();
        let Some(tool_name) = allocate_mcp_tool_name(server_name, &remote_name, registry) else {
            warn!(
                server = server_name,
                remote_tool = %remote_name,
                "invalid MCP tool name after normalization"
            );
            entries.push(ToolManifestEntry {
                name: remote_name.clone(),
                kind: ExternalToolKind::Mcp,
                available: false,
                description: description.clone(),
                server_name: Some(server_name.to_owned()),
                remote_name: Some(remote_name),
            });
            continue;
        };
        let tool_description = remote_tool
            .description
            .as_ref()
            .map(ToString::to_string)
            .or_else(|| remote_tool.title.clone())
            .unwrap_or_else(|| format!("External MCP tool {remote_name} from {server_name}"));
        let tool_def = ToolDef {
            name: tool_name.clone(),
            description: tool_description.clone(),
            extended_description: Some(format!(
                "External MCP tool '{remote_name}' served by '{server_name}'."
            )),
            input_schema: input_schema_from_mcp(&remote_tool),
            category: ToolCategory::Research,
            reversibility: reversibility_from_mcp(&remote_tool),
            auto_activate: false,
            groups: vec![ToolGroupId::Mcp],
            tags: tags_from_mcp(&remote_tool),
        };
        let executor = ExternalMcpToolExecutor {
            server_name: server_name.to_owned(),
            remote_name: remote_name.clone(),
            client: client.clone(),
        };
        match registry.register(tool_def, Box::new(executor)) {
            Ok(()) => {
                info!(
                    server = server_name,
                    remote_tool = %remote_name,
                    tool = %tool_name,
                    "external MCP tool registered"
                );
                entries.push(ToolManifestEntry {
                    name: tool_name.as_str().to_owned(),
                    kind: ExternalToolKind::Mcp,
                    available: true,
                    description: tool_description,
                    server_name: Some(server_name.to_owned()),
                    remote_name: Some(remote_name.clone()),
                });
                registry.set_origin(
                    tool_name.clone(),
                    ToolOrigin {
                        local_name: tool_name.as_str().to_owned(),
                        server_name: server_name.to_owned(),
                        remote_name: remote_name.clone(),
                    },
                );
            }
            Err(e) => {
                warn!(
                    server = server_name,
                    remote_tool = %remote_name,
                    error = %e,
                    "failed to register external MCP tool"
                );
                entries.push(ToolManifestEntry {
                    name: tool_name.as_str().to_owned(),
                    kind: ExternalToolKind::Mcp,
                    available: false,
                    description: tool_description,
                    server_name: Some(server_name.to_owned()),
                    remote_name: Some(remote_name.clone()),
                });
            }
        }
    }

    entries
}

#[cfg(feature = "mcp")]
async fn connect_mcp_server(
    server_name: &str,
    entry: &ExternalToolEntry,
) -> diaporeia::error::Result<ExternalMcpClient> {
    if let Some(endpoint) = entry.endpoint.as_deref() {
        let auth = entry.auth.as_ref().map(mcp_auth_to_diaporeia);
        return ExternalMcpClient::connect_streamable_http_with_auth(endpoint, auth.as_ref()).await;
    }

    let command = entry
        .command
        .clone()
        .unwrap_or_else(|| server_name.to_owned());
    let config = StdioMcpServerConfig {
        command,
        args: entry.args.clone(),
        cwd: entry.cwd.clone(),
        env: entry.env.clone(),
    };
    ExternalMcpClient::connect_stdio(&config).await
}

#[cfg(feature = "mcp")]
fn mcp_auth_to_diaporeia(auth: &ExternalToolAuth) -> diaporeia::client::McpAuth {
    match auth {
        ExternalToolAuth::Bearer { token } => diaporeia::client::McpAuth::Bearer {
            token: token.expose_secret().to_owned(),
        },
        ExternalToolAuth::Header { name, value } => diaporeia::client::McpAuth::Header {
            name: name.clone(),
            value: value.expose_secret().to_owned(),
        },
        ExternalToolAuth::EnvToken {
            header_name,
            env_var,
        } => diaporeia::client::McpAuth::EnvToken {
            header_name: header_name.clone(),
            env_var: env_var.clone(),
        },
    }
}

#[cfg(not(feature = "mcp"))]
fn register_mcp_server(
    server_name: &str,
    entry: &ExternalToolEntry,
    _registry: &mut ToolRegistry,
    description: String,
) -> Vec<ToolManifestEntry> {
    let _ = entry;
    warn!(
        server = server_name,
        "MCP external tools require rebuilding aletheia with the `mcp` feature"
    );
    vec![ToolManifestEntry {
        name: server_name.to_owned(),
        kind: ExternalToolKind::Mcp,
        available: false,
        description,
        server_name: Some(server_name.to_owned()),
        remote_name: None,
    }]
}

#[cfg(feature = "mcp")]
fn allocate_mcp_tool_name(
    server_name: &str,
    remote_name: &str,
    registry: &ToolRegistry,
) -> Option<ToolName> {
    // WHY: Always namespace remote MCP tools as `mcp_<server>_<remote>`. Using
    // the bare normalized remote name when it happens not to collide makes the
    // public tool name depend on registration order, which breaks prompts,
    // allowlists, audit comparisons, and user expectations whenever built-ins
    // or other external tools are added or removed (#4632).
    let prefixed = normalize_tool_name(&format!("mcp_{server_name}_{remote_name}"));
    ToolName::new(prefixed)
        .ok()
        .filter(|candidate| registry.get_def(candidate).is_none())
}

#[cfg(feature = "mcp")]
fn normalize_tool_name(name: &str) -> String {
    let mut normalized: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    normalized = normalized.trim_matches('_').to_owned();
    if normalized.is_empty() {
        normalized.push_str("mcp_tool");
    }
    if normalized.len() > 128 {
        normalized.truncate(128);
    }
    normalized
}

#[cfg(feature = "mcp")]
fn input_schema_from_mcp(tool: &rmcp::model::Tool) -> InputSchema {
    let schema = serde_json::Value::Object(tool.input_schema.as_ref().clone());
    let required = schema
        .get("required")
        .and_then(serde_json::Value::as_array)
        .map(|values| {
            values
                .iter()
                .filter_map(serde_json::Value::as_str)
                .map(ToOwned::to_owned)
                .collect()
        })
        // kanon:ignore RUST/no-result-unwrap-or-default — serde_json "required" field is optional; empty vec is correct default when absent
        .unwrap_or_default();
    let mut properties = IndexMap::new();
    if let Some(props) = schema
        .get("properties")
        .and_then(serde_json::Value::as_object)
    {
        for (name, prop) in props {
            properties.insert(name.clone(), property_def_from_json(prop));
        }
    }

    InputSchema {
        properties,
        required,
    }
}

#[cfg(feature = "mcp")]
fn property_def_from_json(value: &serde_json::Value) -> PropertyDef {
    let property_type = value
        .get("type")
        .and_then(serde_json::Value::as_str)
        .map_or(PropertyType::String, property_type_from_str);
    let description = value
        .get("description")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("External MCP tool parameter")
        .to_owned();
    let enum_values = value
        .get("enum")
        .and_then(serde_json::Value::as_array)
        .map(|values| {
            values
                .iter()
                .filter_map(serde_json::Value::as_str)
                .map(ToOwned::to_owned)
                .collect()
        });
    let default = value.get("default").cloned();
    PropertyDef {
        property_type,
        description,
        enum_values,
        default,
        ..Default::default()
    }
}

#[cfg(feature = "mcp")]
fn property_type_from_str(value: &str) -> PropertyType {
    match value {
        "number" => PropertyType::Number,
        "integer" => PropertyType::Integer,
        "boolean" => PropertyType::Boolean,
        "array" => PropertyType::Array,
        "object" => PropertyType::Object,
        _ => PropertyType::String,
    }
}

#[cfg(feature = "mcp")]
fn reversibility_from_mcp(tool: &rmcp::model::Tool) -> Reversibility {
    match tool
        .annotations
        .as_ref()
        .and_then(|annotations| annotations.read_only_hint)
    {
        Some(true) => Reversibility::FullyReversible,
        _ => Reversibility::Irreversible,
    }
}

#[cfg(feature = "mcp")]
fn tags_from_mcp(tool: &rmcp::model::Tool) -> Vec<ToolTag> {
    if tool
        .annotations
        .as_ref()
        .and_then(|annotations| annotations.read_only_hint)
        == Some(true)
    {
        vec![ToolTag::Fetch]
    } else {
        vec![ToolTag::Fetch, ToolTag::Execute]
    }
}

fn effective_http_groups(entry: &ExternalToolEntry) -> Vec<ToolGroupId> {
    entry.groups.as_deref().map_or_else(
        || vec![ToolGroupId::Mcp],
        |groups| groups.iter().copied().map(tool_group_from_config).collect(),
    )
}

fn tool_group_from_config(group: ExternalToolGroupId) -> ToolGroupId {
    match group {
        ExternalToolGroupId::Read => ToolGroupId::Read,
        ExternalToolGroupId::Edit => ToolGroupId::Edit,
        ExternalToolGroupId::Command => ToolGroupId::Command,
        ExternalToolGroupId::SpawnSubtask => ToolGroupId::SpawnSubtask,
        ExternalToolGroupId::Plan => ToolGroupId::Plan,
        ExternalToolGroupId::Verify => ToolGroupId::Verify,
        _ => ToolGroupId::Mcp,
    }
}

fn effective_http_reversibility(entry: &ExternalToolEntry) -> Reversibility {
    entry
        .reversibility
        .map_or(Reversibility::Irreversible, reversibility_from_config)
}

fn reversibility_from_config(reversibility: ExternalToolReversibility) -> Reversibility {
    match reversibility {
        ExternalToolReversibility::FullyReversible => Reversibility::FullyReversible,
        ExternalToolReversibility::Reversible => Reversibility::Reversible,
        ExternalToolReversibility::PartiallyReversible => Reversibility::PartiallyReversible,
        _ => Reversibility::Irreversible,
    }
}

fn tags_from_http_policy(reversibility: Reversibility) -> Vec<ToolTag> {
    if reversibility == Reversibility::FullyReversible {
        vec![ToolTag::Fetch]
    } else {
        vec![ToolTag::Fetch, ToolTag::Execute]
    }
}

fn format_tool_groups(groups: &[ToolGroupId]) -> String {
    groups
        .iter()
        .map(std::string::ToString::to_string)
        .collect::<Vec<_>>()
        .join(",")
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
            ..Default::default()
        },
    );
    properties.insert(
        "params".to_owned(),
        PropertyDef {
            property_type: PropertyType::Object,
            description: "Additional parameters as a JSON object (tool-specific)".to_owned(),
            enum_values: None,
            default: None,
            ..Default::default()
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
/// Forwards tool calls to the configured endpoint using the configured HTTP
/// method. The response body is returned as the tool result text.
struct ExternalToolExecutor {
    name: String,
    endpoint: String,
    client: reqwest::Client,
    kind: ExternalToolKind,
    method: ExternalToolMethod,
}

impl ExternalToolExecutor {
    /// Build a [`reqwest::Method`] from the configured method.
    #[must_use]
    fn reqwest_method(&self) -> reqwest::Method {
        match self.method {
            ExternalToolMethod::Get => reqwest::Method::GET,
            ExternalToolMethod::Put => reqwest::Method::PUT,
            ExternalToolMethod::Delete => reqwest::Method::DELETE,
            ExternalToolMethod::Patch => reqwest::Method::PATCH,
            _ => reqwest::Method::POST,
        }
    }
}

impl ToolExecutor for ExternalToolExecutor {
    fn execute<'a>(
        &'a self,
        input: &'a ToolInput,
        _ctx: &'a ToolContext,
    ) -> Pin<Box<dyn Future<Output = organon::error::Result<ToolResult>> + Send + 'a>> {
        Box::pin(async move {
            let payload = serde_json::json!({
                "tool": self.name,
                "kind": format!("{:?}", self.kind),
                "arguments": input.arguments,
            });

            let method = self.reqwest_method();
            let mut builder = self
                .client
                .request(method, &self.endpoint)
                .timeout(std::time::Duration::from_secs(30));
            if matches!(
                self.method,
                ExternalToolMethod::Post | ExternalToolMethod::Put | ExternalToolMethod::Patch
            ) {
                builder = builder.json(&payload);
            }

            let response = builder.send().await;

            match response {
                Ok(resp) => {
                    let status = resp.status();
                    // kanon:ignore RUST/no-result-unwrap-or-default — HTTP response body read failure is non-fatal; status code is primary signal
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

#[cfg(feature = "mcp")]
struct ExternalMcpToolExecutor {
    server_name: String,
    remote_name: String,
    client: ExternalMcpClient,
}

#[cfg(feature = "mcp")]
impl ToolExecutor for ExternalMcpToolExecutor {
    fn execute<'a>(
        &'a self,
        input: &'a ToolInput,
        _ctx: &'a ToolContext,
    ) -> Pin<Box<dyn Future<Output = organon::error::Result<ToolResult>> + Send + 'a>> {
        Box::pin(async move {
            let Some(arguments) = input.arguments.as_object().cloned() else {
                return Ok(ToolResult::error(format!(
                    "External MCP tool '{tool}' expected object arguments",
                    tool = self.remote_name
                )));
            };

            let result = self.client.call_tool(&self.remote_name, arguments).await;
            match result {
                Ok(result) => Ok(mcp_result_to_tool_result(result)),
                Err(e) => Ok(ToolResult::error(format!(
                    "External MCP tool '{tool}' on server '{server}' failed: {e}",
                    tool = self.remote_name,
                    server = self.server_name
                ))),
            }
        })
    }
}

#[cfg(feature = "mcp")]
fn mcp_result_to_tool_result(result: rmcp::model::CallToolResult) -> ToolResult {
    let text_blocks: Vec<String> = result
        .content
        .iter()
        .filter_map(|content| content.as_text().map(|text| text.text.clone()))
        .collect();
    let rendered = if !text_blocks.is_empty() {
        text_blocks.join("\n")
    } else if let Some(value) = result.structured_content {
        value.to_string()
    } else {
        serde_json::to_string(&result).unwrap_or_else(|e| format!("unrenderable MCP result: {e}"))
    };

    if result.is_error == Some(true) {
        ToolResult::error(rendered)
    } else {
        ToolResult::text(rendered)
    }
}

// ── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    #![expect(clippy::expect_used, reason = "test assertions")]
    use super::*;

    /// WHY: reqwest requires a rustls `CryptoProvider` to be installed. In
    /// production this happens in `main()`. Tests must install it themselves.
    fn ensure_crypto_provider() {
        let _ = rustls::crypto::ring::default_provider().install_default();
    }

    fn entry(
        kind: ExternalToolKind,
        endpoint: Option<&str>,
        description: Option<&str>,
    ) -> ExternalToolEntry {
        ExternalToolEntry {
            kind,
            endpoint: endpoint.map(ToOwned::to_owned),
            command: None,
            args: Vec::new(),
            cwd: None,
            env: std::collections::HashMap::new(),
            description: description.map(ToOwned::to_owned),
            method: ExternalToolMethod::default(),
            groups: None,
            reversibility: None,
            auth: None,
        }
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

    #[tokio::test]
    async fn register_builtin_tool_checks_registry() {
        ensure_crypto_provider();
        let mut registry = ToolRegistry::new();
        let entry = entry(ExternalToolKind::Builtin, None, Some("built-in file ops"));
        let result = register_single_tool(
            "nonexistent_builtin",
            &entry,
            &mut registry,
            &reqwest::Client::new(),
        )
        .await;
        // NOTE: builtin entry for a tool that doesn't exist in the registry
        assert_eq!(result.len(), 1);
        let entry = result.first().expect("manifest entry");
        assert!(!entry.available);
        assert_eq!(entry.kind, ExternalToolKind::Builtin);
    }

    #[tokio::test]
    async fn register_mcp_tool_without_endpoint() {
        ensure_crypto_provider();
        let mut registry = ToolRegistry::new();
        let entry = entry(ExternalToolKind::Mcp, None, Some("missing endpoint"));
        let result =
            register_single_tool("bad_tool", &entry, &mut registry, &reqwest::Client::new()).await;
        assert_eq!(result.len(), 1);
        assert!(!result.first().expect("manifest entry").available);
    }

    #[tokio::test]
    async fn register_http_tool_with_endpoint() {
        ensure_crypto_provider();
        let mut registry = ToolRegistry::new();
        let entry = entry(
            ExternalToolKind::Http,
            Some("http://localhost:3100"),
            Some("Semantic Scholar"),
        );
        let result = register_single_tool(
            "semantic_scholar",
            &entry,
            &mut registry,
            &reqwest::Client::new(),
        )
        .await;
        assert_eq!(result.len(), 1);
        let entry = result.first().expect("manifest entry");
        assert!(entry.available);
        assert_eq!(entry.name, "semantic_scholar");

        // NOTE: verify the tool is actually in the registry
        let tool_name = ToolName::new("semantic_scholar").expect("valid name");
        let def = registry.get_def(&tool_name).expect("tool def");
        assert_eq!(def.groups, vec![ToolGroupId::Mcp]);
        assert_eq!(def.reversibility, Reversibility::Irreversible);
        assert_eq!(
            ApprovalRequirement::from(def.reversibility),
            ApprovalRequirement::Mandatory
        );
        assert!(def.tags.contains(&ToolTag::Fetch));
        assert!(def.tags.contains(&ToolTag::Execute));
    }

    #[tokio::test]
    async fn register_http_tool_uses_explicit_safety_policy() {
        ensure_crypto_provider();
        let mut registry = ToolRegistry::new();
        let mut entry = entry(
            ExternalToolKind::Http,
            Some("http://localhost:3100"),
            Some("Read-only search"),
        );
        entry.method = ExternalToolMethod::Get;
        entry.groups = Some(vec![ExternalToolGroupId::Read]);
        entry.reversibility = Some(ExternalToolReversibility::FullyReversible);

        let result =
            register_single_tool("search", &entry, &mut registry, &reqwest::Client::new()).await;
        assert_eq!(result.len(), 1);
        assert!(result.first().is_some_and(|entry| entry.available));

        let tool_name = ToolName::new("search").expect("valid name");
        let def = registry.get_def(&tool_name).expect("tool def");
        assert_eq!(def.groups, vec![ToolGroupId::Read]);
        assert_eq!(def.reversibility, Reversibility::FullyReversible);
        assert_eq!(
            ApprovalRequirement::from(def.reversibility),
            ApprovalRequirement::None
        );
        assert!(def.tags.contains(&ToolTag::Fetch));
        assert!(!def.tags.contains(&ToolTag::Execute));
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
                    server_name: None,
                    remote_name: None,
                },
                ToolManifestEntry {
                    name: "missing".to_owned(),
                    kind: ExternalToolKind::Mcp,
                    available: false,
                    description: "missing".to_owned(),
                    server_name: Some("missing_server".to_owned()),
                    remote_name: Some("missing".to_owned()),
                },
            ],
            optional: vec![ToolManifestEntry {
                name: "scholar".to_owned(),
                kind: ExternalToolKind::Mcp,
                available: true,
                description: "scholar".to_owned(),
                server_name: Some("scholar_server".to_owned()),
                remote_name: Some("scholar".to_owned()),
            }],
        };
        assert_eq!(manifest.available_count(), 2);
        assert_eq!(manifest.missing_required_count(), 1);
    }

    #[tokio::test]
    async fn full_registration_flow() {
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

        let manifest = register_external_tools(&config, &mut registry, &client).await;

        // NOTE: file_ops is builtin but not registered in empty registry → unavailable
        assert_eq!(manifest.required.len(), 1);
        assert!(!manifest.required.iter().any(|e| e.available));

        // NOTE: test_tool should register, no_endpoint should not
        let test_entry = manifest.optional.iter().find(|e| e.name == "test_tool");
        assert!(test_entry.is_some_and(|e| e.available));

        let no_ep = manifest.optional.iter().find(|e| e.name == "no_endpoint");
        assert!(no_ep.is_some_and(|e| !e.available));
    }

    #[cfg(feature = "mcp")]
    #[allow(clippy::disallowed_methods)]
    fn fake_mcp_server_script() -> (tempfile::TempDir, std::path::PathBuf) {
        let dir = tempfile::tempdir().expect("tempdir");
        let script = dir.path().join("fake-mcp.sh");
        {
            use std::io::Write as _;
            std::fs::OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .open(&script)
                .expect("open script")
                .write_all(
                    br#"#!/bin/sh
IFS= read -r init
printf '%s\n' '{"jsonrpc":"2.0","id":0,"result":{"protocolVersion":"2024-11-05","capabilities":{"tools":{}},"serverInfo":{"name":"fake","version":"0.1.0"}}}'
IFS= read -r initialized
IFS= read -r list
printf '%s\n' '{"jsonrpc":"2.0","id":1,"result":{"tools":[{"name":"echo","description":"Echo through MCP","inputSchema":{"type":"object","properties":{"message":{"type":"string","description":"Message"}},"required":["message"]},"annotations":{"readOnlyHint":true}}]}}'
while IFS= read -r line; do
  case "$line" in
    *'"method":"tools/call"'*)
      printf '%s\n' '{"jsonrpc":"2.0","id":2,"result":{"content":[{"type":"text","text":"mcp-response"}],"isError":false}}'
      ;;
  esac
done
"#,
                )
                .expect("write script");
        }
        let mut perms = std::fs::metadata(&script).expect("metadata").permissions();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            perms.set_mode(0o755);
            std::fs::set_permissions(&script, perms).expect("chmod");
        }
        (dir, script)
    }

    #[cfg(feature = "mcp")]
    fn test_ctx() -> ToolContext {
        use std::collections::HashSet;
        use std::sync::{Arc, RwLock};

        ToolContext {
            nous_id: koina::id::NousId::new("alice").expect("valid nous id"),
            session_id: koina::id::SessionId::new(),
            turn_number: 0,
            workspace: std::env::current_dir().expect("cwd"),
            allowed_roots: vec![std::env::current_dir().expect("cwd")],
            services: None,
            active_tools: Arc::new(RwLock::new(HashSet::new())),
            tool_config: Arc::new(taxis::config::ToolLimitsConfig::default()),
        }
    }

    #[cfg(feature = "mcp")]
    #[tokio::test]
    async fn register_mcp_server_discovers_tools_and_execute_routes_call() {
        ensure_crypto_provider();
        let (_dir, script) = fake_mcp_server_script();
        let mut registry = ToolRegistry::new();
        let mut entry = entry(ExternalToolKind::Mcp, None, Some("Fake MCP"));
        entry.command = Some(script.display().to_string());

        let result =
            register_single_tool("fake", &entry, &mut registry, &reqwest::Client::new()).await;
        assert_eq!(result.len(), 1);
        assert!(result.first().is_some_and(|e| e.available));
        assert_eq!(
            result.first().map(|e| e.name.as_str()),
            Some("mcp_fake_echo")
        );
        assert_eq!(
            result.first().and_then(|e| e.server_name.as_deref()),
            Some("fake")
        );
        assert_eq!(
            result.first().and_then(|e| e.remote_name.as_deref()),
            Some("echo")
        );

        let tool_name = ToolName::new("mcp_fake_echo").expect("tool name");
        let def = registry.get_def(&tool_name).expect("tool def");
        assert_eq!(def.groups, vec![ToolGroupId::Mcp]);
        assert!(def.tags.contains(&ToolTag::Fetch));

        let input = ToolInput {
            name: tool_name,
            tool_use_id: "call-1".to_owned(),
            arguments: serde_json::json!({"message": "hello"}),
        };
        let output = registry
            .execute(&input, &test_ctx())
            .await
            .expect("execute");
        assert!(!output.is_error);
        let organon::types::ToolResultContent::Text(text) = output.content else {
            panic!("expected text tool result");
        };
        assert_eq!(text, "mcp-response");
    }

    #[cfg(feature = "mcp")]
    #[tokio::test]
    async fn execute_checked_denies_mcp_group_when_role_lacks_group() {
        ensure_crypto_provider();
        let (_dir, script) = fake_mcp_server_script();
        let mut registry = ToolRegistry::new();
        let mut entry = entry(ExternalToolKind::Mcp, None, Some("Fake MCP"));
        entry.command = Some(script.display().to_string());

        let result =
            register_single_tool("fake", &entry, &mut registry, &reqwest::Client::new()).await;
        assert!(result.iter().any(|entry| entry.available));

        let input = ToolInput {
            name: ToolName::new("mcp_fake_echo").expect("tool name"),
            tool_use_id: "call-1".to_owned(),
            arguments: serde_json::json!({"message": "hello"}),
        };
        let err = registry
            .execute_checked(
                &input,
                &test_ctx(),
                "reader",
                &organon::types::ToolGroupPolicy::groups(vec![ToolGroupId::Read]),
            )
            .await
            .expect_err("mcp group denied");
        assert!(err.to_string().contains("tool group violation"));
    }

    #[cfg(feature = "mcp")]
    #[tokio::test]
    async fn mcp_tool_name_is_stable_even_when_bare_name_is_free() {
        ensure_crypto_provider();
        let (_dir, script) = fake_mcp_server_script();
        let mut registry = ToolRegistry::new();

        // WHY: Pre-register a built-in named "echo" so the bare normalized
        // remote name is occupied. A stable namespace must still produce
        // `mcp_fake_echo`, not fall back to a different name.
        registry
            .register(
                ToolDef {
                    name: ToolName::new("echo").expect("valid tool name"),
                    description: "Built-in echo".to_owned(),
                    extended_description: None,
                    input_schema: external_tool_schema(),
                    category: ToolCategory::Workspace,
                    reversibility: Reversibility::FullyReversible,
                    auto_activate: false,
                    groups: vec![ToolGroupId::Read],
                    tags: vec![ToolTag::Fetch],
                },
                Box::new(DummyExecutor),
            )
            .expect("register dummy echo");

        let mut entry = entry(ExternalToolKind::Mcp, None, Some("Fake MCP"));
        entry.command = Some(script.display().to_string());

        let result =
            register_single_tool("fake", &entry, &mut registry, &reqwest::Client::new()).await;
        assert_eq!(result.len(), 1);
        assert!(result.first().is_some_and(|e| e.available));
        assert_eq!(
            result.first().map(|e| e.name.as_str()),
            Some("mcp_fake_echo")
        );
    }

    /// Test double for pre-registering a tool that occupies a name.
    #[cfg(feature = "mcp")]
    struct DummyExecutor;

    #[cfg(feature = "mcp")]
    impl ToolExecutor for DummyExecutor {
        fn execute<'a>(
            &'a self,
            _input: &'a ToolInput,
            _ctx: &'a ToolContext,
        ) -> Pin<Box<dyn Future<Output = organon::error::Result<ToolResult>> + Send + 'a>> {
            Box::pin(async move { Ok(ToolResult::text("dummy")) })
        }
    }

    #[test]
    fn external_tool_schema_has_query() {
        let schema = external_tool_schema();
        assert!(schema.properties.contains_key("query"));
        assert!(schema.properties.contains_key("params"));
        assert!(schema.required.contains(&"query".to_owned()));
    }

    #[test]
    fn http_method_defaults_to_post() {
        let toml_str = r#"
[optional]
search = { type = "http", endpoint = "http://localhost:3100" }
"#;
        let config: ExternalToolsConfig = toml::from_str(toml_str).expect("valid config");
        let entry = config.optional.get("search").expect("search entry");
        assert_eq!(entry.method, ExternalToolMethod::Post);
    }
}

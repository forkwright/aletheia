//! Effective tool-surface resolution for prompt, provider, and dispatch paths.

use std::collections::{HashMap, HashSet};

use hermeneus::types::{ServerToolDefinition, ToolDefinition};
use koina::id::ToolName;
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};

use crate::types::{
    ApprovalRequirement, Reversibility, ServerToolConfig, ToolCallCapabilityRule, ToolCategory,
    ToolDef, ToolGroupId, ToolGroupPolicy, ToolOrigin,
};

const SURFACE_HASH_PREFIX: &str = "ts1:";
const SURFACE_VERSION: u8 = 1;
pub(crate) const ENABLE_TOOL: &str = "enable_tool";

/// Canonical empty JSON schema used for deferred-schema tool summaries.
///
/// WHY: Deferred-schemas mode omits full `input_schema` from provider requests;
/// agents must call `tool_schema` to retrieve the schema before invoking a tool.
/// Centralizing the literal avoids drift between registry summaries, byte-size
/// estimates, and surface provider summaries.
pub(crate) fn deferred_schema_placeholder() -> serde_json::Value {
    serde_json::json!({"type": "object", "properties": {}, "required": []})
}

/// Inputs that determine the effective tool surface for one LLM iteration.
#[derive(Clone, Copy)]
pub struct SurfaceInputs<'a> {
    /// Tool-group policy resolved for the active agent.
    pub policy: &'a ToolGroupPolicy,
    /// Optional per-agent tool allowlist.
    pub allowlist: Option<&'a [String]>,
    /// Snapshot of tools activated for the session.
    pub active: &'a HashSet<ToolName>,
    /// Provider-side tools already active for this request.
    pub server_tools: &'a [ServerToolDefinition],
    /// Provider-side tools available for lazy activation.
    pub server_tool_config: Option<&'a ServerToolConfig>,
}

/// Registry-owned data projected into the resolver.
#[derive(Clone, Copy)]
pub(crate) struct RegistrySurfaceTool<'a> {
    pub(crate) def: &'a ToolDef,
    pub(crate) call_capability: Option<&'a ToolCallCapabilityRule>,
    pub(crate) origin: Option<&'a ToolOrigin>,
}

/// Versioned stable digest for a resolved tool surface.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ToolSurfaceHash(String);

impl ToolSurfaceHash {
    /// Return the opaque hash reference string.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for ToolSurfaceHash {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// Why a known tool is not callable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DenialReason {
    /// Denied by the agent's configured tool-group policy.
    GroupPolicy,
    /// Denied by the agent's configured tool allowlist.
    Allowlist,
    /// Denied because multiple tool planes expose the same bare name.
    NameCollision,
}

impl DenialReason {
    /// Stable wire string.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::GroupPolicy => "group_policy",
            Self::Allowlist => "allowlist",
            Self::NameCollision => "name_collision",
        }
    }
}

/// Whether a known tool is callable in the resolved surface.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "state", content = "reason")]
pub enum SurfaceAvailability {
    /// Tool can be called now.
    Callable,
    /// Tool is permitted but requires `enable_tool` first.
    Inactive,
    /// Tool is known but denied by policy.
    Denied(DenialReason),
}

impl SurfaceAvailability {
    /// Stable availability label.
    #[must_use]
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Callable => "callable",
            Self::Inactive => "inactive",
            Self::Denied(_) => "denied",
        }
    }

    /// Denial reason when availability is denied.
    #[must_use]
    pub const fn denial_reason(&self) -> Option<DenialReason> {
        match self {
            Self::Denied(reason) => Some(*reason),
            Self::Callable | Self::Inactive => None,
        }
    }

    /// Whether the entry is callable.
    #[must_use]
    pub const fn is_callable(&self) -> bool {
        matches!(self, Self::Callable)
    }
}

/// Source kind for a tool-surface entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SurfaceEntryKind {
    /// Local registry tool executed by Organon.
    Registry,
    /// Provider-side tool sent through the LLM API.
    Server,
}

impl SurfaceEntryKind {
    /// Stable entry-kind label.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Registry => "registry",
            Self::Server => "server",
        }
    }
}

/// One tool entry in the effective surface.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SurfaceEntry {
    /// Tool name.
    pub name: ToolName,
    /// Description shown to agents and introspection callers.
    pub description: String,
    /// Local JSON schema when this entry comes from the registry.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_schema: Option<serde_json::Value>,
    /// Local category when this entry comes from the registry.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub category: Option<ToolCategory>,
    /// Tool groups used by policy resolution.
    pub groups: Vec<ToolGroupId>,
    /// Reversibility metadata.
    pub reversibility: Reversibility,
    /// Approval level derived from reversibility.
    pub approval: ApprovalRequirement,
    /// Whether this tool activates without `enable_tool`.
    pub auto_activate: bool,
    /// Whether this local tool has argument-sensitive capability metadata.
    pub has_capability_rule: bool,
    /// Stable digest of argument-sensitive capability metadata.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub capability_rule_sha256: Option<String>,
    /// Stable digest of provider-side tool definition metadata.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub server_definition_sha256: Option<String>,
    /// Entry source kind.
    pub kind: SurfaceEntryKind,
    /// Effective availability.
    pub availability: SurfaceAvailability,
    /// Origin metadata when the tool is externally provided.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub origin: Option<ToolOrigin>,
}

impl SurfaceEntry {
    /// Return true when this entry is callable now.
    #[must_use]
    pub fn is_callable(&self) -> bool {
        self.availability.is_callable()
    }

    /// Compact diagnostic label that preserves source and provenance.
    #[must_use]
    pub fn diagnostic_label(&self) -> String {
        let source = match self.kind {
            SurfaceEntryKind::Registry => "local",
            SurfaceEntryKind::Server => "provider_server",
        };
        let base = format!(
            "{source}:{} (reversibility={}, approval={})",
            self.name.as_str(),
            self.reversibility,
            self.approval
        );
        match &self.origin {
            Some(origin) => format!(
                "{base}, origin(local={}, server={}, remote={})",
                origin.local_name, origin.server_name, origin.remote_name
            ),
            None => base,
        }
    }

    /// Return this entry as a provider local-tool definition.
    #[must_use]
    pub fn to_provider_tool(&self) -> Option<ToolDefinition> {
        if self.kind != SurfaceEntryKind::Registry || !self.is_callable() {
            return None;
        }
        Some(ToolDefinition {
            name: self.name.as_str().to_owned(),
            description: self.description.clone(),
            input_schema: self.input_schema.clone()?,
            disable_passthrough: None,
        })
    }

    /// Return this entry as a provider local-tool summary.
    #[must_use]
    pub fn to_provider_summary(&self) -> Option<ToolDefinition> {
        if self.kind != SurfaceEntryKind::Registry || !self.is_callable() {
            return None;
        }
        Some(ToolDefinition {
            name: self.name.as_str().to_owned(),
            description: self.description.clone(),
            input_schema: deferred_schema_placeholder(),
            disable_passthrough: None,
        })
    }
}

/// Lookup result for a tool name.
#[derive(Debug, Clone, Copy)]
pub enum SurfaceLookup<'a> {
    /// More than one source exposes this bare name.
    Ambiguous {
        /// First colliding candidate.
        first: &'a SurfaceEntry,
        /// Second colliding candidate.
        second: &'a SurfaceEntry,
    },
    /// Known and callable.
    Callable(&'a SurfaceEntry),
    /// Known but not active.
    Inactive(&'a SurfaceEntry),
    /// Known but denied by policy.
    Denied(&'a SurfaceEntry),
    /// Not present in the resolved surface.
    Unknown,
}

/// Effective tool surface for one LLM iteration.
#[derive(Debug, Clone)]
pub struct EffectiveToolSurface {
    policy: ToolGroupPolicy,
    allowlist: Option<Vec<String>>,
    entries: Vec<SurfaceEntry>,
    server_tools: Vec<ServerToolDefinition>,
    hash: ToolSurfaceHash,
}

impl EffectiveToolSurface {
    pub(crate) fn resolve<'a>(
        registry_tools: impl IntoIterator<Item = RegistrySurfaceTool<'a>>,
        inputs: SurfaceInputs<'_>,
    ) -> Self {
        let mut entries = Vec::new();
        for tool in registry_tools {
            entries.push(resolve_registry_entry(tool, &inputs));
        }
        entries.extend(resolve_server_entries(&inputs));
        reject_name_collisions(&mut entries);
        entries.sort_by(|left, right| {
            left.name
                .as_str()
                .cmp(right.name.as_str())
                .then_with(|| left.kind.as_str().cmp(right.kind.as_str()))
        });

        let mut server_tools = inputs.server_tools.to_vec();
        let entries_by_name: HashMap<&str, &SurfaceEntry> = entries
            .iter()
            .map(|entry| (entry.name.as_str(), entry))
            .collect();
        server_tools.retain(|tool| {
            entries_by_name
                .get(tool.name.as_str())
                .is_some_and(|entry| entry.kind == SurfaceEntryKind::Server && entry.is_callable())
        });
        server_tools.sort_by(|left, right| left.name.cmp(&right.name));

        let allowlist = inputs.allowlist.map(|values| {
            let mut values = values.to_vec();
            values.sort();
            values
        });
        let hash = compute_surface_hash(inputs.policy, allowlist.as_deref(), &entries);

        Self {
            policy: inputs.policy.clone(),
            allowlist,
            entries,
            server_tools,
            hash,
        }
    }

    /// Tool-group policy used to resolve this surface.
    #[must_use]
    pub fn policy(&self) -> &ToolGroupPolicy {
        &self.policy
    }

    /// Optional sorted allowlist used to resolve this surface.
    #[must_use]
    pub fn allowlist(&self) -> Option<&[String]> {
        self.allowlist.as_deref()
    }

    /// Stable hash for this effective surface.
    #[must_use]
    pub fn hash(&self) -> &ToolSurfaceHash {
        &self.hash
    }

    /// All known entries, sorted by name.
    #[must_use]
    pub fn entries(&self) -> &[SurfaceEntry] {
        &self.entries
    }

    /// Lookup one tool name in the surface.
    #[must_use]
    pub fn lookup(&self, name: &ToolName) -> SurfaceLookup<'_> {
        let mut matches = self.entries.iter().filter(|entry| entry.name == *name);
        let Some(entry) = matches.next() else {
            return SurfaceLookup::Unknown;
        };
        if let Some(second) = matches.next() {
            return SurfaceLookup::Ambiguous {
                first: entry,
                second,
            };
        }
        match entry.availability {
            SurfaceAvailability::Callable => SurfaceLookup::Callable(entry),
            SurfaceAvailability::Inactive => SurfaceLookup::Inactive(entry),
            SurfaceAvailability::Denied(_) => SurfaceLookup::Denied(entry),
        }
    }

    /// Callable local tool definitions for provider requests.
    #[must_use]
    pub fn provider_tools(&self) -> Vec<ToolDefinition> {
        self.entries
            .iter()
            .filter_map(SurfaceEntry::to_provider_tool)
            .collect()
    }

    /// Callable local tool summaries for deferred-schema provider requests.
    #[must_use]
    pub fn provider_summaries(&self) -> Vec<ToolDefinition> {
        self.entries
            .iter()
            .filter_map(SurfaceEntry::to_provider_summary)
            .collect()
    }

    /// Callable provider-side tools for provider requests.
    #[must_use]
    pub fn provider_server_tools(&self) -> Vec<ServerToolDefinition> {
        self.server_tools.clone()
    }

    /// Inactive permitted tools available through `enable_tool`.
    #[must_use]
    pub fn lazy_catalog(&self) -> Vec<(ToolName, String)> {
        self.entries
            .iter()
            .filter(|entry| matches!(entry.availability, SurfaceAvailability::Inactive))
            .map(|entry| (entry.name.clone(), entry.description.clone()))
            .collect()
    }

    /// Callable local entries for prompt summaries and introspection.
    #[must_use]
    pub fn callable_registry_entries(&self) -> Vec<&SurfaceEntry> {
        self.entries
            .iter()
            .filter(|entry| entry.kind == SurfaceEntryKind::Registry && entry.is_callable())
            .collect()
    }
}

fn resolve_registry_entry(
    tool: RegistrySurfaceTool<'_>,
    inputs: &SurfaceInputs<'_>,
) -> SurfaceEntry {
    let availability = resolve_availability(
        &tool.def.name,
        &tool.def.groups,
        tool.def.auto_activate,
        inputs,
    );
    SurfaceEntry {
        name: tool.def.name.clone(),
        description: tool.def.description.clone(),
        input_schema: Some(tool.def.input_schema.to_json_schema()),
        category: Some(tool.def.category),
        groups: sorted_groups(tool.def.groups.clone()),
        reversibility: tool.def.reversibility,
        approval: ApprovalRequirement::from(tool.def.reversibility),
        auto_activate: tool.def.auto_activate,
        has_capability_rule: tool.call_capability.is_some(),
        capability_rule_sha256: tool
            .call_capability
            .and_then(|rule| serde_json::to_value(rule).ok())
            .map(|value| stable_value_hash(&value)),
        server_definition_sha256: None,
        origin: tool.origin.cloned(),
        kind: SurfaceEntryKind::Registry,
        availability,
    }
}

fn resolve_server_entries(inputs: &SurfaceInputs<'_>) -> Vec<SurfaceEntry> {
    let mut entries: HashMap<String, SurfaceEntry> = HashMap::new();
    if let Some(config) = inputs.server_tool_config {
        for catalog_entry in config.catalog_entries_with_metadata() {
            let name = catalog_entry.name;
            let (groups, reversibility) = server_tool_capability(name.as_str());
            let availability = resolve_availability(&name, &groups, false, inputs);
            entries.insert(
                name.as_str().to_owned(),
                SurfaceEntry {
                    name,
                    description: catalog_entry.description,
                    input_schema: None,
                    category: None,
                    groups: sorted_groups(groups),
                    reversibility,
                    approval: ApprovalRequirement::from(reversibility),
                    auto_activate: false,
                    has_capability_rule: false,
                    capability_rule_sha256: None,
                    server_definition_sha256: None,
                    origin: None,
                    kind: SurfaceEntryKind::Server,
                    availability,
                },
            );
        }
    }

    for server_tool in inputs.server_tools {
        let Ok(name) = ToolName::new(server_tool.name.clone()) else {
            continue;
        };
        let (groups, reversibility) = server_tool_capability(name.as_str());
        let availability = resolve_server_availability(&name, &groups, inputs);
        let definition_sha256 = serde_json::to_value(server_tool)
            .ok()
            .map(|value| stable_value_hash(&value));
        entries.insert(
            name.as_str().to_owned(),
            SurfaceEntry {
                name,
                description: server_tool_description(server_tool),
                input_schema: None,
                category: None,
                groups: sorted_groups(groups),
                reversibility,
                approval: ApprovalRequirement::from(reversibility),
                auto_activate: true,
                has_capability_rule: false,
                capability_rule_sha256: None,
                server_definition_sha256: definition_sha256,
                origin: None,
                kind: SurfaceEntryKind::Server,
                availability,
            },
        );
    }

    entries.into_values().collect()
}

fn reject_name_collisions(entries: &mut [SurfaceEntry]) {
    let mut counts: HashMap<String, usize> = HashMap::new();
    for entry in entries.iter() {
        *counts.entry(entry.name.as_str().to_owned()).or_default() += 1;
    }
    for entry in entries.iter_mut() {
        if counts.get(entry.name.as_str()).copied().unwrap_or_default() > 1 {
            entry.availability = SurfaceAvailability::Denied(DenialReason::NameCollision);
        }
    }
}

fn resolve_availability(
    name: &ToolName,
    groups: &[ToolGroupId],
    auto_activate: bool,
    inputs: &SurfaceInputs<'_>,
) -> SurfaceAvailability {
    if !inputs.policy.permits(groups) {
        return SurfaceAvailability::Denied(DenialReason::GroupPolicy);
    }
    if !allowlist_permits(name, inputs.allowlist) {
        return SurfaceAvailability::Denied(DenialReason::Allowlist);
    }
    if auto_activate || inputs.active.contains(name) || name.as_str() == ENABLE_TOOL {
        SurfaceAvailability::Callable
    } else {
        SurfaceAvailability::Inactive
    }
}

fn resolve_server_availability(
    name: &ToolName,
    groups: &[ToolGroupId],
    inputs: &SurfaceInputs<'_>,
) -> SurfaceAvailability {
    if !inputs.policy.permits(groups) {
        return SurfaceAvailability::Denied(DenialReason::GroupPolicy);
    }
    if !allowlist_permits(name, inputs.allowlist) {
        return SurfaceAvailability::Denied(DenialReason::Allowlist);
    }
    SurfaceAvailability::Callable
}

fn allowlist_permits(name: &ToolName, allowlist: Option<&[String]>) -> bool {
    allowlist.is_none_or(|allowlist| {
        name.as_str() == ENABLE_TOOL || allowlist.iter().any(|allowed| allowed == name.as_str())
    })
}

fn server_tool_capability(name: &str) -> (Vec<ToolGroupId>, Reversibility) {
    match name {
        "web_search" => (
            vec![ToolGroupId::Read, ToolGroupId::Mcp],
            Reversibility::FullyReversible,
        ),
        "code_execution" => (
            vec![ToolGroupId::Command],
            Reversibility::PartiallyReversible,
        ),
        _ => (vec![ToolGroupId::Mcp], Reversibility::Irreversible),
    }
}

fn server_tool_description(definition: &ServerToolDefinition) -> String {
    match definition.name.as_str() {
        "web_search" => "Search the web using the provider's server-side tool".to_owned(),
        "code_execution" => "Execute code using the provider's server-side sandbox".to_owned(),
        name => format!("Provider server-side tool {name}"),
    }
}

fn sorted_groups(mut groups: Vec<ToolGroupId>) -> Vec<ToolGroupId> {
    groups.sort_by_key(std::string::ToString::to_string);
    groups
}

fn compute_surface_hash(
    policy: &ToolGroupPolicy,
    allowlist: Option<&[String]>,
    entries: &[SurfaceEntry],
) -> ToolSurfaceHash {
    let entries: Vec<serde_json::Value> = entries
        .iter()
        .map(|entry| {
            serde_json::json!({
                "name": entry.name.as_str(),
                "availability": entry.availability.as_str(),
                "denial_reason": entry.availability.denial_reason().map(DenialReason::as_str),
                "groups": entry.groups.iter().map(ToString::to_string).collect::<Vec<_>>(),
                "reversibility": entry.reversibility.to_string(),
                "approval": entry.approval.to_string(),
                "auto_activate": entry.auto_activate,
                "kind": entry.kind.as_str(),
                "schema_sha256": entry.input_schema.as_ref().map(stable_value_hash),
                "description_sha256": stable_str_hash(&entry.description),
                "capability_rule_sha256": entry.capability_rule_sha256.as_deref(),
                "server_definition_sha256": entry.server_definition_sha256.as_deref(),
                "origin": entry.origin.as_ref().map(|o| serde_json::json!({
                    "local_name": o.local_name,
                    "server_name": o.server_name,
                    "remote_name": o.remote_name,
                })),
            })
        })
        .collect();

    let canonical = serde_json::json!({
        "surface_version": SURFACE_VERSION,
        "policy": policy,
        "allowlist": allowlist,
        "entries": entries,
    });
    let digest = stable_str_hash(&canonical.to_string());
    ToolSurfaceHash(format!("{SURFACE_HASH_PREFIX}{digest}"))
}

fn stable_value_hash(value: &serde_json::Value) -> String {
    stable_str_hash(&value.to_string())
}

fn stable_str_hash(value: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(value.as_bytes());
    let digest = hasher.finalize();
    let mut out = String::with_capacity(digest.len() * 2);
    for byte in digest {
        use std::fmt::Write as _;
        let _ = write!(out, "{byte:02x}");
    }
    out
}

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test assertions")]
mod tests {
    use super::*;
    use crate::types::{InputSchema, ToolOrigin};

    fn read_tool(name: &str, auto_activate: bool) -> ToolDef {
        ToolDef {
            name: ToolName::new(name).expect("valid test tool name"),
            description: format!("Read tool {name}."),
            extended_description: None,
            input_schema: InputSchema {
                properties: indexmap::IndexMap::new(),
                required: Vec::new(),
            },
            category: ToolCategory::Workspace,
            reversibility: Reversibility::FullyReversible,
            auto_activate,
            groups: vec![ToolGroupId::Read],
            tags: Vec::new(),
        }
    }

    fn surface_for(
        defs: &[ToolDef],
        policy: &ToolGroupPolicy,
        allowlist: Option<&[String]>,
        active: &HashSet<ToolName>,
    ) -> EffectiveToolSurface {
        EffectiveToolSurface::resolve(
            defs.iter().map(|def| RegistrySurfaceTool {
                def,
                call_capability: None,
                origin: None,
            }),
            SurfaceInputs {
                policy,
                allowlist,
                active,
                server_tools: &[],
                server_tool_config: None,
            },
        )
    }

    #[test]
    fn policy_denial_wins_before_allowlist() {
        let defs = vec![read_tool("read", true)];
        let allowlist = vec!["read".to_owned()];
        let surface = surface_for(
            &defs,
            &ToolGroupPolicy::DenyAll,
            Some(&allowlist),
            &HashSet::new(),
        );
        let lookup = surface.lookup(&ToolName::from_static("read"));
        assert!(
            matches!(lookup, SurfaceLookup::Denied(entry) if entry.availability == SurfaceAvailability::Denied(DenialReason::GroupPolicy))
        );
    }

    #[test]
    fn allowlist_denies_known_tool_after_group_policy() {
        let defs = vec![read_tool("read", true), read_tool("other", true)];
        let allowlist = vec!["read".to_owned()];
        let surface = surface_for(
            &defs,
            &ToolGroupPolicy::groups(vec![ToolGroupId::Read]),
            Some(&allowlist),
            &HashSet::new(),
        );
        let lookup = surface.lookup(&ToolName::from_static("other"));
        assert!(
            matches!(lookup, SurfaceLookup::Denied(entry) if entry.availability == SurfaceAvailability::Denied(DenialReason::Allowlist))
        );
    }

    #[test]
    fn inactive_becomes_callable_when_active() {
        let defs = vec![read_tool("lazy_read", false)];
        let policy = ToolGroupPolicy::groups(vec![ToolGroupId::Read]);
        let inactive = surface_for(&defs, &policy, None, &HashSet::new());
        assert!(matches!(
            inactive.lookup(&ToolName::from_static("lazy_read")),
            SurfaceLookup::Inactive(_)
        ));

        let active = HashSet::from([ToolName::from_static("lazy_read")]);
        let callable = surface_for(&defs, &policy, None, &active);
        assert!(matches!(
            callable.lookup(&ToolName::from_static("lazy_read")),
            SurfaceLookup::Callable(_)
        ));
        assert_ne!(inactive.hash().as_str(), callable.hash().as_str());
    }

    #[test]
    fn local_and_provider_web_search_collision_is_denied_with_diagnostics() {
        let local = read_tool("web_search", false);
        let origin = ToolOrigin {
            local_name: "web_search".to_owned(),
            server_name: "external-search".to_owned(),
            remote_name: "web_search".to_owned(),
        };
        let server_config = ServerToolConfig {
            web_search: true,
            web_search_max_uses: Some(5),
            code_execution: false,
        };
        let active = HashSet::new();
        let policy = ToolGroupPolicy::AllowAll {
            reason: "test".to_owned(),
        };
        let surface = EffectiveToolSurface::resolve(
            [RegistrySurfaceTool {
                def: &local,
                call_capability: None,
                origin: Some(&origin),
            }],
            SurfaceInputs {
                policy: &policy,
                allowlist: None,
                active: &active,
                server_tools: &[],
                server_tool_config: Some(&server_config),
            },
        );

        let entries = surface
            .entries()
            .iter()
            .filter(|entry| entry.name.as_str() == "web_search")
            .collect::<Vec<_>>();
        assert_eq!(entries.len(), 2);
        assert!(entries.iter().all(|entry| {
            entry.availability == SurfaceAvailability::Denied(DenialReason::NameCollision)
        }));
        assert!(entries.iter().any(|entry| {
            entry.kind == SurfaceEntryKind::Registry
                && entry.reversibility == Reversibility::FullyReversible
                && entry.origin.as_ref() == Some(&origin)
        }));
        assert!(entries.iter().any(|entry| {
            entry.kind == SurfaceEntryKind::Server
                && entry.reversibility == Reversibility::FullyReversible
                && entry.origin.is_none()
        }));
        assert!(surface.provider_tools().is_empty());
        assert!(surface.provider_server_tools().is_empty());
        assert!(matches!(
            surface.lookup(&ToolName::from_static("web_search")),
            SurfaceLookup::Ambiguous { .. }
        ));
    }

    #[test]
    fn hash_is_stable_for_registration_order() {
        let policy = ToolGroupPolicy::groups(vec![ToolGroupId::Read]);
        let left_defs = vec![read_tool("a", true), read_tool("b", true)];
        let right_defs = vec![read_tool("b", true), read_tool("a", true)];
        let left = surface_for(&left_defs, &policy, None, &HashSet::new());
        let right = surface_for(&right_defs, &policy, None, &HashSet::new());
        assert_eq!(left.hash().as_str(), right.hash().as_str());
        assert!(left.hash().as_str().starts_with(SURFACE_HASH_PREFIX));
    }
}

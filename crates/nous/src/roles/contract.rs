//! Versioned role-behavior contracts, wired into ephemeral sub-agent spawns.
//!
//! Each role gets a contract defining expected behaviors, constraints, and
//! spawn policy (`tool_groups`, `episteme_cohort`, `private`, `domains`,
//! `model`). When a role's behavior changes, the version increments,
//! enabling QA (dokimion) to validate against the correct version.
//!
//! Contracts are loaded from `roles.toml` in the oikos cascade
//! (nous/{id}/ -> shared/ -> theke/). Hardcoded defaults are used when
//! no file is found. `SpawnServiceImpl::resolve_contract` (spawn_svc.rs) is
//! the production caller (#4775) — behaviors/constraints append to the
//! spawned agent's system prompt, `tool_groups` refines the coarse
//! tool-group gate, `episteme_cohort`/`private`/`domains` replace the
//! hardcoded constants a spawned agent previously always got, and `model`
//! (wave 3.3) overrides the compiled `RoleTemplate` model a spawned agent
//! would otherwise resolve to.

use std::collections::HashMap;
use std::path::Path;

use serde::{Deserialize, Serialize};
use tracing::{debug, info, warn};

use organon::types::{ToolGroupId, ToolGroupPolicy};

use crate::error::{self, Result};
use crate::roles::Role;

/// A versioned behavior contract for a single role.
///
/// Defines what a role MUST do (behaviors) and what it MUST NOT do
/// (constraints). The version field increments when behaviors change,
/// allowing QA scenarios to pin against a specific contract version.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RoleContract {
    /// Role name (e.g. "coder", "reviewer").
    pub role: String,
    /// Contract version. Increments when behaviors or constraints change.
    pub version: u32,
    /// Expected behaviors: what this role MUST do.
    pub behaviors: Vec<String>,
    /// Constraints: what this role MUST NOT do.
    pub constraints: Vec<String>,
    /// Tool-group policy. Missing or empty configuration denies all tools.
    #[serde(default)]
    pub tool_groups: ToolGroupPolicy,
    /// Episteme knowledge-store cohort for agents spawned under this role.
    ///
    /// `None` defers to the spawn service's own baseline cohort rather than
    /// asserting one, so an unconfigured role keeps prior behavior exactly.
    #[serde(default)]
    pub episteme_cohort: Option<String>,
    /// Whether agents spawned under this role are hidden from public
    /// cross-nous discovery.
    #[serde(default)]
    pub private: bool,
    /// Domain tags applied to agents spawned under this role.
    #[serde(default)]
    pub domains: Vec<String>,
    /// Per-role model override.
    ///
    /// `None` defers to the role template's compiled model (itself
    /// `koina::models::task_role_default`'s tier reference), so an
    /// unconfigured role keeps prior behavior exactly. Resolution order at
    /// spawn time is `request.model -> contract.model -> template.model ->
    /// SONNET_MODEL` (`spawn_svc.rs::build_spawn_config`) — this field only
    /// ever applies to a spawn request whose role string resolves to a known
    /// `Role` variant; an unrecognized role never reaches a contract at all
    /// (ADR-005's conservative fallback), so a `model` entry under a role
    /// name with no Rust template is parsed and stored but never consulted.
    #[serde(default)]
    pub model: Option<String>,
}

impl RoleContract {
    /// Format the contract as a system prompt section.
    ///
    /// Produces a markdown-formatted block suitable for injection into
    /// the bootstrap system prompt.
    #[must_use]
    #[expect(
        clippy::format_push_string,
        reason = "push_str with format is clearer than write!+expect for infallible String writes"
    )]
    pub fn to_prompt_section(&self) -> String {
        let mut out = format!("## Role Contract: {} (v{})\n\n", self.role, self.version);

        if !self.behaviors.is_empty() {
            out.push_str("### Expected Behaviors\n\n");
            for behavior in &self.behaviors {
                out.push_str(&format!("- {behavior}\n"));
            }
            out.push('\n');
        }

        match &self.tool_groups {
            ToolGroupPolicy::Groups(groups) => {
                out.push_str("### Allowed Tool Groups\n\n");
                for group in groups {
                    out.push_str(&format!("- {group}\n"));
                }
                out.push('\n');
            }
            ToolGroupPolicy::AllowAll { reason } => {
                out.push_str("### Tool Group Policy\n\n");
                out.push_str(&format!("- all ({reason})\n\n"));
            }
            ToolGroupPolicy::DenyAll => {
                out.push_str("### Tool Group Policy\n\n");
                out.push_str("- deny\n\n");
            }
            _ => { /* no markdown representation for this policy variant */ }
        }

        if !self.constraints.is_empty() {
            out.push_str("### Constraints\n\n");
            for constraint in &self.constraints {
                out.push_str(&format!("- MUST NOT: {constraint}\n"));
            }
            out.push('\n');
        }

        out
    }
}

/// TOML file structure for `roles.toml`.
///
/// Each role is a table key containing version, behaviors, and constraints.
///
/// ```toml
/// [coder]
/// version = 1
/// behaviors = ["Write and modify code to complete tasks", ...]
/// constraints = ["Refactor code outside the assigned scope", ...]
/// ```
#[derive(Debug, Clone, Deserialize)]
struct RolesFile {
    /// Flattened map of role name -> contract fields.
    #[serde(flatten)]
    roles: HashMap<String, RoleContractToml>,
}

/// Per-role TOML fields (without the role name, which is the table key).
#[derive(Debug, Clone, Deserialize)]
struct RoleContractToml {
    version: u32,
    #[serde(default)]
    behaviors: Vec<String>,
    #[serde(default)]
    constraints: Vec<String>,
    #[serde(default)]
    tool_groups: ToolGroupPolicy,
    #[serde(default)]
    episteme_cohort: Option<String>,
    #[serde(default)]
    private: bool,
    #[serde(default)]
    domains: Vec<String>,
    #[serde(default)]
    model: Option<String>,
}

/// Registry of role contracts, keyed by role name.
#[derive(Debug, Clone)]
pub struct ContractRegistry {
    contracts: HashMap<String, RoleContract>,
}

impl ContractRegistry {
    /// Create a registry with hardcoded default contracts for all roles.
    #[must_use]
    pub fn defaults() -> Self {
        let mut contracts = HashMap::new();
        for role in Role::all() {
            let contract = default_contract(*role);
            contracts.insert(role.as_str().to_owned(), contract);
        }
        Self { contracts }
    }

    /// Load contracts from a TOML file, falling back to defaults for
    /// any role not present in the file.
    ///
    /// # Errors
    ///
    /// Returns [`error::Error::RoleContract`] if the file exists but
    /// cannot be parsed as valid TOML.
    pub fn load_from_file(path: &Path) -> Result<Self> {
        let content = match std::fs::read_to_string(path) {
            Ok(c) => c,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                debug!(?path, "roles.toml not found, using defaults");
                return Ok(Self::defaults());
            }
            Err(e) => {
                warn!(?path, error = %e, "failed to read roles.toml, using defaults");
                return Ok(Self::defaults());
            }
        };

        Self::parse_toml(&content, path)
    }

    /// Load contracts from a TOML string (for testing and embedded use).
    ///
    /// # Errors
    ///
    /// Returns [`error::Error::RoleContract`] if the string is not valid
    /// roles TOML.
    pub fn from_toml(content: &str) -> Result<Self> {
        Self::parse_toml(content, Path::new("<inline>"))
    }

    /// Parse TOML content into a registry, merging with defaults.
    fn parse_toml(content: &str, source: &Path) -> Result<Self> {
        let file: RolesFile = toml::from_str(content).map_err(|e| {
            error::RoleContractSnafu {
                message: format!("failed to parse {}: {e}", source.display()),
            }
            .build()
        })?;

        // WHY: start with defaults so roles missing from the file still have contracts
        let mut registry = Self::defaults();

        for (role_name, toml_contract) in file.roles {
            let contract = RoleContract {
                role: role_name.clone(),
                version: toml_contract.version,
                behaviors: toml_contract.behaviors,
                constraints: toml_contract.constraints,
                tool_groups: toml_contract.tool_groups,
                episteme_cohort: toml_contract.episteme_cohort,
                private: toml_contract.private,
                domains: toml_contract.domains,
                model: toml_contract.model,
            };
            info!(
                role = %role_name,
                version = contract.version,
                behaviors = contract.behaviors.len(),
                constraints = contract.constraints.len(),
                tool_group_policy = %contract.tool_groups.description(),
                model = ?contract.model,
                "loaded role contract from file"
            );
            registry.contracts.insert(role_name, contract);
        }

        Ok(registry)
    }

    /// Look up the contract for a role by name.
    #[must_use]
    pub fn get(&self, role: &str) -> Option<&RoleContract> {
        self.contracts.get(role)
    }

    /// All contracts in the registry.
    #[must_use]
    pub fn all(&self) -> &HashMap<String, RoleContract> {
        &self.contracts
    }

    /// Number of contracts in the registry.
    #[must_use]
    pub fn len(&self) -> usize {
        self.contracts.len()
    }

    /// Whether the registry is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.contracts.is_empty()
    }
}

impl Default for ContractRegistry {
    fn default() -> Self {
        Self::defaults()
    }
}

// ── Default contracts for built-in roles ────────────────────────────────

fn default_contract(role: Role) -> RoleContract {
    match role {
        Role::Coder => coder_contract(),
        Role::Researcher => researcher_contract(),
        Role::Reviewer => reviewer_contract(),
        Role::Explorer => explorer_contract(),
        Role::Runner => runner_contract(),
    }
}

fn coder_contract() -> RoleContract {
    RoleContract {
        role: "coder".to_owned(),
        version: 1,
        behaviors: vec![
            "Read relevant files before making changes".to_owned(),
            "Make the specified changes precisely".to_owned(),
            "Verify changes compile by running the build".to_owned(),
            "Run relevant tests if they exist".to_owned(),
            "Report what was changed with file paths".to_owned(),
            "Match existing code patterns and style".to_owned(),
            "Make conservative choices on ambiguity and note them".to_owned(),
        ],
        constraints: vec![
            "Refactor code outside the assigned scope".to_owned(),
            "Add features not requested".to_owned(),
            "Ask clarifying questions instead of making conservative choices".to_owned(),
            "Leave the build broken".to_owned(),
        ],
        tool_groups: ToolGroupPolicy::groups(vec![
            ToolGroupId::Read,
            ToolGroupId::Edit,
            ToolGroupId::Command,
            ToolGroupId::Verify,
        ]),
        episteme_cohort: None,
        private: false,
        domains: Vec::new(),
        model: None,
    }
}

fn researcher_contract() -> RoleContract {
    RoleContract {
        role: "researcher".to_owned(),
        version: 1,
        behaviors: vec![
            "Cite sources for every claim".to_owned(),
            "Distinguish fact from inference".to_owned(),
            "Respect scope constraints".to_owned(),
            "Prefer the most recent documentation".to_owned(),
            "Admit gaps when information is unavailable".to_owned(),
            "Synthesize findings into structured reports".to_owned(),
        ],
        constraints: vec![
            "Present inference as established fact".to_owned(),
            "Ignore scope constraints".to_owned(),
            "Omit source citations".to_owned(),
            "Modify files or execute commands".to_owned(),
        ],
        tool_groups: ToolGroupPolicy::groups(vec![
            ToolGroupId::Read,
            ToolGroupId::Mcp,
            ToolGroupId::Plan,
        ]),
        episteme_cohort: None,
        private: false,
        domains: Vec::new(),
        model: None,
    }
}

fn reviewer_contract() -> RoleContract {
    RoleContract {
        role: "reviewer".to_owned(),
        version: 1,
        behaviors: vec![
            "Provide specific findings with file paths and line numbers".to_owned(),
            "Categorize issues by severity (error, warning, info)".to_owned(),
            "Check correctness, edge cases, error handling, style, backward compatibility"
                .to_owned(),
            "Check test coverage for new code paths".to_owned(),
            "Acknowledge clean code without inventing problems".to_owned(),
        ],
        constraints: vec![
            "Fix or modify code".to_owned(),
            "Write to files or execute commands".to_owned(),
            "Invent problems to appear thorough".to_owned(),
            "Provide vague feedback without specific locations".to_owned(),
        ],
        tool_groups: ToolGroupPolicy::groups(vec![
            ToolGroupId::Read,
            ToolGroupId::Verify,
            ToolGroupId::Mcp,
        ]),
        episteme_cohort: None,
        private: false,
        domains: Vec::new(),
        model: None,
    }
}

fn explorer_contract() -> RoleContract {
    RoleContract {
        role: "explorer".to_owned(),
        version: 1,
        behaviors: vec![
            "Use grep/find before reading whole files".to_owned(),
            "Include file paths and line numbers for every finding".to_owned(),
            "Trace call chains from entry point to final execution".to_owned(),
            "Summarize findings rather than dumping raw content".to_owned(),
        ],
        constraints: vec![
            "Write, edit, or execute anything".to_owned(),
            "Dump entire file contents without summarizing".to_owned(),
            "Report findings without file paths".to_owned(),
        ],
        tool_groups: ToolGroupPolicy::groups(vec![ToolGroupId::Read, ToolGroupId::Plan]),
        episteme_cohort: None,
        private: false,
        domains: Vec::new(),
        model: None,
    }
}

fn runner_contract() -> RoleContract {
    RoleContract {
        role: "runner".to_owned(),
        version: 1,
        behaviors: vec![
            "Run exactly the commands requested".to_owned(),
            "Capture exit codes, stdout, and stderr".to_owned(),
            "Report test results with counts and failure details".to_owned(),
            "Report hangs and timeouts".to_owned(),
        ],
        constraints: vec![
            "Run destructive commands unless explicitly part of the task".to_owned(),
            "Diagnose or suggest fixes for failures".to_owned(),
            "Add extra commands not requested".to_owned(),
            "Retry commands unless instructed".to_owned(),
        ],
        tool_groups: ToolGroupPolicy::groups(vec![
            ToolGroupId::Read,
            ToolGroupId::Command,
            ToolGroupId::Verify,
        ]),
        episteme_cohort: None,
        private: false,
        domains: Vec::new(),
        model: None,
    }
}

#[cfg(test)]
#[path = "contract_tests.rs"]
mod tests;

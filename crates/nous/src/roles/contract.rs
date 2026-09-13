//! Versioned role-behavior contracts, wired into ephemeral sub-agent spawns.
//!
//! Each role gets a contract defining expected behaviors, constraints, and
//! spawn policy (`tool_groups`, `episteme_cohort`, `private`, `domains`,
//! `model`). When a role's behavior changes, the version increments,
//! enabling QA (dokimion) to validate against the correct version.
//!
//! Contracts are loaded from `roles.toml` in the oikos cascade
//! (nous/{id}/ -> shared/ -> theke/). Hardcoded defaults are used when
//! no file is found; an existing file that cannot be read or parsed is a
//! hard error, not a fallback to those defaults (#7169) — see
//! [`ContractRegistry::load_from_file`]. `SpawnServiceImpl::resolve_contract`
//! (spawn_svc.rs) is the production caller (#4775) — behaviors/constraints
//! append to the spawned agent's system prompt, `tool_groups` refines the
//! coarse tool-group gate, `episteme_cohort`/`private`/`domains` replace the
//! hardcoded constants a spawned agent previously always got, and `model`
//! (wave 3.3) overrides the compiled `RoleTemplate` model a spawned agent
//! would otherwise resolve to.
//!
//! A tier that has never had a `roles.toml` keeps resolving `NotFound` to
//! [`ContractRegistry::defaults()`] (#4775) with no ceremony. But once a
//! `roles.toml` has been loaded successfully from a given path, that path
//! is "contracts required": a later `NotFound` there means the contract
//! was removed, not that one was never configured, and is refused rather
//! than silently re-opened to the liberal defaults (#7323, the residual
//! risk #7169's decision record called out — deleting a restrictive
//! `roles.toml` is cheaper than corrupting one, and lands on the same
//! `NotFound` #7169 deliberately left alone). See
//! [`has_been_configured`].

use std::collections::HashMap;
use std::path::{Path, PathBuf};

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
    /// A missing file is not an error the first time: it means "no
    /// override configured" and returns [`Self::defaults()`]. But if this
    /// exact `path` has previously been loaded from successfully
    /// ([`has_been_configured`]), a later `NotFound` means an
    /// operator-configured contract was removed, not that one was never
    /// set, and is fail-closed the same way #7169 fails closed on a
    /// corrupt file (#7323) — silently substituting the liberal defaults
    /// for a contract that used to restrict this tier is a
    /// privilege-restoration bug, not a harmless degrade.
    ///
    /// Any other read failure (permission denied, not a regular file, ...)
    /// or a parse failure is fail-closed unconditionally, as before
    /// (#7169).
    ///
    /// # Errors
    ///
    /// Returns [`error::Error::RoleContract`] if: the file exists but
    /// cannot be read (any [`std::io::Error`] other than
    /// [`std::io::ErrorKind::NotFound`]); it cannot be parsed as valid
    /// TOML; or it is `NotFound` at a `path` that has previously carried a
    /// successfully-loaded contract (#7323).
    pub fn load_from_file(path: &Path) -> Result<Self> {
        let content = match std::fs::read_to_string(path) {
            Ok(c) => c,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                if has_been_configured(path) {
                    return error::RoleContractSnafu {
                        message: format!(
                            "{} is missing, but a role contract was previously configured at \
                             this path; refusing to silently restore the liberal defaults \
                             (#7323) — restore {} to return the operator-configured contract, \
                             or delete the sentinel at {} to deliberately return this tier to \
                             hardcoded defaults",
                            path.display(),
                            path.display(),
                            sentinel_path(path).display()
                        ),
                    }
                    .fail();
                }
                debug!(?path, "roles.toml not found, using defaults");
                return Ok(Self::defaults());
            }
            Err(e) => {
                return error::RoleContractSnafu {
                    message: format!("failed to read {}: {e}", path.display()),
                }
                .fail();
            }
        };

        let registry = Self::parse_toml(&content, path)?;
        mark_configured(path);
        Ok(registry)
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

// ── "Contracts required" posture (#7323) ────────────────────────────────
//
// A tiny marker file, colocated with a `roles.toml`, that records "a
// contract was successfully loaded from this exact path at least once".
// It is the detection mechanism #7323's decision record left open
// ("a sentinel/marker file, an explicit allowlist of tiers that must
// carry a contract, an audit-logged prior-state check, ..."), scoped to
// the exact path a caller resolves (one path per cascade tier), so it
// composes with the existing nous/shared/theke cascade without needing to
// know about tiers itself.
//
// WHY: best-effort, not a hard guarantee — a sentinel write failure (e.g.
// a read-only config directory) is logged and does not fail the load that
// triggered it, because refusing to use a roles.toml an operator can
// legitimately read would be a worse regression than the gap this closes.
// An operator who deletes both the contract and its sentinel in one
// motion is out of scope here, same as #7169's own read/parse fail-closed
// posture does not defend against deleting the file outright — this
// closes the *cheaper* escalation (delete only), not every escalation.

/// The sentinel path for a given `roles.toml` path.
///
/// `pub(crate)` (not just used internally) so a cascade-aware caller
/// (`SpawnServiceImpl::resolve_contract` in `spawn_svc.rs`) can name the
/// exact sentinel path in its own refusal message for a tier whose file
/// is currently absent, matching the guidance [`load_from_file`] gives
/// for the path it read directly.
pub(crate) fn sentinel_path(path: &Path) -> PathBuf {
    let marker = match path.file_name() {
        Some(name) => format!(".{}.contract-seen", name.to_string_lossy()),
        None => ".roles.toml.contract-seen".to_owned(),
    };
    match path.parent() {
        Some(parent) => parent.join(marker),
        None => PathBuf::from(marker),
    }
}

/// Whether a role contract has ever been loaded successfully from `path`.
///
/// `true` turns a later `NotFound` at `path` into a fail-closed error
/// instead of [`ContractRegistry::defaults()`] — see
/// [`ContractRegistry::load_from_file`]. Exposed (not just used
/// internally) so a cascade-aware caller (`SpawnServiceImpl::resolve_contract`
/// in `spawn_svc.rs`) can apply the same check to a tier whose file is
/// currently absent everywhere, not only to a path it is about to read.
#[must_use]
pub fn has_been_configured(path: &Path) -> bool {
    sentinel_path(path).exists()
}

/// Record that `path` was loaded successfully, for future [`has_been_configured`] checks.
///
/// Best-effort: a write failure is logged, not propagated — see the
/// module-level WHY above.
#[expect(
    clippy::disallowed_methods,
    reason = "sentinel write is a synchronous best-effort side effect of a synchronous, \
              non-async load_from_file call; the disallowed-methods reason (async/testability) \
              does not apply to a zero-byte marker write on the error-logging path only"
)]
fn mark_configured(path: &Path) {
    let marker = sentinel_path(path);
    if let Err(e) = std::fs::write(&marker, b"") {
        warn!(
            ?path,
            marker = %marker.display(),
            error = %e,
            "failed to write roles.toml contract-seen sentinel; a future deletion of this \
             roles.toml will not be detected as contract removal"
        );
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

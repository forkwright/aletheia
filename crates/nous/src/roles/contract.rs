//! Versioned role-behavior contracts for QA validation.
//!
//! Each role gets a contract defining expected behaviors and constraints.
//! When a role's behavior changes, the version increments, enabling QA
//! (dokimion) to validate against the correct version.
//!
//! Contracts are loaded from `roles.toml` in the oikos cascade
//! (nous/{id}/ -> shared/ -> theke/). Hardcoded defaults are used when
//! no file is found.

use std::collections::HashMap;
use std::path::Path;

use serde::{Deserialize, Serialize};
use tracing::{debug, info, warn};

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
}

impl RoleContract {
    /// Format the contract as a system prompt section.
    ///
    /// Produces a markdown-formatted block suitable for injection into
    /// the bootstrap system prompt.
    #[must_use]
    pub fn to_prompt_section(&self) -> String {
        use std::fmt::Write;

        let mut out = format!(
            "## Role Contract: {} (v{})\n\n",
            self.role, self.version
        );

        if !self.behaviors.is_empty() {
            out.push_str("### Expected Behaviors\n\n");
            for behavior in &self.behaviors {
                let _ = writeln!(out, "- {behavior}");
            }
            out.push('\n');
        }

        if !self.constraints.is_empty() {
            out.push_str("### Constraints\n\n");
            for constraint in &self.constraints {
                let _ = writeln!(out, "- MUST NOT: {constraint}");
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
            };
            info!(
                role = %role_name,
                version = contract.version,
                behaviors = contract.behaviors.len(),
                constraints = contract.constraints.len(),
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
    }
}

fn reviewer_contract() -> RoleContract {
    RoleContract {
        role: "reviewer".to_owned(),
        version: 1,
        behaviors: vec![
            "Provide specific findings with file paths and line numbers".to_owned(),
            "Categorize issues by severity (error, warning, info)".to_owned(),
            "Check correctness, edge cases, error handling, style, backward compatibility".to_owned(),
            "Check test coverage for new code paths".to_owned(),
            "Acknowledge clean code without inventing problems".to_owned(),
        ],
        constraints: vec![
            "Fix or modify code".to_owned(),
            "Write to files or execute commands".to_owned(),
            "Invent problems to appear thorough".to_owned(),
            "Provide vague feedback without specific locations".to_owned(),
        ],
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
    }
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions may panic on failure")]
#[expect(
    clippy::disallowed_methods,
    reason = "test fixtures use std::fs to write tempdir files synchronously"
)]
mod tests {
    use super::*;

    #[test]
    fn default_registry_has_all_roles() {
        let registry = ContractRegistry::defaults();
        assert_eq!(registry.len(), 5, "must have contracts for all 5 roles");
        for role in Role::all() {
            assert!(
                registry.get(role.as_str()).is_some(),
                "missing contract for {role}"
            );
        }
    }

    #[test]
    fn default_contracts_have_version_one() {
        let registry = ContractRegistry::defaults();
        for (name, contract) in registry.all() {
            assert_eq!(
                contract.version, 1,
                "default contract for {name} should be version 1"
            );
        }
    }

    #[test]
    fn default_contracts_have_behaviors_and_constraints() {
        let registry = ContractRegistry::defaults();
        for (name, contract) in registry.all() {
            assert!(
                !contract.behaviors.is_empty(),
                "contract for {name} has no behaviors"
            );
            assert!(
                !contract.constraints.is_empty(),
                "contract for {name} has no constraints"
            );
        }
    }

    #[test]
    fn from_toml_parses_valid_config() {
        let toml = r#"
[coder]
version = 2
behaviors = ["Write code", "Run tests"]
constraints = ["Break the build"]

[reviewer]
version = 1
behaviors = ["Review code"]
constraints = ["Modify code"]
"#;
        let registry = ContractRegistry::from_toml(toml).unwrap();
        let coder = registry.get("coder").unwrap();
        assert_eq!(coder.version, 2);
        assert_eq!(coder.behaviors.len(), 2);
        assert_eq!(coder.constraints.len(), 1);

        // WHY: reviewer is overridden from file, not default
        let reviewer = registry.get("reviewer").unwrap();
        assert_eq!(reviewer.behaviors.len(), 1);
    }

    #[test]
    fn from_toml_preserves_defaults_for_missing_roles() {
        let toml = r#"
[coder]
version = 2
behaviors = ["Write code"]
constraints = ["Break things"]
"#;
        let registry = ContractRegistry::from_toml(toml).unwrap();

        // Coder was overridden
        assert_eq!(registry.get("coder").unwrap().version, 2);

        // Other roles still have defaults
        let runner = registry.get("runner").unwrap();
        assert_eq!(runner.version, 1);
        assert!(!runner.behaviors.is_empty());
    }

    #[test]
    fn from_toml_allows_custom_roles() {
        let toml = r#"
[planner]
version = 1
behaviors = ["Create plans"]
constraints = ["Execute plans"]
"#;
        let registry = ContractRegistry::from_toml(toml).unwrap();
        let planner = registry.get("planner").unwrap();
        assert_eq!(planner.role, "planner");
        assert_eq!(planner.version, 1);

        // Built-in defaults still present
        assert!(registry.get("coder").is_some());
    }

    #[test]
    fn from_toml_rejects_invalid_toml() {
        let result = ContractRegistry::from_toml("this is not { valid toml");
        assert!(result.is_err());
    }

    #[test]
    fn to_prompt_section_formats_correctly() {
        let contract = RoleContract {
            role: "coder".to_owned(),
            version: 2,
            behaviors: vec!["Write code".to_owned(), "Run tests".to_owned()],
            constraints: vec!["Break the build".to_owned()],
        };

        let section = contract.to_prompt_section();
        assert!(
            section.contains("Role Contract: coder (v2)"),
            "should contain role and version"
        );
        assert!(
            section.contains("- Write code"),
            "should list behaviors"
        );
        assert!(
            section.contains("- MUST NOT: Break the build"),
            "should list constraints with MUST NOT prefix"
        );
    }

    #[test]
    fn to_prompt_section_handles_empty_lists() {
        let contract = RoleContract {
            role: "empty".to_owned(),
            version: 1,
            behaviors: Vec::new(),
            constraints: Vec::new(),
        };
        let section = contract.to_prompt_section();
        assert!(
            !section.contains("Expected Behaviors"),
            "should omit behaviors header when empty"
        );
        assert!(
            !section.contains("Constraints"),
            "should omit constraints header when empty"
        );
    }

    #[test]
    fn load_from_file_returns_defaults_for_missing_file() {
        let registry =
            ContractRegistry::load_from_file(Path::new("/nonexistent/roles.toml")).unwrap();
        assert_eq!(registry.len(), 5, "should fall back to defaults");
    }

    #[test]
    fn load_from_file_reads_real_file() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("roles.toml");
        std::fs::write(
            &path,
            r#"
[coder]
version = 3
behaviors = ["Custom behavior"]
constraints = ["Custom constraint"]
"#,
        )
        .unwrap();

        let registry = ContractRegistry::load_from_file(&path).unwrap();
        let coder = registry.get("coder").unwrap();
        assert_eq!(coder.version, 3);
        assert_eq!(coder.behaviors, vec!["Custom behavior"]);
    }

    #[test]
    fn contract_serde_roundtrip() {
        let contract = RoleContract {
            role: "coder".to_owned(),
            version: 2,
            behaviors: vec!["Write code".to_owned()],
            constraints: vec!["Break things".to_owned()],
        };
        let json = serde_json::to_string(&contract).unwrap();
        let back: RoleContract = serde_json::from_str(&json).unwrap();
        assert_eq!(contract, back);
    }

    #[test]
    fn registry_default_trait() {
        let registry = ContractRegistry::default();
        assert_eq!(registry.len(), 5);
    }

    #[test]
    fn registry_is_empty() {
        let registry = ContractRegistry::defaults();
        assert!(!registry.is_empty());
    }

    #[test]
    fn role_name_matches_contract_role_field() {
        let registry = ContractRegistry::defaults();
        for (name, contract) in registry.all() {
            assert_eq!(
                name, &contract.role,
                "registry key should match contract.role"
            );
        }
    }
}

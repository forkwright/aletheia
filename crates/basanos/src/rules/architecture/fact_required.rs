//! Architecture rule: ARCHITECTURE/fact-required.
//!
//! Warns when a PR touches a crate's `src/lib.rs` or adds a new public
//! module declaration without a corresponding `ArchitectureFact` annotation
//! in the fact store.
//!
//! # Heuristic (v1)
//!
//! For each file scanned:
//!
//! 1. If the file is `src/lib.rs`, check whether an `ArchitectureFact` JSON
//!    file exists for that crate (id prefix: `<project-prefix>.<crate-name>.*`).
//! 2. If the file contains a `pub mod <name>` declaration, check whether a
//!    fact with id `<project-prefix>.<crate>.<module>` exists.
//!
//! When no fact store is present (fresh install), the rule emits no violations
//! — absence of the store is not itself a violation; the first PR that lands
//! after the store is created will trigger.
//!
//! # Severity
//!
//! Warn for v1.  The rule is intentionally non-blocking so that the fleet
//! builds discipline gradually before moving to Error severity.
//!
//! # Future
//!
//! A v2 implementation should query the `architecture_fact` MCP tool via
//! the Kanon CI surface instead of scanning the flat directory directly.
//! The file-level heuristic is acceptable for v1 per the acceptance criteria.

use std::fs;
use std::path::{Path, PathBuf};

use snafu::ResultExt;

use crate::error::{self, Result};
use crate::rules::{Rule, Violation};

/// Rule: ARCHITECTURE/fact-required.
///
/// Scan crate source files for architectural seams (lib.rs, pub mod
/// declarations) and warn when no architecture fact is present for them.
pub struct FactRequiredRule {
    config: FactRequiredConfig,
}

/// Configuration for [`FactRequiredRule`].
#[derive(Debug, Clone)]
pub struct FactRequiredConfig {
    /// Whether the policy is active. Defaults to `true`.
    pub enabled: bool,
    /// Directory containing flat JSON architecture facts.
    pub facts_dir: PathBuf,
    /// Prefix used when deriving expected fact IDs.
    pub project_prefix: String,
}

impl FactRequiredConfig {
    fn fact_prefix(&self, crate_nm: &str) -> String {
        format!("{}.{crate_nm}.", self.project_prefix)
    }

    fn module_fact_id(&self, crate_nm: &str, module_name: &str) -> String {
        format!("{}.{crate_nm}.{module_name}", self.project_prefix)
    }
}

impl Default for FactRequiredConfig {
    fn default() -> Self {
        let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_owned());
        Self {
            enabled: true,
            facts_dir: PathBuf::from(home).join("aletheia/instance/facts"),
            project_prefix: "aletheia".to_owned(),
        }
    }
}

impl FactRequiredRule {
    /// Create a rule with explicit configuration.
    #[must_use]
    pub fn with_config(config: FactRequiredConfig) -> Self {
        Self { config }
    }
}

impl Default for FactRequiredRule {
    fn default() -> Self {
        Self::with_config(FactRequiredConfig::default())
    }
}

/// Rule severity label — Warn for v1.
const SEVERITY: &str = "warn";

impl Rule for FactRequiredRule {
    fn id(&self) -> &'static str {
        "ARCHITECTURE/fact-required"
    }

    fn check(&self, project_root: &str) -> Result<Vec<Violation>> {
        let root = Path::new(project_root);
        let mut violations = Vec::new();

        if !self.config.enabled {
            return Ok(violations);
        }

        // Walk crates/ looking for Rust crate roots.
        let crates_dir = root.join("crates");
        if !crates_dir.is_dir() {
            return Ok(violations);
        }

        for entry in fs::read_dir(&crates_dir).with_context(|_| error::ReadDirSnafu {
            path: crates_dir.clone(),
        })? {
            let entry = entry.with_context(|_| error::ReadDirSnafu {
                path: crates_dir.clone(),
            })?;
            let crate_path = entry.path();
            if !crate_path.is_dir() {
                continue;
            }
            let lib_rs = crate_path.join("src").join("lib.rs");
            if lib_rs.is_file() {
                check_lib_rs(&lib_rs, &crate_path, &self.config, &mut violations)?;
            }
        }

        Ok(violations)
    }
}

/// Derive the crate name from its directory path (last component).
fn crate_name(crate_path: &Path) -> String {
    crate_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("unknown")
        .to_owned()
}

/// Check `src/lib.rs` for missing architecture facts.
///
/// - Checks for a crate-level fact: any `<project>.<crate_name>.*` fact file.
/// - Checks each `pub mod <name>` declaration for a module-level fact.
fn check_lib_rs(
    lib_rs: &Path,
    crate_path: &Path,
    config: &FactRequiredConfig,
    violations: &mut Vec<Violation>,
) -> Result<()> {
    // If the fact store directory does not exist, skip — store is not yet
    // initialised for this installation.
    if !config.facts_dir.is_dir() {
        return Ok(());
    }

    let crate_nm = crate_name(crate_path);
    let content = fs::read_to_string(lib_rs).with_context(|_| error::ReadFileSnafu {
        path: lib_rs.to_path_buf(),
    })?;

    let crate_prefix = config.fact_prefix(&crate_nm);
    let has_crate_fact = fact_exists_with_prefix(&config.facts_dir, &crate_prefix);

    if !has_crate_fact {
        violations.push(Violation {
            rule: "ARCHITECTURE/fact-required".to_owned(),
            path: lib_rs.display().to_string(),
            line: 1,
            message: format!(
                "[{SEVERITY}] crate '{crate_nm}' has no architecture fact (expected id prefix \
                 '{crate_prefix}*' in {facts_dir}). \
                 Add a fact via the `architecture_fact` MCP tool.",
                facts_dir = config.facts_dir.display(),
            ),
        });
    }

    // Check each `pub mod <name>` for a corresponding module-level fact.
    for (idx, line) in content.lines().enumerate() {
        let trimmed = line.trim();
        if let Some(module_name) = extract_pub_mod(trimmed) {
            let fact_id = config.module_fact_id(&crate_nm, module_name);
            if !fact_file_exists(&config.facts_dir, &fact_id) {
                violations.push(Violation {
                    rule: "ARCHITECTURE/fact-required".to_owned(),
                    path: lib_rs.display().to_string(),
                    line: idx + 1,
                    message: format!(
                        "[{SEVERITY}] public module '{module_name}' in crate '{crate_nm}' has no \
                         architecture fact (expected id '{fact_id}'). \
                         Add a fact via the `architecture_fact` MCP tool.",
                    ),
                });
            }
        }
    }

    Ok(())
}

/// Check whether any `.json` file in `facts_dir` starts with `prefix`.
fn fact_exists_with_prefix(facts_dir: &Path, prefix: &str) -> bool {
    let Ok(entries) = fs::read_dir(facts_dir) else {
        return false;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        if name_str.ends_with(".json") && name_str.starts_with(prefix) {
            return true;
        }
    }
    false
}

/// Check whether a fact file for `id` exists (exact filename match).
fn fact_file_exists(facts_dir: &Path, id: &str) -> bool {
    // Mirror the FactStore::id_to_filename logic: replace `/` → `-`, append `.json`.
    let mut filename: String = id
        .chars()
        .map(|c| if c == '/' || c == '\\' { '-' } else { c })
        .collect();
    filename.push_str(".json");
    facts_dir.join(filename).exists()
}

/// Extract the module name from a `pub mod <name>` or `pub mod <name>;` line.
///
/// Returns `None` for non-pub-mod lines or conditional mods (`#[cfg…]` is on
/// the preceding line, not this one).
fn extract_pub_mod(line: &str) -> Option<&str> {
    // Match: `pub mod <name>` or `pub mod <name>;` or `pub mod <name> {`
    let rest = line.strip_prefix("pub mod ")?;
    let name = rest.trim_end_matches([';', '{', ' ']).trim();
    if name.is_empty() || name.contains(' ') || name.contains('(') {
        return None;
    }
    Some(name)
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test assertions")]
mod tests {
    use std::fs;

    use super::*;

    // ── extract_pub_mod ──────────────────────────────────────────────────

    #[test]
    fn extract_pub_mod_basic() {
        assert_eq!(extract_pub_mod("pub mod knowledge;"), Some("knowledge"));
        assert_eq!(extract_pub_mod("pub mod knowledge {"), Some("knowledge"));
        assert_eq!(extract_pub_mod("pub mod knowledge"), Some("knowledge"));
    }

    #[test]
    fn extract_pub_mod_ignores_non_pub() {
        assert!(extract_pub_mod("mod private;").is_none());
        assert!(extract_pub_mod("use std::io;").is_none());
    }

    #[test]
    fn extract_pub_mod_ignores_conditional() {
        // Line itself is not a pub mod line if it has attributes mixed in.
        assert!(extract_pub_mod("#[cfg(test)] pub mod tests").is_none());
    }

    // ── fact_exists_with_prefix ──────────────────────────────────────────

    #[test]
    fn fact_exists_with_prefix_positive() {
        let dir = tempfile::tempdir().expect("tempdir");
        fs::write(dir.path().join("aletheia.eidos.dep.json"), b"{}").expect("write");
        assert!(fact_exists_with_prefix(dir.path(), "aletheia.eidos."));
    }

    #[test]
    fn fact_exists_with_prefix_negative() {
        let dir = tempfile::tempdir().expect("tempdir");
        fs::write(dir.path().join("aletheia.organon.dep.json"), b"{}").expect("write");
        assert!(!fact_exists_with_prefix(dir.path(), "aletheia.eidos."));
    }

    // ── full rule: no fact → warn ────────────────────────────────────────

    #[test]
    fn rule_warns_when_no_fact_for_crate() {
        let project = tempfile::tempdir().expect("tempdir");
        let facts = tempfile::tempdir().expect("facts tempdir");

        // Create a minimal crate structure.
        let crate_dir = project.path().join("crates").join("eidos");
        fs::create_dir_all(crate_dir.join("src")).expect("mkdir");
        fs::write(crate_dir.join("src").join("lib.rs"), b"//! eidos lib\n").expect("write lib.rs");

        let rule = FactRequiredRule::with_config(FactRequiredConfig {
            facts_dir: facts.path().to_path_buf(),
            ..FactRequiredConfig::default()
        });
        let violations = rule
            .check(project.path().to_str().expect("valid path"))
            .expect("check");
        assert!(
            !violations.is_empty(),
            "expected violation for crate without fact"
        );
        #[expect(
            clippy::indexing_slicing,
            reason = "test assertion: non-empty checked above"
        )]
        let (rule_id, message) = (violations[0].rule.clone(), violations[0].message.clone());
        assert!(rule_id == "ARCHITECTURE/fact-required");
        assert!(
            message.contains("aletheia.eidos."),
            "message should reference expected fact prefix"
        );
    }

    // ── full rule: fact present → no warn ───────────────────────────────

    #[test]
    fn rule_no_warn_when_fact_present() {
        let project = tempfile::tempdir().expect("tempdir");
        let facts = tempfile::tempdir().expect("facts tempdir");

        // Create crate.
        let crate_dir = project.path().join("crates").join("eidos");
        fs::create_dir_all(crate_dir.join("src")).expect("mkdir");
        fs::write(crate_dir.join("src").join("lib.rs"), b"//! eidos lib\n").expect("write lib.rs");

        // Create a fact file that satisfies the prefix check.
        fs::write(
            facts
                .path()
                .join("aletheia.eidos.dependency-direction.json"),
            b"{\"id\":\"aletheia.eidos.dependency-direction\"}",
        )
        .expect("write fact");

        let rule = FactRequiredRule::with_config(FactRequiredConfig {
            facts_dir: facts.path().to_path_buf(),
            ..FactRequiredConfig::default()
        });
        let violations = rule
            .check(project.path().to_str().expect("valid path"))
            .expect("check");
        assert!(
            violations.is_empty(),
            "expected no violations when fact is present; got: {violations:?}"
        );
    }

    // ── pub mod check ────────────────────────────────────────────────────

    #[test]
    fn rule_warns_for_pub_mod_without_fact() {
        let project = tempfile::tempdir().expect("tempdir");
        let facts = tempfile::tempdir().expect("facts tempdir");

        let crate_dir = project.path().join("crates").join("mylib");
        fs::create_dir_all(crate_dir.join("src")).expect("mkdir");
        // lib.rs with a pub mod and a crate fact.
        fs::write(
            crate_dir.join("src").join("lib.rs"),
            b"//! mylib\npub mod knowledge;\n",
        )
        .expect("write lib.rs");
        // Satisfy the crate-level prefix so only the module check triggers.
        fs::write(
            facts.path().join("aletheia.mylib.toplevel.json"),
            b"{\"id\":\"aletheia.mylib.toplevel\"}",
        )
        .expect("write crate fact");

        let rule = FactRequiredRule::with_config(FactRequiredConfig {
            facts_dir: facts.path().to_path_buf(),
            ..FactRequiredConfig::default()
        });
        let violations = rule
            .check(project.path().to_str().expect("valid path"))
            .expect("check");
        // Should warn for the `knowledge` module.
        assert!(
            violations.iter().any(|v| v.message.contains("knowledge")),
            "expected violation for pub mod knowledge; got: {violations:?}"
        );
    }

    #[test]
    fn rule_can_be_disabled() {
        let project = tempfile::tempdir().expect("tempdir");
        let facts = tempfile::tempdir().expect("facts tempdir");
        let crate_dir = project.path().join("crates").join("eidos");
        fs::create_dir_all(crate_dir.join("src")).expect("mkdir");
        fs::write(crate_dir.join("src").join("lib.rs"), b"//! eidos lib\n").expect("write lib.rs");

        let rule = FactRequiredRule::with_config(FactRequiredConfig {
            enabled: false,
            facts_dir: facts.path().to_path_buf(),
            ..FactRequiredConfig::default()
        });
        let violations = rule
            .check(project.path().to_str().expect("valid path"))
            .expect("check");
        assert!(violations.is_empty(), "disabled policy should not emit");
    }

    #[test]
    fn rule_uses_configured_project_prefix() {
        let project = tempfile::tempdir().expect("tempdir");
        let facts = tempfile::tempdir().expect("facts tempdir");
        let crate_dir = project.path().join("crates").join("basanos");
        fs::create_dir_all(crate_dir.join("src")).expect("mkdir");
        fs::write(crate_dir.join("src").join("lib.rs"), b"//! basanos lib\n")
            .expect("write lib.rs");
        fs::write(
            facts.path().join("kanon.basanos.architecture.json"),
            b"{\"id\":\"kanon.basanos.architecture\"}",
        )
        .expect("write fact");

        let rule = FactRequiredRule::with_config(FactRequiredConfig {
            facts_dir: facts.path().to_path_buf(),
            project_prefix: "kanon".to_owned(),
            ..FactRequiredConfig::default()
        });
        let violations = rule
            .check(project.path().to_str().expect("valid path"))
            .expect("check");
        assert!(
            violations.is_empty(),
            "configured prefix should satisfy facts; got: {violations:?}"
        );
    }
}

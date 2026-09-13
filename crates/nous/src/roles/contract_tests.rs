//! Test suite for `crates/nous/src/roles/contract.rs`.
//!
//! Split into a sibling file (RUST/file-too-long) — the same pattern
//! `pipeline/stages.rs` -> `pipeline/stages_tests.rs` already uses.

#![expect(clippy::unwrap_used, reason = "test assertions may panic on failure")]
#![expect(
    clippy::disallowed_methods,
    reason = "test fixtures use std::fs to write tempdir files synchronously"
)]

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
    assert_eq!(
        planner.tool_groups,
        ToolGroupPolicy::DenyAll,
        "missing tool_groups should deny all tools"
    );

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
        tool_groups: ToolGroupPolicy::groups(vec![ToolGroupId::Read, ToolGroupId::Edit]),
        episteme_cohort: None,
        private: false,
        domains: Vec::new(),
        model: None,
    };

    let section = contract.to_prompt_section();
    assert!(
        section.contains("Role Contract: coder (v2)"),
        "should contain role and version"
    );
    assert!(section.contains("- Write code"), "should list behaviors");
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
        tool_groups: ToolGroupPolicy::DenyAll,
        episteme_cohort: None,
        private: false,
        domains: Vec::new(),
        model: None,
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
    let registry = ContractRegistry::load_from_file(Path::new("/nonexistent/roles.toml")).unwrap();
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
    assert_eq!(coder.tool_groups, ToolGroupPolicy::DenyAll);
}

// WHY(#7169): an unreadable `roles.toml` (permission denied, not a
// regular file, ...) must fail closed, not silently return
// `ContractRegistry::defaults()` -- those defaults are the liberal end
// of the contract range, so treating a read error as "no override
// configured" is a privilege-restoration bug. Only `NotFound` means
// "no override configured".
#[test]
#[cfg(unix)]
fn load_from_file_fails_closed_on_unreadable_file() {
    use std::os::unix::fs::PermissionsExt;

    let dir = tempfile::TempDir::new().unwrap();
    let path = dir.path().join("roles.toml");
    std::fs::write(&path, "[coder]\nversion = 2\n").unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o000)).unwrap();

    // NOTE: skip if running as root: root bypasses file permission checks
    let is_root = std::fs::read_to_string(&path).is_ok();
    if is_root {
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        return;
    }

    let result = ContractRegistry::load_from_file(&path);

    // restore permissions so the tempdir cleans up regardless of assertion outcome
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();

    assert!(
        result.is_err(),
        "an unreadable roles.toml must fail closed rather than fall back to defaults"
    );
}

#[test]
fn contract_serde_roundtrip() {
    let contract = RoleContract {
        role: "coder".to_owned(),
        version: 2,
        behaviors: vec!["Write code".to_owned()],
        constraints: vec!["Break things".to_owned()],
        tool_groups: ToolGroupPolicy::groups(vec![ToolGroupId::Read, ToolGroupId::Edit]),
        episteme_cohort: Some("isolated".to_owned()),
        private: true,
        domains: vec!["medical".to_owned()],
        model: Some("test-role-model-override".to_owned()),
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

#[test]
fn default_contracts_have_tool_groups() {
    let registry = ContractRegistry::defaults();
    for (name, contract) in registry.all() {
        assert!(
            !contract.tool_groups.allowed_groups().is_empty(),
            "contract for {name} should have non-empty tool_groups"
        );
    }
}

#[test]
fn coder_has_edit_and_command_groups() {
    let registry = ContractRegistry::defaults();
    let coder = registry.get("coder").unwrap();
    let groups = coder.tool_groups.allowed_groups();
    assert!(groups.contains(&ToolGroupId::Read));
    assert!(groups.contains(&ToolGroupId::Edit));
    assert!(groups.contains(&ToolGroupId::Command));
    assert!(groups.contains(&ToolGroupId::Verify));
}

#[test]
fn explorer_is_read_only_plus_plan() {
    let registry = ContractRegistry::defaults();
    let explorer = registry.get("explorer").unwrap();
    let groups = explorer.tool_groups.allowed_groups();
    assert!(groups.contains(&ToolGroupId::Read));
    assert!(groups.contains(&ToolGroupId::Plan));
    assert!(!groups.contains(&ToolGroupId::Edit));
    assert!(!groups.contains(&ToolGroupId::Command));
}

#[test]
fn to_prompt_section_includes_tool_groups() {
    let contract = RoleContract {
        role: "coder".to_owned(),
        version: 1,
        behaviors: vec!["Write code".to_owned()],
        constraints: vec!["Break things".to_owned()],
        tool_groups: ToolGroupPolicy::groups(vec![ToolGroupId::Read, ToolGroupId::Edit]),
        episteme_cohort: None,
        private: false,
        domains: Vec::new(),
        model: None,
    };
    let section = contract.to_prompt_section();
    assert!(
        section.contains("Allowed Tool Groups"),
        "prompt section should list allowed tool groups"
    );
    assert!(
        section.contains("- read"),
        "prompt section should contain 'read'"
    );
    assert!(
        section.contains("- edit"),
        "prompt section should contain 'edit'"
    );
}

#[test]
fn to_prompt_section_marks_deny_all() {
    let contract = RoleContract {
        role: "empty".to_owned(),
        version: 1,
        behaviors: vec!["Behave".to_owned()],
        constraints: vec!["Misbehave".to_owned()],
        tool_groups: ToolGroupPolicy::DenyAll,
        episteme_cohort: None,
        private: false,
        domains: Vec::new(),
        model: None,
    };
    let section = contract.to_prompt_section();
    assert!(
        section.contains("Tool Group Policy"),
        "prompt section should state deny-all policy"
    );
    assert!(
        section.contains("- deny"),
        "prompt section should state deny-all policy"
    );
}

#[test]
fn from_toml_parses_tool_groups() {
    let toml = r#"
[coder]
version = 2
behaviors = ["Write code"]
constraints = ["Break the build"]
tool_groups = ["read", "edit"]
"#;
    let registry = ContractRegistry::from_toml(toml).unwrap();
    let coder = registry.get("coder").unwrap();
    let groups = coder.tool_groups.allowed_groups();
    assert_eq!(groups.len(), 2);
    assert!(groups.contains(&ToolGroupId::Read));
    assert!(groups.contains(&ToolGroupId::Edit));
}

#[test]
fn from_toml_parses_allow_all_policy() {
    let toml = r#"
[admin]
version = 1
tool_groups = "all"
"#;
    let registry = ContractRegistry::from_toml(toml).unwrap();
    assert!(matches!(
        registry.get("admin").unwrap().tool_groups,
        ToolGroupPolicy::AllowAll { .. }
    ));
}

#[test]
fn from_toml_empty_groups_denies_all() {
    let toml = r"
[empty]
version = 1
tool_groups = []
";
    let registry = ContractRegistry::from_toml(toml).unwrap();
    assert_eq!(
        registry.get("empty").unwrap().tool_groups,
        ToolGroupPolicy::DenyAll
    );
}

// WHY(#5087): role contracts are the operator-approved spawn policy
// alternative to threading a parent's live config through the spawn
// path — cohort/privacy/domains must be overridable per role from TOML,
// not just tool_groups.
#[test]
fn from_toml_parses_spawn_policy_fields() {
    let toml = r#"
[reviewer]
version = 2
episteme_cohort = "isolated"
private = true
domains = ["medical", "legal"]
"#;
    let registry = ContractRegistry::from_toml(toml).unwrap();
    let reviewer = registry.get("reviewer").unwrap();
    assert_eq!(reviewer.episteme_cohort.as_deref(), Some("isolated"));
    assert!(reviewer.private);
    assert_eq!(reviewer.domains, vec!["medical", "legal"]);
}

// WHY(wave 3.3): `model` follows the exact `episteme_cohort` Option
// pattern — an operator can pin a role to a specific model from
// `roles.toml` without touching the compiled `RoleTemplate` default.
#[test]
fn from_toml_parses_model_override() {
    let toml = r#"
[reviewer]
version = 2
model = "test-role-model-override"
"#;
    let registry = ContractRegistry::from_toml(toml).unwrap();
    let reviewer = registry.get("reviewer").unwrap();
    assert_eq!(reviewer.model.as_deref(), Some("test-role-model-override"));
}

#[test]
fn from_toml_omits_spawn_policy_fields_defaults_to_none() {
    let toml = r#"
[coder]
version = 2
behaviors = ["Write code"]
"#;
    let registry = ContractRegistry::from_toml(toml).unwrap();
    let coder = registry.get("coder").unwrap();
    assert_eq!(
        coder.episteme_cohort, None,
        "omitted episteme_cohort must not assert a cohort"
    );
    assert!(!coder.private, "omitted private must default to false");
    assert!(
        coder.domains.is_empty(),
        "omitted domains must default to empty"
    );
    assert_eq!(
        coder.model, None,
        "omitted model must not assert an override"
    );
}

// WHY(wave 3.3): config parsing must fail closed on a malformed `model`
// value rather than silently coercing it — a role's model override is a
// dispatch-affecting field, so a typo'd TOML shape (wrong type, here) is
// a parse error like any other malformed `roles.toml`, which
// `ContractRegistry::load_from_file` already degrades to hardcoded
// defaults for (`spawn_config_malformed_roles_toml_falls_back_to_defaults`
// in spawn_svc.rs), not a value that reaches a spawned agent unvetted.
#[test]
fn from_toml_rejects_non_string_model() {
    let toml = r"
[coder]
version = 2
model = 6
";
    let result = ContractRegistry::from_toml(toml);
    assert!(
        result.is_err(),
        "a non-string model value must fail parsing, not coerce silently"
    );
}

#[test]
fn default_contracts_have_no_spawn_policy_overrides() {
    // WHY: default contracts must not silently assert a cohort/privacy/
    // domains/model policy that spawn_svc.rs did not previously apply —
    // wiring the contract registry into production must not change
    // default spawned-agent behavior when roles.toml is absent.
    let registry = ContractRegistry::defaults();
    for (name, contract) in registry.all() {
        assert_eq!(
            contract.episteme_cohort, None,
            "default contract for {name} must not assert a cohort"
        );
        assert_eq!(
            contract.model, None,
            "default contract for {name} must not assert a model override"
        );
        assert!(
            !contract.private,
            "default contract for {name} must not be private"
        );
        assert!(
            contract.domains.is_empty(),
            "default contract for {name} must have no domains"
        );
    }
}

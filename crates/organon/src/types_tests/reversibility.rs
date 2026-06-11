//! Reversibility classification tests for tool types.

#![expect(clippy::expect_used, reason = "test assertions")]

use super::super::*;

// ── Reversibility tests ──────────────────────────────────────────────

#[test]
fn reversibility_display_all_variants() {
    assert_eq!(
        Reversibility::FullyReversible.to_string(),
        "fully_reversible",
        "FullyReversible display"
    );
    assert_eq!(
        Reversibility::Reversible.to_string(),
        "reversible",
        "Reversible display"
    );
    assert_eq!(
        Reversibility::PartiallyReversible.to_string(),
        "partially_reversible",
        "PartiallyReversible display"
    );
    assert_eq!(
        Reversibility::Irreversible.to_string(),
        "irreversible",
        "Irreversible display"
    );
}

#[test]
fn reversibility_default_is_irreversible() {
    assert_eq!(
        Reversibility::default(),
        Reversibility::Irreversible,
        "default reversibility should be Irreversible"
    );
}

#[test]
fn reversibility_supports_dry_run() {
    assert!(
        Reversibility::FullyReversible.supports_dry_run(),
        "FullyReversible should support dry run"
    );
    assert!(
        Reversibility::Reversible.supports_dry_run(),
        "Reversible should support dry run"
    );
    assert!(
        !Reversibility::PartiallyReversible.supports_dry_run(),
        "PartiallyReversible should not support dry run"
    );
    assert!(
        !Reversibility::Irreversible.supports_dry_run(),
        "Irreversible should not support dry run"
    );
}

#[test]
fn reversibility_serde_roundtrip() {
    for rev in [
        Reversibility::FullyReversible,
        Reversibility::Reversible,
        Reversibility::PartiallyReversible,
        Reversibility::Irreversible,
    ] {
        let json = serde_json::to_string(&rev).expect("serialize");
        let back: Reversibility = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(rev, back, "roundtrip for {rev}");
    }
}

#[test]
fn approval_requirement_from_reversibility() {
    assert_eq!(
        ApprovalRequirement::from(Reversibility::FullyReversible),
        ApprovalRequirement::None,
        "FullyReversible -> None"
    );
    assert_eq!(
        ApprovalRequirement::from(Reversibility::Reversible),
        ApprovalRequirement::Advisory,
        "Reversible -> Advisory"
    );
    assert_eq!(
        ApprovalRequirement::from(Reversibility::PartiallyReversible),
        ApprovalRequirement::Required,
        "PartiallyReversible -> Required"
    );
    assert_eq!(
        ApprovalRequirement::from(Reversibility::Irreversible),
        ApprovalRequirement::Mandatory,
        "Irreversible -> Mandatory"
    );
}

#[test]
fn approval_requirement_display() {
    assert_eq!(
        ApprovalRequirement::None.to_string(),
        "none",
        "None display"
    );
    assert_eq!(
        ApprovalRequirement::Advisory.to_string(),
        "advisory",
        "Advisory display"
    );
    assert_eq!(
        ApprovalRequirement::Required.to_string(),
        "required",
        "Required display"
    );
    assert_eq!(
        ApprovalRequirement::Mandatory.to_string(),
        "mandatory",
        "Mandatory display"
    );
}

#[test]
fn tool_call_metadata_serde_roundtrip() {
    let meta = ToolCallMetadata {
        reversibility: Reversibility::PartiallyReversible,
        approval: ApprovalRequirement::Required,
        dry_run: true,
    };
    let json = serde_json::to_string(&meta).expect("serialize");
    let back: ToolCallMetadata = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(
        back.reversibility,
        Reversibility::PartiallyReversible,
        "reversibility roundtrip"
    );
    assert_eq!(
        back.approval,
        ApprovalRequirement::Required,
        "approval roundtrip"
    );
    assert!(back.dry_run, "dry_run roundtrip");
}

#[test]
fn tool_def_includes_reversibility_in_serde() {
    let def = ToolDef {
        name: ToolName::new("test").expect("valid"),
        description: "test".to_owned(),
        extended_description: None,
        input_schema: InputSchema {
            properties: IndexMap::new(),
            required: vec![],
        },
        category: ToolCategory::Workspace,
        reversibility: Reversibility::Reversible,
        auto_activate: false,
        groups: vec![ToolGroupId::Read],
        tags: vec![],
    };
    let json = serde_json::to_string(&def).expect("serialize");
    let back: ToolDef = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(
        back.reversibility,
        Reversibility::Reversible,
        "reversibility should survive serde roundtrip"
    );
}

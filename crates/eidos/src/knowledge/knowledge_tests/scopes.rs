//! (See parent module for full rationale.)

use super::super::*;
use super::test_timestamp;
use crate::id::FactId;

// Split: MemoryScope / ScopeAccessPolicy / Fact with scope tests.

// ---------------------------------------------------------------------------
// MemoryScope tests
// ---------------------------------------------------------------------------

#[test]
fn memory_scope_serde_roundtrip() {
    for scope in MemoryScope::ALL {
        let json = serde_json::to_string(&scope).expect("MemoryScope serialization is infallible");
        let back: MemoryScope =
            serde_json::from_str(&json).expect("MemoryScope should deserialize from its own JSON");
        assert_eq!(scope, back, "MemoryScope should survive serde roundtrip");
    }
}

#[test]
fn memory_scope_as_str_matches_serde() {
    for scope in MemoryScope::ALL {
        let json = serde_json::to_string(&scope).expect("MemoryScope serialization is infallible");
        let expected = format!("\"{}\"", scope.as_str());
        assert_eq!(
            json, expected,
            "MemoryScope json should match as_str representation"
        );
    }
}

#[test]
fn memory_scope_from_str_roundtrip() {
    for scope in MemoryScope::ALL {
        let parsed: MemoryScope = scope
            .as_str()
            .parse()
            .expect("MemoryScope as_str() should round-trip through FromStr");
        assert_eq!(
            scope, parsed,
            "MemoryScope should survive as_str/parse roundtrip"
        );
    }
}

#[test]
fn memory_scope_from_str_unknown() {
    assert!(
        "bogus".parse::<MemoryScope>().is_err(),
        "unrecognized string should fail to parse as MemoryScope"
    );
}

#[test]
fn memory_scope_display() {
    assert_eq!(
        MemoryScope::User.to_string(),
        "user",
        "User should display as 'user'"
    );
    assert_eq!(
        MemoryScope::Feedback.to_string(),
        "feedback",
        "Feedback should display as 'feedback'"
    );
    assert_eq!(
        MemoryScope::Project.to_string(),
        "project",
        "Project should display as 'project'"
    );
    assert_eq!(
        MemoryScope::Reference.to_string(),
        "reference",
        "Reference should display as 'reference'"
    );
}

#[test]
fn memory_scope_dir_names_match_as_str() {
    for scope in MemoryScope::ALL {
        assert_eq!(
            scope.as_dir_name(),
            scope.as_str(),
            "dir name should match as_str for {scope:?}"
        );
    }
}

#[test]
fn memory_scope_all_has_four_variants() {
    assert_eq!(
        MemoryScope::ALL.len(),
        4,
        "ALL should contain exactly 4 scope variants"
    );
}

#[test]
fn memory_scope_from_str_opt_returns_some_for_valid() {
    assert_eq!(
        MemoryScope::from_str_opt("user"),
        Some(MemoryScope::User),
        "from_str_opt should return Some(User) for 'user'"
    );
    assert_eq!(
        MemoryScope::from_str_opt("feedback"),
        Some(MemoryScope::Feedback),
        "from_str_opt should return Some(Feedback) for 'feedback'"
    );
}

#[test]
fn memory_scope_from_str_opt_returns_none_for_invalid() {
    assert_eq!(
        MemoryScope::from_str_opt("invalid"),
        None,
        "from_str_opt should return None for unrecognized scope"
    );
    assert_eq!(
        MemoryScope::from_str_opt(""),
        None,
        "from_str_opt should return None for empty string"
    );
}

// ---------------------------------------------------------------------------
// ScopeAccessPolicy tests
// ---------------------------------------------------------------------------

#[test]
fn user_scope_is_private() {
    let policy = MemoryScope::User.access_policy();
    assert!(
        !policy.permits_agent_read(),
        "agents must not read user scope"
    );
    assert!(
        !policy.permits_agent_write(),
        "agents must not write user scope"
    );
    assert!(
        policy.user_write_only,
        "user scope should be user-write-only"
    );
}

#[test]
fn feedback_scope_is_read_only_for_agents() {
    let policy = MemoryScope::Feedback.access_policy();
    assert!(
        policy.permits_agent_read(),
        "agents must read feedback scope"
    );
    assert!(
        !policy.permits_agent_write(),
        "agents must not write feedback scope"
    );
    assert!(
        policy.user_write_only,
        "feedback scope should be user-write-only"
    );
}

#[test]
fn project_scope_is_shared_read_write() {
    let policy = MemoryScope::Project.access_policy();
    assert!(
        policy.permits_agent_read(),
        "agents must read project scope"
    );
    assert!(
        policy.permits_agent_write(),
        "agents must write project scope"
    );
    assert!(
        !policy.user_write_only,
        "project scope should not be user-write-only"
    );
}

#[test]
fn reference_scope_is_hybrid() {
    let policy = MemoryScope::Reference.access_policy();
    assert!(
        policy.permits_agent_read(),
        "agents must read reference scope"
    );
    assert!(
        !policy.permits_agent_write(),
        "agents must not write reference scope"
    );
    assert!(
        policy.user_write_only,
        "reference scope should be user-write-only"
    );
}

// ---------------------------------------------------------------------------
// Fact with scope tests
// ---------------------------------------------------------------------------

#[test]
fn fact_scope_none_omitted_in_json() {
    let fact = Fact {
        id: FactId::new("f-no-scope").expect("valid test id"),
        nous_id: "syn".to_owned(),
        content: "legacy fact without scope".to_owned(),
        fact_type: String::new(),
        scope: None,
        temporal: FactTemporal {
            valid_from: test_timestamp("2026-01-01"),
            valid_to: far_future(),
            recorded_at: test_timestamp("2026-01-01T00:00:00Z"),
        },
        provenance: FactProvenance {
            confidence: 0.5,
            tier: EpistemicTier::Assumed,
            source_session_id: None,
            stability_hours: 72.0,
        },
        lifecycle: FactLifecycle {
            superseded_by: None,
            is_forgotten: false,
            forgotten_at: None,
            forget_reason: None,
        },
        access: FactAccess {
            access_count: 0,
            last_accessed_at: None,
        },
        sensitivity: FactSensitivity::Public,
        visibility: Visibility::Private,
    };
    let json = serde_json::to_string(&fact).expect("Fact serialization is infallible");
    assert!(
        !json.contains("\"scope\""),
        "scope: None should be omitted from serialized JSON"
    );
    let back: Fact =
        serde_json::from_str(&json).expect("Fact should deserialize from its own JSON");
    assert_eq!(
        back.scope, None,
        "deserialized scope should be None when omitted"
    );
}

#[test]
fn fact_scope_some_included_in_json() {
    let fact = Fact {
        id: FactId::new("f-scoped").expect("valid test id"),
        nous_id: "syn".to_owned(),
        content: "team project fact".to_owned(),
        fact_type: "project".to_owned(),
        scope: Some(MemoryScope::Project),
        temporal: FactTemporal {
            valid_from: test_timestamp("2026-03-01"),
            valid_to: far_future(),
            recorded_at: test_timestamp("2026-03-01T00:00:00Z"),
        },
        provenance: FactProvenance {
            confidence: 0.9,
            tier: EpistemicTier::Verified,
            source_session_id: None,
            stability_hours: 720.0,
        },
        lifecycle: FactLifecycle {
            superseded_by: None,
            is_forgotten: false,
            forgotten_at: None,
            forget_reason: None,
        },
        access: FactAccess {
            access_count: 0,
            last_accessed_at: None,
        },
        sensitivity: FactSensitivity::Public,
        visibility: Visibility::Private,
    };
    let json = serde_json::to_string(&fact).expect("Fact serialization is infallible");
    assert!(
        json.contains("\"scope\":\"project\""),
        "scope: Some(Project) should appear in serialized JSON"
    );
    let back: Fact =
        serde_json::from_str(&json).expect("Fact should deserialize from its own JSON");
    assert_eq!(
        back.scope,
        Some(MemoryScope::Project),
        "deserialized scope should be Some(Project)"
    );
}

#[test]
fn fact_backward_compat_no_scope_field() {
    // WHY: Existing JSON without a `scope` field must deserialize to `scope: None`.
    let json = r#"{
        "id": "f-legacy",
        "nous_id": "syn",
        "fact_type": "observation",
        "content": "old fact from before team memory",
        "valid_from": "2026-01-01T00:00:00Z",
        "valid_to": "9999-01-01T00:00:00Z",
        "recorded_at": "2026-01-01T00:00:00Z",
        "confidence": 0.5,
        "tier": "assumed",
        "source_session_id": null,
        "stability_hours": 72.0,
        "superseded_by": null,
        "is_forgotten": false,
        "forgotten_at": null,
        "forget_reason": null,
        "access_count": 0,
        "last_accessed_at": null
    }"#;
    let fact: Fact =
        serde_json::from_str(json).expect("legacy JSON without scope field should deserialize");
    assert_eq!(
        fact.scope, None,
        "legacy fact without scope field should deserialize to scope: None"
    );
}

#[test]
fn fact_scope_all_variants_roundtrip() {
    for scope in MemoryScope::ALL {
        let fact = Fact {
            id: FactId::new("f-scope-test").expect("valid test id"),
            nous_id: "syn".to_owned(),
            content: format!("fact in {scope} scope"),
            fact_type: String::new(),
            scope: Some(scope),
            temporal: FactTemporal {
                valid_from: test_timestamp("2026-01-01"),
                valid_to: far_future(),
                recorded_at: test_timestamp("2026-01-01T00:00:00Z"),
            },
            provenance: FactProvenance {
                confidence: 0.5,
                tier: EpistemicTier::Assumed,
                source_session_id: None,
                stability_hours: 72.0,
            },
            lifecycle: FactLifecycle {
                superseded_by: None,
                is_forgotten: false,
                forgotten_at: None,
                forget_reason: None,
            },
            access: FactAccess {
                access_count: 0,
                last_accessed_at: None,
            },
            sensitivity: FactSensitivity::Public,
            visibility: Visibility::Private,
        };
        let json = serde_json::to_string(&fact).expect("Fact serialization is infallible");
        let back: Fact =
            serde_json::from_str(&json).expect("Fact should deserialize from its own JSON");
        assert_eq!(
            back.scope,
            Some(scope),
            "scope {scope:?} should survive serde roundtrip"
        );
    }
}

//! (Split from `config_tests.rs` — see parent mod.)

#![expect(clippy::expect_used, reason = "test assertions")]
#![expect(
    clippy::indexing_slicing,
    reason = "test: map/vec indexing with keys/indices asserted present by surrounding context"
)]

use super::super::*;

#[test]
fn agency_default_is_standard() {
    let config = AletheiaConfig::default();
    assert_eq!(
        config.agents.defaults.agency,
        AgencyLevel::Standard,
        "default agency level should be standard"
    );
}

#[test]
fn agency_serde_roundtrip() {
    let json = serde_json::to_string(&AgencyLevel::Unrestricted).expect("serialize");
    assert_eq!(
        json, "\"unrestricted\"",
        "unrestricted agency should serialize to string 'unrestricted'"
    );
    let back: AgencyLevel = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(
        back,
        AgencyLevel::Unrestricted,
        "unrestricted agency should roundtrip through serde"
    );

    let json = serde_json::to_string(&AgencyLevel::Restricted).expect("serialize");
    assert_eq!(
        json, "\"restricted\"",
        "restricted agency should serialize to string 'restricted'"
    );
}

#[test]
fn resolve_agency_inherits_global_default() {
    let config = AletheiaConfig::default();
    let resolved = resolve_nous(&config, "any");
    assert_eq!(
        resolved.capabilities.agency,
        AgencyLevel::Standard,
        "global default agency should be standard"
    );
    assert_eq!(
        resolved.capabilities.max_tool_iterations, 200,
        "standard agency should use default max tool iterations"
    );
}

#[test]
fn resolve_agency_unrestricted_sets_high_iterations() {
    let mut config = AletheiaConfig::default();
    config.agents.list.push(NousDefinition {
        id: "free".to_owned(),
        name: None,
        model: None,
        workspace: "/home/user/nous/free".to_owned(),
        thinking_enabled: None,
        agency: Some(AgencyLevel::Unrestricted),
        allowed_roots: Vec::new(),
        domains: Vec::new(),
        default: false,
        recall: None,
        behavior: None,
    });

    let resolved = resolve_nous(&config, "free");
    assert_eq!(
        resolved.capabilities.agency,
        AgencyLevel::Unrestricted,
        "agent agency override should be unrestricted"
    );
    assert_eq!(
        resolved.capabilities.max_tool_iterations, 10_000,
        "unrestricted agency should set max tool iterations to 10k"
    );
}

#[test]
fn resolve_agency_restricted_uses_old_defaults() {
    let mut config = AletheiaConfig::default();
    config.agents.list.push(NousDefinition {
        id: "safe".to_owned(),
        name: None,
        model: None,
        workspace: "/home/user/nous/safe".to_owned(),
        thinking_enabled: None,
        agency: Some(AgencyLevel::Restricted),
        allowed_roots: Vec::new(),
        domains: Vec::new(),
        default: false,
        recall: None,
        behavior: None,
    });

    let resolved = resolve_nous(&config, "safe");
    assert_eq!(
        resolved.capabilities.agency,
        AgencyLevel::Restricted,
        "agent agency override should be restricted"
    );
    assert_eq!(
        resolved.capabilities.max_tool_iterations, 50,
        "restricted agency should use low max tool iterations"
    );
}

#[test]
fn resolve_agency_per_agent_overrides_global() {
    let mut config = AletheiaConfig::default();
    config.agents.defaults.agency = AgencyLevel::Restricted;
    config.agents.list.push(NousDefinition {
        id: "override".to_owned(),
        name: None,
        model: None,
        workspace: "/home/user/nous/override".to_owned(),
        thinking_enabled: None,
        agency: Some(AgencyLevel::Unrestricted),
        allowed_roots: Vec::new(),
        domains: Vec::new(),
        default: false,
        recall: None,
        behavior: None,
    });

    let resolved = resolve_nous(&config, "override");
    assert_eq!(
        resolved.capabilities.agency,
        AgencyLevel::Unrestricted,
        "per-agent unrestricted should override global restricted"
    );
    assert_eq!(
        resolved.capabilities.max_tool_iterations, 10_000,
        "per-agent unrestricted override should set iterations to 10k"
    );

    let other = resolve_nous(&config, "other");
    assert_eq!(
        other.capabilities.agency,
        AgencyLevel::Restricted,
        "agent without override should use global restricted agency"
    );
    assert_eq!(
        other.capabilities.max_tool_iterations, 50,
        "agent using global restricted should get low max tool iterations"
    );
}

#[test]
fn agency_from_json() {
    let json = r#"{
        "agents": {
            "defaults": {
                "agency": "unrestricted"
            },
            "list": [{
                "id": "restricted-agent",
                "workspace": "/tmp/ws",
                "agency": "restricted"
            }]
        }
    }"#;
    let config: AletheiaConfig = serde_json::from_str(json).expect("parse agency");
    assert_eq!(
        config.agents.defaults.agency,
        AgencyLevel::Unrestricted,
        "global agency override from json should be unrestricted"
    );
    assert_eq!(
        config.agents.list[0].agency,
        Some(AgencyLevel::Restricted),
        "per-agent restricted override from json should be applied"
    );
}

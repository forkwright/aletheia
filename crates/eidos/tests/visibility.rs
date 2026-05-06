//! Integration tests for the `Visibility` enum.

#![expect(clippy::expect_used, reason = "test assertions")]

use std::str::FromStr;

use eidos::knowledge::Visibility;

#[test]
fn default_is_private() {
    assert_eq!(Visibility::default(), Visibility::Private);
}

#[test]
fn as_str_matches_serde_snake_case() {
    assert_eq!(Visibility::Private.as_str(), "private");
    assert_eq!(Visibility::Shared.as_str(), "shared");
    assert_eq!(Visibility::Restricted.as_str(), "restricted");
    assert_eq!(Visibility::Published.as_str(), "published");
}

#[test]
fn display_matches_as_str() {
    assert_eq!(Visibility::Private.to_string(), "private");
    assert_eq!(Visibility::Shared.to_string(), "shared");
    assert_eq!(Visibility::Restricted.to_string(), "restricted");
    assert_eq!(Visibility::Published.to_string(), "published");
}

#[test]
fn serde_roundtrip_snake_case() {
    for vis in [
        Visibility::Private,
        Visibility::Shared,
        Visibility::Restricted,
        Visibility::Published,
    ] {
        let json = serde_json::to_string(&vis).expect("serialize");
        let back: Visibility = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back, vis);
    }
}

#[test]
fn from_str_round_trips_all_variants() {
    for vis in [
        Visibility::Private,
        Visibility::Shared,
        Visibility::Restricted,
        Visibility::Published,
    ] {
        let s = vis.as_str();
        let parsed = Visibility::from_str(s).expect("round trip");
        assert_eq!(parsed, vis);
    }
}

#[test]
fn from_str_rejects_unknown() {
    assert!(Visibility::from_str("global").is_err());
    assert!(Visibility::from_str("").is_err());
    assert!(Visibility::from_str("public").is_err());
}

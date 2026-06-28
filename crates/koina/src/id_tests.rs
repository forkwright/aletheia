#![expect(clippy::unwrap_used, reason = "test assertions")]

use super::*;

#[test]
fn valid_nous_id() {
    assert!(NousId::new("syn").is_ok());
    assert!(NousId::new("demiurge").is_ok());
    assert!(NousId::new("worker-1").is_ok());
}

#[test]
fn invalid_nous_id_empty() {
    assert!(matches!(NousId::new(""), Err(IdError::Empty { .. })));
}

#[test]
fn invalid_nous_id_too_long() {
    let long = "a".repeat(65);
    assert!(matches!(NousId::new(long), Err(IdError::TooLong { .. })));
}

#[test]
fn nous_id_display() {
    let id = NousId::new("syn").unwrap();
    assert_eq!(id.to_string(), "syn");
    assert_eq!(id.as_str(), "syn");
}

#[test]
fn nous_id_serde_roundtrip() {
    let id = NousId::new("syn").unwrap();
    let json = serde_json::to_string(&id).unwrap();
    assert_eq!(json, r#""syn""#);
    let back: NousId = serde_json::from_str(&json).unwrap();
    assert_eq!(id, back);
}

#[test]
fn session_id_unique() {
    let a = SessionId::new();
    let b = SessionId::new();
    assert_ne!(a, b);
}

#[test]
fn session_id_parse_roundtrip() {
    let id = SessionId::new();
    let s = id.to_string();
    let back = SessionId::parse(&s).unwrap();
    assert_eq!(id, back);
}

#[test]
fn turn_id_ordering() {
    let a = TurnId::new(1);
    let b = TurnId::new(2);
    assert!(a < b);
    assert_eq!(a.next(), b);
}

#[test]
fn valid_tool_name() {
    assert!(ToolName::new("exec").is_ok());
    assert!(ToolName::new("web_search").is_ok());
    assert!(ToolName::new("sessions-ask").is_ok());
}

#[test]
fn invalid_tool_name_spaces() {
    assert!(matches!(
        ToolName::new("my tool"),
        Err(IdError::InvalidFormat { .. })
    ));
}

#[test]
fn tool_name_serde_roundtrip() {
    let name = ToolName::new("exec").unwrap();
    let json = serde_json::to_string(&name).unwrap();
    let back: ToolName = serde_json::from_str(&json).unwrap();
    assert_eq!(name, back);
}

#[test]
fn nous_id_max_length_accepted() {
    let max = "a".repeat(64);
    assert!(NousId::new(max).is_ok());
}

#[test]
fn nous_id_normalizes_case_and_underscores() {
    let id = NousId::new("Research_Agent").unwrap();
    assert_eq!(id.as_str(), "research-agent");
}

#[test]
fn nous_id_trims_outer_whitespace() {
    let id = NousId::new("  Syn  ").unwrap();
    assert_eq!(id.as_str(), "syn");
}

#[test]
fn nous_id_rejects_leading_hyphen_after_normalization() {
    assert!(matches!(
        NousId::new("-syn"),
        Err(IdError::InvalidFormat { .. })
    ));
}

#[test]
fn nous_id_rejects_trailing_hyphen_after_normalization() {
    assert!(matches!(
        NousId::new("syn_"),
        Err(IdError::InvalidFormat { .. })
    ));
}

#[test]
fn nous_id_digits_only() {
    assert!(NousId::new("123").is_ok());
}

#[test]
fn nous_id_special_chars_rejected() {
    assert!(matches!(
        NousId::new("syn.1"),
        Err(IdError::InvalidFormat { .. })
    ));
    assert!(matches!(
        NousId::new("syn 1"),
        Err(IdError::InvalidFormat { .. })
    ));
}

#[test]
fn nous_id_rejects_path_separators() {
    for raw in ["syn/one", "syn\\one", "../syn", "syn%2fone"] {
        assert!(
            matches!(NousId::new(raw), Err(IdError::InvalidFormat { .. })),
            "{raw:?} must be rejected"
        );
    }
}

#[test]
fn nous_id_rejects_reserved_internal_prefix() {
    let err = NousId::new("Cross:alice").unwrap_err();
    assert!(
        err.to_string().contains("reserved internal prefix"),
        "got: {err}"
    );
}

#[test]
fn normalize_nous_id_returns_route_safe_id() {
    let id = normalize_nous_id("Desk_Agent42").unwrap();
    assert_eq!(id.as_str(), "desk-agent42");
    assert!(
        id.as_str()
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
    );
}

#[test]
fn tool_name_max_length_accepted() {
    let max = "a".repeat(128);
    assert!(ToolName::new(max).is_ok());
}

#[test]
fn tool_name_empty_rejected() {
    assert!(matches!(ToolName::new(""), Err(IdError::Empty { .. })));
}

#[test]
fn tool_name_too_long_rejected() {
    let long = "a".repeat(129);
    assert!(matches!(ToolName::new(long), Err(IdError::TooLong { .. })));
}

#[test]
fn tool_name_only_hyphens_underscores() {
    assert!(ToolName::new("--__--").is_ok());
}

#[test]
fn session_id_parse_invalid() {
    assert!(SessionId::parse("").is_err());
    assert!(SessionId::parse("not-a-uuid").is_err());
    assert!(SessionId::parse("too-short").is_err());
}

#[test]
fn session_id_deserialize_valid_uuid() {
    let valid_uuid = "550e8400-e29b-41d4-a716-446655440000";
    let json = format!("\"{valid_uuid}\"");
    let result: Result<SessionId, _> = serde_json::from_str(&json);
    assert!(result.is_ok());
    assert_eq!(result.unwrap().to_string(), valid_uuid);
}

#[test]
fn session_id_deserialize_invalid_uuid_fails() {
    let json = "\"not-a-valid-uuid\"";
    let result: Result<SessionId, _> = serde_json::from_str(json);
    assert!(result.is_err(), "deserializing invalid UUID should fail");
}

#[test]
fn session_id_display_is_uuid_format() {
    let id = SessionId::new();
    let s = id.to_string();
    // UUID hyphenated format: xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx (36 chars)
    assert_eq!(s.len(), 36, "session ID must be 36-char hyphenated UUID");
    assert!(
        s.chars().all(|c| c.is_ascii_hexdigit() || c == '-'),
        "session ID must be hex and hyphens"
    );
}

#[test]
fn session_id_serde_roundtrip_is_quoted_uuid_string() {
    let id = SessionId::new();
    let json = serde_json::to_string(&id).unwrap();
    assert!(
        json.starts_with('"') && json.ends_with('"'),
        "SessionId must serialize to a quoted UUID string, got {json}"
    );
    let inner = json.trim_matches('"');
    assert_eq!(inner, id.to_string());
    let back: SessionId = serde_json::from_str(&json).unwrap();
    assert_eq!(id, back);
}

#[test]
fn turn_id_zero() {
    let t = TurnId::new(0);
    assert_eq!(t.as_u64(), 0);
    assert_eq!(t.next(), TurnId::new(1));
}

#[test]
fn turn_id_display() {
    assert_eq!(TurnId::new(42).to_string(), "42");
    assert_eq!(TurnId::new(0).to_string(), "0");
}

#[test]
fn nous_id_as_ref_and_borrow() {
    let id = NousId::new("syn").unwrap();
    let s: &str = id.as_ref();
    assert_eq!(s, "syn");
    let b: &str = id.borrow();
    assert_eq!(b, "syn");
}

#[test]
fn nous_id_borrow_hashmap_lookup() {
    let id = NousId::new("syn").unwrap();
    let mut map = std::collections::HashMap::new();
    map.insert(id, 42);
    assert_eq!(map.get("syn"), Some(&42));
}

#[test]
fn session_id_parse_roundtrip_uuid() {
    let id = SessionId::new();
    let s = id.to_string();
    let back = SessionId::parse(&s).unwrap();
    assert_eq!(id, back, "parse-roundtrip must be identity");
}

#[test]
fn turn_id_from_u64_roundtrip() {
    let n: u64 = 42;
    let id = TurnId::from(n);
    let back: u64 = id.into();
    assert_eq!(n, back);
}

#[test]
fn turn_id_from_matches_new() {
    assert_eq!(TurnId::from(7), TurnId::new(7));
}

#[test]
fn tool_name_as_ref_and_borrow() {
    let name = ToolName::new("exec").unwrap();
    let s: &str = name.as_ref();
    assert_eq!(s, "exec");
    let b: &str = name.borrow();
    assert_eq!(b, "exec");
}

#[test]
fn tool_name_borrow_hashmap_lookup() {
    let name = ToolName::new("exec").unwrap();
    let mut map = std::collections::HashMap::new();
    map.insert(name, 99);
    assert_eq!(map.get("exec"), Some(&99));
}

#[test]
fn id_error_display_formats() {
    let empty = IdError::Empty { kind: "NousId" };
    assert_eq!(empty.to_string(), "NousId cannot be empty");

    let long = IdError::TooLong {
        kind: "NousId",
        max: 64,
        actual: 100,
    };
    assert!(long.to_string().contains("100"));

    let fmt = IdError::InvalidFormat {
        kind: "NousId",
        value: "Bad".to_owned(),
        reason: "uppercase".to_owned(),
    };
    assert!(fmt.to_string().contains("Bad"));
}

mod newtype_id_macro {
    newtype_id!(
        /// Test ID using String inner type.
        pub struct TestStringId(String)
    );

    #[test]
    fn new_and_as_str() {
        let id = TestStringId::new("abc");
        assert_eq!(id.as_str(), "abc");
    }

    #[test]
    fn into_inner_returns_owned() {
        let id = TestStringId::new("abc");
        let inner: String = id.into_inner();
        assert_eq!(inner, "abc");
    }

    #[test]
    fn display_writes_inner() {
        let id = TestStringId::new("x-1");
        assert_eq!(id.to_string(), "x-1");
    }

    #[test]
    fn from_str_infallible() {
        let id: TestStringId = "hello".parse().unwrap();
        assert_eq!(id.as_str(), "hello");
    }

    #[test]
    fn from_string_and_str() {
        let a = TestStringId::from("abc");
        let b = TestStringId::from(String::from("abc"));
        assert_eq!(a, b);
    }

    #[test]
    fn into_string() {
        let id = TestStringId::new("val");
        let s: String = id.into();
        assert_eq!(s, "val");
    }

    #[test]
    fn deref_to_str() {
        let id = TestStringId::new("deref");
        assert_eq!(&*id, "deref");
        assert!(id.starts_with("de"));
    }

    #[test]
    fn partial_eq_str() {
        let id = TestStringId::new("cmp");
        assert!(id == *"cmp");
    }

    #[test]
    fn borrow_hashmap_lookup() {
        let id = TestStringId::new("key");
        let mut map = std::collections::HashMap::new();
        map.insert(id, 1);
        assert_eq!(map.get("key"), Some(&1));
    }

    #[test]
    fn serde_roundtrip() {
        let id = TestStringId::new("serde-test");
        let json = serde_json::to_string(&id).unwrap();
        assert_eq!(json, r#""serde-test""#);
        let back: TestStringId = serde_json::from_str(&json).unwrap();
        assert_eq!(id, back);
    }
}

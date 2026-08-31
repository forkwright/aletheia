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
fn invalid_nous_id_uppercase() {
    assert!(matches!(
        NousId::new("Syn"),
        Err(IdError::InvalidFormat { .. })
    ));
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
fn nous_id_from_static_matches_validated_construction() {
    let literal = NousId::from_static("unset");
    let validated = NousId::new("unset").unwrap();
    assert_eq!(literal, validated);
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
fn nous_id_leading_or_trailing_hyphen_rejected() {
    // WHY(#4638): a leading hyphen is a shell-argument-injection footgun
    // (an unquoted `-agent` reads as a flag to `rm`/`ls`/etc.) and the CLI
    // (`add-nous`, import `--nous-id`) and HTTP create path already reject
    // it independently — this was the one divergent validator.
    assert!(matches!(
        NousId::new("-syn"),
        Err(IdError::InvalidFormat { .. })
    ));
    assert!(matches!(
        NousId::new("syn-"),
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
        NousId::new("syn_1"),
        Err(IdError::InvalidFormat { .. })
    ));
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
fn nous_id_path_separators_rejected() {
    // WHY(#4638): the id is joined directly into `nous/<id>` on disk and
    // into route paths (`/api/v1/nous/{id}`) — any separator must be
    // caught by the one shared validator, not left to callers to notice.
    assert!(matches!(
        NousId::new("syn/1"),
        Err(IdError::InvalidFormat { .. })
    ));
    assert!(matches!(
        NousId::new("../etc"),
        Err(IdError::InvalidFormat { .. })
    ));
    assert!(matches!(
        NousId::new("syn\\1"),
        Err(IdError::InvalidFormat { .. })
    ));
}

#[test]
fn nous_id_reserved_template_prefix_rejected() {
    // WHY(#4638): `nous/_default` and `nous/_template` are reserved
    // scaffold directories (crates/aletheia/src/init/scaffold.rs). The
    // shared charset (lowercase alphanumeric + hyphen only) already
    // excludes underscore, so a reserved-shaped id is rejected as a
    // side effect — documented here so the exclusion stays intentional.
    assert!(matches!(
        NousId::new("_default"),
        Err(IdError::InvalidFormat { .. })
    ));
    assert!(matches!(
        NousId::new("_template"),
        Err(IdError::InvalidFormat { .. })
    ));
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
    use super::*;

    newtype_id!(
        /// Test ID using String inner type.
        pub struct TestStringId(String)
    );

    #[test]
    fn new_and_as_str() {
        let id = TestStringId::new("abc").unwrap();
        assert_eq!(id.as_str(), "abc");
    }

    #[test]
    fn into_inner_returns_owned() {
        let id = TestStringId::new("abc").unwrap();
        let inner: String = id.into_inner();
        assert_eq!(inner, "abc");
    }

    #[test]
    fn display_writes_inner() {
        let id = TestStringId::new("x-1").unwrap();
        assert_eq!(id.to_string(), "x-1");
    }

    #[test]
    fn new_rejects_empty() {
        assert!(matches!(
            TestStringId::new(""),
            Err(IdError::Empty {
                kind: "TestStringId"
            })
        ));
    }

    #[test]
    fn new_rejects_oversized() {
        let oversized = "a".repeat(NEWTYPE_ID_MAX_LEN + 1);
        assert!(matches!(
            TestStringId::new(oversized),
            Err(IdError::TooLong {
                kind: "TestStringId",
                ..
            })
        ));
    }

    #[test]
    fn new_accepts_max_length() {
        let max = "a".repeat(NEWTYPE_ID_MAX_LEN);
        assert!(TestStringId::new(max).is_ok());
    }

    #[test]
    fn new_rejects_control_character() {
        assert!(matches!(
            TestStringId::new("bad\u{0}id"),
            Err(IdError::InvalidFormat {
                kind: "TestStringId",
                ..
            })
        ));
        assert!(matches!(
            TestStringId::new("bad\nid"),
            Err(IdError::InvalidFormat {
                kind: "TestStringId",
                ..
            })
        ));
    }

    #[test]
    fn from_str_validates_like_new() {
        // WHY(#7088): FromStr used to be `Infallible` and accept anything,
        // which made a parsed id weaker than a constructed one. It now routes
        // through `new()`, so every text entrypoint (clap, `.parse()`) gets
        // the same validation floor.
        let id: TestStringId = "hello".parse().unwrap();
        assert_eq!(id.as_str(), "hello");

        let oversized = "a".repeat(NEWTYPE_ID_MAX_LEN + 1);
        for candidate in ["", "bad\u{0}id", "bad\nid", oversized.as_str()] {
            assert!(
                candidate.parse::<TestStringId>().is_err(),
                "FromStr must reject {candidate:?}"
            );
            assert_eq!(
                candidate
                    .parse::<TestStringId>()
                    .err()
                    .map(|e| e.to_string()),
                TestStringId::new(candidate).err().map(|e| e.to_string()),
                "FromStr and new must fail identically on {candidate:?}"
            );
        }
    }

    #[test]
    fn deserialize_rejects_empty() {
        // WHY(#7088): deserialization is the path that carries untrusted
        // input; it must fail exactly where the validated constructor does.
        let err = serde_json::from_str::<TestStringId>(r#""""#).unwrap_err();
        let expected = TestStringId::new("").unwrap_err();
        assert!(
            err.to_string().contains(&expected.to_string()),
            "serde error {err:?} must carry the constructor error: {expected}"
        );
    }

    #[test]
    fn deserialize_rejects_oversized() {
        let raw = "a".repeat(NEWTYPE_ID_MAX_LEN + 1);
        let json = format!("\"{raw}\"");
        let err = serde_json::from_str::<TestStringId>(&json).unwrap_err();
        let expected = TestStringId::new(raw).unwrap_err();
        assert!(
            err.to_string().contains(&expected.to_string()),
            "serde error {err:?} must carry the constructor error: {expected}"
        );
    }

    #[test]
    fn deserialize_rejects_control_character() {
        let err = serde_json::from_str::<TestStringId>(r#""bad\nid""#).unwrap_err();
        let expected = TestStringId::new("bad\nid").unwrap_err();
        assert!(
            err.to_string().contains(&expected.to_string()),
            "serde error {err:?} must carry the constructor error: {expected}"
        );
    }

    #[test]
    fn deserialize_accepts_valid() {
        let id: TestStringId = serde_json::from_str(r#""ok-1""#).unwrap();
        assert_eq!(id.as_str(), "ok-1");
    }

    #[test]
    fn from_string_and_str() {
        let a = TestStringId::from("abc");
        let b = TestStringId::from(String::from("abc"));
        assert_eq!(a, b);
    }

    #[test]
    fn into_string() {
        let id = TestStringId::new("val").unwrap();
        let s: String = id.into();
        assert_eq!(s, "val");
    }

    #[test]
    fn deref_to_str() {
        let id = TestStringId::new("deref").unwrap();
        assert_eq!(&*id, "deref");
        assert!(id.starts_with("de"));
    }

    #[test]
    fn partial_eq_str() {
        let id = TestStringId::new("cmp").unwrap();
        assert_eq!(id, *"cmp");
    }

    #[test]
    fn borrow_hashmap_lookup() {
        let id = TestStringId::new("key").unwrap();
        let mut map = std::collections::HashMap::new();
        map.insert(id, 1);
        assert_eq!(map.get("key"), Some(&1));
    }

    #[test]
    fn serde_roundtrip() {
        let id = TestStringId::new("serde-test").unwrap();
        let json = serde_json::to_string(&id).unwrap();
        assert_eq!(json, r#""serde-test""#);
        let back: TestStringId = serde_json::from_str(&json).unwrap();
        assert_eq!(id, back);
    }
}

#[test]
fn nous_id_from_str_validates() {
    // WHY both directions: a hand-written impl that forgot to validate would still compile, still
    // parse, and silently make a parsed id weaker than a constructed one.
    assert_eq!("worker-1".parse::<NousId>().unwrap().as_str(), "worker-1");
    assert!("Worker_1".parse::<NousId>().is_err());
    assert!("".parse::<NousId>().is_err());
    assert!("../etc".parse::<NousId>().is_err());
}

#[test]
fn nous_id_from_str_agrees_with_new() {
    // The two constructors must not diverge: clap will use `FromStr`, everything else uses `new`.
    for candidate in ["syn", "Syn", "a-b-c", "-lead", "", "with/slash"] {
        assert_eq!(
            candidate.parse::<NousId>().is_ok(),
            NousId::new(candidate).is_ok(),
            "FromStr and new disagree on {candidate:?}"
        );
    }
}

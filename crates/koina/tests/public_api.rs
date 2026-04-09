//! Integration tests for koina's public API surface.
//!
//! WHY: koina had zero `crates/koina/tests/` integration tests prior to
//! this. The crate is foundational — every other crate consumes
//! `SecretString`, `Ulid`, `Uuid`, `EventEmitter`, etc. — so any
//! breakage in the public API ripples across the workspace.
//!
//! These tests run against the published API surface only (no
//! `pub(crate)` access), the same way nous/pylon/symbolon consume it.

#![expect(clippy::expect_used, reason = "test assertions")]
#![expect(
    clippy::indexing_slicing,
    reason = "known-length Vec indexing in UUID segment/version assertions"
)]

// --- SecretString ---

mod secret_string {
    use aletheia_koina::secret::SecretString;

    #[test]
    fn from_string_round_trips_via_expose_secret() {
        let s = SecretString::from("hunter2".to_owned());
        assert_eq!(s.expose_secret(), "hunter2");
    }

    #[test]
    fn from_str_round_trips() {
        let s = SecretString::from("api-token");
        assert_eq!(s.expose_secret(), "api-token");
    }

    #[test]
    fn debug_does_not_leak_secret() {
        // WHY: contract — Debug must redact the inner value so secrets
        // never accidentally hit logs via {:?}. Reaching for the inner
        // string requires the explicit expose_secret() call.
        let s = SecretString::from("super-secret-value");
        let debug_output = format!("{s:?}");
        assert!(
            !debug_output.contains("super-secret-value"),
            "Debug must not include the secret literal, got: {debug_output}"
        );
    }

    #[test]
    fn display_does_not_leak_secret() {
        let s = SecretString::from("super-secret-value");
        let display = format!("{s}");
        assert!(
            !display.contains("super-secret-value"),
            "Display must not include the secret literal, got: {display}"
        );
    }

    #[test]
    fn equality_compares_inner_strings() {
        let a = SecretString::from("same");
        let b = SecretString::from("same");
        let c = SecretString::from("different");
        assert_eq!(a, b);
        assert_ne!(a, c);
    }

    #[test]
    fn clone_yields_independent_secret_with_same_value() {
        let original = SecretString::from("token");
        let cloned = original.clone();
        assert_eq!(original.expose_secret(), cloned.expose_secret());
    }

    #[test]
    fn empty_secret_is_well_defined() {
        let s = SecretString::from(String::new());
        assert_eq!(s.expose_secret(), "");
    }
}

// --- Ulid ---

mod ulid {
    use aletheia_koina::ulid::Ulid;

    #[test]
    fn new_produces_unique_ids() {
        let a = Ulid::new();
        let b = Ulid::new();
        assert_ne!(a, b, "two consecutive ULIDs must differ");
    }

    #[test]
    fn to_string_is_26_crockford_chars() {
        let id = Ulid::new();
        let s = id.to_string();
        assert_eq!(s.len(), 26, "ULID string form must be 26 chars");
        // Crockford base32 alphabet (no I, L, O, U)
        for c in s.chars() {
            assert!(
                c.is_ascii_alphanumeric() && !matches!(c, 'I' | 'L' | 'O' | 'U'),
                "char {c} not in Crockford alphabet"
            );
        }
    }

    #[test]
    fn round_trips_through_string() {
        let original = Ulid::new();
        let s = original.to_string();
        let parsed: Ulid = s.parse().expect("ULID round trip");
        assert_eq!(original, parsed);
    }

    #[test]
    fn ordering_reflects_creation_time() {
        // WHY: ULIDs are timestamp-prefixed and sortable. Two ULIDs
        // generated in sequence must order earlier-first when sorted.
        let a = Ulid::new();
        std::thread::sleep(std::time::Duration::from_millis(2));
        let b = Ulid::new();
        assert!(
            a.to_string() < b.to_string(),
            "later ULID must sort after earlier ULID"
        );
    }

    #[test]
    fn from_u128_round_trips_via_as_u128() {
        let value: u128 = 0x0123_4567_89AB_CDEF_0123_4567_89AB_CDEF;
        let id = Ulid::from_u128(value);
        assert_eq!(id.as_u128(), value);
    }
}

// --- Uuid ---

mod uuid {
    use aletheia_koina::uuid::{Uuid, uuid_v4};

    #[test]
    fn new_v4_produces_unique_ids() {
        let a = Uuid::new_v4();
        let b = Uuid::new_v4();
        assert_ne!(a, b);
    }

    #[test]
    fn uuid_v4_string_form_is_canonical() {
        let s = uuid_v4();
        // Canonical UUID form: 8-4-4-4-12 hex with hyphens, total 36 chars
        assert_eq!(s.len(), 36, "UUID string form must be 36 chars");
        let segments: Vec<&str> = s.split('-').collect();
        assert_eq!(segments.len(), 5);
        assert_eq!(segments[0].len(), 8);
        assert_eq!(segments[1].len(), 4);
        assert_eq!(segments[2].len(), 4);
        assert_eq!(segments[3].len(), 4);
        assert_eq!(segments[4].len(), 12);
        for seg in &segments {
            assert!(
                seg.chars().all(|c| c.is_ascii_hexdigit()),
                "segment {seg} must be hex"
            );
        }
    }

    #[test]
    fn parse_str_round_trips() {
        let s = uuid_v4();
        let parsed = Uuid::parse_str(&s).expect("uuid round-trips");
        assert_eq!(parsed.to_string(), s);
    }

    #[test]
    fn parse_str_rejects_invalid() {
        // WHY: parse_str must reject any non-canonical input. Three
        // categories: wrong length, non-hex, missing hyphens.
        assert!(Uuid::parse_str("not-a-uuid").is_err());
        assert!(Uuid::parse_str("zzzzzzzz-zzzz-zzzz-zzzz-zzzzzzzzzzzz").is_err());
        assert!(Uuid::parse_str("0000000000000000000000000000000000000000").is_err());
    }

    #[test]
    fn version_4_marker_in_canonical_form() {
        // WHY: UUID v4 specification: the 13th hex character must be '4'
        // (the version nibble), and the 17th must be 8/9/a/b (variant).
        let s = uuid_v4();
        let chars: Vec<char> = s.chars().collect();
        // Index after first 8 hex + first hyphen + 4 hex + second hyphen = 14
        assert_eq!(
            chars[14], '4',
            "UUID v4 version marker should be '4' at position 14, got: {s}"
        );
        assert!(
            matches!(chars[19], '8' | '9' | 'a' | 'b'),
            "UUID v4 variant should be 8/9/a/b at position 19, got: {s}"
        );
    }
}

// --- HTTP constants ---

mod http_constants {
    use aletheia_koina::http::{API_HEALTH, API_V1, BEARER_PREFIX, CONTENT_TYPE_JSON};

    #[test]
    fn api_v1_starts_with_slash() {
        assert!(
            API_V1.starts_with('/'),
            "API_V1 must be a leading-slash path"
        );
    }

    #[test]
    fn api_health_is_unversioned_path() {
        // WHY: /api/health is intentionally unversioned — health checks
        // bypass the v1 prefix so existing monitoring stays stable across
        // versioned API changes.
        assert_eq!(API_HEALTH, "/api/health");
        assert_eq!(API_V1, "/api/v1");
        assert!(!API_HEALTH.starts_with(API_V1));
    }

    #[test]
    fn bearer_prefix_includes_trailing_space() {
        assert_eq!(BEARER_PREFIX, "Bearer ");
    }

    #[test]
    fn content_type_json_is_canonical() {
        assert_eq!(CONTENT_TYPE_JSON, "application/json");
    }
}

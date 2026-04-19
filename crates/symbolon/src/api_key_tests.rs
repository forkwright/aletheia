#![expect(clippy::unwrap_used, reason = "test assertions")]
#![expect(
    clippy::indexing_slicing,
    reason = "test: parts[2] is valid after splitn(3) produces 3 parts"
)]

use std::time::Duration;

use super::{KEY_PREFIX, generate, list, parse_key, revoke, time_from_unix, validate};
use crate::store::AuthStore;
use crate::types::{ApiKeyRecord, Claims, Role, TokenKind};

fn memory_store() -> AuthStore {
    AuthStore::open_in_memory().unwrap()
}

// ── parse_key / malformed / missing ───────────────────────────────────────────

#[test]
fn generate_and_validate_roundtrip() {
    let store = memory_store();
    let (key, record) = generate(&store, "test", Role::Operator, None, None).unwrap();

    assert!(key.starts_with("ale_test_"));
    assert_eq!(record.prefix, "test");
    assert_eq!(record.role, Role::Operator);

    let claims = validate(&store, &key).unwrap();
    assert_eq!(claims.sub, "apikey:test");
    assert_eq!(claims.role, Role::Operator);
}

#[test]
fn generate_agent_key_with_nous_id() {
    let store = memory_store();
    let (key, _) = generate(&store, "syn", Role::Agent, Some("syn"), None).unwrap();
    let claims = validate(&store, &key).unwrap();
    assert_eq!(claims.role, Role::Agent);
    assert_eq!(claims.nous_id.as_deref(), Some("syn"));
}

#[test]
fn revoked_key_rejected() {
    let store = memory_store();
    let (key, record) = generate(&store, "test", Role::Operator, None, None).unwrap();

    revoke(&store, &record.id).unwrap();
    let result = validate(&store, &key);
    assert!(result.is_err());
}

#[test]
fn malformed_key_rejected() {
    let store = memory_store();
    assert!(validate(&store, "not-a-key").is_err());
    assert!(validate(&store, "ale_").is_err());
    assert!(validate(&store, "ale__secret").is_err());
    assert!(validate(&store, "xyz_test_secret").is_err());
}

#[test]
fn nonexistent_key_rejected() {
    let store = memory_store();
    assert!(validate(&store, "ale_test_nonexistent").is_err());
}

#[test]
fn list_returns_all_keys() {
    let store = memory_store();
    generate(&store, "a", Role::Operator, None, None).unwrap();
    generate(&store, "b", Role::Agent, Some("syn"), None).unwrap();

    let keys = list(&store).unwrap();
    assert_eq!(keys.len(), 2);
}

#[test]
fn parse_key_format() {
    let (prefix, holder, secret) = parse_key("ale_syn_abc123").unwrap();
    assert_eq!(prefix, "ale");
    assert_eq!(holder, "syn");
    assert_eq!(secret, "abc123");
}

#[test]
fn key_secret_is_64_hex_chars() {
    let store = memory_store();
    let (key, _) = generate(&store, "test", Role::Operator, None, None).unwrap();
    let parts: Vec<&str> = key.splitn(3, '_').collect();
    assert_eq!(parts[2].len(), 64); // NOTE: 32 bytes * 2 hex chars
}

// ── generate: format, role, nous_id, expiry (mutants 31–48) ──────────────────

/// WHY: mutant replaces `Ok((full_key, record))` with `Ok(("xyzzy", Default::default()))`
/// or `Ok((String::new(), Default::default()))`. Assert the returned key has the exact
/// `ale_<prefix>_<64 hex>` shape and the record mirrors the input arguments.
#[test]
fn generate_returns_well_formed_full_key_string() {
    let store = memory_store();
    let (key, _record) = generate(&store, "holder", Role::Operator, None, None).unwrap();

    // Three underscore-delimited segments.
    let parts: Vec<&str> = key.splitn(3, '_').collect();
    assert_eq!(parts.len(), 3);
    assert_eq!(parts[0], KEY_PREFIX);
    assert_eq!(parts[0], "ale");
    assert_eq!(parts[1], "holder");
    assert_eq!(parts[2].len(), 64);
    // All secret chars are lowercase hex.
    assert!(
        parts[2]
            .chars()
            .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
    );
    // Full key is non-empty and starts with the documented prefix.
    assert!(!key.is_empty());
    assert!(key.starts_with("ale_holder_"));
}

/// WHY: catches `Default::default()` mutant on the returned record. Default `ApiKeyRecord`
/// would have empty prefix / Role::Readonly / `nous_id == None`, which must fail here.
#[test]
fn generate_record_reflects_requested_role_and_nous_id() {
    let store = memory_store();
    let (_k, record) = generate(&store, "syn", Role::Agent, Some("syn"), None).unwrap();

    assert_eq!(record.prefix, "syn");
    assert_eq!(record.role, Role::Agent);
    assert_eq!(record.nous_id.as_deref(), Some("syn"));
    assert!(record.revoked_at.is_none());
    assert!(record.last_used_at.is_none());
    // id is a ULID (Crockford base32, 26 chars).
    assert_eq!(record.id.len(), 26);
    // key_hash is blake3 hex (64 chars).
    assert_eq!(record.key_hash.len(), 64);
    assert!(record.key_hash.chars().all(|c| c.is_ascii_hexdigit()));
}

/// WHY: `None` expiry path vs `Some(duration)` — explicit check that no expiry
/// is stored when `expires_in` is None. Covers the `.map(...)` closure being
/// mutated to always-Some.
#[test]
fn generate_without_expiry_leaves_expires_at_none() {
    let store = memory_store();
    let (_k, record) = generate(&store, "noexp", Role::Readonly, None, None).unwrap();
    assert!(record.expires_at.is_none());
}

/// WHY: line 39 `SystemTime::now() + d` is mutable to `*` or `-`. `*` produces
/// an absurd future date (centuries out) and `-` produces a pre-1970 timestamp
/// that panics or saturates. Assert the stored `expires_at` parses back to a
/// value within ≤2s of `now + expires_in`.
#[test]
fn generate_with_expiry_sets_expires_at_near_requested() {
    let store = memory_store();
    let expires_in = Duration::from_secs(3600); // 1 hour

    let before = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let (_k, record) = generate(&store, "exp", Role::Operator, None, Some(expires_in)).unwrap();
    let after = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();

    let stored = record
        .expires_at
        .expect("expires_at set when expires_in is Some");
    // Parse the ISO string back to a unix-seconds value via the inverse of time_from_unix.
    let parsed_secs = iso8601_to_unix_secs(&stored);

    // Expected range: [before + 3600, after + 3600].
    let expected_low = before + 3600;
    let expected_high = after + 3600;

    // Allow ≤2s slack on each side per the issue's acceptance criterion.
    assert!(
        parsed_secs >= expected_low.saturating_sub(2),
        "expires_at={parsed_secs} too low; expected >= {}",
        expected_low.saturating_sub(2)
    );
    assert!(
        parsed_secs <= expected_high + 2,
        "expires_at={parsed_secs} too high; expected <= {}",
        expected_high + 2
    );
}

/// Parse `YYYY-MM-DDTHH:MM:SS.000Z` back to seconds-since-epoch.
/// Intentionally independent of `time_from_unix` so the test does not just
/// round-trip through the same arithmetic it is trying to verify.
fn iso8601_to_unix_secs(s: &str) -> u64 {
    // Format: YYYY-MM-DDTHH:MM:SS.000Z (24 chars).
    let year: u64 = s[0..4].parse().unwrap();
    let month: u64 = s[5..7].parse().unwrap();
    let day: u64 = s[8..10].parse().unwrap();
    let hour: u64 = s[11..13].parse().unwrap();
    let minute: u64 = s[14..16].parse().unwrap();
    let second: u64 = s[17..19].parse().unwrap();

    // Howard Hinnant civil-to-days, inverse of util::days_to_date.
    let y = if month <= 2 { year - 1 } else { year };
    let era = y / 400;
    let yoe = y - era * 400;
    let m = month;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    days * 86400 + hour * 3600 + minute * 60 + second
}

// ── validate: expiry boundary + claims round-trip (mutants 79, 92) ────────────

/// Store an API key record whose `expires_at` is a known ISO-8601 string, then
/// validate the corresponding raw key. Returns the validate() Result so callers
/// can check both Ok(claims) and Err(expired) outcomes.
fn validate_with_stored_expiry(expires_at: &str) -> (crate::error::Result<Claims>, String) {
    let store = memory_store();
    // Generate a real key to get valid (raw_key, key_hash) pair.
    let (raw_key, record) = generate(&store, "exp", Role::Agent, Some("syn"), None).unwrap();
    // Overwrite the stored record with a controlled expires_at.
    let overridden = ApiKeyRecord {
        expires_at: Some(expires_at.to_owned()),
        ..record
    };
    store.store_api_key(&overridden).unwrap();
    let result = validate(&store, &raw_key);
    (result, raw_key)
}

/// WHY: line 92 `if *expires_at < now`. A stored expiry of "1970-..." is in the
/// strict past — must trigger ExpiredToken, catching `>`/`==` mutants.
#[test]
fn validate_rejects_expired_key() {
    let (result, _) = validate_with_stored_expiry("1970-01-01T00:00:00.000Z");
    let err = result.expect_err("expired expires_at must fail validation");
    assert!(
        matches!(err, crate::error::Error::ExpiredToken { .. }),
        "expected ExpiredToken, got {err:?}"
    );
}

/// WHY: an `expires_at` far in the future must succeed. Catches the `>` mutant
/// (would flip the branch and reject valid keys) and the whole-function stub.
#[test]
fn validate_accepts_unexpired_key() {
    let (result, _) = validate_with_stored_expiry("9999-12-31T23:59:59.000Z");
    let claims = result.expect("future expires_at must pass validation");
    assert_eq!(claims.role, Role::Agent);
    assert_eq!(claims.nous_id.as_deref(), Some("syn"));
}

/// WHY: boundary case — `expires_at == now_iso()` must NOT be rejected, because
/// the production code uses strict `<`. Catches `<=` and `==` mutants which
/// would flip this case.
#[test]
fn validate_equal_to_now_is_not_rejected() {
    // Build an ISO timestamp equal to right-now. Since now_iso() uses the same
    // time_from_unix formatter, we match its exact second-truncation.
    let now = time_from_unix(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs(),
    );
    let (result, _) = validate_with_stored_expiry(&now);
    // Either Ok (strict <) or Err::ExpiredToken (<=) — assert Ok to pin strict <.
    assert!(
        result.is_ok(),
        "expires_at == now must not be considered expired (strict <). got {result:?}"
    );
}

/// WHY: line 79 `Result<Claims>` can be replaced with `Ok(Default::default())`.
/// Assert every field of the returned Claims carries the stored record data.
/// Uses a table over (role, nous_id, prefix) to cover multiple shapes.
#[test]
fn validate_claims_round_trip_all_fields() {
    let cases: &[(Role, Option<&str>, &str)] = &[
        (Role::Admin, None, "admin-holder"),
        (Role::Operator, None, "op-holder"),
        (Role::Agent, Some("syn"), "syn"),
        (Role::Agent, Some("akron"), "akron"),
        (Role::Readonly, None, "dash"),
    ];

    for &(role, nous_id, prefix) in cases {
        let store = memory_store();
        let (key, record) = generate(&store, prefix, role, nous_id, None).unwrap();
        let claims = validate(&store, &key).unwrap();

        assert_eq!(claims.role, role, "role mismatch for prefix={prefix}");
        assert_eq!(
            claims.nous_id.as_deref(),
            nous_id,
            "nous_id mismatch for prefix={prefix}"
        );
        assert_eq!(
            claims.sub,
            format!("apikey:{prefix}"),
            "sub mismatch for prefix={prefix}"
        );
        assert_eq!(claims.iss, "aletheia");
        assert_eq!(claims.jti, record.id);
        assert!(matches!(claims.kind, TokenKind::Access));
    }
}

/// WHY: catches the whole-function `Ok(Default::default())` mutant on validate
/// — a Default Claims has empty `jti`, which must differ from the stored id.
#[test]
fn validate_returns_stored_jti() {
    let store = memory_store();
    let (key, record) = generate(&store, "jti", Role::Operator, None, None).unwrap();
    let claims = validate(&store, &key).unwrap();
    assert!(!claims.jti.is_empty());
    assert_eq!(claims.jti, record.id);
}

// ── time_from_unix: exact formatting for known timestamps ────────────────────
//
// WHY: every arithmetic op in this function (`/ 86400`, `% 86400`, `/ 3600`,
// `(% 3600) / 60`, `% 60`) is a mutation target. Prior tests treated the
// timestamp as opaque. These assert exact output strings at fixtures that
// exercise every component: days, hours, minutes, seconds — including year,
// month, day-of-month, leap days, and far-future dates.

#[test]
fn time_from_unix_epoch() {
    assert_eq!(time_from_unix(0), "1970-01-01T00:00:00.000Z");
}

#[test]
fn time_from_unix_y2k() {
    // 2000-01-01T00:00:00 UTC
    assert_eq!(time_from_unix(946_684_800), "2000-01-01T00:00:00.000Z");
}

#[test]
fn time_from_unix_leap_day_2000_02_29() {
    // 2000-02-29T00:00:00 UTC — year 2000 is a leap year (divisible by 400).
    assert_eq!(time_from_unix(951_782_400), "2000-02-29T00:00:00.000Z");
}

#[test]
fn time_from_unix_hours_minutes_seconds() {
    // 2000-01-01T01:02:03 UTC — exercises all three sub-day components.
    assert_eq!(
        time_from_unix(946_684_800 + 3723),
        "2000-01-01T01:02:03.000Z"
    );
}

#[test]
fn time_from_unix_end_of_day() {
    // 2000-01-01T23:59:59 UTC — max hours/minutes/seconds within a day.
    assert_eq!(
        time_from_unix(946_684_800 + 86_399),
        "2000-01-01T23:59:59.000Z"
    );
}

#[test]
fn time_from_unix_far_future_3000_12_31() {
    // 3000-12-31T23:59:59 UTC — far past any realistic key lifetime.
    assert_eq!(time_from_unix(32_535_215_999), "3000-12-31T23:59:59.000Z");
}

#[test]
fn time_from_unix_known_2023_timestamp() {
    // 2023-11-14T22:13:20 UTC — a round unix-seconds value (1_700_000_000).
    assert_eq!(time_from_unix(1_700_000_000), "2023-11-14T22:13:20.000Z");
}

/// WHY: the whole function can be replaced with `String::new()` or `"xyzzy"`.
/// A single equality on a non-empty, well-formed string catches that stub.
#[test]
fn time_from_unix_shape_is_iso8601_z() {
    let out = time_from_unix(1_700_000_000);
    assert_eq!(out.len(), 24);
    assert!(out.ends_with(".000Z"));
    assert_eq!(out.as_bytes()[4], b'-');
    assert_eq!(out.as_bytes()[7], b'-');
    assert_eq!(out.as_bytes()[10], b'T');
    assert_eq!(out.as_bytes()[13], b':');
    assert_eq!(out.as_bytes()[16], b':');
}

//! (See parent module for full rationale.)

use std::path::Path;

use super::super::*;
use super::test_timestamp;

// Split: validate_memory_path() positive + adversarial + cross-layer + Verification types.

// validate_memory_path() — adversarial tests per layer
// ---------------------------------------------------------------------------

// Layer 1: NullByte

#[test]
fn rejects_null_byte_in_path() {
    let path = Path::new("file\0.md");
    let result = validate_memory_path(path, Path::new("/test/memory"), MemoryScope::Project);
    assert!(result.is_err(), "null byte should be rejected");
    let err = result.unwrap_err();
    assert_eq!(
        err.layer(),
        PathValidationLayer::NullByte,
        "error should identify NullByte layer"
    );
}

#[test]
fn rejects_null_byte_at_end() {
    let path_str = "file.md\0";
    let path = Path::new(path_str);
    let result = validate_memory_path(path, Path::new("/test/memory"), MemoryScope::Project);
    assert!(result.is_err(), "trailing null byte should be rejected");
    assert_eq!(result.unwrap_err().layer(), PathValidationLayer::NullByte);
}

// Layer 2: Canonicalization

#[test]
fn rejects_parent_directory_traversal() {
    let path = Path::new("../user/secret.md");
    let result = validate_memory_path(path, Path::new("/test/memory"), MemoryScope::Project);
    assert!(result.is_err(), ".. traversal should be rejected");
    assert_eq!(
        result.unwrap_err().layer(),
        PathValidationLayer::Canonicalization
    );
}

#[test]
fn rejects_deep_traversal() {
    let path = Path::new("sub/../../user/secret.md");
    let result = validate_memory_path(path, Path::new("/test/memory"), MemoryScope::Project);
    assert!(result.is_err(), "deep .. traversal should be rejected");
    assert_eq!(
        result.unwrap_err().layer(),
        PathValidationLayer::Canonicalization
    );
}

#[test]
fn rejects_backslash_in_path() {
    let path = Path::new("sub\\..\\file.md");
    let result = validate_memory_path(path, Path::new("/test/memory"), MemoryScope::Project);
    assert!(result.is_err(), "backslash should be rejected");
    assert_eq!(
        result.unwrap_err().layer(),
        PathValidationLayer::Canonicalization
    );
}

// Layer 3: URL-encoded traversal

#[test]
fn rejects_url_encoded_dot() {
    let path = Path::new("%2e%2e%2fuser/secret.md");
    let result = validate_memory_path(path, Path::new("/test/memory"), MemoryScope::Project);
    assert!(result.is_err(), "URL-encoded .. should be rejected");
    assert_eq!(
        result.unwrap_err().layer(),
        PathValidationLayer::UrlEncodedTraversal
    );
}

#[test]
fn rejects_url_encoded_slash() {
    let path = Path::new("sub%2fparent");
    let result = validate_memory_path(path, Path::new("/test/memory"), MemoryScope::Project);
    assert!(result.is_err(), "URL-encoded / should be rejected");
    assert_eq!(
        result.unwrap_err().layer(),
        PathValidationLayer::UrlEncodedTraversal
    );
}

#[test]
fn rejects_url_encoded_backslash() {
    let path = Path::new("sub%5cfile");
    let result = validate_memory_path(path, Path::new("/test/memory"), MemoryScope::Project);
    assert!(result.is_err(), "URL-encoded \\ should be rejected");
    assert_eq!(
        result.unwrap_err().layer(),
        PathValidationLayer::UrlEncodedTraversal
    );
}

#[test]
fn rejects_mixed_case_url_encoding() {
    let path = Path::new("sub%2Efile");
    let result = validate_memory_path(path, Path::new("/test/memory"), MemoryScope::Project);
    assert!(
        result.is_err(),
        "mixed-case URL encoding should be rejected"
    );
    assert_eq!(
        result.unwrap_err().layer(),
        PathValidationLayer::UrlEncodedTraversal
    );
}

// Layer 4: Unicode normalization

#[test]
fn rejects_fullwidth_period() {
    // U+FF0E (fullwidth period) normalizes to '.' under NFKC
    let path_str = "file\u{FF0E}md";
    let path = Path::new(path_str);
    let result = validate_memory_path(path, Path::new("/test/memory"), MemoryScope::Project);
    assert!(result.is_err(), "fullwidth period should be rejected");
    let err = result.unwrap_err();
    assert_eq!(err.layer(), PathValidationLayer::UnicodeNormalization);
    if let PathValidationError::UnicodeNormalization { offending_char, .. } = err {
        assert_eq!(offending_char, '\u{FF0E}');
    }
}

#[test]
fn rejects_fullwidth_solidus() {
    // U+FF0F (fullwidth solidus) normalizes to '/' under NFKC
    let path_str = "sub\u{FF0F}file";
    let path = Path::new(path_str);
    let result = validate_memory_path(path, Path::new("/test/memory"), MemoryScope::Project);
    assert!(result.is_err(), "fullwidth solidus should be rejected");
    assert_eq!(
        result.unwrap_err().layer(),
        PathValidationLayer::UnicodeNormalization
    );
}

#[test]
fn rejects_fullwidth_backslash() {
    // U+FF3C (fullwidth reverse solidus) normalizes to '\' under NFKC
    let path_str = "sub\u{FF3C}file";
    let path = Path::new(path_str);
    let result = validate_memory_path(path, Path::new("/test/memory"), MemoryScope::Project);
    assert!(
        result.is_err(),
        "fullwidth reverse solidus should be rejected"
    );
    assert_eq!(
        result.unwrap_err().layer(),
        PathValidationLayer::UnicodeNormalization
    );
}

// Layer 5: Scope containment

#[test]
fn rejects_wrong_scope_directory() {
    // WHY: A project-scope path must be under project/, not user/.
    let path = Path::new("/test/memory/user/secret.md");
    let result = validate_memory_path(path, Path::new("/test/memory"), MemoryScope::Project);
    assert!(result.is_err(), "wrong scope directory should be rejected");
    assert_eq!(
        result.unwrap_err().layer(),
        PathValidationLayer::ScopeContainment
    );
}

#[test]
fn rejects_path_escaping_root() {
    let path = Path::new("/etc/passwd");
    let result = validate_memory_path(path, Path::new("/test/memory"), MemoryScope::Project);
    assert!(result.is_err(), "absolute path outside root should fail");
    assert_eq!(
        result.unwrap_err().layer(),
        PathValidationLayer::ScopeContainment
    );
}

#[test]
fn rejects_root_directory_itself() {
    // WHY: The memory root itself is not a valid scope path.
    let path = Path::new("/test/memory");
    let result = validate_memory_path(path, Path::new("/test/memory"), MemoryScope::Project);
    assert!(
        result.is_err(),
        "root directory itself should not validate as a scope path"
    );
}

// Layers 6–7: Symlink resolution, dangling symlink, loop detection (I/O)

#[cfg(unix)]
#[test]
fn rejects_symlink_escaping_root() {
    let tmp = tempfile::tempdir().expect("tempdir should succeed");
    let root = tmp.path().join("memory");
    let scope_dir = root.join("project");
    std::fs::create_dir_all(&scope_dir).expect("create scope dir");

    // Create a target outside root
    let outside = tmp.path().join("outside");
    std::fs::create_dir_all(&outside).expect("create outside dir");
    std::fs::write(outside.join("secret.txt"), b"secret").expect("write secret");

    // Create symlink inside scope pointing outside root
    let link = scope_dir.join("escape");
    std::os::unix::fs::symlink(&outside, &link).expect("create symlink");

    let result = validate_memory_path(Path::new("escape"), &root, MemoryScope::Project);
    assert!(result.is_err(), "symlink escaping root should be rejected");
    let err = result.unwrap_err();
    assert!(
        matches!(
            err.layer(),
            PathValidationLayer::SymlinkResolution | PathValidationLayer::ScopeContainment
        ),
        "error should be SymlinkResolution or ScopeContainment, got {:?}",
        err.layer()
    );
}

#[cfg(unix)]
#[test]
fn rejects_dangling_symlink() {
    let tmp = tempfile::tempdir().expect("tempdir should succeed");
    let root = tmp.path().join("memory");
    let scope_dir = root.join("project");
    std::fs::create_dir_all(&scope_dir).expect("create scope dir");

    // Create symlink to nonexistent target
    let link = scope_dir.join("dangling");
    std::os::unix::fs::symlink("/nonexistent/target", &link).expect("create symlink");

    let result = validate_memory_path(Path::new("dangling"), &root, MemoryScope::Project);
    assert!(result.is_err(), "dangling symlink should be rejected");
    assert_eq!(
        result.unwrap_err().layer(),
        PathValidationLayer::DanglingSymlink,
        "error should identify DanglingSymlink layer"
    );
}

#[cfg(unix)]
#[test]
fn accepts_valid_symlink_within_scope() {
    let tmp = tempfile::tempdir().expect("tempdir should succeed");
    let root = tmp.path().join("memory");
    let scope_dir = root.join("project");
    let sub = scope_dir.join("sub");
    std::fs::create_dir_all(&sub).expect("create sub dir");

    // Create a real file and a symlink to it within the same scope
    let real_file = sub.join("real.md");
    std::fs::write(&real_file, b"content").expect("write real file");
    let link = scope_dir.join("alias.md");
    std::os::unix::fs::symlink(&real_file, &link).expect("create symlink");

    let result = validate_memory_path(Path::new("alias.md"), &root, MemoryScope::Project);
    assert!(
        result.is_ok(),
        "symlink within scope should be accepted: {:?}",
        result.err()
    );
}

// ---------------------------------------------------------------------------
// validate_memory_path() — cross-layer combos
// ---------------------------------------------------------------------------

#[test]
fn null_byte_caught_before_traversal() {
    // WHY: Layer ordering means null byte is checked before canonicalization.
    let path = Path::new("../\0secret.md");
    let result = validate_memory_path(path, Path::new("/test/memory"), MemoryScope::Project);
    assert!(result.is_err());
    // NOTE: The first layer to fire wins. Both null byte and traversal are
    // present; null byte is checked first (Layer 1).
    assert_eq!(
        result.unwrap_err().layer(),
        PathValidationLayer::NullByte,
        "null byte should fire before canonicalization"
    );
}

#[test]
fn traversal_caught_before_url_encoding() {
    // WHY: Canonicalization (Layer 2) fires before URL decoding (Layer 3).
    let path = Path::new("../%2e%2e/secret.md");
    let result = validate_memory_path(path, Path::new("/test/memory"), MemoryScope::Project);
    assert!(result.is_err());
    assert_eq!(
        result.unwrap_err().layer(),
        PathValidationLayer::Canonicalization,
        "canonicalization should fire before URL-encoded traversal"
    );
}

#[test]
fn url_encoding_caught_before_unicode() {
    // WHY: URL-encoded traversal (Layer 3) fires before Unicode (Layer 4).
    let path_str = "%2e\u{FF0E}file";
    let path = Path::new(path_str);
    let result = validate_memory_path(path, Path::new("/test/memory"), MemoryScope::Project);
    assert!(result.is_err());
    assert_eq!(
        result.unwrap_err().layer(),
        PathValidationLayer::UrlEncodedTraversal,
        "URL encoding should fire before unicode normalization"
    );
}

#[test]
fn unicode_caught_before_scope_containment() {
    // WHY: Unicode normalization (Layer 4) fires before scope check (Layer 5).
    let path_str = "/wrong/scope/\u{FF0E}file";
    let path = Path::new(path_str);
    let result = validate_memory_path(path, Path::new("/test/memory"), MemoryScope::Project);
    assert!(result.is_err());
    assert_eq!(
        result.unwrap_err().layer(),
        PathValidationLayer::UnicodeNormalization,
        "unicode normalization should fire before scope containment"
    );
}

#[test]
fn scope_containment_catches_cross_scope_access() {
    // WHY: Agent in project scope cannot access user scope memories.
    for wrong_scope in &[
        MemoryScope::User,
        MemoryScope::Feedback,
        MemoryScope::Reference,
    ] {
        let path_string = format!("/test/memory/{}/secret.md", wrong_scope.as_str());
        let path = Path::new(&path_string);
        let result = validate_memory_path(path, Path::new("/test/memory"), MemoryScope::Project);
        assert!(
            result.is_err(),
            "accessing {wrong_scope} from project scope should fail"
        );
        assert_eq!(
            result.unwrap_err().layer(),
            PathValidationLayer::ScopeContainment
        );
    }
}

#[test]
fn double_url_encoded_traversal() {
    // WHY: Double-encoding like %252e could bypass single-pass decoding.
    // Our check catches %2e at the first pass.
    let path = Path::new("%252e%252e%252f");
    // NOTE: %25 is '%', so %252e decodes to %2e in a two-pass decode.
    // Our single-pass check doesn't catch this, but the scope containment
    // layer will catch the resulting path if it escapes.
    let result = validate_memory_path(path, Path::new("/test/memory"), MemoryScope::Project);
    // This path is literal "%252e%252e%252f" — no traversal at string level
    // and it stays within scope, so it passes. That's acceptable because
    // the filename is literally "%252e%252e%252f" after scope_dir join.
    assert!(
        result.is_ok(),
        "double-encoded path should pass (treated as literal filename)"
    );
}

#[test]
fn backslash_plus_url_encoding_combo() {
    let path = Path::new("sub\\%2e%2e");
    let result = validate_memory_path(path, Path::new("/test/memory"), MemoryScope::Project);
    assert!(result.is_err());
    // Backslash is caught first (Layer 2, Canonicalization).
    assert_eq!(
        result.unwrap_err().layer(),
        PathValidationLayer::Canonicalization
    );
}

#[test]
fn all_scopes_accept_valid_relative_path() {
    for scope in MemoryScope::ALL {
        let result =
            validate_memory_path(Path::new("valid-file.md"), Path::new("/test/memory"), scope);
        assert!(
            result.is_ok(),
            "valid relative path should pass for {} scope: {:?}",
            scope.as_str(),
            result.err()
        );
        assert_eq!(
            result.expect("checked above").scope(),
            scope,
            "validated scope should match input"
        );
    }
}

// ---------------------------------------------------------------------------
// Verification types
// ---------------------------------------------------------------------------

#[test]
fn verification_fact_type_roundtrip() {
    let ft = FactType::Verification;
    assert_eq!(ft.as_str(), "verification");
    assert_eq!(FactType::from_str_lossy("verification"), ft);
    assert_eq!(ft.to_string(), "verification");
}

#[test]
fn verification_fact_type_serde_roundtrip() {
    let ft = FactType::Verification;
    let json = serde_json::to_string(&ft).expect("FactType serialization is infallible");
    assert_eq!(
        json, r#""verification""#,
        "should serialize as snake_case string"
    );
    let back: FactType =
        serde_json::from_str(&json).expect("FactType should deserialize from its own JSON");
    assert_eq!(
        ft, back,
        "FactType::Verification should survive serde roundtrip"
    );
}

#[test]
fn verification_source_as_str_and_parse() {
    for (variant, expected) in [
        (VerificationSource::Command, "command"),
        (VerificationSource::Query, "query"),
        (VerificationSource::Arithmetic, "arithmetic"),
        (VerificationSource::Reference, "reference"),
    ] {
        assert_eq!(variant.as_str(), expected, "{variant:?} as_str mismatch");
        assert_eq!(
            VerificationSource::from_str_opt(expected),
            Some(variant),
            "from_str_opt({expected}) should return {variant:?}"
        );
        assert_eq!(
            variant.to_string(),
            expected,
            "{variant:?} Display mismatch"
        );
    }
    assert_eq!(
        VerificationSource::from_str_opt("bogus"),
        None,
        "unknown source should return None"
    );
}

#[test]
fn verification_source_serde_roundtrip() {
    for src in [
        VerificationSource::Command,
        VerificationSource::Query,
        VerificationSource::Arithmetic,
        VerificationSource::Reference,
    ] {
        let json =
            serde_json::to_string(&src).expect("VerificationSource serialization is infallible");
        let back: VerificationSource = serde_json::from_str(&json)
            .expect("VerificationSource should deserialize from its own JSON");
        assert_eq!(
            src, back,
            "VerificationSource should survive serde roundtrip"
        );
    }
}

#[test]
fn verification_status_as_str_and_parse() {
    for (variant, expected) in [
        (VerificationStatus::Pass, "pass"),
        (VerificationStatus::Fail, "fail"),
        (VerificationStatus::Stale, "stale"),
    ] {
        assert_eq!(variant.as_str(), expected, "{variant:?} as_str mismatch");
        assert_eq!(
            VerificationStatus::from_str_opt(expected),
            Some(variant),
            "from_str_opt({expected}) should return {variant:?}"
        );
        assert_eq!(
            variant.to_string(),
            expected,
            "{variant:?} Display mismatch"
        );
    }
    assert_eq!(
        VerificationStatus::from_str_opt("unknown"),
        None,
        "unknown status should return None"
    );
}

#[test]
fn verification_status_serde_roundtrip() {
    for status in [
        VerificationStatus::Pass,
        VerificationStatus::Fail,
        VerificationStatus::Stale,
    ] {
        let json =
            serde_json::to_string(&status).expect("VerificationStatus serialization is infallible");
        let back: VerificationStatus = serde_json::from_str(&json)
            .expect("VerificationStatus should deserialize from its own JSON");
        assert_eq!(
            status, back,
            "VerificationStatus should survive serde roundtrip"
        );
    }
}

#[test]
fn verification_record_serde_roundtrip() {
    let record = VerificationRecord {
        claim: "total line count is 383".to_owned(),
        source: VerificationSource::Command,
        expected: serde_json::json!(383),
        actual: serde_json::json!(383),
        tolerance: 0.0,
        status: VerificationStatus::Pass,
        verified_at: test_timestamp("2026-03-15T10:30:00Z"),
    };
    let json =
        serde_json::to_string(&record).expect("VerificationRecord serialization is infallible");
    let back: VerificationRecord = serde_json::from_str(&json)
        .expect("VerificationRecord should deserialize from its own JSON");
    assert_eq!(back.claim, record.claim, "claim should survive roundtrip");
    assert_eq!(
        back.source, record.source,
        "source should survive roundtrip"
    );
    assert_eq!(
        back.expected, record.expected,
        "expected should survive roundtrip"
    );
    assert_eq!(
        back.actual, record.actual,
        "actual should survive roundtrip"
    );
    assert!(
        (back.tolerance - record.tolerance).abs() < f64::EPSILON,
        "tolerance should survive roundtrip"
    );
    assert_eq!(
        back.status, record.status,
        "status should survive roundtrip"
    );
    assert_eq!(
        back.verified_at, record.verified_at,
        "verified_at should survive roundtrip"
    );
}

#[test]
fn verification_record_fail_with_tolerance() {
    let record = VerificationRecord {
        claim: "build time is 120s".to_owned(),
        source: VerificationSource::Arithmetic,
        expected: serde_json::json!(120),
        actual: serde_json::json!(135),
        tolerance: 0.1,
        status: VerificationStatus::Fail,
        verified_at: test_timestamp("2026-03-15T11:00:00Z"),
    };
    let json = serde_json::to_string(&record).expect("serialization should succeed");
    assert!(
        json.contains(r#""status":"fail""#),
        "JSON should contain fail status"
    );
    assert!(
        json.contains(r#""tolerance":0.1"#),
        "JSON should contain tolerance"
    );
}

// ---------------------------------------------------------------------------
// validate_memory_path_async() — non-blocking symlink I/O
// ---------------------------------------------------------------------------

#[cfg(unix)]
#[tokio::test]
async fn async_accepts_valid_symlink_within_scope() {
    let tmp = tempfile::tempdir().expect("tempdir should succeed");
    let root = tmp.path().join("memory");
    let scope_dir = root.join("project");
    let sub = scope_dir.join("sub");
    std::fs::create_dir_all(&sub).expect("create sub dir");

    let real_file = sub.join("real.md");
    std::fs::write(&real_file, b"content").expect("write real file");
    let link = scope_dir.join("alias.md");
    std::os::unix::fs::symlink(&real_file, &link).expect("create symlink");

    let result =
        validate_memory_path_async(Path::new("alias.md"), &root, MemoryScope::Project).await;
    assert!(
        result.is_ok(),
        "async symlink within scope should be accepted: {:?}",
        result.err()
    );
}

#[cfg(unix)]
#[tokio::test]
async fn async_rejects_symlink_escaping_root() {
    let tmp = tempfile::tempdir().expect("tempdir should succeed");
    let root = tmp.path().join("memory");
    let scope_dir = root.join("project");
    std::fs::create_dir_all(&scope_dir).expect("create scope dir");

    let outside = tmp.path().join("outside");
    std::fs::create_dir_all(&outside).expect("create outside dir");
    std::fs::write(outside.join("secret.txt"), b"secret").expect("write secret");

    let link = scope_dir.join("escape");
    std::os::unix::fs::symlink(&outside, &link).expect("create symlink");

    let err = validate_memory_path_async(Path::new("escape"), &root, MemoryScope::Project)
        .await
        .expect_err("symlink escaping root should be rejected");
    assert!(
        matches!(
            err.layer(),
            PathValidationLayer::SymlinkResolution | PathValidationLayer::ScopeContainment
        ),
        "error should be SymlinkResolution or ScopeContainment, got {:?}",
        err.layer()
    );
}

#[cfg(unix)]
#[tokio::test]
async fn async_rejects_dangling_symlink() {
    let tmp = tempfile::tempdir().expect("tempdir should succeed");
    let root = tmp.path().join("memory");
    let scope_dir = root.join("project");
    std::fs::create_dir_all(&scope_dir).expect("create scope dir");

    let link = scope_dir.join("dangling");
    std::os::unix::fs::symlink("/nonexistent/target", &link).expect("create symlink");

    let err = validate_memory_path_async(Path::new("dangling"), &root, MemoryScope::Project)
        .await
        .expect_err("dangling symlink should be rejected");
    assert_eq!(
        err.layer(),
        PathValidationLayer::DanglingSymlink,
        "error should identify DanglingSymlink layer"
    );
}

#[cfg(unix)]
#[tokio::test]
async fn async_rejects_symlink_loop() {
    let tmp = tempfile::tempdir().expect("tempdir should succeed");
    let root = tmp.path().join("memory");
    let scope_dir = root.join("project");
    std::fs::create_dir_all(&scope_dir).expect("create scope dir");

    let a = scope_dir.join("a");
    let b = scope_dir.join("b");
    std::os::unix::fs::symlink(&b, &a).expect("create a -> b");
    std::os::unix::fs::symlink(&a, &b).expect("create b -> a");

    let err = validate_memory_path_async(Path::new("a"), &root, MemoryScope::Project)
        .await
        .expect_err("symlink loop should be rejected");
    assert_eq!(
        err.layer(),
        PathValidationLayer::LoopDetection,
        "error should identify LoopDetection layer"
    );
}

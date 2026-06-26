use std::path::{Path, PathBuf};

use super::super::*;

#[test]
fn path_validation_layer_serde_roundtrip() {
    for layer in PathValidationLayer::ALL {
        let json =
            serde_json::to_string(&layer).expect("PathValidationLayer serialization is infallible");
        let back: PathValidationLayer = serde_json::from_str(&json)
            .expect("PathValidationLayer should deserialize from its own JSON");
        assert_eq!(
            layer, back,
            "PathValidationLayer should survive serde roundtrip"
        );
    }
}

#[test]
fn path_validation_layer_as_str_matches_serde() {
    for layer in PathValidationLayer::ALL {
        let json =
            serde_json::to_string(&layer).expect("PathValidationLayer serialization is infallible");
        let expected = format!("\"{}\"", layer.as_str());
        assert_eq!(
            json, expected,
            "PathValidationLayer json should match as_str representation"
        );
    }
}

#[test]
fn path_validation_layer_from_str_roundtrip() {
    for layer in PathValidationLayer::ALL {
        let parsed: PathValidationLayer = layer
            .as_str()
            .parse()
            .expect("PathValidationLayer as_str() should round-trip through FromStr");
        assert_eq!(
            layer, parsed,
            "PathValidationLayer should survive as_str/parse roundtrip"
        );
    }
}

#[test]
fn path_validation_layer_from_str_unknown() {
    assert!(
        "bogus".parse::<PathValidationLayer>().is_err(),
        "unrecognized string should fail to parse as PathValidationLayer"
    );
}

#[test]
fn path_validation_layer_display() {
    assert_eq!(
        PathValidationLayer::NullByte.to_string(),
        "null_byte",
        "NullByte should display as 'null_byte'"
    );
    assert_eq!(
        PathValidationLayer::UnicodeNormalization.to_string(),
        "unicode_normalization",
        "UnicodeNormalization should display as 'unicode_normalization'"
    );
    assert_eq!(
        PathValidationLayer::ScopeContainment.to_string(),
        "scope_containment",
        "ScopeContainment should display as 'scope_containment'"
    );
}

#[test]
fn path_validation_layer_all_has_eight_entries() {
    assert_eq!(
        PathValidationLayer::ALL.len(),
        8,
        "ALL should contain exactly 8 validation layers"
    );
}

#[test]
fn path_validation_layer_io_classification() {
    // WHY: Only filesystem-interacting layers should require I/O.
    let io_layers = PathValidationLayer::ALL
        .iter()
        .filter(|l| l.requires_io())
        .count();
    assert_eq!(
        io_layers, 3,
        "exactly 3 layers (symlink resolution, dangling symlink, loop detection) require I/O"
    );
    assert!(
        PathValidationLayer::SymlinkResolution.requires_io(),
        "symlink resolution requires I/O"
    );
    assert!(
        PathValidationLayer::DanglingSymlink.requires_io(),
        "dangling symlink detection requires I/O"
    );
    assert!(
        PathValidationLayer::LoopDetection.requires_io(),
        "loop detection requires I/O"
    );
    assert!(
        !PathValidationLayer::NullByte.requires_io(),
        "null byte check does not require I/O"
    );
    assert!(
        !PathValidationLayer::Canonicalization.requires_io(),
        "canonicalization does not require I/O"
    );
    assert!(
        !PathValidationLayer::UrlEncodedTraversal.requires_io(),
        "URL-encoded traversal check does not require I/O"
    );
    assert!(
        !PathValidationLayer::UnicodeNormalization.requires_io(),
        "unicode normalization check does not require I/O"
    );
    assert!(
        !PathValidationLayer::ScopeContainment.requires_io(),
        "scope containment check does not require I/O"
    );
}

#[test]
fn path_validation_fs_layers_constant() {
    assert_eq!(
        PATH_VALIDATION_FS_LAYERS, 7,
        "PATH_VALIDATION_FS_LAYERS should be 7"
    );
}

#[test]
fn symlink_hop_limit_matches_linux_eloop() {
    assert_eq!(
        SYMLINK_HOP_LIMIT, 40,
        "SYMLINK_HOP_LIMIT should match Linux ELOOP limit of 40"
    );
}

#[test]
fn path_validation_error_layer_mapping() {
    // WHY: Every error variant must map back to the correct layer for logging.
    let cases: Vec<(PathValidationError, PathValidationLayer)> = vec![
        (
            PathValidationError::NullByte {
                path: String::new(),
            },
            PathValidationLayer::NullByte,
        ),
        (
            PathValidationError::Canonicalization {
                path: String::new(),
                component: String::new(),
            },
            PathValidationLayer::Canonicalization,
        ),
        (
            PathValidationError::SymlinkResolution {
                path: PathBuf::new(),
                root: PathBuf::new(),
            },
            PathValidationLayer::SymlinkResolution,
        ),
        (
            PathValidationError::DanglingSymlink {
                path: PathBuf::new(),
            },
            PathValidationLayer::DanglingSymlink,
        ),
        (
            PathValidationError::LoopDetection {
                path: PathBuf::new(),
                hops: 0,
            },
            PathValidationLayer::LoopDetection,
        ),
        (
            PathValidationError::UrlEncodedTraversal {
                path: String::new(),
                decoded_fragment: String::new(),
            },
            PathValidationLayer::UrlEncodedTraversal,
        ),
        (
            PathValidationError::UnicodeNormalization {
                path: String::new(),
                offending_char: '.',
            },
            PathValidationLayer::UnicodeNormalization,
        ),
        (
            PathValidationError::ScopeContainment {
                path: PathBuf::new(),
                scope: MemoryScope::User,
                expected_dir: PathBuf::new(),
            },
            PathValidationLayer::ScopeContainment,
        ),
    ];
    assert_eq!(
        cases.len(),
        PathValidationLayer::ALL.len(),
        "every PathValidationLayer must have a corresponding error variant"
    );
    for (error, expected_layer) in cases {
        assert_eq!(
            error.layer(),
            expected_layer,
            "error variant should map to {expected_layer}"
        );
    }
}

#[test]
fn path_validation_error_display() {
    let err = PathValidationError::NullByte {
        path: "bad\0path".to_owned(),
    };
    assert!(
        err.to_string().contains("null byte"),
        "NullByte display should mention null byte"
    );

    let err = PathValidationError::ScopeContainment {
        path: PathBuf::from("/escape"),
        scope: MemoryScope::Project,
        expected_dir: PathBuf::from("/root/project"),
    };
    let msg = err.to_string();
    assert!(msg.contains("project"), "display should mention the scope");
    assert!(msg.contains("escapes"), "display should mention escape");
}

#[test]
fn path_validation_error_is_std_error() {
    // WHY: PathValidationError must implement std::error::Error for
    // compatibility with snafu context propagation.
    let err = PathValidationError::NullByte {
        path: String::new(),
    };
    let _: &dyn std::error::Error = &err;
}

#[test]
fn validated_path_accessors() {
    // WHY: ValidatedPath's public API must expose path and scope without
    // revealing the inner PathBuf directly.
    let vp = validate_memory_path(
        Path::new("notes.md"),
        Path::new("/test/memory"),
        MemoryScope::Project,
    )
    .expect("valid path should pass validation");

    assert_eq!(
        vp.as_path(),
        Path::new("/test/memory/project/notes.md"),
        "as_path should return the normalized full path"
    );
    assert_eq!(
        vp.scope(),
        MemoryScope::Project,
        "scope should return the validated scope"
    );

    let as_ref: &Path = vp.as_ref();
    assert_eq!(
        as_ref,
        Path::new("/test/memory/project/notes.md"),
        "AsRef<Path> should match as_path"
    );
}

#[test]
fn validated_path_into_path_buf() {
    let vp = validate_memory_path(
        Path::new("file.md"),
        Path::new("/test/memory"),
        MemoryScope::User,
    )
    .expect("valid path should pass validation");

    let pb = vp.into_path_buf();
    assert_eq!(
        pb,
        PathBuf::from("/test/memory/user/file.md"),
        "into_path_buf should return the inner PathBuf"
    );
}

#[test]
fn validated_path_display() {
    let vp = validate_memory_path(
        Path::new("file.md"),
        Path::new("/test/memory"),
        MemoryScope::Feedback,
    )
    .expect("valid path should pass validation");

    assert_eq!(
        vp.to_string(),
        "/test/memory/feedback/file.md",
        "Display should render the path"
    );
}

#[test]
fn valid_user_scope_path() {
    let result = validate_memory_path(
        Path::new("preferences.md"),
        Path::new("/test/memory"),
        MemoryScope::User,
    );
    assert!(result.is_ok(), "simple file in user scope should validate");
    let vp = result.expect("checked above");
    assert_eq!(vp.scope(), MemoryScope::User);
    assert_eq!(vp.as_path(), Path::new("/test/memory/user/preferences.md"));
}

#[test]
fn valid_feedback_scope_path() {
    let result = validate_memory_path(
        Path::new("corrections.md"),
        Path::new("/test/memory"),
        MemoryScope::Feedback,
    );
    assert!(
        result.is_ok(),
        "simple file in feedback scope should validate"
    );
    let vp = result.expect("checked above");
    assert_eq!(vp.scope(), MemoryScope::Feedback);
}

#[test]
fn valid_project_scope_path() {
    let result = validate_memory_path(
        Path::new("roadmap.md"),
        Path::new("/test/memory"),
        MemoryScope::Project,
    );
    assert!(
        result.is_ok(),
        "simple file in project scope should validate"
    );
    let vp = result.expect("checked above");
    assert_eq!(vp.scope(), MemoryScope::Project);
}

#[test]
fn valid_reference_scope_path() {
    let result = validate_memory_path(
        Path::new("links.md"),
        Path::new("/test/memory"),
        MemoryScope::Reference,
    );
    assert!(
        result.is_ok(),
        "simple file in reference scope should validate"
    );
    let vp = result.expect("checked above");
    assert_eq!(vp.scope(), MemoryScope::Reference);
}

#[test]
fn valid_nested_subdirectory_path() {
    let result = validate_memory_path(
        Path::new("sub/dir/deep.md"),
        Path::new("/test/memory"),
        MemoryScope::Project,
    );
    assert!(
        result.is_ok(),
        "nested subdirectories within scope should validate"
    );
    assert_eq!(
        result.expect("checked above").as_path(),
        Path::new("/test/memory/project/sub/dir/deep.md")
    );
}

#[test]
fn valid_path_with_dots_in_filename() {
    let result = validate_memory_path(
        Path::new("file.backup.2026.md"),
        Path::new("/test/memory"),
        MemoryScope::Project,
    );
    assert!(
        result.is_ok(),
        "dots in filename (not traversal) should validate"
    );
}

#[tokio::test]
async fn validated_path_async_read_round_trip() {
    // WHY: async_read must return the exact bytes previously written via
    // async_write, proving the async I/O gate preserves data integrity.
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    std::fs::create_dir_all(root.join(MemoryScope::Project.as_dir_name())).expect("mkdir scope");

    let vp = validate_memory_path(Path::new("note.md"), root, MemoryScope::Project)
        .expect("valid path should pass validation");

    let data = b"async memory gate contents";
    vp.async_write(data)
        .await
        .expect("async_write should succeed");

    let read_back = vp.async_read().await.expect("async_read should succeed");
    assert_eq!(
        read_back, data,
        "async_read must return async_write payload"
    );
}

#[tokio::test]
async fn validated_path_async_write_creates_parent_dirs() {
    // WHY: memory facts are stored under nested scope subdirectories; the
    // async gate must create them on demand without blocking the runtime.
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    std::fs::create_dir_all(root.join(MemoryScope::User.as_dir_name())).expect("mkdir scope");

    let vp = validate_memory_path(Path::new("deeply/nested/file.md"), root, MemoryScope::User)
        .expect("valid nested path should pass validation");

    vp.async_write(b"nested data")
        .await
        .expect("async_write should create parents");

    let read_back = vp.async_read().await.expect("async_read should succeed");
    assert_eq!(read_back, b"nested data");
}

#[tokio::test]
async fn validated_path_async_read_missing_file_errors() {
    // WHY: callers must receive a standard io::Error when the path does not
    // exist, identical to the sync API contract.
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    std::fs::create_dir_all(root.join(MemoryScope::Reference.as_dir_name())).expect("mkdir scope");

    let vp = validate_memory_path(Path::new("missing.md"), root, MemoryScope::Reference)
        .expect("valid path should pass validation");

    let err = vp
        .async_read()
        .await
        .expect_err("async_read on missing file must error");
    assert_eq!(err.kind(), std::io::ErrorKind::NotFound);
}

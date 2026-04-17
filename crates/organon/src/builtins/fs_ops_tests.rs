#![expect(clippy::expect_used, reason = "test assertions")]
use std::collections::HashSet;
use std::sync::{Arc, RwLock};

use koina::id::{NousId, SessionId, ToolName};

use crate::registry::ToolExecutor as _;
use crate::types::{ToolContext, ToolInput};

use super::*;

fn test_ctx(dir: &std::path::Path) -> ToolContext {
    ToolContext {
        nous_id: NousId::new("alice").expect("valid"),
        session_id: SessionId::new(),
        workspace: dir.to_path_buf(),
        allowed_roots: vec![dir.to_path_buf()],
        services: None,
        active_tools: Arc::new(RwLock::new(HashSet::new())),
        tool_config: Arc::new(taxis::config::ToolLimitsConfig::default()),
    }
}

fn tool_input(name: &str, args: serde_json::Value) -> ToolInput {
    ToolInput {
        name: ToolName::new(name).expect("valid"),
        tool_use_id: "toolu_test".to_owned(),
        arguments: args,
    }
}

fn write_file(path: &std::path::Path, content: &str) {
    #[expect(
        clippy::disallowed_methods,
        reason = "test fixture setup; exercising tool executor contract"
    )]
    std::fs::write(path, content).expect("write fixture");
}

// -------------------------------------------------------------------------
// mkdir
// -------------------------------------------------------------------------

#[tokio::test]
async fn mkdir_creates_directory() {
    let dir = tempfile::tempdir().expect("tmpdir");
    let ctx = test_ctx(dir.path());
    let input = tool_input("mkdir", serde_json::json!({ "path": "acme/reports" }));
    let result = MkdirExecutor.execute(&input, &ctx).await.expect("exec");
    assert!(!result.is_error, "mkdir should succeed");
    assert!(
        dir.path().join("acme/reports").is_dir(),
        "expected nested directory to exist"
    );
}

#[tokio::test]
async fn mkdir_is_idempotent() {
    let dir = tempfile::tempdir().expect("tmpdir");
    std::fs::create_dir(dir.path().join("already")).expect("mkdir");
    let ctx = test_ctx(dir.path());
    let input = tool_input("mkdir", serde_json::json!({ "path": "already" }));
    let result = MkdirExecutor.execute(&input, &ctx).await.expect("exec");
    assert!(
        !result.is_error,
        "mkdir on existing directory should not error"
    );
    assert!(
        result.content.text_summary().contains("already exists"),
        "response should note idempotent no-op"
    );
}

#[tokio::test]
async fn mkdir_rejects_outside_root() {
    let dir = tempfile::tempdir().expect("tmpdir");
    let ctx = test_ctx(dir.path());
    let input = tool_input("mkdir", serde_json::json!({ "path": "/etc/evil" }));
    let err = MkdirExecutor
        .execute(&input, &ctx)
        .await
        .expect_err("outside root must fail");
    assert!(
        err.to_string().contains("outside allowed roots"),
        "error should identify outside-root path"
    );
}

// -------------------------------------------------------------------------
// mv
// -------------------------------------------------------------------------

#[tokio::test]
async fn mv_renames_file() {
    let dir = tempfile::tempdir().expect("tmpdir");
    write_file(&dir.path().join("alice.txt"), "hello");
    let ctx = test_ctx(dir.path());
    let input = tool_input(
        "mv",
        serde_json::json!({ "from": "alice.txt", "to": "bob.txt" }),
    );
    let result = MvExecutor.execute(&input, &ctx).await.expect("exec");
    assert!(!result.is_error, "mv should succeed");
    assert!(
        dir.path().join("bob.txt").exists(),
        "destination should exist after mv"
    );
    assert!(
        !dir.path().join("alice.txt").exists(),
        "source should not exist after mv"
    );
}

#[tokio::test]
async fn mv_missing_source_errors() {
    let dir = tempfile::tempdir().expect("tmpdir");
    let ctx = test_ctx(dir.path());
    let input = tool_input(
        "mv",
        serde_json::json!({ "from": "nope.txt", "to": "yes.txt" }),
    );
    let result = MvExecutor.execute(&input, &ctx).await.expect("exec");
    assert!(result.is_error, "missing source should be an error result");
    assert!(
        result.content.text_summary().contains("source not found"),
        "error should name the issue"
    );
}

#[tokio::test]
async fn mv_refuses_protected_target() {
    let dir = tempfile::tempdir().expect("tmpdir");
    write_file(&dir.path().join("draft.md"), "x");
    let ctx = test_ctx(dir.path());
    let input = tool_input(
        "mv",
        serde_json::json!({ "from": "draft.md", "to": "IDENTITY.md" }),
    );
    let result = MvExecutor.execute(&input, &ctx).await.expect("exec");
    assert!(result.is_error, "moving onto protected path must fail");
}

// -------------------------------------------------------------------------
// cp
// -------------------------------------------------------------------------

#[tokio::test]
async fn cp_copies_file() {
    let dir = tempfile::tempdir().expect("tmpdir");
    write_file(&dir.path().join("alice.txt"), "greetings");
    let ctx = test_ctx(dir.path());
    let input = tool_input(
        "cp",
        serde_json::json!({ "from": "alice.txt", "to": "alice-copy.txt" }),
    );
    let result = CpExecutor.execute(&input, &ctx).await.expect("exec");
    assert!(!result.is_error, "cp should succeed");
    let copied = std::fs::read_to_string(dir.path().join("alice-copy.txt")).expect("read copy");
    assert_eq!(copied, "greetings", "copy should match source contents");
}

#[tokio::test]
async fn cp_directory_requires_recursive_flag() {
    let dir = tempfile::tempdir().expect("tmpdir");
    std::fs::create_dir(dir.path().join("src_dir")).expect("mkdir");
    write_file(&dir.path().join("src_dir/inner.txt"), "nested");
    let ctx = test_ctx(dir.path());
    let input = tool_input(
        "cp",
        serde_json::json!({ "from": "src_dir", "to": "dst_dir" }),
    );
    let result = CpExecutor.execute(&input, &ctx).await.expect("exec");
    assert!(result.is_error, "cp of directory without flag should fail");
    assert!(
        result.content.text_summary().contains("recursive=true"),
        "error should mention the required flag"
    );
}

#[tokio::test]
async fn cp_directory_recursive_copies_contents() {
    let dir = tempfile::tempdir().expect("tmpdir");
    std::fs::create_dir(dir.path().join("src_dir")).expect("mkdir");
    write_file(&dir.path().join("src_dir/inner.txt"), "nested");
    let ctx = test_ctx(dir.path());
    let input = tool_input(
        "cp",
        serde_json::json!({ "from": "src_dir", "to": "dst_dir", "recursive": true }),
    );
    let result = CpExecutor.execute(&input, &ctx).await.expect("exec");
    assert!(!result.is_error, "recursive cp should succeed");
    let nested = std::fs::read_to_string(dir.path().join("dst_dir/inner.txt")).expect("read");
    assert_eq!(nested, "nested", "nested file should be copied");
}

// -------------------------------------------------------------------------
// rm
// -------------------------------------------------------------------------

#[tokio::test]
async fn rm_removes_file() {
    let dir = tempfile::tempdir().expect("tmpdir");
    write_file(&dir.path().join("junk.txt"), "x");
    let ctx = test_ctx(dir.path());
    let input = tool_input("rm", serde_json::json!({ "path": "junk.txt" }));
    let result = RmExecutor.execute(&input, &ctx).await.expect("exec");
    assert!(!result.is_error, "rm should succeed");
    assert!(
        !dir.path().join("junk.txt").exists(),
        "file should be gone after rm"
    );
}

#[tokio::test]
async fn rm_directory_requires_recursive_flag() {
    let dir = tempfile::tempdir().expect("tmpdir");
    std::fs::create_dir(dir.path().join("trash")).expect("mkdir");
    let ctx = test_ctx(dir.path());
    let input = tool_input("rm", serde_json::json!({ "path": "trash" }));
    let result = RmExecutor.execute(&input, &ctx).await.expect("exec");
    assert!(result.is_error, "rm on directory without flag must fail");
}

#[tokio::test]
async fn rm_refuses_protected_path() {
    let dir = tempfile::tempdir().expect("tmpdir");
    write_file(&dir.path().join("IDENTITY.md"), "name: alice");
    let ctx = test_ctx(dir.path());
    let input = tool_input("rm", serde_json::json!({ "path": "IDENTITY.md" }));
    let result = RmExecutor.execute(&input, &ctx).await.expect("exec");
    assert!(result.is_error, "rm of protected file must fail");
    assert!(
        result.content.text_summary().contains("protected"),
        "error should mention protection"
    );
}

#[tokio::test]
async fn rm_missing_path_errors() {
    let dir = tempfile::tempdir().expect("tmpdir");
    let ctx = test_ctx(dir.path());
    let input = tool_input("rm", serde_json::json!({ "path": "nope.txt" }));
    let result = RmExecutor.execute(&input, &ctx).await.expect("exec");
    assert!(result.is_error, "missing path should be an error result");
}

// -------------------------------------------------------------------------
// registration
// -------------------------------------------------------------------------

#[test]
fn all_fs_ops_tools_registered() {
    let mut reg = crate::registry::ToolRegistry::new();
    register(&mut reg).expect("register");
    for name in ["mkdir", "mv", "cp", "rm"] {
        let tn = ToolName::new(name).expect("valid");
        assert!(reg.get_def(&tn).is_some(), "{name} should be registered");
    }
}

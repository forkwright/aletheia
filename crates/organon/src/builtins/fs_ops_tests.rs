#![expect(clippy::expect_used, reason = "test assertions")]
use std::collections::HashSet;
use std::sync::{Arc, RwLock};

use koina::id::{NousId, SessionId, ToolName};

use crate::registry::{ToolExecutor as _, ToolRegistry};
use crate::sandbox::SandboxConfig;
use crate::types::{ToolContext, ToolInput};

use super::*;

fn test_ctx(dir: &std::path::Path) -> ToolContext {
    ToolContext {
        nous_id: NousId::new("alice").expect("valid"),
        session_id: SessionId::new(),
        turn_number: 0,
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

fn registered_mutation_tools() -> ToolRegistry {
    let mut registry = ToolRegistry::new();
    crate::builtins::workspace::register(&mut registry, SandboxConfig::default())
        .expect("register workspace tools");
    register(&mut registry).expect("register fs mutation tools");
    registry
}

#[derive(Clone, Copy)]
enum MutationPathCase {
    Write,
    Append,
    Edit,
    Mkdir,
    Rm,
    MvSource,
    MvDestination,
    CpSource,
    CpDestination,
}

impl MutationPathCase {
    const ALL: [Self; 9] = [
        Self::Write,
        Self::Append,
        Self::Edit,
        Self::Mkdir,
        Self::Rm,
        Self::MvSource,
        Self::MvDestination,
        Self::CpSource,
        Self::CpDestination,
    ];

    fn label(self) -> &'static str {
        match self {
            Self::Write => "write",
            Self::Append => "write append",
            Self::Edit => "edit",
            Self::Mkdir => "mkdir",
            Self::Rm => "rm",
            Self::MvSource => "mv source",
            Self::MvDestination => "mv destination",
            Self::CpSource => "cp source",
            Self::CpDestination => "cp destination",
        }
    }

    fn protected_input(self, protected_path: &str) -> ToolInput {
        match self {
            Self::Write => tool_input(
                "write",
                serde_json::json!({ "path": protected_path, "content": "blocked" }),
            ),
            Self::Append => tool_input(
                "write",
                serde_json::json!({ "path": protected_path, "content": "blocked", "append": true }),
            ),
            Self::Edit => tool_input(
                "edit",
                serde_json::json!({ "path": protected_path, "old_text": "old", "new_text": "new" }),
            ),
            Self::Mkdir => tool_input("mkdir", serde_json::json!({ "path": protected_path })),
            Self::Rm => tool_input("rm", serde_json::json!({ "path": protected_path })),
            Self::MvSource => tool_input(
                "mv",
                serde_json::json!({ "from": protected_path, "to": "safe-move-destination.txt" }),
            ),
            Self::MvDestination => tool_input(
                "mv",
                serde_json::json!({ "from": "safe-source.txt", "to": protected_path }),
            ),
            Self::CpSource => tool_input(
                "cp",
                serde_json::json!({ "from": protected_path, "to": "safe-copy-destination.txt" }),
            ),
            Self::CpDestination => tool_input(
                "cp",
                serde_json::json!({ "from": "safe-source.txt", "to": protected_path }),
            ),
        }
    }
}

#[derive(Clone, Copy)]
enum AllowedMutationCase {
    Write,
    Append,
    Edit,
    Mkdir,
    Rm,
    Mv,
    Cp,
}

impl AllowedMutationCase {
    const ALL: [Self; 7] = [
        Self::Write,
        Self::Append,
        Self::Edit,
        Self::Mkdir,
        Self::Rm,
        Self::Mv,
        Self::Cp,
    ];

    fn label(self) -> &'static str {
        match self {
            Self::Write => "write",
            Self::Append => "write append",
            Self::Edit => "edit",
            Self::Mkdir => "mkdir",
            Self::Rm => "rm",
            Self::Mv => "mv",
            Self::Cp => "cp",
        }
    }

    fn seed(self, root: &std::path::Path) {
        match self {
            Self::Append => write_file(&root.join("journal.txt"), "old"),
            Self::Edit => write_file(&root.join("draft.txt"), "old"),
            Self::Rm => write_file(&root.join("trash.txt"), "trash"),
            Self::Mv => write_file(&root.join("move-source.txt"), "move"),
            Self::Cp => write_file(&root.join("copy-source.txt"), "copy"),
            Self::Write | Self::Mkdir => {}
        }
    }

    fn input(self) -> ToolInput {
        match self {
            Self::Write => tool_input(
                "write",
                serde_json::json!({ "path": "notes.txt", "content": "safe" }),
            ),
            Self::Append => tool_input(
                "write",
                serde_json::json!({ "path": "journal.txt", "content": " text", "append": true }),
            ),
            Self::Edit => tool_input(
                "edit",
                serde_json::json!({ "path": "draft.txt", "old_text": "old", "new_text": "new" }),
            ),
            Self::Mkdir => tool_input("mkdir", serde_json::json!({ "path": "reports/2026" })),
            Self::Rm => tool_input("rm", serde_json::json!({ "path": "trash.txt" })),
            Self::Mv => tool_input(
                "mv",
                serde_json::json!({ "from": "move-source.txt", "to": "move-dest.txt" }),
            ),
            Self::Cp => tool_input(
                "cp",
                serde_json::json!({ "from": "copy-source.txt", "to": "copy-dest.txt" }),
            ),
        }
    }

    fn verify(self, root: &std::path::Path) {
        match self {
            Self::Write => {
                let content = std::fs::read_to_string(root.join("notes.txt")).expect("read notes");
                assert_eq!(content, "safe");
            }
            Self::Append => {
                let content =
                    std::fs::read_to_string(root.join("journal.txt")).expect("read journal");
                assert_eq!(content, "old text");
            }
            Self::Edit => {
                let content = std::fs::read_to_string(root.join("draft.txt")).expect("read draft");
                assert_eq!(content, "new");
            }
            Self::Mkdir => assert!(root.join("reports/2026").is_dir()),
            Self::Rm => assert!(!root.join("trash.txt").exists()),
            Self::Mv => {
                assert!(!root.join("move-source.txt").exists());
                let content =
                    std::fs::read_to_string(root.join("move-dest.txt")).expect("read moved");
                assert_eq!(content, "move");
            }
            Self::Cp => {
                let source =
                    std::fs::read_to_string(root.join("copy-source.txt")).expect("read source");
                let destination =
                    std::fs::read_to_string(root.join("copy-dest.txt")).expect("read copy");
                assert_eq!(source, "copy");
                assert_eq!(destination, "copy");
            }
        }
    }
}

const PROTECTED_PATH_EXAMPLES: &[&str] = &[
    ".env",
    ".env.production",
    ".credentials.json",
    "credentials/anthropic.json",
    "secrets/client.pem",
    "secrets/service.key",
    ".ssh/id_ed25519", // pii-allow: protected-path test fixture (filename, not key material)
    ".ssh/known_hosts",
    ".git/config",
    ".claude/settings.json",
    ".codex/auth.json",
    "config/env",
    "config/aletheia.toml",
];

#[tokio::test]
async fn mutating_tools_reject_shared_protected_paths() {
    let registry = registered_mutation_tools();

    for protected_path in PROTECTED_PATH_EXAMPLES {
        for case in MutationPathCase::ALL {
            let dir = tempfile::tempdir().expect("tmpdir");
            write_file(&dir.path().join("safe-source.txt"), "safe");
            let ctx = test_ctx(dir.path());
            let input = case.protected_input(protected_path);

            let result = registry
                .execute(&input, &ctx)
                .await
                .unwrap_or_else(|err| panic!("{} {protected_path} failed: {err}", case.label()));

            let message = result.content.text_summary();
            assert!(
                result.is_error,
                "{} should reject protected path {protected_path}; got {message}",
                case.label()
            );
            assert!(
                message.contains("protected"),
                "{} error should identify a protected-path policy violation: {message}",
                case.label()
            );
            assert!(
                !message.contains(&dir.path().display().to_string()),
                "{} error must not leak the workspace root: {message}",
                case.label()
            );
        }
    }
}

#[tokio::test]
async fn mutating_tools_allow_non_sensitive_paths() {
    let registry = registered_mutation_tools();

    for case in AllowedMutationCase::ALL {
        let dir = tempfile::tempdir().expect("tmpdir");
        case.seed(dir.path());
        let ctx = test_ctx(dir.path());
        let input = case.input();

        let result = registry
            .execute(&input, &ctx)
            .await
            .unwrap_or_else(|err| panic!("{} should execute: {err}", case.label()));

        assert!(
            !result.is_error,
            "{} should allow non-sensitive paths: {}",
            case.label(),
            result.content.text_summary()
        );
        case.verify(dir.path());
    }
}

#[derive(Clone, Copy)]
enum RecursiveMutationCase {
    Rm,
    Mv,
    Cp,
}

impl RecursiveMutationCase {
    const ALL: [Self; 3] = [Self::Rm, Self::Mv, Self::Cp];

    fn label(self) -> &'static str {
        match self {
            Self::Rm => "rm",
            Self::Mv => "mv",
            Self::Cp => "cp",
        }
    }

    fn input(self) -> ToolInput {
        match self {
            Self::Rm => tool_input(
                "rm",
                serde_json::json!({ "path": "safe-parent", "recursive": true }),
            ),
            Self::Mv => tool_input(
                "mv",
                serde_json::json!({ "from": "safe-parent", "to": "moved-parent" }),
            ),
            Self::Cp => tool_input(
                "cp",
                serde_json::json!({ "from": "safe-parent", "to": "copied-parent", "recursive": true }),
            ),
        }
    }
}

#[tokio::test]
async fn recursive_mutations_reject_protected_descendants() {
    let registry = registered_mutation_tools();

    for case in RecursiveMutationCase::ALL {
        let dir = tempfile::tempdir().expect("tmpdir");
        let protected_parent = dir.path().join("safe-parent/nested");
        std::fs::create_dir_all(&protected_parent).expect("mkdir protected fixture");
        write_file(&protected_parent.join(".env"), "secret=synthetic");

        let ctx = test_ctx(dir.path());
        let input = case.input();

        let result = registry
            .execute(&input, &ctx)
            .await
            .unwrap_or_else(|err| panic!("{} should execute: {err}", case.label()));

        let message = result.content.text_summary();
        assert!(
            result.is_error,
            "{} should reject protected descendants: {message}",
            case.label()
        );
        assert!(
            message.contains("protected"),
            "{} error should identify protected descendant policy: {message}",
            case.label()
        );
        assert!(
            dir.path().join("safe-parent/nested/.env").exists(),
            "{} must leave the protected descendant untouched",
            case.label()
        );
    }
}

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

#[cfg(unix)]
#[tokio::test]
async fn cp_recursive_rejects_symlink() {
    let dir = tempfile::tempdir().expect("tmpdir");
    std::fs::create_dir(dir.path().join("src_dir")).expect("mkdir");
    write_file(&dir.path().join("src_dir/ok.txt"), "ok");
    std::os::unix::fs::symlink(
        dir.path().join("src_dir/ok.txt"),
        dir.path().join("src_dir/link.txt"),
    )
    .expect("symlink");

    let ctx = test_ctx(dir.path());
    let input = tool_input(
        "cp",
        serde_json::json!({ "from": "src_dir", "to": "dst_dir", "recursive": true }),
    );
    let result = CpExecutor.execute(&input, &ctx).await.expect("exec");
    assert!(
        result.is_error,
        "recursive copy containing symlink must fail"
    );
    let msg = result.content.text_summary();
    assert!(
        msg.contains("symlink"),
        "error should name symlink issue: {msg}"
    );
}

#[cfg(unix)]
#[test]
fn revalidate_rejects_swapped_symlink_target() {
    let dir = tempfile::tempdir().expect("tmpdir");
    let allowed = dir.path().join("allowed");
    std::fs::create_dir(&allowed).expect("mkdir allowed");
    let outside = dir.path().join("outside");
    std::fs::create_dir(&outside).expect("mkdir outside");
    write_file(&outside.join("secret.txt"), "secret");

    let mut ctx = test_ctx(dir.path());
    ctx.allowed_roots = vec![allowed.clone()];

    let tool_name = ToolName::new("mv").expect("valid");
    let validated = validate_path("allowed/target.txt", &ctx, &tool_name).expect("valid");
    write_file(&validated, "inside");

    std::fs::remove_file(&validated).expect("remove");
    std::os::unix::fs::symlink(outside.join("secret.txt"), &validated).expect("symlink");

    let err = revalidate_before_mutation(&validated, &ctx, &tool_name).expect_err("must fail");
    assert!(
        err.to_string().contains("outside allowed roots"),
        "swapped symlink target must be rejected: {err}"
    );
}

#[cfg(unix)]
#[test]
fn revalidate_rejects_swapped_destination_parent() {
    let dir = tempfile::tempdir().expect("tmpdir");
    let allowed = dir.path().join("allowed");
    std::fs::create_dir(&allowed).expect("mkdir allowed");
    let outside = dir.path().join("outside");
    std::fs::create_dir(&outside).expect("mkdir outside");
    write_file(&outside.join("anchor.txt"), "anchor");

    let mut ctx = test_ctx(dir.path());
    ctx.allowed_roots = vec![allowed.clone()];

    let tool_name = ToolName::new("cp").expect("valid");
    let validated = validate_path("allowed/subdir/new.txt", &ctx, &tool_name).expect("valid");

    std::fs::remove_dir_all(allowed.join("subdir")).ok();
    std::os::unix::fs::symlink(&outside, allowed.join("subdir")).expect("symlink");

    let err = revalidate_before_mutation(&validated, &ctx, &tool_name).expect_err("must fail");
    assert!(
        err.to_string().contains("outside allowed roots"),
        "swapped destination parent must be rejected: {err}"
    );
}

#[test]
fn revalidate_accepts_unchanged_allowed_path() {
    let dir = tempfile::tempdir().expect("tmpdir");
    write_file(&dir.path().join("stable.txt"), "x");
    let ctx = test_ctx(dir.path());
    let tool_name = ToolName::new("mv").expect("valid");
    let validated = validate_path("stable.txt", &ctx, &tool_name).expect("valid");
    assert!(
        revalidate_before_mutation(&validated, &ctx, &tool_name).is_ok(),
        "unchanged allowed path should remain valid"
    );
}

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

#[test]
fn all_fs_ops_tools_registered() {
    let mut reg = crate::registry::ToolRegistry::new();
    register(&mut reg).expect("register");
    for name in ["mkdir", "mv", "cp", "rm"] {
        let tn = ToolName::new(name).expect("valid");
        assert!(reg.get_def(&tn).is_some(), "{name} should be registered");
    }
}

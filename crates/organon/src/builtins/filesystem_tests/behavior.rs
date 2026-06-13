//! (Split from `filesystem_tests.rs` — see parent mod.)

#![expect(clippy::expect_used, reason = "test assertions")]

use super::super::*;
use super::{find_executor, grep_executor, test_ctx, test_sandbox, tool_input};

#[tokio::test]
async fn grep_finds_pattern() {
    let dir = tempfile::tempdir().expect("tmpdir");
    #[expect(
        clippy::disallowed_methods,
        reason = "organon workspace tools directly implement filesystem operations exposed to agents; synchronous access matches the tool executor contract"
    )]
    std::fs::write(
        dir.path().join("hello.rs"),
        "fn main() {\n    println!(\"hello\");\n}",
    )
    .expect("write");

    let ctx = test_ctx(dir.path());
    let input = tool_input("grep", serde_json::json!({ "pattern": "println" }));
    let result = grep_executor().execute(&input, &ctx).await.expect("exec");
    assert!(!result.is_error, "grep should succeed without error");
    assert!(
        result.content.text_summary().contains("println"),
        "grep output should contain the matched pattern"
    );
}

#[tokio::test]
async fn grep_with_glob_filter() {
    let dir = tempfile::tempdir().expect("tmpdir");
    #[expect(
        clippy::disallowed_methods,
        reason = "organon workspace tools directly implement filesystem operations exposed to agents; synchronous access matches the tool executor contract"
    )]
    std::fs::write(dir.path().join("code.rs"), "fn rust_func() {}").expect("write");
    #[expect(
        clippy::disallowed_methods,
        reason = "organon workspace tools directly implement filesystem operations exposed to agents; synchronous access matches the tool executor contract"
    )]
    std::fs::write(dir.path().join("code.ts"), "function tsFunc() {}").expect("write");

    let ctx = test_ctx(dir.path());
    let input = tool_input(
        "grep",
        serde_json::json!({ "pattern": "func", "glob": "*.rs" }),
    );
    let result = grep_executor().execute(&input, &ctx).await.expect("exec");
    assert!(
        !result.is_error,
        "grep with glob filter should succeed without error"
    );
    let text = result.content.text_summary();
    assert!(
        text.contains("rust_func"),
        "grep should find rust_func in .rs file"
    );
    assert!(
        !text.contains("tsFunc"),
        "grep should not find tsFunc when filtering to .rs files"
    );
}

#[tokio::test]
async fn grep_case_insensitive() {
    let dir = tempfile::tempdir().expect("tmpdir");
    #[expect(
        clippy::disallowed_methods,
        reason = "organon workspace tools directly implement filesystem operations exposed to agents; synchronous access matches the tool executor contract"
    )]
    std::fs::write(dir.path().join("test.txt"), "Hello World\nhello world").expect("write");

    let ctx = test_ctx(dir.path());
    let input = tool_input(
        "grep",
        serde_json::json!({ "pattern": "HELLO", "caseSensitive": false }),
    );
    let result = grep_executor().execute(&input, &ctx).await.expect("exec");
    assert!(
        !result.is_error,
        "case-insensitive grep should succeed without error"
    );
    let text = result.content.text_summary();
    assert!(
        text.contains("Hello"),
        "case-insensitive grep should match Hello"
    );
    assert!(
        text.contains("hello"),
        "case-insensitive grep should match hello"
    );
}

#[tokio::test]
async fn grep_no_matches_not_error() {
    let dir = tempfile::tempdir().expect("tmpdir");
    #[expect(
        clippy::disallowed_methods,
        reason = "organon workspace tools directly implement filesystem operations exposed to agents; synchronous access matches the tool executor contract"
    )]
    std::fs::write(dir.path().join("test.txt"), "nothing here").expect("write");

    let ctx = test_ctx(dir.path());
    let input = tool_input("grep", serde_json::json!({ "pattern": "zzzznotfound" }));
    let result = grep_executor().execute(&input, &ctx).await.expect("exec");
    assert!(
        !result.is_error,
        "grep with no matches should not return an error"
    );
    assert_eq!(
        result.content.text_summary(),
        "No matches found.",
        "grep with no matches should return the standard no-matches message"
    );
}

#[cfg(unix)]
#[test]
fn grep_subprocess_uses_shared_env_policy() {
    use std::os::unix::fs::PermissionsExt as _;

    let _guard = crate::subprocess::SUBPROCESS_ENV_LOCK
        .lock()
        .expect("env lock");
    let dir = tempfile::tempdir().expect("tmpdir");
    let bin_dir = tempfile::tempdir().expect("bindir");
    let fake_rg = bin_dir.path().join("rg");
    #[expect(
        clippy::disallowed_methods,
        reason = "test creates a fake helper binary on disk"
    )]
    std::fs::write(
        &fake_rg,
        "#!/bin/sh\nprintf 'ALETHEIA_TOKEN=%s\\n' \"${ALETHEIA_TOKEN-unset}\"\n",
    )
    .expect("write fake rg");
    std::fs::set_permissions(&fake_rg, std::fs::Permissions::from_mode(0o755))
        .expect("chmod fake rg");

    #[expect(
        unsafe_code,
        reason = "set_var requires unsafe in Rust 2024; test controls env"
    )]
    unsafe {
        std::env::set_var("ALETHEIA_TOKEN", "read-helper-secret");
    }

    let ctx = test_ctx(dir.path());
    let input = tool_input("grep", serde_json::json!({ "pattern": "anything" }));
    let executor = GrepExecutor {
        runner: crate::subprocess::SubprocessRunner::new(test_sandbox()),
        rg_program: fake_rg.into_os_string(),
        grep_program: "grep".into(),
    };
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");
    let result = runtime
        .block_on(executor.execute(&input, &ctx))
        .expect("exec");

    #[expect(
        unsafe_code,
        reason = "remove_var requires unsafe in Rust 2024; test cleanup"
    )]
    unsafe {
        std::env::remove_var("ALETHEIA_TOKEN");
    }

    let text = result.content.text_summary();
    assert!(
        text.contains("ALETHEIA_TOKEN=unset"),
        "fake rg should observe the sanitized child environment: {text}"
    );
    assert!(
        !text.contains("read-helper-secret"),
        "read helper subprocess must not inherit sensitive env"
    );
}

#[tokio::test]
async fn find_locates_files() {
    let dir = tempfile::tempdir().expect("tmpdir");
    #[expect(
        clippy::disallowed_methods,
        reason = "organon workspace tools directly implement filesystem operations exposed to agents; synchronous access matches the tool executor contract"
    )]
    std::fs::write(dir.path().join("app.rs"), "").expect("write");
    #[expect(
        clippy::disallowed_methods,
        reason = "organon workspace tools directly implement filesystem operations exposed to agents; synchronous access matches the tool executor contract"
    )]
    std::fs::write(dir.path().join("app.ts"), "").expect("write");

    let ctx = test_ctx(dir.path());
    let input = tool_input("find", serde_json::json!({ "pattern": "app" }));
    let result = find_executor().execute(&input, &ctx).await.expect("exec");
    assert!(!result.is_error, "find should succeed without error");
    let text = result.content.text_summary();
    assert!(
        text.contains("app"),
        "find output should contain files matching the pattern"
    );
}

#[tokio::test]
async fn find_type_filter() {
    let dir = tempfile::tempdir().expect("tmpdir");
    std::fs::create_dir(dir.path().join("subdir")).expect("mkdir");
    #[expect(
        clippy::disallowed_methods,
        reason = "organon workspace tools directly implement filesystem operations exposed to agents; synchronous access matches the tool executor contract"
    )]
    std::fs::write(dir.path().join("file.txt"), "").expect("write");

    let ctx = test_ctx(dir.path());
    let input = tool_input("find", serde_json::json!({ "pattern": ".", "type": "d" }));
    let result = find_executor().execute(&input, &ctx).await.expect("exec");
    assert!(
        !result.is_error,
        "find with type filter should succeed without error"
    );
    let text = result.content.text_summary();
    assert!(
        text.contains("subdir"),
        "find with directory type filter should include the subdirectory"
    );
}

#[tokio::test]
async fn find_max_depth() {
    let dir = tempfile::tempdir().expect("tmpdir");
    let deep = dir.path().join("a").join("b").join("c");
    std::fs::create_dir_all(&deep).expect("mkdirs");
    #[expect(
        clippy::disallowed_methods,
        reason = "organon workspace tools directly implement filesystem operations exposed to agents; synchronous access matches the tool executor contract"
    )]
    std::fs::write(deep.join("deep.txt"), "").expect("write");
    #[expect(
        clippy::disallowed_methods,
        reason = "organon workspace tools directly implement filesystem operations exposed to agents; synchronous access matches the tool executor contract"
    )]
    std::fs::write(dir.path().join("shallow.txt"), "").expect("write");

    let ctx = test_ctx(dir.path());
    let input = tool_input(
        "find",
        serde_json::json!({ "pattern": "txt", "maxDepth": 1 }),
    );
    let result = find_executor().execute(&input, &ctx).await.expect("exec");
    assert!(
        !result.is_error,
        "find with max depth should succeed without error"
    );
    let text = result.content.text_summary();
    assert!(
        text.contains("shallow"),
        "find should include shallow file within max depth"
    );
    assert!(
        !text.contains("deep"),
        "find should not include deeply nested file beyond max depth"
    );
}

#[tokio::test]
async fn ls_lists_directory() {
    let dir = tempfile::tempdir().expect("tmpdir");
    #[expect(
        clippy::disallowed_methods,
        reason = "organon workspace tools directly implement filesystem operations exposed to agents; synchronous access matches the tool executor contract"
    )]
    std::fs::write(dir.path().join("file.txt"), "content").expect("write");
    std::fs::create_dir(dir.path().join("subdir")).expect("mkdir");

    let ctx = test_ctx(dir.path());
    let input = tool_input("ls", serde_json::json!({}));
    let result = LsExecutor.execute(&input, &ctx).await.expect("exec");
    assert!(!result.is_error, "ls should succeed without error");
    let text = result.content.text_summary();
    assert!(
        text.contains("subdir/"),
        "ls output should include the subdirectory"
    );
    assert!(
        text.contains("file.txt"),
        "ls output should include the file"
    );
}

#[tokio::test]
async fn ls_hides_dotfiles() {
    let dir = tempfile::tempdir().expect("tmpdir");
    #[expect(
        clippy::disallowed_methods,
        reason = "organon workspace tools directly implement filesystem operations exposed to agents; synchronous access matches the tool executor contract"
    )]
    std::fs::write(dir.path().join(".hidden"), "secret").expect("write");
    #[expect(
        clippy::disallowed_methods,
        reason = "organon workspace tools directly implement filesystem operations exposed to agents; synchronous access matches the tool executor contract"
    )]
    std::fs::write(dir.path().join("visible.txt"), "public").expect("write");

    let ctx = test_ctx(dir.path());
    let input = tool_input("ls", serde_json::json!({}));
    let result = LsExecutor.execute(&input, &ctx).await.expect("exec");
    assert!(!result.is_error, "ls should succeed without error");
    let text = result.content.text_summary();
    assert!(
        !text.contains(".hidden"),
        "ls should not show dotfiles by default"
    );
    assert!(
        text.contains("visible.txt"),
        "ls should show non-hidden files"
    );
}

#[tokio::test]
async fn ls_shows_dotfiles_with_all() {
    let dir = tempfile::tempdir().expect("tmpdir");
    #[expect(
        clippy::disallowed_methods,
        reason = "organon workspace tools directly implement filesystem operations exposed to agents; synchronous access matches the tool executor contract"
    )]
    std::fs::write(dir.path().join(".hidden"), "secret").expect("write");
    #[expect(
        clippy::disallowed_methods,
        reason = "organon workspace tools directly implement filesystem operations exposed to agents; synchronous access matches the tool executor contract"
    )]
    std::fs::write(dir.path().join("visible.txt"), "public").expect("write");

    let ctx = test_ctx(dir.path());
    let input = tool_input("ls", serde_json::json!({ "all": true }));
    let result = LsExecutor.execute(&input, &ctx).await.expect("exec");
    assert!(
        !result.is_error,
        "ls with all flag should succeed without error"
    );
    let text = result.content.text_summary();
    assert!(
        text.contains(".hidden"),
        "ls with all flag should show dotfiles"
    );
    assert!(
        text.contains("visible.txt"),
        "ls with all flag should still show non-hidden files"
    );
}

#[tokio::test]
async fn ls_dirs_sorted_before_files() {
    let dir = tempfile::tempdir().expect("tmpdir");
    #[expect(
        clippy::disallowed_methods,
        reason = "organon workspace tools directly implement filesystem operations exposed to agents; synchronous access matches the tool executor contract"
    )]
    std::fs::write(dir.path().join("zebra.txt"), "").expect("write");
    std::fs::create_dir(dir.path().join("alpha")).expect("mkdir");

    let ctx = test_ctx(dir.path());
    let input = tool_input("ls", serde_json::json!({}));
    let result = LsExecutor.execute(&input, &ctx).await.expect("exec");
    let text = result.content.text_summary();
    let alpha_pos = text.find("alpha/").expect("alpha/ present");
    let zebra_pos = text.find("zebra.txt").expect("zebra.txt present");
    assert!(
        alpha_pos < zebra_pos,
        "directories should sort before files"
    );
}

#[tokio::test]
async fn path_validation_rejects_outside_roots() {
    let dir = tempfile::tempdir().expect("tmpdir");
    let ctx = test_ctx(dir.path());
    let input = tool_input(
        "grep",
        serde_json::json!({ "pattern": "x", "path": "/etc" }),
    );
    let err = grep_executor()
        .execute(&input, &ctx)
        .await
        .expect_err("should reject outside root");
    assert!(
        err.to_string().contains("outside allowed roots"),
        "error should indicate path is outside allowed roots"
    );
}

#[tokio::test]
async fn all_tools_registered() {
    let mut reg = crate::registry::ToolRegistry::new();
    register(&mut reg).expect("register");

    for name in ["grep", "find", "ls"] {
        let tn = ToolName::new(name).expect("valid");
        assert!(reg.get_def(&tn).is_some(), "{name} should be registered");
    }
}

#[tokio::test]
async fn test_grep_when_pattern_argument_missing_returns_error() {
    let dir = tempfile::tempdir().expect("tmpdir");
    let ctx = test_ctx(dir.path());
    let input = tool_input("grep", serde_json::json!({}));
    let err = grep_executor()
        .execute(&input, &ctx)
        .await
        .expect_err("missing pattern should error");
    assert!(
        err.to_string().contains("missing or invalid field"),
        "error should indicate the pattern field is missing or invalid"
    );
}

#[tokio::test]
async fn test_find_when_pattern_argument_missing_returns_error() {
    let dir = tempfile::tempdir().expect("tmpdir");
    let ctx = test_ctx(dir.path());
    let input = tool_input("find", serde_json::json!({}));
    let err = find_executor()
        .execute(&input, &ctx)
        .await
        .expect_err("missing pattern should error");
    assert!(
        err.to_string().contains("missing or invalid field"),
        "error should indicate the pattern field is missing or invalid"
    );
}

#[tokio::test]
async fn test_grep_max_results_limits_output_lines() {
    let dir = tempfile::tempdir().expect("tmpdir");
    let content = "match1\nmatch2\nmatch3\nmatch4\nmatch5\nmatch6\nmatch7\nmatch8\n\
                   match9\nmatch10\nmatch11\nmatch12\nmatch13\nmatch14\nmatch15\nmatch16\n\
                   match17\nmatch18\nmatch19\nmatch20\n"
        .to_owned();
    #[expect(
        clippy::disallowed_methods,
        reason = "organon workspace tools directly implement filesystem operations exposed to agents; synchronous access matches the tool executor contract"
    )]
    std::fs::write(dir.path().join("big.txt"), &content).expect("write");

    let ctx = test_ctx(dir.path());
    let input = tool_input(
        "grep",
        serde_json::json!({ "pattern": "match", "maxResults": 5 }),
    );
    let result = grep_executor().execute(&input, &ctx).await.expect("exec");
    assert!(
        !result.is_error,
        "grep with maxResults should succeed without error"
    );
    let text = result.content.text_summary();
    let match_count = text.lines().count();
    assert!(
        match_count <= 5,
        "expected at most 5 lines, got {match_count}"
    );
}

#[tokio::test]
async fn test_grep_case_sensitive_does_not_match_wrong_case() {
    let dir = tempfile::tempdir().expect("tmpdir");
    #[expect(
        clippy::disallowed_methods,
        reason = "organon workspace tools directly implement filesystem operations exposed to agents; synchronous access matches the tool executor contract"
    )]
    std::fs::write(dir.path().join("f.txt"), "HELLO world").expect("write");

    let ctx = test_ctx(dir.path());
    let input = tool_input(
        "grep",
        serde_json::json!({ "pattern": "hello", "caseSensitive": true }),
    );
    let result = grep_executor().execute(&input, &ctx).await.expect("exec");
    assert!(
        !result.is_error,
        "case-sensitive grep should succeed without error"
    );
    assert_eq!(
        result.content.text_summary(),
        "No matches found.",
        "case-sensitive search should not match HELLO with hello"
    );
}

#[tokio::test]
async fn test_grep_returns_error_result_for_invalid_path_outside_roots() {
    let dir = tempfile::tempdir().expect("tmpdir");
    let ctx = test_ctx(dir.path());
    let input = tool_input(
        "grep",
        serde_json::json!({ "pattern": "x", "path": "/root/secret" }),
    );
    let err = grep_executor()
        .execute(&input, &ctx)
        .await
        .expect_err("outside root should fail");
    assert!(
        err.to_string().contains("outside allowed roots"),
        "error should indicate path is outside allowed roots"
    );
}

#[tokio::test]
async fn test_find_empty_results_returns_not_error_message() {
    let dir = tempfile::tempdir().expect("tmpdir");
    let ctx = test_ctx(dir.path());
    let input = tool_input("find", serde_json::json!({ "pattern": "zzz_never_exists" }));
    let result = find_executor().execute(&input, &ctx).await.expect("exec");
    assert!(
        !result.is_error,
        "find with no matches should not return an error"
    );
    assert_eq!(
        result.content.text_summary(),
        "No files found.",
        "find with no matches should return the standard no-files message"
    );
}

#[tokio::test]
async fn test_find_glob_extension_filter_matches_correctly() {
    let dir = tempfile::tempdir().expect("tmpdir");
    #[expect(
        clippy::disallowed_methods,
        reason = "organon workspace tools directly implement filesystem operations exposed to agents; synchronous access matches the tool executor contract"
    )]
    std::fs::write(dir.path().join("main.rs"), "").expect("write");
    #[expect(
        clippy::disallowed_methods,
        reason = "organon workspace tools directly implement filesystem operations exposed to agents; synchronous access matches the tool executor contract"
    )]
    std::fs::write(dir.path().join("main.py"), "").expect("write");
    #[expect(
        clippy::disallowed_methods,
        reason = "organon workspace tools directly implement filesystem operations exposed to agents; synchronous access matches the tool executor contract"
    )]
    std::fs::write(dir.path().join("lib.rs"), "").expect("write");

    let ctx = test_ctx(dir.path());
    let input = tool_input("find", serde_json::json!({ "pattern": "*.rs" }));
    let result = find_executor().execute(&input, &ctx).await.expect("exec");
    assert!(
        !result.is_error,
        "find with glob extension filter should succeed without error"
    );
    let text = result.content.text_summary();
    assert!(text.contains(".rs"), "should find .rs files");
}

#[tokio::test]
async fn test_find_max_results_limits_output() {
    let dir = tempfile::tempdir().expect("tmpdir");
    for i in 0..10 {
        #[expect(
            clippy::disallowed_methods,
            reason = "organon workspace tools directly implement filesystem operations exposed to agents; synchronous access matches the tool executor contract"
        )]
        std::fs::write(dir.path().join(format!("file{i}.txt")), "").expect("write");
    }

    let ctx = test_ctx(dir.path());
    let input = tool_input(
        "find",
        serde_json::json!({ "pattern": "file", "maxResults": 3 }),
    );
    let result = find_executor().execute(&input, &ctx).await.expect("exec");
    assert!(
        !result.is_error,
        "find with maxResults should succeed without error"
    );
    let text = result.content.text_summary();
    let line_count = text.lines().count();
    assert!(
        line_count <= 3,
        "expected at most 3 results, got {line_count}"
    );
}

#[tokio::test]
async fn test_ls_nonexistent_directory_returns_error_result() {
    let dir = tempfile::tempdir().expect("tmpdir");
    let ctx = test_ctx(dir.path());
    let input = tool_input(
        "ls",
        serde_json::json!({ "path": dir.path().join("ghost").to_string_lossy().as_ref() }),
    );
    let result = LsExecutor.execute(&input, &ctx).await.expect("exec");
    assert!(
        result.is_error,
        "ls on nonexistent directory should return an error result"
    );
    assert!(
        result
            .content
            .text_summary()
            .contains("cannot read directory"),
        "error message should indicate the directory cannot be read"
    );
}

#[tokio::test]
async fn test_ls_empty_directory_returns_descriptive_message() {
    let dir = tempfile::tempdir().expect("tmpdir");
    let ctx = test_ctx(dir.path());
    let input = tool_input("ls", serde_json::json!({}));
    let result = LsExecutor.execute(&input, &ctx).await.expect("exec");
    assert!(
        !result.is_error,
        "ls on empty directory should succeed without error"
    );
    assert_eq!(
        result.content.text_summary(),
        "Directory is empty.",
        "ls on empty directory should return the standard empty message"
    );
}

#[tokio::test]
async fn test_ls_output_includes_file_size() {
    let dir = tempfile::tempdir().expect("tmpdir");
    #[expect(
        clippy::disallowed_methods,
        reason = "organon workspace tools directly implement filesystem operations exposed to agents; synchronous access matches the tool executor contract"
    )]
    std::fs::write(dir.path().join("sized.txt"), "12345").expect("write");
    let ctx = test_ctx(dir.path());
    let input = tool_input("ls", serde_json::json!({}));
    let result = LsExecutor.execute(&input, &ctx).await.expect("exec");
    assert!(!result.is_error, "ls should succeed without error");
    let text = result.content.text_summary();
    assert!(text.contains('5'), "should show file size 5");
}

#[tokio::test]
async fn test_ls_output_includes_date_column() {
    let dir = tempfile::tempdir().expect("tmpdir");
    #[expect(
        clippy::disallowed_methods,
        reason = "organon workspace tools directly implement filesystem operations exposed to agents; synchronous access matches the tool executor contract"
    )]
    std::fs::write(dir.path().join("dated.txt"), "content").expect("write");
    let ctx = test_ctx(dir.path());
    let input = tool_input("ls", serde_json::json!({}));
    let result = LsExecutor.execute(&input, &ctx).await.expect("exec");
    assert!(!result.is_error, "ls should succeed without error");
    let text = result.content.text_summary();
    assert!(text.contains('-'), "should show date with hyphens");
    assert!(text.contains(':'), "should show time with colon");
}

#[tokio::test]
async fn test_ls_uses_workspace_when_path_not_specified() {
    let dir = tempfile::tempdir().expect("tmpdir");
    #[expect(
        clippy::disallowed_methods,
        reason = "organon workspace tools directly implement filesystem operations exposed to agents; synchronous access matches the tool executor contract"
    )]
    std::fs::write(dir.path().join("sentinel.txt"), "").expect("write");
    let ctx = test_ctx(dir.path());
    let input = tool_input("ls", serde_json::json!({}));
    let result = LsExecutor.execute(&input, &ctx).await.expect("exec");
    assert!(
        !result.is_error,
        "ls without explicit path should succeed without error"
    );
    assert!(
        result.content.text_summary().contains("sentinel.txt"),
        "ls without explicit path should list workspace contents"
    );
}

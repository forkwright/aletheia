#![expect(clippy::expect_used, reason = "test assertions")]
use std::collections::HashSet;
use std::sync::{Arc, RwLock};

use aletheia_koina::id::{NousId, SessionId, ToolName};

use super::*;

fn test_ctx(dir: &Path) -> ToolContext {
    ToolContext {
        nous_id: NousId::new("test-agent").expect("valid"),
        session_id: SessionId::new(),
        workspace: dir.to_path_buf(),
        allowed_roots: vec![dir.to_path_buf()],
        services: None,
        active_tools: Arc::new(RwLock::new(HashSet::new())),
    }
}

fn tool_input(name: &str, args: serde_json::Value) -> ToolInput {
    ToolInput {
        name: ToolName::new(name).expect("valid"),
        tool_use_id: "toolu_test".to_owned(),
        arguments: args,
    }
}

// -- GrepExecutor -------------------------------------------------------

#[tokio::test]
async fn grep_finds_pattern() {
    let dir = tempfile::tempdir().expect("tmpdir");
    std::fs::write(
        dir.path().join("hello.rs"),
        "fn main() {\n    println!(\"hello\");\n}",
    )
    .expect("write");

    let ctx = test_ctx(dir.path());
    let input = tool_input("grep", serde_json::json!({ "pattern": "println" }));
    let result = GrepExecutor.execute(&input, &ctx).await.expect("exec");
    assert!(!result.is_error);
    assert!(result.content.text_summary().contains("println"));
}

#[tokio::test]
async fn grep_with_glob_filter() {
    let dir = tempfile::tempdir().expect("tmpdir");
    std::fs::write(dir.path().join("code.rs"), "fn rust_func() {}").expect("write");
    std::fs::write(dir.path().join("code.ts"), "function tsFunc() {}").expect("write");

    let ctx = test_ctx(dir.path());
    let input = tool_input(
        "grep",
        serde_json::json!({ "pattern": "func", "glob": "*.rs" }),
    );
    let result = GrepExecutor.execute(&input, &ctx).await.expect("exec");
    assert!(!result.is_error);
    let text = result.content.text_summary();
    assert!(text.contains("rust_func"));
    assert!(!text.contains("tsFunc"));
}

#[tokio::test]
async fn grep_case_insensitive() {
    let dir = tempfile::tempdir().expect("tmpdir");
    std::fs::write(dir.path().join("test.txt"), "Hello World\nhello world").expect("write");

    let ctx = test_ctx(dir.path());
    let input = tool_input(
        "grep",
        serde_json::json!({ "pattern": "HELLO", "caseSensitive": false }),
    );
    let result = GrepExecutor.execute(&input, &ctx).await.expect("exec");
    assert!(!result.is_error);
    let text = result.content.text_summary();
    assert!(text.contains("Hello"));
    assert!(text.contains("hello"));
}

#[tokio::test]
async fn grep_no_matches_not_error() {
    let dir = tempfile::tempdir().expect("tmpdir");
    std::fs::write(dir.path().join("test.txt"), "nothing here").expect("write");

    let ctx = test_ctx(dir.path());
    let input = tool_input("grep", serde_json::json!({ "pattern": "zzzznotfound" }));
    let result = GrepExecutor.execute(&input, &ctx).await.expect("exec");
    assert!(!result.is_error);
    assert_eq!(result.content.text_summary(), "No matches found.");
}

// -- FindExecutor -------------------------------------------------------

#[tokio::test]
async fn find_locates_files() {
    let dir = tempfile::tempdir().expect("tmpdir");
    std::fs::write(dir.path().join("app.rs"), "").expect("write");
    std::fs::write(dir.path().join("app.ts"), "").expect("write");

    let ctx = test_ctx(dir.path());
    let input = tool_input("find", serde_json::json!({ "pattern": "app" }));
    let result = FindExecutor.execute(&input, &ctx).await.expect("exec");
    assert!(!result.is_error);
    let text = result.content.text_summary();
    assert!(text.contains("app"));
}

#[tokio::test]
async fn find_type_filter() {
    let dir = tempfile::tempdir().expect("tmpdir");
    std::fs::create_dir(dir.path().join("subdir")).expect("mkdir");
    std::fs::write(dir.path().join("file.txt"), "").expect("write");

    let ctx = test_ctx(dir.path());
    let input = tool_input("find", serde_json::json!({ "pattern": ".", "type": "d" }));
    let result = FindExecutor.execute(&input, &ctx).await.expect("exec");
    assert!(!result.is_error);
    let text = result.content.text_summary();
    assert!(text.contains("subdir"));
}

#[tokio::test]
async fn find_max_depth() {
    let dir = tempfile::tempdir().expect("tmpdir");
    let deep = dir.path().join("a").join("b").join("c");
    std::fs::create_dir_all(&deep).expect("mkdirs");
    std::fs::write(deep.join("deep.txt"), "").expect("write");
    std::fs::write(dir.path().join("shallow.txt"), "").expect("write");

    let ctx = test_ctx(dir.path());
    let input = tool_input(
        "find",
        serde_json::json!({ "pattern": "txt", "maxDepth": 1 }),
    );
    let result = FindExecutor.execute(&input, &ctx).await.expect("exec");
    assert!(!result.is_error);
    let text = result.content.text_summary();
    assert!(text.contains("shallow"));
    assert!(!text.contains("deep"));
}

// -- LsExecutor ---------------------------------------------------------

#[tokio::test]
async fn ls_lists_directory() {
    let dir = tempfile::tempdir().expect("tmpdir");
    std::fs::write(dir.path().join("file.txt"), "content").expect("write");
    std::fs::create_dir(dir.path().join("subdir")).expect("mkdir");

    let ctx = test_ctx(dir.path());
    let input = tool_input("ls", serde_json::json!({}));
    let result = LsExecutor.execute(&input, &ctx).await.expect("exec");
    assert!(!result.is_error);
    let text = result.content.text_summary();
    assert!(text.contains("subdir/"));
    assert!(text.contains("file.txt"));
}

#[tokio::test]
async fn ls_hides_dotfiles() {
    let dir = tempfile::tempdir().expect("tmpdir");
    std::fs::write(dir.path().join(".hidden"), "secret").expect("write");
    std::fs::write(dir.path().join("visible.txt"), "public").expect("write");

    let ctx = test_ctx(dir.path());
    let input = tool_input("ls", serde_json::json!({}));
    let result = LsExecutor.execute(&input, &ctx).await.expect("exec");
    assert!(!result.is_error);
    let text = result.content.text_summary();
    assert!(!text.contains(".hidden"));
    assert!(text.contains("visible.txt"));
}

#[tokio::test]
async fn ls_shows_dotfiles_with_all() {
    let dir = tempfile::tempdir().expect("tmpdir");
    std::fs::write(dir.path().join(".hidden"), "secret").expect("write");
    std::fs::write(dir.path().join("visible.txt"), "public").expect("write");

    let ctx = test_ctx(dir.path());
    let input = tool_input("ls", serde_json::json!({ "all": true }));
    let result = LsExecutor.execute(&input, &ctx).await.expect("exec");
    assert!(!result.is_error);
    let text = result.content.text_summary();
    assert!(text.contains(".hidden"));
    assert!(text.contains("visible.txt"));
}

#[tokio::test]
async fn ls_dirs_sorted_before_files() {
    let dir = tempfile::tempdir().expect("tmpdir");
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

// -- Path validation ----------------------------------------------------

#[tokio::test]
async fn path_validation_rejects_outside_roots() {
    let dir = tempfile::tempdir().expect("tmpdir");
    let ctx = test_ctx(dir.path());
    let input = tool_input(
        "grep",
        serde_json::json!({ "pattern": "x", "path": "/etc" }),
    );
    let err = GrepExecutor
        .execute(&input, &ctx)
        .await
        .expect_err("should reject outside root");
    assert!(err.to_string().contains("outside allowed roots"));
}

// -- Registration -------------------------------------------------------

#[tokio::test]
async fn all_tools_registered() {
    let mut reg = crate::registry::ToolRegistry::new();
    register(&mut reg).expect("register");

    for name in ["grep", "find", "ls"] {
        let tn = ToolName::new(name).expect("valid");
        assert!(reg.get_def(&tn).is_some(), "{name} should be registered");
    }
}

// -- Parameter validation -----------------------------------------------

#[tokio::test]
async fn test_grep_when_pattern_argument_missing_returns_error() {
    let dir = tempfile::tempdir().expect("tmpdir");
    let ctx = test_ctx(dir.path());
    let input = tool_input("grep", serde_json::json!({}));
    let err = GrepExecutor
        .execute(&input, &ctx)
        .await
        .expect_err("missing pattern should error");
    assert!(err.to_string().contains("missing or invalid field"));
}

#[tokio::test]
async fn test_find_when_pattern_argument_missing_returns_error() {
    let dir = tempfile::tempdir().expect("tmpdir");
    let ctx = test_ctx(dir.path());
    let input = tool_input("find", serde_json::json!({}));
    let err = FindExecutor
        .execute(&input, &ctx)
        .await
        .expect_err("missing pattern should error");
    assert!(err.to_string().contains("missing or invalid field"));
}

// -- Grep result formatting ---------------------------------------------

#[tokio::test]
async fn test_grep_max_results_limits_output_lines() {
    let dir = tempfile::tempdir().expect("tmpdir");
    let content = "match1\nmatch2\nmatch3\nmatch4\nmatch5\nmatch6\nmatch7\nmatch8\n\
                   match9\nmatch10\nmatch11\nmatch12\nmatch13\nmatch14\nmatch15\nmatch16\n\
                   match17\nmatch18\nmatch19\nmatch20\n"
        .to_owned();
    std::fs::write(dir.path().join("big.txt"), &content).expect("write");

    let ctx = test_ctx(dir.path());
    let input = tool_input(
        "grep",
        serde_json::json!({ "pattern": "match", "maxResults": 5 }),
    );
    let result = GrepExecutor.execute(&input, &ctx).await.expect("exec");
    assert!(!result.is_error);
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
    std::fs::write(dir.path().join("f.txt"), "HELLO world").expect("write");

    let ctx = test_ctx(dir.path());
    let input = tool_input(
        "grep",
        serde_json::json!({ "pattern": "hello", "caseSensitive": true }),
    );
    let result = GrepExecutor.execute(&input, &ctx).await.expect("exec");
    assert!(!result.is_error);
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
    let err = GrepExecutor
        .execute(&input, &ctx)
        .await
        .expect_err("outside root should fail");
    assert!(err.to_string().contains("outside allowed roots"));
}

// -- Find result formatting ---------------------------------------------

#[tokio::test]
async fn test_find_empty_results_returns_not_error_message() {
    let dir = tempfile::tempdir().expect("tmpdir");
    let ctx = test_ctx(dir.path());
    let input = tool_input("find", serde_json::json!({ "pattern": "zzz_never_exists" }));
    let result = FindExecutor.execute(&input, &ctx).await.expect("exec");
    assert!(!result.is_error);
    assert_eq!(result.content.text_summary(), "No files found.");
}

#[tokio::test]
async fn test_find_glob_extension_filter_matches_correctly() {
    let dir = tempfile::tempdir().expect("tmpdir");
    std::fs::write(dir.path().join("main.rs"), "").expect("write");
    std::fs::write(dir.path().join("main.py"), "").expect("write");
    std::fs::write(dir.path().join("lib.rs"), "").expect("write");

    let ctx = test_ctx(dir.path());
    let input = tool_input("find", serde_json::json!({ "pattern": "*.rs" }));
    let result = FindExecutor.execute(&input, &ctx).await.expect("exec");
    assert!(!result.is_error);
    let text = result.content.text_summary();
    assert!(text.contains(".rs"), "should find .rs files");
}

#[tokio::test]
async fn test_find_max_results_limits_output() {
    let dir = tempfile::tempdir().expect("tmpdir");
    for i in 0..10 {
        std::fs::write(dir.path().join(format!("file{i}.txt")), "").expect("write");
    }

    let ctx = test_ctx(dir.path());
    let input = tool_input(
        "find",
        serde_json::json!({ "pattern": "file", "maxResults": 3 }),
    );
    let result = FindExecutor.execute(&input, &ctx).await.expect("exec");
    assert!(!result.is_error);
    let text = result.content.text_summary();
    let line_count = text.lines().count();
    assert!(
        line_count <= 3,
        "expected at most 3 results, got {line_count}"
    );
}

// -- Ls result formatting -----------------------------------------------

#[tokio::test]
async fn test_ls_nonexistent_directory_returns_error_result() {
    let dir = tempfile::tempdir().expect("tmpdir");
    let ctx = test_ctx(dir.path());
    let input = tool_input(
        "ls",
        serde_json::json!({ "path": dir.path().join("ghost").to_string_lossy().as_ref() }),
    );
    let result = LsExecutor.execute(&input, &ctx).await.expect("exec");
    assert!(result.is_error);
    assert!(
        result
            .content
            .text_summary()
            .contains("cannot read directory")
    );
}

#[tokio::test]
async fn test_ls_empty_directory_returns_descriptive_message() {
    let dir = tempfile::tempdir().expect("tmpdir");
    let ctx = test_ctx(dir.path());
    let input = tool_input("ls", serde_json::json!({}));
    let result = LsExecutor.execute(&input, &ctx).await.expect("exec");
    assert!(!result.is_error);
    assert_eq!(result.content.text_summary(), "Directory is empty.");
}

#[tokio::test]
async fn test_ls_output_includes_file_size() {
    let dir = tempfile::tempdir().expect("tmpdir");
    std::fs::write(dir.path().join("sized.txt"), "12345").expect("write");
    let ctx = test_ctx(dir.path());
    let input = tool_input("ls", serde_json::json!({}));
    let result = LsExecutor.execute(&input, &ctx).await.expect("exec");
    assert!(!result.is_error);
    let text = result.content.text_summary();
    assert!(text.contains('5'), "should show file size 5");
}

#[tokio::test]
async fn test_ls_output_includes_date_column() {
    let dir = tempfile::tempdir().expect("tmpdir");
    std::fs::write(dir.path().join("dated.txt"), "content").expect("write");
    let ctx = test_ctx(dir.path());
    let input = tool_input("ls", serde_json::json!({}));
    let result = LsExecutor.execute(&input, &ctx).await.expect("exec");
    assert!(!result.is_error);
    let text = result.content.text_summary();
    // Date format: YYYY-MM-DD HH:MM
    assert!(text.contains('-'), "should show date with hyphens");
    assert!(text.contains(':'), "should show time with colon");
}

#[tokio::test]
async fn test_ls_uses_workspace_when_path_not_specified() {
    let dir = tempfile::tempdir().expect("tmpdir");
    std::fs::write(dir.path().join("sentinel.txt"), "").expect("write");
    let ctx = test_ctx(dir.path());
    let input = tool_input("ls", serde_json::json!({}));
    let result = LsExecutor.execute(&input, &ctx).await.expect("exec");
    assert!(!result.is_error);
    assert!(result.content.text_summary().contains("sentinel.txt"));
}

// -- Helper function unit tests -----------------------------------------

#[test]
fn test_is_glob_pattern_detects_star() {
    assert!(is_glob_pattern("*.rs"));
}

#[test]
fn test_is_glob_pattern_detects_question_mark() {
    assert!(is_glob_pattern("file?.txt"));
}

#[test]
fn test_is_glob_pattern_detects_brackets() {
    assert!(is_glob_pattern("[abc]def"));
}

#[test]
fn test_is_glob_pattern_detects_braces() {
    assert!(is_glob_pattern("{foo,bar}"));
}

#[test]
fn test_is_glob_pattern_returns_false_for_plain_word() {
    assert!(!is_glob_pattern("hello"));
    assert!(!is_glob_pattern("file.txt"));
    assert!(!is_glob_pattern("some_identifier"));
}

#[test]
fn test_days_to_ymd_known_epoch_gives_1970_01_01() {
    let (y, m, d) = days_to_ymd(0);
    assert_eq!((y, m, d), (1970, 1, 1));
}

#[test]
fn test_days_to_ymd_365_days_gives_1971_01_01() {
    // 1970 was not a leap year, so day 365 = Jan 1, 1971
    let (y, m, d) = days_to_ymd(365);
    assert_eq!((y, m, d), (1971, 1, 1));
}

#[test]
fn test_days_to_ymd_known_date_2000_01_01() {
    // Days from 1970-01-01 to 2000-01-01 = 30 * 365 + 8 leap days = 10957
    let (y, m, d) = days_to_ymd(10957);
    assert_eq!((y, m, d), (2000, 1, 1));
}

#[test]
fn test_truncate_output_short_string_returned_unchanged() {
    let short = "hello world".to_owned();
    assert_eq!(truncate_output(short.clone()), short);
}

#[test]
fn test_truncate_output_long_string_appends_truncation_marker() {
    let long = "x".repeat(MAX_OUTPUT_BYTES + 100);
    let result = truncate_output(long);
    assert!(
        result.ends_with("[output truncated]"),
        "should end with truncation marker"
    );
    assert!(
        result.len() <= MAX_OUTPUT_BYTES + 20,
        "truncated result should be close to limit"
    );
}

#[test]
fn test_truncate_output_exactly_at_limit_unchanged() {
    let exactly = "y".repeat(MAX_OUTPUT_BYTES);
    let result = truncate_output(exactly.clone());
    assert_eq!(result, exactly, "exactly at limit should be unchanged");
}

// -- Tool definition schema tests ---------------------------------------

#[test]
fn test_grep_def_has_pattern_as_required() {
    let mut reg = crate::registry::ToolRegistry::new();
    register(&mut reg).expect("register");
    let tn = ToolName::new("grep").expect("valid");
    let def = reg.get_def(&tn).expect("grep registered");
    assert!(def.input_schema.required.contains(&"pattern".to_owned()));
}

#[test]
fn test_find_def_has_pattern_as_required() {
    let mut reg = crate::registry::ToolRegistry::new();
    register(&mut reg).expect("register");
    let tn = ToolName::new("find").expect("valid");
    let def = reg.get_def(&tn).expect("find registered");
    assert!(def.input_schema.required.contains(&"pattern".to_owned()));
}

#[test]
fn test_ls_def_has_no_required_fields() {
    let mut reg = crate::registry::ToolRegistry::new();
    register(&mut reg).expect("register");
    let tn = ToolName::new("ls").expect("valid");
    let def = reg.get_def(&tn).expect("ls registered");
    assert!(
        def.input_schema.required.is_empty(),
        "ls should have no required fields"
    );
}

#[test]
fn test_find_def_type_field_has_enum_values() {
    let mut reg = crate::registry::ToolRegistry::new();
    register(&mut reg).expect("register");
    let tn = ToolName::new("find").expect("valid");
    let def = reg.get_def(&tn).expect("find registered");
    let type_prop = def.input_schema.properties.get("type").expect("type prop");
    let enum_vals = type_prop.enum_values.as_ref().expect("enum values");
    assert!(enum_vals.contains(&"f".to_owned()));
    assert!(enum_vals.contains(&"d".to_owned()));
}

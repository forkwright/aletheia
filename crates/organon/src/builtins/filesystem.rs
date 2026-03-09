//! Filesystem navigation tools: grep, find, ls.

use std::fmt::Write as _;
use std::future::Future;
use std::io::Read as _;
use std::path::Path;
use std::pin::Pin;
use std::process::{Command, Stdio};
use std::time::SystemTime;

use aletheia_koina::id::ToolName;
use indexmap::IndexMap;

use crate::error::Result;
use crate::process_guard::ProcessGuard;
use crate::registry::{ToolExecutor, ToolRegistry};
use crate::types::{
    InputSchema, PropertyDef, PropertyType, ToolCategory, ToolContext, ToolDef, ToolInput,
    ToolResult,
};

use super::workspace::{extract_opt_bool, extract_opt_u64, extract_str, validate_path};

const MAX_OUTPUT_BYTES: usize = 50 * 1024;

fn extract_opt_str<'a>(args: &'a serde_json::Value, field: &str) -> Option<&'a str> {
    args.get(field).and_then(serde_json::Value::as_str)
}

fn truncate_output(mut output: String) -> String {
    if output.len() > MAX_OUTPUT_BYTES {
        output.truncate(MAX_OUTPUT_BYTES);
        output.push_str("\n[output truncated]");
    }
    output
}

fn run_command(cmd: &mut Command) -> std::io::Result<std::process::Output> {
    // Wrap in ProcessGuard so the child is killed and reaped on any early
    // return (I/O error, panic, etc.) before we reach wait().
    let mut guard =
        ProcessGuard::new(cmd.stdout(Stdio::piped()).stderr(Stdio::piped()).spawn()?);

    let mut stdout = String::new();
    if let Some(ref mut pipe) = guard.get_mut().stdout {
        let _ = pipe.read_to_string(&mut stdout);
    }
    let mut stderr = String::new();
    if let Some(ref mut pipe) = guard.get_mut().stderr {
        let _ = pipe.read_to_string(&mut stderr);
    }

    // Detach the guard (no kill needed) and reap the process.
    let status = guard.detach().wait()?;
    Ok(std::process::Output {
        status,
        stdout: stdout.into_bytes(),
        stderr: stderr.into_bytes(),
    })
}

// ---------------------------------------------------------------------------
// Grep
// ---------------------------------------------------------------------------

struct GrepExecutor;

impl ToolExecutor for GrepExecutor {
    fn execute<'a>(
        &'a self,
        input: &'a ToolInput,
        ctx: &'a ToolContext,
    ) -> Pin<Box<dyn Future<Output = Result<ToolResult>> + Send + 'a>> {
        Box::pin(async {
            let pattern = extract_str(&input.arguments, "pattern", &input.name)?;
            let max_results = extract_opt_u64(&input.arguments, "maxResults").unwrap_or(50);
            let case_sensitive =
                extract_opt_bool(&input.arguments, "caseSensitive").unwrap_or(true);
            let glob_filter = extract_opt_str(&input.arguments, "glob");

            let path = match extract_opt_str(&input.arguments, "path") {
                Some(p) => validate_path(p, ctx, &input.name)?,
                None => ctx.workspace.clone(),
            };

            let output = try_rg(pattern, &path, max_results, case_sensitive, glob_filter)
                .or_else(|_| try_grep_fallback(pattern, &path, case_sensitive, glob_filter));

            match output {
                Ok(out) => {
                    let code = out.status.code().unwrap_or(-1);
                    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();

                    if code == 1 && stdout.trim().is_empty() {
                        return Ok(ToolResult::text("No matches found."));
                    }

                    let text = truncate_output(stdout);
                    // Enforce the line limit in case the backend (grep fallback) doesn't
                    // natively support --max-count.
                    let n = usize::try_from(max_results).unwrap_or(usize::MAX);
                    let text = text.lines().take(n).collect::<Vec<_>>().join("\n");
                    Ok(ToolResult::text(text))
                }
                Err(e) => Ok(ToolResult::error(format!("grep failed: {e}"))),
            }
        })
    }
}

fn try_rg(
    pattern: &str,
    path: &Path,
    max_results: u64,
    case_sensitive: bool,
    glob_filter: Option<&str>,
) -> std::io::Result<std::process::Output> {
    let mut cmd = Command::new("rg");
    cmd.arg("--no-heading")
        .arg("--line-number")
        .arg("--max-count")
        .arg(max_results.to_string());

    if !case_sensitive {
        cmd.arg("--ignore-case");
    }
    if let Some(g) = glob_filter {
        cmd.arg("--glob").arg(g);
    }

    cmd.arg(pattern).arg(path);
    run_command(&mut cmd)
}

fn try_grep_fallback(
    pattern: &str,
    path: &Path,
    case_sensitive: bool,
    glob_filter: Option<&str>,
) -> std::io::Result<std::process::Output> {
    let mut cmd = Command::new("grep");
    cmd.arg("-rn");
    if !case_sensitive {
        cmd.arg("-i");
    }
    if let Some(glob) = glob_filter {
        cmd.arg(format!("--include={glob}"));
    }
    cmd.arg(pattern).arg(path);
    run_command(&mut cmd)
}

// ---------------------------------------------------------------------------
// Find
// ---------------------------------------------------------------------------

struct FindExecutor;

impl ToolExecutor for FindExecutor {
    fn execute<'a>(
        &'a self,
        input: &'a ToolInput,
        ctx: &'a ToolContext,
    ) -> Pin<Box<dyn Future<Output = Result<ToolResult>> + Send + 'a>> {
        Box::pin(async {
            let pattern = extract_str(&input.arguments, "pattern", &input.name)?;
            let max_results = extract_opt_u64(&input.arguments, "maxResults").unwrap_or(100);
            let type_filter = extract_opt_str(&input.arguments, "type");
            let max_depth = extract_opt_u64(&input.arguments, "maxDepth");

            let path = match extract_opt_str(&input.arguments, "path") {
                Some(p) => validate_path(p, ctx, &input.name)?,
                None => ctx.workspace.clone(),
            };

            let output = try_fd(pattern, &path, max_results, type_filter, max_depth)
                .or_else(|_| try_find_fallback(pattern, &path, type_filter, max_depth));

            match output {
                Ok(out) => {
                    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
                    if stdout.trim().is_empty() {
                        return Ok(ToolResult::text("No files found."));
                    }
                    let text = truncate_output(stdout);
                    // Enforce the result limit in case the backend (find fallback) doesn't
                    // natively support --max-results.
                    let n = usize::try_from(max_results).unwrap_or(usize::MAX);
                    let text = text.lines().take(n).collect::<Vec<_>>().join("\n");
                    Ok(ToolResult::text(text))
                }
                Err(e) => Ok(ToolResult::error(format!("find failed: {e}"))),
            }
        })
    }
}

/// Returns `true` if the pattern contains glob metacharacters (`*`, `?`, `[`, `{`).
fn is_glob_pattern(pattern: &str) -> bool {
    pattern.contains('*') || pattern.contains('?') || pattern.contains('[') || pattern.contains('{')
}

fn try_fd(
    pattern: &str,
    path: &Path,
    max_results: u64,
    type_filter: Option<&str>,
    max_depth: Option<u64>,
) -> std::io::Result<std::process::Output> {
    let mut cmd = Command::new("fd");
    // --no-ignore: workspace dirs live under gitignored paths (instance/).
    // --glob: only when the pattern uses glob metacharacters (*, ?, [, {).
    //         Otherwise fd's default regex mode gives better substring matching.
    cmd.arg("--no-ignore");
    if is_glob_pattern(pattern) {
        cmd.arg("--glob");
    }
    cmd.arg(pattern)
        .arg(path)
        .arg("--max-results")
        .arg(max_results.to_string());

    if let Some(t) = type_filter {
        cmd.arg("--type").arg(t);
    }
    if let Some(d) = max_depth {
        cmd.arg("--max-depth").arg(d.to_string());
    }

    run_command(&mut cmd)
}

fn try_find_fallback(
    pattern: &str,
    path: &Path,
    type_filter: Option<&str>,
    max_depth: Option<u64>,
) -> std::io::Result<std::process::Output> {
    let mut cmd = Command::new("find");
    cmd.arg(path);

    if let Some(d) = max_depth {
        cmd.arg("-maxdepth").arg(d.to_string());
    }
    if let Some(t) = type_filter {
        cmd.arg("-type").arg(t);
    }

    // fd uses regex/substring matching by default; map to find's glob matching.
    // Wrap plain strings in wildcards so "foo" matches "foo.rs", "myfoo", etc.
    let name_pattern = if pattern.is_empty() || pattern == "." {
        "*".to_owned()
    } else if is_glob_pattern(pattern) {
        pattern.to_owned()
    } else {
        format!("*{pattern}*")
    };
    cmd.arg("-name").arg(name_pattern);
    run_command(&mut cmd)
}

// ---------------------------------------------------------------------------
// Ls
// ---------------------------------------------------------------------------

struct LsExecutor;

impl ToolExecutor for LsExecutor {
    fn execute<'a>(
        &'a self,
        input: &'a ToolInput,
        ctx: &'a ToolContext,
    ) -> Pin<Box<dyn Future<Output = Result<ToolResult>> + Send + 'a>> {
        Box::pin(async {
            let show_all = extract_opt_bool(&input.arguments, "all").unwrap_or(false);

            let path = match extract_opt_str(&input.arguments, "path") {
                Some(p) => validate_path(p, ctx, &input.name)?,
                None => ctx.workspace.clone(),
            };

            let entries = match std::fs::read_dir(&path) {
                Ok(rd) => rd,
                Err(e) => {
                    return Ok(ToolResult::error(format!(
                        "cannot read directory {}: {e}",
                        path.display()
                    )));
                }
            };

            let mut dirs: Vec<(String, u64, SystemTime)> = Vec::new();
            let mut files: Vec<(String, u64, SystemTime)> = Vec::new();

            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().into_owned();

                if !show_all && name.starts_with('.') {
                    continue;
                }

                let Ok(meta) = entry.metadata() else {
                    continue;
                };

                let size = meta.len();
                let modified = meta.modified().unwrap_or(SystemTime::UNIX_EPOCH);

                if meta.is_dir() {
                    dirs.push((format!("{name}/"), size, modified));
                } else {
                    files.push((name, size, modified));
                }
            }

            dirs.sort_by(|a, b| a.0.cmp(&b.0));
            files.sort_by(|a, b| a.0.cmp(&b.0));

            let mut output = String::new();
            for (name, size, modified) in dirs.iter().chain(files.iter()) {
                let modified_str = format_system_time(modified);
                let _ = writeln!(output, "{name:<40} {size:>10} {modified_str}");
            }

            if output.is_empty() {
                return Ok(ToolResult::text("Directory is empty."));
            }

            Ok(ToolResult::text(output.trim_end().to_owned()))
        })
    }
}

fn format_system_time(time: &SystemTime) -> String {
    match time.duration_since(SystemTime::UNIX_EPOCH) {
        Ok(dur) => {
            let secs = dur.as_secs();
            // Simple UTC date/time formatting without external deps
            let days = secs / 86400;
            let time_of_day = secs % 86400;
            let hours = time_of_day / 3600;
            let minutes = (time_of_day % 3600) / 60;

            // Days since epoch to Y-M-D (simplified)
            let (year, month, day) = days_to_ymd(days);
            format!("{year:04}-{month:02}-{day:02} {hours:02}:{minutes:02}")
        }
        Err(_) => "unknown".to_owned(),
    }
}

#[allow(clippy::similar_names)] // doe/doy are standard names in Hinnant's date algorithm
fn days_to_ymd(days: u64) -> (u64, u64, u64) {
    // Algorithm from http://howardhinnant.github.io/date_algorithms.html
    let z = days + 719_468;
    let era = z / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y, m, d)
}

// ---------------------------------------------------------------------------
// Tool definitions
// ---------------------------------------------------------------------------

pub fn register(registry: &mut ToolRegistry) -> Result<()> {
    registry.register(grep_def(), Box::new(GrepExecutor))?;
    registry.register(find_def(), Box::new(FindExecutor))?;
    registry.register(ls_def(), Box::new(LsExecutor))?;
    Ok(())
}

fn grep_def() -> ToolDef {
    ToolDef {
        name: ToolName::new("grep").expect("valid tool name"),
        description: "Search file contents for a pattern using ripgrep".to_owned(),
        extended_description: None,
        input_schema: InputSchema {
            properties: IndexMap::from([
                (
                    "pattern".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::String,
                        description: "Search pattern (regex supported)".to_owned(),
                        enum_values: None,
                        default: None,
                    },
                ),
                (
                    "path".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::String,
                        description: "Directory or file to search (default: workspace root)"
                            .to_owned(),
                        enum_values: None,
                        default: None,
                    },
                ),
                (
                    "glob".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::String,
                        description: "File glob filter (e.g., '*.ts', '*.md')".to_owned(),
                        enum_values: None,
                        default: None,
                    },
                ),
                (
                    "caseSensitive".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::Boolean,
                        description: "Case-sensitive search (default: true)".to_owned(),
                        enum_values: None,
                        default: Some(serde_json::json!(true)),
                    },
                ),
                (
                    "maxResults".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::Number,
                        description: "Maximum matching lines per file (default: 50)".to_owned(),
                        enum_values: None,
                        default: Some(serde_json::json!(50)),
                    },
                ),
            ]),
            required: vec!["pattern".to_owned()],
        },
        category: ToolCategory::Workspace,
        auto_activate: true,
    }
}

fn find_def() -> ToolDef {
    ToolDef {
        name: ToolName::new("find").expect("valid tool name"),
        description: "Find files by name pattern using fd".to_owned(),
        extended_description: None,
        input_schema: InputSchema {
            properties: IndexMap::from([
                (
                    "pattern".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::String,
                        description: "File name pattern (glob or regex)".to_owned(),
                        enum_values: None,
                        default: None,
                    },
                ),
                (
                    "path".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::String,
                        description: "Directory to search (default: workspace root)".to_owned(),
                        enum_values: None,
                        default: None,
                    },
                ),
                (
                    "type".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::String,
                        description: "Filter: 'f' for files, 'd' for directories".to_owned(),
                        enum_values: Some(vec!["f".to_owned(), "d".to_owned()]),
                        default: None,
                    },
                ),
                (
                    "maxDepth".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::Number,
                        description: "Maximum directory depth".to_owned(),
                        enum_values: None,
                        default: None,
                    },
                ),
                (
                    "maxResults".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::Number,
                        description: "Maximum results (default: 100)".to_owned(),
                        enum_values: None,
                        default: Some(serde_json::json!(100)),
                    },
                ),
            ]),
            required: vec!["pattern".to_owned()],
        },
        category: ToolCategory::Workspace,
        auto_activate: true,
    }
}

fn ls_def() -> ToolDef {
    ToolDef {
        name: ToolName::new("ls").expect("valid tool name"),
        description: "List directory contents with file sizes and modification times".to_owned(),
        extended_description: None,
        input_schema: InputSchema {
            properties: IndexMap::from([
                (
                    "path".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::String,
                        description: "Directory to list (default: workspace root)".to_owned(),
                        enum_values: None,
                        default: None,
                    },
                ),
                (
                    "all".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::Boolean,
                        description: "Include hidden files (default: false)".to_owned(),
                        enum_values: None,
                        default: Some(serde_json::json!(false)),
                    },
                ),
            ]),
            required: vec![],
        },
        category: ToolCategory::Workspace,
        auto_activate: true,
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
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
}

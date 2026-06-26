//! Filesystem navigation tools: grep, find, ls.

use std::ffi::{OsStr, OsString};
use std::fmt::Write as _;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::time::{Duration, SystemTime};

use indexmap::IndexMap;

use koina::defaults::MAX_OUTPUT_BYTES;
use koina::id::ToolName;

use crate::error::Result;
use crate::registry::{ToolExecutor, ToolRegistry};
use crate::subprocess::{SubprocessError, SubprocessOutput, SubprocessRequest, SubprocessRunner};
use crate::types::{
    InputSchema, PropertyDef, PropertyType, Reversibility, ToolCategory, ToolContext, ToolDef,
    ToolGroupId, ToolInput, ToolResult, ToolTag,
};

use super::workspace::{
    extract_opt_bool, extract_opt_str, extract_opt_u64, extract_str, validate_path,
};

/// WHY: Close TOCTOU window between `validate_path` and actual filesystem access.
/// A symlink could be swapped after validation to point outside allowed roots.
/// Canonicalize resolves symlinks; if the canonical path differs, re-validate.
fn canonicalize_and_revalidate(
    validated_path: PathBuf,
    ctx: &ToolContext,
    tool_name: &ToolName,
) -> crate::error::Result<PathBuf> {
    if validated_path.exists()
        && let Ok(canonical) = std::fs::canonicalize(&validated_path)
        && canonical != validated_path
    {
        validate_path(&canonical.to_string_lossy(), ctx, tool_name)?;
        return Ok(canonical);
    }
    Ok(validated_path)
}

/// WHY: Unbounded patterns can trigger catastrophic backtracking in the regex
/// engine (`ReDoS`). Cap at 1000 chars which covers all legitimate search
/// patterns.
/// Fallback default; runtime reads `ctx.tool_config.max_pattern_length`.
pub const MAX_PATTERN_LENGTH: usize = 1000;

fn truncate_output(mut output: String) -> String {
    if output.len() > MAX_OUTPUT_BYTES {
        // WHY: Truncating at an arbitrary byte position can split a multi-byte
        // UTF-8 character, producing invalid UTF-8. Walk backwards to the
        // nearest char boundary before truncating.
        let mut end = MAX_OUTPUT_BYTES;
        while end > 0 && !output.is_char_boundary(end) {
            end -= 1;
        }
        output.truncate(end);
        output.push_str("\n[output truncated]");
    }
    output
}

/// WHY: Enforce the line limit when the backend (grep fallback) doesn't
/// natively support --max-count. The ripgrep path passes `--max-count`
/// directly, so it already honors the per-file contract.
fn limit_lines(output: &str, max_lines: usize) -> String {
    output
        .lines()
        .take(max_lines)
        .fold(String::new(), |mut acc, line| {
            if !acc.is_empty() {
                acc.push('\n');
            }
            acc.push_str(line);
            acc
        })
}

/// WHY: Subprocess commands (grep, find, ls) must not run indefinitely.
/// A 60-second wall-clock timeout prevents hung processes from consuming
/// resources. On timeout the shared runner kills and reaps the child.
/// Fallback default; runtime reads `ctx.tool_config.subprocess_timeout_secs`.
pub const SUBPROCESS_TIMEOUT: Duration = Duration::from_mins(1);

fn run_command(
    runner: &SubprocessRunner,
    ctx: &ToolContext,
    request: SubprocessRequest,
) -> std::result::Result<SubprocessOutput, SubprocessError> {
    runner.run(
        request
            .timeout(SUBPROCESS_TIMEOUT)
            .max_output_bytes(MAX_OUTPUT_BYTES),
        ctx,
    )
}

struct GrepExecutor {
    runner: SubprocessRunner,
    rg_program: OsString,
    grep_program: OsString,
}

impl GrepExecutor {
    fn new(runner: SubprocessRunner) -> Self {
        Self {
            runner,
            rg_program: OsString::from("rg"),
            grep_program: OsString::from("grep"),
        }
    }
}

impl ToolExecutor for GrepExecutor {
    fn execute<'a>(
        &'a self,
        input: &'a ToolInput,
        ctx: &'a ToolContext,
    ) -> Pin<Box<dyn Future<Output = Result<ToolResult>> + Send + 'a>> {
        Box::pin(async {
            let pattern = extract_str(&input.arguments, "pattern", &input.name)?;

            let max_pat = ctx.tool_config.max_pattern_length;
            if pattern.len() > max_pat {
                return Ok(ToolResult::error(format!(
                    "pattern too long: {} chars (max {max_pat})",
                    pattern.len()
                )));
            }

            let max_results = extract_opt_u64(&input.arguments, "maxResults").unwrap_or(50);
            let case_sensitive =
                extract_opt_bool(&input.arguments, "caseSensitive").unwrap_or(true);
            let glob_filter = extract_opt_str(&input.arguments, "glob");

            let path = match extract_opt_str(&input.arguments, "path") {
                Some(p) => validate_path(p, ctx, &input.name)?,
                None => ctx.workspace.clone(),
            };

            // WHY: Close TOCTOU window: a symlink validated above could be
            // swapped before the subprocess reads it. Canonicalize and
            // re-validate the resolved target.
            let path = canonicalize_and_revalidate(path, ctx, &input.name)?;

            let output = try_rg(
                &self.runner,
                ctx,
                RgRequest {
                    program: self.rg_program.as_os_str(),
                    pattern,
                    path: &path,
                    max_results,
                    case_sensitive,
                    glob_filter,
                },
            )
            .or_else(|_| {
                try_grep_fallback(
                    &self.runner,
                    ctx,
                    RgRequest {
                        program: self.grep_program.as_os_str(),
                        pattern,
                        path: &path,
                        max_results,
                        case_sensitive,
                        glob_filter,
                    },
                )
            });

            match output {
                Ok(out) => {
                    if out.exit_code == 1 && out.stdout.trim().is_empty() {
                        return Ok(ToolResult::text("No matches found."));
                    }

                    let text = truncate_output(out.stdout);
                    Ok(ToolResult::text(text))
                }
                Err(SubprocessError::Timeout(_)) => Ok(ToolResult::error(
                    "grep timed out after 60s: try a narrower path or pattern".to_owned(),
                )),
                Err(e) => Ok(ToolResult::error(format!("grep failed: {e}"))),
            }
        })
    }
}

#[derive(Clone, Copy)]
struct RgRequest<'a> {
    program: &'a OsStr,
    pattern: &'a str,
    path: &'a Path,
    max_results: u64,
    case_sensitive: bool,
    glob_filter: Option<&'a str>,
}

fn try_rg(
    runner: &SubprocessRunner,
    ctx: &ToolContext,
    request: RgRequest<'_>,
) -> std::result::Result<SubprocessOutput, SubprocessError> {
    let mut args = vec![
        "--no-heading".to_owned(),
        "--line-number".to_owned(),
        "--max-count".to_owned(),
        request.max_results.to_string(),
    ];

    if !request.case_sensitive {
        args.push("--ignore-case".to_owned());
    }
    if let Some(g) = request.glob_filter {
        args.push("--glob".to_owned());
        args.push(g.to_owned());
    }

    args.push(request.pattern.to_owned());
    args.push(request.path.to_string_lossy().into_owned());
    run_command(
        runner,
        ctx,
        SubprocessRequest::new(request.program.to_os_string(), ctx.workspace.clone()).args(args),
    )
}

fn try_grep_fallback(
    runner: &SubprocessRunner,
    ctx: &ToolContext,
    request: RgRequest<'_>,
) -> std::result::Result<SubprocessOutput, SubprocessError> {
    let mut args = vec!["-rn".to_owned()];
    if !request.case_sensitive {
        args.push("-i".to_owned());
    }
    if let Some(glob) = request.glob_filter {
        args.push(format!("--include={glob}"));
    }
    args.push(request.pattern.to_owned());
    args.push(request.path.to_string_lossy().into_owned());
    let mut output = run_command(
        runner,
        ctx,
        SubprocessRequest::new(request.program.to_os_string(), ctx.workspace.clone()).args(args),
    )?;
    let n = usize::try_from(request.max_results).unwrap_or(usize::MAX);
    output.stdout = limit_lines(&output.stdout, n);
    Ok(output)
}

struct FindExecutor {
    runner: SubprocessRunner,
    fd_program: OsString,
    find_program: OsString,
}

impl FindExecutor {
    fn new(runner: SubprocessRunner) -> Self {
        Self {
            runner,
            fd_program: OsString::from("fd"),
            find_program: OsString::from("find"),
        }
    }
}

impl ToolExecutor for FindExecutor {
    fn execute<'a>(
        &'a self,
        input: &'a ToolInput,
        ctx: &'a ToolContext,
    ) -> Pin<Box<dyn Future<Output = Result<ToolResult>> + Send + 'a>> {
        Box::pin(async {
            let pattern = extract_str(&input.arguments, "pattern", &input.name)?;

            let max_pat = ctx.tool_config.max_pattern_length;
            if pattern.len() > max_pat {
                return Ok(ToolResult::error(format!(
                    "pattern too long: {} chars (max {max_pat})",
                    pattern.len()
                )));
            }

            let max_results = extract_opt_u64(&input.arguments, "maxResults").unwrap_or(100);
            let type_filter = extract_opt_str(&input.arguments, "type");
            let max_depth = extract_opt_u64(&input.arguments, "maxDepth");

            let path = match extract_opt_str(&input.arguments, "path") {
                Some(p) => validate_path(p, ctx, &input.name)?,
                None => ctx.workspace.clone(),
            };

            let path = canonicalize_and_revalidate(path, ctx, &input.name)?;

            let output = try_fd(
                &self.runner,
                ctx,
                FdRequest {
                    program: self.fd_program.as_os_str(),
                    pattern,
                    path: &path,
                    max_results,
                    type_filter,
                    max_depth,
                },
            )
            .or_else(|_| {
                try_find_fallback(
                    &self.runner,
                    ctx,
                    &self.find_program,
                    pattern,
                    &path,
                    type_filter,
                    max_depth,
                )
            });

            match output {
                Ok(out) => {
                    if out.stdout.trim().is_empty() {
                        return Ok(ToolResult::text("No files found."));
                    }
                    let text = truncate_output(out.stdout);
                    // WHY: Enforce the result limit in case the backend (find fallback) doesn't
                    // natively support --max-results.
                    let n = usize::try_from(max_results).unwrap_or(usize::MAX);
                    let text = text.lines().take(n).fold(String::new(), |mut acc, line| {
                        if !acc.is_empty() {
                            acc.push('\n');
                        }
                        acc.push_str(line);
                        acc
                    });
                    Ok(ToolResult::text(text))
                }
                Err(SubprocessError::Timeout(_)) => Ok(ToolResult::error(
                    "find timed out after 60s: try a narrower path or pattern".to_owned(),
                )),
                Err(e) => Ok(ToolResult::error(format!("find failed: {e}"))),
            }
        })
    }
}

/// Returns `true` if the pattern contains glob metacharacters (`*`, `?`, `[`, `{`).
fn is_glob_pattern(pattern: &str) -> bool {
    pattern.contains('*') || pattern.contains('?') || pattern.contains('[') || pattern.contains('{')
}

#[derive(Clone, Copy)]
struct FdRequest<'a> {
    program: &'a OsStr,
    pattern: &'a str,
    path: &'a Path,
    max_results: u64,
    type_filter: Option<&'a str>,
    max_depth: Option<u64>,
}

fn try_fd(
    runner: &SubprocessRunner,
    ctx: &ToolContext,
    request: FdRequest<'_>,
) -> std::result::Result<SubprocessOutput, SubprocessError> {
    let mut args = Vec::new();
    // NOTE: --no-ignore: workspace dirs live under gitignored paths (instance/).
    // --glob: only when the pattern uses glob metacharacters (*, ?, [, {).
    //         Otherwise fd's default regex mode gives better substring matching.
    args.push("--no-ignore".to_owned());
    if is_glob_pattern(request.pattern) {
        args.push("--glob".to_owned());
    }
    args.push(request.pattern.to_owned());
    args.push(request.path.to_string_lossy().into_owned());
    args.push("--max-results".to_owned());
    args.push(request.max_results.to_string());

    if let Some(t) = request.type_filter {
        args.push("--type".to_owned());
        args.push(t.to_owned());
    }
    if let Some(d) = request.max_depth {
        args.push("--max-depth".to_owned());
        args.push(d.to_string());
    }

    run_command(
        runner,
        ctx,
        SubprocessRequest::new(request.program.to_os_string(), ctx.workspace.clone()).args(args),
    )
}

fn try_find_fallback(
    runner: &SubprocessRunner,
    ctx: &ToolContext,
    program: &OsStr,
    pattern: &str,
    path: &Path,
    type_filter: Option<&str>,
    max_depth: Option<u64>,
) -> std::result::Result<SubprocessOutput, SubprocessError> {
    let mut args = vec![path.to_string_lossy().into_owned()];

    if let Some(d) = max_depth {
        args.push("-maxdepth".to_owned());
        args.push(d.to_string());
    }
    if let Some(t) = type_filter {
        args.push("-type".to_owned());
        args.push(t.to_owned());
    }

    // NOTE: fd uses regex/substring matching by default; map to find's glob matching.
    // Wrap plain strings in wildcards so "foo" matches "foo.rs", "myfoo", etc.
    let name_pattern = if pattern.is_empty() || pattern == "." {
        "*".to_owned()
    } else if is_glob_pattern(pattern) {
        pattern.to_owned()
    } else {
        format!("*{pattern}*")
    };
    args.push("-name".to_owned());
    args.push(name_pattern);
    run_command(
        runner,
        ctx,
        SubprocessRequest::new(program.to_os_string(), ctx.workspace.clone()).args(args),
    )
}

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

            let path = canonicalize_and_revalidate(path, ctx, &input.name)?;

            let mut entries = match tokio::fs::read_dir(&path).await {
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

            while let Ok(Some(entry)) = entries.next_entry().await {
                let name = entry.file_name().to_string_lossy().into_owned();

                if !show_all && name.starts_with('.') {
                    continue;
                }

                let Ok(meta) = entry.metadata().await else {
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
            let days = secs / 86400;
            let time_of_day = secs % 86400;
            let hours = time_of_day / 3600;
            let minutes = (time_of_day % 3600) / 60;

            let (year, month, day) = days_to_ymd(days);
            format!("{year:04}-{month:02}-{day:02} {hours:02}:{minutes:02}")
        }
        Err(_) => "unknown".to_owned(),
    }
}

fn days_to_ymd(days: u64) -> (u64, u64, u64) {
    // NOTE: Algorithm from http://howardhinnant.github.io/date_algorithms.html
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

/// Register filesystem tools (`grep`, `find`, `ls`) into the registry.
#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "unit tests use default registration; runtime injects sandbox config"
    )
)]
pub(crate) fn register(registry: &mut ToolRegistry) -> Result<()> {
    register_with_sandbox(registry, crate::sandbox::SandboxConfig::default())
}

/// Register filesystem tools with a shared subprocess sandbox config.
pub(crate) fn register_with_sandbox(
    registry: &mut ToolRegistry,
    sandbox: crate::sandbox::SandboxConfig,
) -> Result<()> {
    let runner = SubprocessRunner::new(sandbox);
    registry.register(grep_def(), Box::new(GrepExecutor::new(runner.clone())))?;
    registry.register(find_def(), Box::new(FindExecutor::new(runner)))?;
    registry.register(ls_def(), Box::new(LsExecutor))?;
    Ok(())
}

fn grep_def() -> ToolDef {
    ToolDef {
        name: ToolName::from_static("grep"), // kanon:ignore RUST/expect
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
        reversibility: Reversibility::FullyReversible,
        auto_activate: true,
        groups: vec![ToolGroupId::Read],
        tags: vec![ToolTag::Recon],
    }
}

fn find_def() -> ToolDef {
    ToolDef {
        name: ToolName::from_static("find"), // kanon:ignore RUST/expect
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
        reversibility: Reversibility::FullyReversible,
        auto_activate: true,
        groups: vec![ToolGroupId::Read],
        tags: vec![ToolTag::Recon],
    }
}

fn ls_def() -> ToolDef {
    ToolDef {
        name: ToolName::from_static("ls"), // kanon:ignore RUST/expect
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
        reversibility: Reversibility::FullyReversible,
        auto_activate: true,
        groups: vec![ToolGroupId::Read],
        tags: vec![ToolTag::Recon],
    }
}

#[cfg(test)]
#[path = "filesystem_tests/mod.rs"]
mod tests;

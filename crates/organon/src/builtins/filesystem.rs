//! Filesystem navigation tools: grep, find, ls.

use std::fmt::Write as _;
use std::future::Future;
use std::io::Read as _;
use std::path::Path;
use std::pin::Pin;
use std::process::{Command, Stdio};
use std::time::SystemTime;

use indexmap::IndexMap;

use aletheia_koina::defaults::MAX_OUTPUT_BYTES;
use aletheia_koina::id::ToolName;

use crate::error::Result;
use crate::process_guard::ProcessGuard;
use crate::registry::{ToolExecutor, ToolRegistry};
use crate::types::{
    InputSchema, PropertyDef, PropertyType, Reversibility, ToolCategory, ToolContext, ToolDef,
    ToolInput, ToolResult,
};

use super::workspace::{extract_opt_bool, extract_opt_u64, extract_str, validate_path};

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
    // NOTE: Wrap in ProcessGuard so the child is killed and reaped on any early
    // return (I/O error, panic, etc.) before we reach wait().
    let mut guard = ProcessGuard::new(cmd.stdout(Stdio::piped()).stderr(Stdio::piped()).spawn()?);

    let mut stdout = String::new();
    if let Some(ref mut pipe) = guard.get_mut().stdout {
        let _ = pipe.read_to_string(&mut stdout);
    }
    let mut stderr = String::new();
    if let Some(ref mut pipe) = guard.get_mut().stderr {
        let _ = pipe.read_to_string(&mut stderr);
    }

    let status = guard.detach().wait()?;
    Ok(std::process::Output {
        status,
        stdout: stdout.into_bytes(),
        stderr: stderr.into_bytes(),
    })
}

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
                    // WHY: Enforce the line limit in case the backend (grep fallback) doesn't
                    // natively support --max-count.
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
    // NOTE: --no-ignore: workspace dirs live under gitignored paths (instance/).
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

    // NOTE: fd uses regex/substring matching by default; map to find's glob matching.
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
pub(crate) fn register(registry: &mut ToolRegistry) -> Result<()> {
    registry.register(grep_def(), Box::new(GrepExecutor))?;
    registry.register(find_def(), Box::new(FindExecutor))?;
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
    }
}

#[cfg(test)]
#[path = "filesystem_tests.rs"]
mod tests;

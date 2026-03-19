//! Workspace tool executors: read, write, edit, exec.
#![expect(
    clippy::expect_used,
    reason = "ToolName::new() with static string literals is infallible — name validation would only fail on invalid chars which these names don't contain"
)]

use std::future::Future;
use std::io::Read as _;
use std::path::{Component, Path, PathBuf};
use std::pin::Pin;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

#[cfg(unix)]
use std::os::unix::process::CommandExt as _;

use indexmap::IndexMap;

use aletheia_koina::defaults::MAX_OUTPUT_BYTES;
use aletheia_koina::id::ToolName;

use crate::error::{self, Result};
use crate::process_guard::ProcessGuard;
use crate::registry::{ToolExecutor, ToolRegistry};
use crate::types::{
    InputSchema, PropertyDef, PropertyType, ToolCategory, ToolContext, ToolDef, ToolInput,
    ToolResult,
};

/// Strip absolute path prefixes from an error message, showing only the filename.
///
/// WHY: Full filesystem paths in error messages sent to the LLM leak instance
/// directory structure. Show only the filename component instead. Closes #1716.
fn sanitize_path_in_msg(path: &std::path::Path) -> String {
    path.file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("<path>")
        .to_owned()
}

/// Maximum content size for the write tool (10 MB).
///
/// WHY: Prevents disk exhaustion or fork-bomb-like abuse via oversized writes.
/// Closes #1714.
const MAX_WRITE_BYTES: usize = 10 * 1024 * 1024;

/// Expand a leading `~` in a path string to the HOME environment variable.
///
/// If `HOME` is not set, returns the input unchanged so the subsequent
/// path-validation step surfaces a clear "outside allowed roots" error rather
/// than a confusing "no such file" error.
fn expand_tilde_str(raw: &str) -> std::borrow::Cow<'_, str> {
    if let Some(rest) = raw.strip_prefix('~')
        && let Ok(home) = std::env::var("HOME")
    {
        return std::borrow::Cow::Owned(format!("{home}{rest}"));
    }
    std::borrow::Cow::Borrowed(raw)
}

pub(crate) fn validate_path(raw: &str, ctx: &ToolContext, tool_name: &ToolName) -> Result<PathBuf> {
    if raw.is_empty() {
        return Err(error::InvalidInputSnafu {
            name: tool_name.clone(),
            reason: "path must not be empty".to_owned(),
        }
        .build());
    }

    // WHY: LLMs commonly emit `~/file` or `~` to refer to HOME. Without
    // expansion the path resolves relative to workspace, producing a confusing
    // "outside allowed roots" error. Expand before any other resolution so the
    // absolute path check below works correctly: closes #1244.
    let expanded = expand_tilde_str(raw);
    let raw_expanded = expanded.as_ref();

    let resolved = if Path::new(raw_expanded).is_absolute() {
        PathBuf::from(raw_expanded)
    } else {
        ctx.workspace.join(raw_expanded)
    };

    let normalized = normalize(&resolved);

    // PERF: First check the normalized path (fast path, catches obvious traversals)
    let allowed = ctx
        .allowed_roots
        .iter()
        .any(|root| normalized.starts_with(root));

    if !allowed {
        return Err(error::InvalidInputSnafu {
            name: tool_name.clone(),
            reason: format!("path outside allowed roots: {raw}"),
        }
        .build());
    }

    // NOTE: Resolve symlinks to prevent symlink-based escapes.
    // If the file exists, canonicalize it directly.
    // If not (e.g. write to new file), canonicalize the parent directory.
    let canonical = if normalized.exists() {
        normalized.canonicalize()
    } else if let Some(parent) = normalized.parent() {
        // NOTE: For new files: canonicalize parent, then append the filename
        if parent.exists() {
            parent.canonicalize().map(|p| {
                if let Some(name) = normalized.file_name() {
                    p.join(name)
                } else {
                    p
                }
            })
        } else {
            // NOTE: Parent doesn't exist yet (will be created by write): use normalized
            Ok(normalized.clone())
        }
    } else {
        Ok(normalized.clone())
    };

    let canonical = canonical.unwrap_or_else(|_| normalized.clone());

    // NOTE: Re-check canonical path against allowed roots
    let canonical_allowed = ctx.allowed_roots.iter().any(|root| {
        let canon_root = root.canonicalize().unwrap_or_else(|_| root.clone());
        canonical.starts_with(&canon_root)
    });

    if !canonical_allowed {
        return Err(error::InvalidInputSnafu {
            name: tool_name.clone(),
            reason: format!("path outside allowed roots: {raw}"),
        }
        .build());
    }

    Ok(normalized)
}

pub(crate) fn normalize(path: &Path) -> PathBuf {
    let mut result = PathBuf::new();
    for component in path.components() {
        match component {
            Component::ParentDir => {
                result.pop();
            }
            // NOTE: current-dir component (`.`) is a no-op in normalization
            Component::CurDir => {}
            other => result.push(other),
        }
    }
    result
}

pub(crate) fn extract_str<'a>(
    args: &'a serde_json::Value,
    field: &str,
    tool_name: &ToolName,
) -> Result<&'a str> {
    args.get(field)
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| {
            error::InvalidInputSnafu {
                name: tool_name.clone(),
                reason: format!("missing or invalid field: {field}"),
            }
            .build()
        })
}

pub(crate) fn extract_opt_u64(args: &serde_json::Value, field: &str) -> Option<u64> {
    args.get(field).and_then(serde_json::Value::as_u64)
}

pub(crate) fn extract_opt_bool(args: &serde_json::Value, field: &str) -> Option<bool> {
    args.get(field).and_then(serde_json::Value::as_bool)
}

/// Maximum file size the read tool will process.
const MAX_READ_BYTES: u64 = 50 * 1024 * 1024; // 50 MB

/// Maximum command string length for exec.
const MAX_COMMAND_LENGTH: usize = 10_000; // 10 KB

/// Files that the LLM must not overwrite.
const PROTECTED_FILES: &[&str] = &[
    "IDENTITY.md",
    "SOUL.md",
    "GOALS.md",
    "TOOLS.md",
    "MEMORY.md",
    ".claude/settings.json",
    "standards",
];

/// Check if a resolved path matches a protected file pattern.
fn is_protected_file(path: &Path, workspace: &Path) -> Option<&'static str> {
    let relative = path.strip_prefix(workspace).unwrap_or(path);
    let rel_str = relative.to_string_lossy();
    for &protected in PROTECTED_FILES {
        if rel_str == protected || rel_str.starts_with(&format!("{protected}/")) {
            return Some(protected);
        }
    }
    None
}

fn err_result(msg: String) -> ToolResult {
    ToolResult::error(msg)
}

struct ReadExecutor;

impl ToolExecutor for ReadExecutor {
    fn execute<'a>(
        &'a self,
        input: &'a ToolInput,
        ctx: &'a ToolContext,
    ) -> Pin<Box<dyn Future<Output = Result<ToolResult>> + Send + 'a>> {
        Box::pin(async {
            let path_str = extract_str(&input.arguments, "path", &input.name)?;
            let max_lines = extract_opt_u64(&input.arguments, "maxLines");
            let path = validate_path(path_str, ctx, &input.name)?;

            match std::fs::metadata(&path) {
                Ok(meta) if meta.len() > MAX_READ_BYTES => {
                    return Ok(err_result(format!(
                        "file too large: {} bytes (max {} bytes)",
                        meta.len(),
                        MAX_READ_BYTES
                    )));
                }
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                    return Ok(err_result(format!(
                        "file not found: {}",
                        sanitize_path_in_msg(&path)
                    )));
                }
                Err(e) => {
                    return Ok(err_result(format!("read failed: {e}")));
                }
                Ok(_) => {}
            }

            let content = match std::fs::read_to_string(&path) {
                Ok(c) => c,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                    return Ok(err_result(format!(
                        "file not found: {}",
                        sanitize_path_in_msg(&path)
                    )));
                }
                Err(e) => {
                    return Ok(err_result(format!("read failed: {e}")));
                }
            };

            let output = match max_lines {
                Some(n) => {
                    let n = usize::try_from(n).unwrap_or(usize::MAX);
                    content
                        .lines()
                        .take(n)
                        .fold(String::new(), |mut acc, line| {
                            if !acc.is_empty() {
                                acc.push('\n');
                            }
                            acc.push_str(line);
                            acc
                        })
                }
                None => content,
            };

            Ok(ToolResult::text(output))
        })
    }
}

struct WriteExecutor;

impl ToolExecutor for WriteExecutor {
    fn execute<'a>(
        &'a self,
        input: &'a ToolInput,
        ctx: &'a ToolContext,
    ) -> Pin<Box<dyn Future<Output = Result<ToolResult>> + Send + 'a>> {
        Box::pin(async {
            let path_str = extract_str(&input.arguments, "path", &input.name)?;
            let content = extract_str(&input.arguments, "content", &input.name)?;
            let append = extract_opt_bool(&input.arguments, "append").unwrap_or(false);
            let path = validate_path(path_str, ctx, &input.name)?;

            // WHY: Enforce content size limit to prevent disk exhaustion. Closes #1714.
            if content.len() > MAX_WRITE_BYTES {
                return Ok(err_result(format!(
                    "content too large: {} bytes (max {} bytes)",
                    content.len(),
                    MAX_WRITE_BYTES
                )));
            }

            // WHY: Block writes to protected bootstrap files
            if let Some(protected) = is_protected_file(&path, &ctx.workspace) {
                return Ok(err_result(format!(
                    "cannot overwrite protected file: {protected}"
                )));
            }

            if let Some(parent) = path.parent()
                && let Err(e) = std::fs::create_dir_all(parent)
            {
                return Ok(err_result(format!("failed to create directories: {e}")));
            }

            let write_result = if append {
                std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(&path)
                    .and_then(|mut f| {
                        use std::io::Write;
                        f.write_all(content.as_bytes())
                    })
            } else {
                #[expect(
                    clippy::disallowed_methods,
                    reason = "organon workspace tools directly implement filesystem operations exposed to agents; synchronous access matches the tool executor contract"
                )]
                std::fs::write(&path, content)
            };

            match write_result {
                Ok(()) => Ok(ToolResult::text(format!(
                    "wrote {} bytes to {}",
                    content.len(),
                    sanitize_path_in_msg(&path)
                ))),
                Err(e) => Ok(err_result(format!("write failed: {e}"))),
            }
        })
    }
}

struct EditExecutor;

impl ToolExecutor for EditExecutor {
    fn execute<'a>(
        &'a self,
        input: &'a ToolInput,
        ctx: &'a ToolContext,
    ) -> Pin<Box<dyn Future<Output = Result<ToolResult>> + Send + 'a>> {
        Box::pin(async {
            let path_str = extract_str(&input.arguments, "path", &input.name)?;
            let old_text = extract_str(&input.arguments, "old_text", &input.name)?;
            let new_text = extract_str(&input.arguments, "new_text", &input.name)?;
            let path = validate_path(path_str, ctx, &input.name)?;

            let content = match std::fs::read_to_string(&path) {
                Ok(c) => c,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                    return Ok(err_result(format!(
                        "file not found: {}",
                        sanitize_path_in_msg(&path)
                    )));
                }
                Err(e) => {
                    return Ok(err_result(format!("read failed: {e}")));
                }
            };

            let count = content.matches(old_text).count();
            if count == 0 {
                return Ok(err_result(format!(
                    "old_text not found in {}",
                    sanitize_path_in_msg(&path)
                )));
            }
            if count > 1 {
                return Ok(err_result(format!(
                    "old_text found {count} times in {} \u{2014} must be unique",
                    sanitize_path_in_msg(&path)
                )));
            }

            let new_content = content.replacen(old_text, new_text, 1);
            #[expect(
                clippy::disallowed_methods,
                reason = "organon workspace tools directly implement filesystem operations exposed to agents; synchronous access matches the tool executor contract"
            )]
            if let Err(e) = std::fs::write(&path, &new_content) {
                return Ok(err_result(format!("write failed: {e}")));
            }

            Ok(ToolResult::text(format!(
                "edited {}: replaced {} chars with {} chars",
                sanitize_path_in_msg(&path),
                old_text.len(),
                new_text.len()
            )))
        })
    }
}

/// Parse a command string into a program and argument list.
///
/// Supports single-quoted strings (literal), double-quoted strings (with
/// backslash escaping for `\\`, `\"`, `\n`, `\t`), and bare whitespace-
/// separated tokens. Shell metacharacters (`|`, `&`, `;`, `$`, etc.) are
/// treated as literal characters, preventing shell injection.
///
/// WHY: Executing LLM input through `sh -c` allows shell metacharacters to
/// be interpreted, enabling injection of arbitrary commands. Parsing into
/// explicit args avoids the shell entirely.
fn parse_command_args(command: &str) -> std::result::Result<(String, Vec<String>), String> {
    let mut tokens: Vec<String> = Vec::new();
    let mut current = String::new();
    let mut in_single_quote = false;
    let mut in_double_quote = false;
    let mut chars = command.chars().peekable();

    while let Some(c) = chars.next() {
        match (c, in_single_quote, in_double_quote) {
            ('\'', false, false) => in_single_quote = true,
            ('\'', true, _) => in_single_quote = false,
            ('"', false, false) => in_double_quote = true,
            ('"', _, true) => in_double_quote = false,
            ('\\', false, true) => match chars.next() {
                Some('\\') | None => current.push('\\'),
                Some('"') => current.push('"'),
                Some('n') => current.push('\n'),
                Some('t') => current.push('\t'),
                Some(other) => {
                    current.push('\\');
                    current.push(other);
                }
            },
            (c, false, false) if c.is_whitespace() => {
                if !current.is_empty() {
                    tokens.push(std::mem::take(&mut current));
                }
            }
            (c, _, _) => current.push(c),
        }
    }

    if in_single_quote {
        return Err("unterminated single quote".to_owned());
    }
    if in_double_quote {
        return Err("unterminated double quote".to_owned());
    }
    if !current.is_empty() {
        tokens.push(current);
    }

    let mut iter = tokens.into_iter();
    let program = iter.next().ok_or_else(|| "command is empty".to_owned())?;
    let args: Vec<String> = iter.collect();
    Ok((program, args))
}

struct ExecExecutor {
    sandbox: crate::sandbox::SandboxConfig,
}

impl ToolExecutor for ExecExecutor {
    fn execute<'a>(
        &'a self,
        input: &'a ToolInput,
        ctx: &'a ToolContext,
    ) -> Pin<Box<dyn Future<Output = Result<ToolResult>> + Send + 'a>> {
        Box::pin(async {
            let command = extract_str(&input.arguments, "command", &input.name)?;
            let timeout_ms = extract_opt_u64(&input.arguments, "timeout").unwrap_or(120_000);

            if command.len() > MAX_COMMAND_LENGTH {
                return Ok(err_result(format!(
                    "command too long: {} bytes (max {MAX_COMMAND_LENGTH} bytes)",
                    command.len()
                )));
            }

            let (program, args) = match parse_command_args(command) {
                Ok(p) => p,
                Err(e) => return Ok(err_result(format!("invalid command syntax: {e}"))),
            };

            let mut cmd = Command::new(&program);
            cmd.args(&args)
                .current_dir(&ctx.workspace)
                .stdout(Stdio::piped())
                .stderr(Stdio::piped());

            // WHY: Apply process resource limits before sandbox to constrain fork-bombs
            // and runaway CPU usage. RLIMIT_NPROC caps child process count;
            // RLIMIT_CPU caps CPU seconds. Closes #1717.
            #[cfg(target_os = "linux")]
            {
                // SAFETY: setrlimit is async-signal-safe and only modifies the
                // calling process's resource limits. Runs between fork and exec.
                #[expect(
                    unsafe_code,
                    reason = "pre_exec requires unsafe; setrlimit is async-signal-safe"
                )]
                unsafe {
                    cmd.pre_exec(|| {
                        use rustix::process::{Resource, Rlimit, setrlimit};

                        // Cap subprocess count to prevent fork bombs
                        let nproc_limit = Rlimit {
                            current: Some(64),
                            maximum: Some(64),
                        };
                        let _ = setrlimit(Resource::Nproc, nproc_limit);

                        // Cap CPU time to 60 seconds to prevent runaway processes
                        let cpu_limit = Rlimit {
                            current: Some(60),
                            maximum: Some(60),
                        };
                        let _ = setrlimit(Resource::Cpu, cpu_limit);

                        Ok(())
                    });
                }
            }

            if self.sandbox.enabled {
                let policy = self
                    .sandbox
                    .build_policy(&ctx.workspace, &ctx.allowed_roots);
                if let Err(e) = crate::sandbox::apply_sandbox(&mut cmd, policy) {
                    return Ok(err_result(format!("sandbox setup failed: {e}")));
                }
            }

            // NOTE: Wrap immediately so the child is killed on any early return
            // (timeout, wait error, or panic).
            let mut guard = match cmd.spawn() {
                Ok(c) => ProcessGuard::new(c),
                Err(e) => {
                    return Ok(err_result(format!("spawn failed: {e}")));
                }
            };

            let deadline = Instant::now() + Duration::from_millis(timeout_ms);
            let status = loop {
                match guard.get_mut().try_wait() {
                    Ok(Some(s)) => break s,
                    Ok(None) => {
                        if Instant::now() >= deadline {
                            // INVARIANT: kill() must always be followed by wait() to
                            // prevent zombie accumulation. If the process exited between
                            // try_wait() returning None and this kill(), kill() returns
                            // ESRCH (safe to ignore); wait() still reaps the zombie
                            // because no other caller can have waited on this child.
                            let _ = guard.get_mut().kill();
                            let _ = guard.get_mut().wait();
                            return Ok(err_result(format!(
                                "command timed out after {timeout_ms}ms"
                            )));
                        }
                        std::thread::sleep(Duration::from_millis(50));
                    }
                    Err(e) => {
                        return Ok(err_result(format!("wait failed: {e}")));
                    }
                }
            };

            // NOTE: Process exited normally (`try_wait` already reaped the zombie).
            // Read captured stdio via the guard. The guard's Drop will call
            // kill() + wait() on the already-dead process, both of which
            // safely ignore ESRCH / ECHILD errors.
            let mut stdout = String::new();
            if let Some(ref mut pipe) = guard.get_mut().stdout {
                let _ = pipe.read_to_string(&mut stdout);
            }
            let mut stderr = String::new();
            if let Some(ref mut pipe) = guard.get_mut().stderr {
                let _ = pipe.read_to_string(&mut stderr);
            }

            let code = status.code().unwrap_or(-1);
            let mut output = format!("exit={code}\n{stdout}\n{stderr}");
            if output.len() > MAX_OUTPUT_BYTES {
                output.truncate(MAX_OUTPUT_BYTES);
                output.push_str("\n[output truncated]");
            }

            Ok(ToolResult::text(output))
        })
    }
}

/// Register workspace tool executors.
pub fn register(registry: &mut ToolRegistry, sandbox: crate::sandbox::SandboxConfig) -> Result<()> {
    registry.register(read_def(), Box::new(ReadExecutor))?;
    registry.register(write_def(), Box::new(WriteExecutor))?;
    registry.register(edit_def(), Box::new(EditExecutor))?;
    registry.register(exec_def(), Box::new(ExecExecutor { sandbox }))?;
    Ok(())
}

fn read_def() -> ToolDef {
    ToolDef {
        name: ToolName::new("read").expect("valid tool name"),
        description: "Read a file's contents as text".to_owned(),
        extended_description: None,
        input_schema: InputSchema {
            properties: IndexMap::from([
                (
                    "path".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::String,
                        description: "File path (absolute or relative to workspace)".to_owned(),
                        enum_values: None,
                        default: None,
                    },
                ),
                (
                    "maxLines".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::Number,
                        description: "Maximum lines to return (default: all)".to_owned(),
                        enum_values: None,
                        default: None,
                    },
                ),
            ]),
            required: vec!["path".to_owned()],
        },
        category: ToolCategory::Workspace,
        auto_activate: true,
    }
}

fn write_def() -> ToolDef {
    ToolDef {
        name: ToolName::new("write").expect("valid tool name"),
        description: "Write content to a file, creating parent directories as needed".to_owned(),
        extended_description: None,
        input_schema: InputSchema {
            properties: IndexMap::from([
                (
                    "path".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::String,
                        description: "File path (absolute or relative to workspace)".to_owned(),
                        enum_values: None,
                        default: None,
                    },
                ),
                (
                    "content".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::String,
                        description: "Content to write".to_owned(),
                        enum_values: None,
                        default: None,
                    },
                ),
                (
                    "append".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::Boolean,
                        description: "Append instead of overwrite (default: false)".to_owned(),
                        enum_values: None,
                        default: Some(serde_json::json!(false)),
                    },
                ),
            ]),
            required: vec!["path".to_owned(), "content".to_owned()],
        },
        category: ToolCategory::Workspace,
        auto_activate: true,
    }
}

fn edit_def() -> ToolDef {
    ToolDef {
        name: ToolName::new("edit").expect("valid tool name"),
        description: "Replace exact text in a file with new text".to_owned(),
        extended_description: None,
        input_schema: InputSchema {
            properties: IndexMap::from([
                (
                    "path".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::String,
                        description: "File path (absolute or relative to workspace)".to_owned(),
                        enum_values: None,
                        default: None,
                    },
                ),
                (
                    "old_text".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::String,
                        description: "Exact text to find in the file".to_owned(),
                        enum_values: None,
                        default: None,
                    },
                ),
                (
                    "new_text".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::String,
                        description: "Replacement text".to_owned(),
                        enum_values: None,
                        default: None,
                    },
                ),
            ]),
            required: vec![
                "path".to_owned(),
                "old_text".to_owned(),
                "new_text".to_owned(),
            ],
        },
        category: ToolCategory::Workspace,
        auto_activate: true,
    }
}

fn exec_def() -> ToolDef {
    ToolDef {
        name: ToolName::new("exec").expect("valid tool name"),
        description: "Execute a shell command in your workspace and return stdout/stderr"
            .to_owned(),
        extended_description: None,
        input_schema: InputSchema {
            properties: IndexMap::from([
                (
                    "command".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::String,
                        description: "The shell command to execute".to_owned(),
                        enum_values: None,
                        default: None,
                    },
                ),
                (
                    "timeout".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::Number,
                        description: "Timeout in milliseconds (default 30000)".to_owned(),
                        enum_values: None,
                        default: Some(serde_json::json!(30000)),
                    },
                ),
            ]),
            required: vec!["command".to_owned()],
        },
        category: ToolCategory::Workspace,
        auto_activate: true,
    }
}

#[cfg(test)]
#[path = "workspace_tests/mod.rs"]
mod tests;

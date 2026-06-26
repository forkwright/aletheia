//! Workspace tool executors: read, write, edit, exec.

use std::future::Future;
use std::path::{Component, Path, PathBuf};
use std::pin::Pin;
use std::time::Duration;

use indexmap::IndexMap;

use koina::defaults::MAX_OUTPUT_BYTES;
use koina::id::ToolName;
use koina::system::{Environment, RealSystem};

use crate::error::{self, Result};
use crate::registry::{ToolExecutor, ToolRegistry};
use crate::subprocess::{SubprocessRequest, SubprocessRunner};
use crate::types::{
    InputSchema, PropertyDef, PropertyType, Reversibility, ToolCategory, ToolContext, ToolDef,
    ToolDiagnostics, ToolGroupId, ToolInput, ToolResult, ToolTag,
};

/// Strip absolute path prefixes from an error message, showing only the filename.
///
/// WHY: Full filesystem paths in error messages sent to the LLM leak instance
/// directory structure. Show only the filename component instead.
fn sanitize_path_in_msg(path: &std::path::Path) -> String {
    path.file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("<path>")
        .to_owned()
}

/// Maximum content size for the write tool (10 MB).
///
/// WHY: Prevents disk exhaustion or fork-bomb-like abuse via oversized writes.
/// Fallback default; runtime reads `ctx.tool_config.max_write_bytes`.
pub const MAX_WRITE_BYTES: usize = 10 * 1024 * 1024;

/// Expand a leading `~` in a path string to the HOME environment variable.
///
/// If `HOME` is not set, returns the input unchanged so the subsequent
/// path-validation step surfaces a clear "outside allowed roots" error rather
/// than a confusing "no such file" error.
fn expand_tilde_str(raw: &str) -> std::borrow::Cow<'_, str> {
    if let Some(rest) = raw.strip_prefix('~')
        && let Some(home) = RealSystem.var("HOME")
    {
        return std::borrow::Cow::Owned(format!("{home}{rest}"));
    }
    std::borrow::Cow::Borrowed(raw)
}

/// WHY: Shell metacharacters in path arguments could escape to a shell if any
/// downstream code ever passes them unsanitized. Null bytes truncate C-strings
/// causing path confusion. Reject both upfront.
const SHELL_METACHARACTERS: &[char] = &[';', '|', '&', '$', '`', '(', ')'];

pub(crate) fn validate_path(raw: &str, ctx: &ToolContext, tool_name: &ToolName) -> Result<PathBuf> {
    if raw.is_empty() {
        return Err(error::InvalidInputSnafu {
            name: tool_name.clone(),
            reason: "path must not be empty".to_owned(),
        }
        .build());
    }

    if raw.contains('\0') {
        return Err(error::InvalidInputSnafu {
            name: tool_name.clone(),
            reason: "path contains null byte".to_owned(),
        }
        .build());
    }

    if raw.contains(SHELL_METACHARACTERS) {
        return Err(error::InvalidInputSnafu {
            name: tool_name.clone(),
            reason: "path contains shell metacharacter".to_owned(),
        }
        .build());
    }

    // WHY: LLMs commonly emit `~/file` or `~` to refer to HOME. Without
    // expansion the path resolves relative to workspace, producing a confusing
    // "outside allowed roots" error. Expand before any other resolution so the
    // absolute path check below works correctly.
    let expanded = expand_tilde_str(raw);
    let raw_expanded = expanded.as_ref();

    let resolved = if Path::new(raw_expanded).is_absolute() {
        PathBuf::from(raw_expanded)
    } else {
        ctx.workspace.join(raw_expanded)
    };

    let normalized = normalize(&resolved);

    // NOTE: Resolve symlinks to prevent symlink-based escapes.
    // If the file exists, canonicalize it directly.
    // If not (e.g. write to new file), canonicalize the deepest existing ancestor
    // and re-attach the remaining components. This ensures the canonical form is
    // consistent even on macOS, where /var is a symlink to /private/var and
    // tempfile::tempdir() returns /var/folders/... whose canonical form is
    // /private/var/folders/... Comparing a canonicalized root against a path
    // that was only partially canonicalized (parent only, or not at all) produces
    // false "outside allowed roots" rejections.
    let canonical = if normalized.exists() {
        normalized
            .canonicalize()
            .unwrap_or_else(|_| normalized.clone())
    } else {
        // WHY: canonicalize() fails on paths that do not exist yet, so walk up
        // to the deepest existing ancestor, canonicalize that, then re-attach
        // the non-existent trailing components.
        let mut existing = normalized.as_path();
        // INVARIANT: suffix_components stays in forward (top-down) path order —
        // given /a/b/c where /a/b exists but /a/b/c does not, we collect ["c"]
        // and join as /canonical_a_b/c — so each new component is prepended.
        let mut suffix_components: Vec<std::ffi::OsString> = Vec::new();
        loop {
            match existing.parent() {
                Some(parent) if !existing.exists() => {
                    if let Some(name) = existing.file_name() {
                        suffix_components.insert(0, name.to_owned());
                    }
                    existing = parent;
                }
                _ => break,
            }
        }
        let base = if existing.exists() {
            existing
                .canonicalize()
                .unwrap_or_else(|_| existing.to_path_buf())
        } else {
            normalized.clone()
        };
        suffix_components
            .iter()
            .fold(base, |acc, component| acc.join(component))
    };

    // INVARIANT: Authorization must use the canonical, symlink-resolved
    // target only. Accepting the normalized (pre-canonicalization) path
    // allowed an in-root symlink to point outside the allowed roots: the
    // normalized path started with the root while the canonical target did
    // not (#4954).
    //
    // WHY: Each allowed root is canonicalized so non-canonical root forms
    // (symlinks, trailing slashes) still match their resolved children.
    // The containment check is a strict prefix on Path components, so
    // "/allowed-root" correctly rejects "/allowed-root-impostor".
    let allowed = ctx.allowed_roots.iter().any(|root| {
        let canon_root = root.canonicalize().unwrap_or_else(|_| normalize(root));
        canonical.starts_with(&canon_root)
    });

    if !allowed {
        return Err(error::InvalidInputSnafu {
            name: tool_name.clone(),
            reason: format!("path outside allowed roots: {raw}"),
        }
        .build());
    }

    // WHY: return the canonical path so callers operate on the resolved
    // target. Using the normalized (non-canonical) path leaves a TOCTOU
    // window where a symlink could be swapped after validation but before
    // I/O. Returning canonical shrinks this window because the kernel
    // path lookup won't follow the original symlink.
    Ok(canonical)
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

pub(crate) fn extract_opt_str<'a>(args: &'a serde_json::Value, field: &str) -> Option<&'a str> {
    args.get(field).and_then(serde_json::Value::as_str)
}

pub(crate) fn extract_opt_f64(args: &serde_json::Value, field: &str) -> Option<f64> {
    args.get(field).and_then(serde_json::Value::as_f64)
}

/// Maximum file size the read tool will process.
/// Fallback default; runtime reads `ctx.tool_config.max_read_bytes`.
pub const MAX_READ_BYTES: u64 = 50 * 1024 * 1024;

/// Maximum command string length for exec.
/// Fallback default; runtime reads `ctx.tool_config.max_command_length`.
pub const MAX_COMMAND_LENGTH: usize = 10_000;

/// Files that the LLM must not overwrite.
const PROTECTED_FILES: &[&str] = &[
    "IDENTITY.md",
    "SOUL.md",
    "GOALS.md",
    "TOOLS.md",
    "MEMORY.md",
    ".claude/settings.json",
    "standards",
    ".git/config",
    "known_hosts",
];

/// WHY: Sensitive file extensions that must never be written by the LLM.
/// Checked case-insensitively so `.PEM` and `.pem` are both blocked.
const PROTECTED_EXTENSIONS: &[&str] = &["key", "pem", "p12", "pfx"];

/// Filename prefixes (case-sensitive) that identify credential files.
const PROTECTED_PREFIXES: &[&str] = &["id_rsa", "id_ed25519"]; // pii-allow: SSH filename constants guarding access, not key material

/// Filename patterns matched with `starts_with` (case-insensitive).
const PROTECTED_DOT_PREFIXES: &[&str] = &[".env"];

/// Substring patterns for credential files (case-insensitive).
const PROTECTED_SUBSTRINGS: &[&str] = &[".credentials"];

/// Check if a resolved path matches a protected file pattern.
fn is_protected_file(path: &Path, workspace: &Path) -> Option<&'static str> {
    // WHY: `path` is always canonical (resolved via `resolve_path`), but the
    // `workspace` we were handed may not be. On macOS `/var/...` and
    // `/private/var/...` both point to the same directory, and without matching
    // canonical forms `strip_prefix` falls through so `rel_str` becomes the
    // full absolute path and never matches `"IDENTITY.md"`. Canonicalize the
    // workspace here before stripping.
    let workspace_canonical = workspace.canonicalize();
    let ws_ref = workspace_canonical.as_deref().unwrap_or(workspace);
    let relative = path
        .strip_prefix(ws_ref)
        .or_else(|_| path.strip_prefix(workspace))
        .unwrap_or(path);
    let rel_str = relative.to_string_lossy();

    for &protected in PROTECTED_FILES {
        if rel_str == protected || rel_str.starts_with(&format!("{protected}/")) {
            return Some(protected);
        }
    }

    let filename = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
    let filename_lower = filename.to_ascii_lowercase();

    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        let ext_lower = ext.to_ascii_lowercase();
        for &protected_ext in PROTECTED_EXTENSIONS {
            if ext_lower == protected_ext {
                return Some(protected_ext);
            }
        }
    }

    for &prefix in PROTECTED_PREFIXES {
        if filename.starts_with(prefix) {
            return Some(prefix);
        }
    }

    for &dot_prefix in PROTECTED_DOT_PREFIXES {
        if filename_lower.starts_with(dot_prefix) {
            return Some(dot_prefix);
        }
    }

    PROTECTED_SUBSTRINGS
        .iter()
        .copied()
        .find(|&substring| filename_lower.contains(substring))
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
            // WHY: validate_path returns the canonical path (symlinks resolved),
            // so the I/O below operates on the resolved target, not the original
            // symlink. This eliminates the symlink-swap TOCTOU window.
            let path = validate_path(path_str, ctx, &input.name)?;

            let max_read = ctx.tool_config.max_read_bytes;
            match std::fs::metadata(&path) {
                Ok(meta) if meta.len() > max_read => {
                    return Ok(err_result(format!(
                        "file too large: {} bytes (max {max_read} bytes)",
                        meta.len(),
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

            // WHY: Enforce content size limit to prevent disk exhaustion.
            let max_write = ctx.tool_config.max_write_bytes;
            if content.len() > max_write {
                return Ok(err_result(format!(
                    "content too large: {} bytes (max {max_write} bytes)",
                    content.len(),
                )));
            }

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

            if let Some(protected) = is_protected_file(&path, &ctx.workspace) {
                return Ok(err_result(format!(
                    "cannot edit protected file: {protected}"
                )));
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
            let configured_timeout_ms =
                ctx.tool_config.subprocess_timeout_secs.saturating_mul(1000);
            let timeout_ms = extract_opt_u64(&input.arguments, "timeout")
                .map_or(configured_timeout_ms, |timeout| {
                    timeout.min(configured_timeout_ms)
                });

            let max_cmd = ctx.tool_config.max_command_length;
            if command.len() > max_cmd {
                return Ok(err_result(format!(
                    "command too long: {} bytes (max {max_cmd} bytes)",
                    command.len()
                )));
            }

            let (program, args) = match parse_command_args(command) {
                Ok(p) => p,
                Err(e) => return Ok(err_result(format!("invalid command syntax: {e}"))),
            };

            let output_result = SubprocessRunner::new(self.sandbox.clone()).run(
                SubprocessRequest::new(program, ctx.workspace.clone())
                    .args(args)
                    .timeout(Duration::from_millis(timeout_ms))
                    .max_output_bytes(MAX_OUTPUT_BYTES),
                ctx,
            );
            let out = match output_result {
                Ok(out) => out,
                Err(e) => return Ok(err_result(e.to_string())),
            };

            let code = out.exit_code;
            let stdout = out.stdout;
            let stderr = out.stderr;
            let mut output = format!("exit={code}\n{stdout}\n{stderr}");
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

            let stderr_diag = if stderr.is_empty() {
                None
            } else {
                Some(stderr)
            };
            let diagnostics = ToolDiagnostics {
                exit_code: Some(code),
                stderr: stderr_diag,
                sandbox_violations: Vec::new(),
                duration_ms: u64::try_from(out.duration.as_millis()).unwrap_or(u64::MAX),
            };

            Ok(ToolResult::text(output).with_diagnostics(diagnostics))
        })
    }
}

/// Register workspace tool executors.
pub(crate) fn register(
    registry: &mut ToolRegistry,
    sandbox: crate::sandbox::SandboxConfig,
) -> Result<()> {
    registry.register(read_def(), Box::new(ReadExecutor))?;
    registry.register(write_def(), Box::new(WriteExecutor))?;
    registry.register(edit_def(), Box::new(EditExecutor))?;
    registry.register(exec_def(), Box::new(ExecExecutor { sandbox }))?;
    Ok(())
}

fn read_def() -> ToolDef {
    ToolDef {
        name: ToolName::from_static("read"), // kanon:ignore RUST/expect
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
                        ..Default::default(),
                    },
                ),
                (
                    "maxLines".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::Number,
                        description: "Maximum lines to return (default: all)".to_owned(),
                        enum_values: None,
                        default: None,
                        ..Default::default(),
                    },
                ),
            ]),
            required: vec!["path".to_owned()],
        },
        category: ToolCategory::Workspace,
        reversibility: Reversibility::FullyReversible,
        auto_activate: true,
        groups: vec![ToolGroupId::Read],
        tags: vec![ToolTag::Recon],
    }
}

fn write_def() -> ToolDef {
    ToolDef {
        name: ToolName::from_static("write"), // kanon:ignore RUST/expect
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
                        ..Default::default(),
                    },
                ),
                (
                    "content".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::String,
                        description: "Content to write".to_owned(),
                        enum_values: None,
                        default: None,
                        ..Default::default(),
                    },
                ),
                (
                    "append".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::Boolean,
                        description: "Append instead of overwrite (default: false)".to_owned(),
                        enum_values: None,
                        default: Some(serde_json::json!(false)),
                        ..Default::default(),
                    },
                ),
            ]),
            required: vec!["path".to_owned(), "content".to_owned()],
        },
        category: ToolCategory::Workspace,
        reversibility: Reversibility::Reversible,
        auto_activate: true,
        groups: vec![ToolGroupId::Edit],
        tags: vec![ToolTag::Edit],
    }
}

fn edit_def() -> ToolDef {
    ToolDef {
        name: ToolName::from_static("edit"), // kanon:ignore RUST/expect
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
                        ..Default::default(),
                    },
                ),
                (
                    "old_text".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::String,
                        description: "Exact text to find in the file".to_owned(),
                        enum_values: None,
                        default: None,
                        ..Default::default(),
                    },
                ),
                (
                    "new_text".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::String,
                        description: "Replacement text".to_owned(),
                        enum_values: None,
                        default: None,
                        ..Default::default(),
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
        reversibility: Reversibility::Reversible,
        auto_activate: true,
        groups: vec![ToolGroupId::Edit],
        tags: vec![ToolTag::Edit],
    }
}

fn exec_def() -> ToolDef {
    ToolDef {
        name: ToolName::from_static("exec"), // kanon:ignore RUST/expect
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
                        ..Default::default(),
                    },
                ),
                (
                    "timeout".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::Number,
                        description: "Timeout in milliseconds, capped by deployment config"
                            .to_owned(),
                        enum_values: None,
                        default: None,
                        ..Default::default(),
                    },
                ),
            ]),
            required: vec!["command".to_owned()],
        },
        category: ToolCategory::Workspace,
        reversibility: Reversibility::Irreversible,
        auto_activate: true,
        groups: vec![ToolGroupId::Command],
        tags: vec![ToolTag::Execute],
    }
}

#[cfg(test)]
#[path = "workspace_tests/mod.rs"]
mod tests;

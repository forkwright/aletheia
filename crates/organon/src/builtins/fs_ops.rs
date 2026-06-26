//! Filesystem mutation tools: `mkdir`, `mv`, `cp`, `rm`.
//!
//! WHY: organon's `write` tool covers file creation, but agents have no other
//! way to create directories (outside of implicit parent creation during
//! `write`), move or rename paths, copy files, or delete paths. These are
//! standard operations on an agent workspace.
//!
//! Every operation validates paths through `workspace::validate_path` and
//! rejects:
//! - protected-file overwrites (same allowlist the workspace write/edit tools use)
//! - recursive deletion unless the caller explicitly opts in with `recursive=true`
//! - symlink following into otherwise-protected roots (validate_path resolves
//!   symlinks before the check).

use std::future::Future;
use std::path::Path;
use std::pin::Pin;

use indexmap::IndexMap;

use koina::id::ToolName;

use crate::error::Result;
use crate::registry::{ToolExecutor, ToolRegistry};
use crate::types::{
    InputSchema, PropertyDef, PropertyType, Reversibility, ToolCategory, ToolContext, ToolDef,
    ToolGroupId, ToolInput, ToolResult, ToolTag,
};

use super::workspace::{extract_opt_bool, extract_str, validate_path};

/// Sanitize a path to just its filename for error messages.
///
/// WHY: Full filesystem paths in error messages sent to the LLM leak instance
/// directory structure. Mirrors `workspace::sanitize_path_in_msg` (kept local
/// to this module to avoid making that helper `pub(crate)`).
fn sanitize(path: &Path) -> String {
    path.file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("<path>")
        .to_owned()
}

/// Reject mutations to protected files.
///
/// WHY: The write/edit tools block overwriting identity/soul/secret files.
/// Destination-taking mutations (mv, cp, rm) must enforce the same guard so
/// agents cannot route around the protection via rename/delete. We mirror the
/// `PROTECTED_FILES` allowlist used by `workspace::is_protected_file` rather
/// than importing it because the exact match semantics are tool-specific.
const PROTECTED_BASENAMES: &[&str] = &[
    "IDENTITY.md",
    "SOUL.md",
    "GOALS.md",
    "TOOLS.md",
    "MEMORY.md",
    ".git",
    ".claude",
    "standards",
];

fn is_protected(path: &Path) -> Option<&'static str> {
    let filename = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
    PROTECTED_BASENAMES
        .iter()
        .copied()
        .find(|&name| filename == name)
}

/// Re-canonicalize a path immediately before a filesystem mutation and re-check
/// it against the same `allowed_roots` invariant used by `validate_path`.
///
/// WHY: Closes the TOCTOU window between the initial `validate_path` call and
/// the actual mutation. If an attacker swaps a symlink or directory between
/// those two moments, the resolved path may now point outside allowed roots and
/// must be rejected.
fn revalidate_before_mutation(
    validated_path: &Path,
    ctx: &ToolContext,
    tool_name: &ToolName,
) -> crate::error::Result<()> {
    // Re-run the same canonicalization and root check used at validation time.
    // For existing paths this detects a symlink swapped onto the resolved
    // target; for non-existing paths it re-checks the deepest existing ancestor.
    let _ = validate_path(&validated_path.to_string_lossy(), ctx, tool_name)?;
    Ok(())
}

pub(crate) struct MkdirExecutor;

impl ToolExecutor for MkdirExecutor {
    fn execute<'a>(
        &'a self,
        input: &'a ToolInput,
        ctx: &'a ToolContext,
    ) -> Pin<Box<dyn Future<Output = Result<ToolResult>> + Send + 'a>> {
        Box::pin(async {
            let path_str = extract_str(&input.arguments, "path", &input.name)?;
            let parents = extract_opt_bool(&input.arguments, "parents").unwrap_or(true);
            let path = validate_path(path_str, ctx, &input.name)?;

            if path.exists() {
                // WHY: idempotent success mirrors POSIX `mkdir -p` behavior so
                // agents can safely call mkdir in a retry loop without
                // branching on existence.
                return Ok(ToolResult::text(format!(
                    "directory already exists: {}",
                    sanitize(&path)
                )));
            }

            let result = if parents {
                std::fs::create_dir_all(&path)
            } else {
                std::fs::create_dir(&path)
            };

            match result {
                Ok(()) => Ok(ToolResult::text(format!(
                    "created directory: {}",
                    sanitize(&path)
                ))),
                Err(e) => Ok(ToolResult::error(format!("mkdir failed: {e}"))),
            }
        })
    }
}

pub(crate) struct MvExecutor;

impl ToolExecutor for MvExecutor {
    fn execute<'a>(
        &'a self,
        input: &'a ToolInput,
        ctx: &'a ToolContext,
    ) -> Pin<Box<dyn Future<Output = Result<ToolResult>> + Send + 'a>> {
        Box::pin(async {
            let from_str = extract_str(&input.arguments, "from", &input.name)?;
            let to_str = extract_str(&input.arguments, "to", &input.name)?;
            let from = validate_path(from_str, ctx, &input.name)?;
            let to = validate_path(to_str, ctx, &input.name)?;

            if let Some(name) = is_protected(&from) {
                return Ok(ToolResult::error(format!(
                    "cannot move protected path: {name}"
                )));
            }
            if let Some(name) = is_protected(&to) {
                return Ok(ToolResult::error(format!(
                    "cannot overwrite protected path: {name}"
                )));
            }

            if !from.exists() {
                return Ok(ToolResult::error(format!(
                    "source not found: {}",
                    sanitize(&from)
                )));
            }

            // WHY: Close the TOCTOU window before the actual mutation. A
            // validated path could have its canonical target swapped (e.g. via
            // a symlink race) after the initial `validate_path` call.
            revalidate_before_mutation(&from, ctx, &input.name)?;
            revalidate_before_mutation(&to, ctx, &input.name)?;

            // WHY: std::fs::rename across mount points returns EXDEV. Fall
            // back to copy+remove so agents can move files between, for
            // example, tmpfs and the real workspace without surprises.
            match std::fs::rename(&from, &to) {
                Ok(()) => Ok(ToolResult::text(format!(
                    "moved {} -> {}",
                    sanitize(&from),
                    sanitize(&to)
                ))),
                Err(e) if e.raw_os_error() == Some(18) => {
                    // EXDEV: cross-device link
                    revalidate_before_mutation(&from, ctx, &input.name)?;
                    revalidate_before_mutation(&to, ctx, &input.name)?;
                    let copy_result = if from.is_dir() {
                        copy_dir_recursive(&from, &to)
                    } else {
                        std::fs::copy(&from, &to).map(|_| ())
                    };
                    match copy_result {
                        Ok(()) => {
                            revalidate_before_mutation(&from, ctx, &input.name)?;
                            match remove_path(&from) {
                                Ok(()) => Ok(ToolResult::text(format!(
                                    "moved (cross-device) {} -> {}",
                                    sanitize(&from),
                                    sanitize(&to)
                                ))),
                                Err(e2) => Ok(ToolResult::error(format!(
                                    "cross-device move copied but failed to remove source: {e2}"
                                ))),
                            }
                        }
                        Err(e2) => Ok(ToolResult::error(format!(
                            "cross-device move failed during copy: {e2}"
                        ))),
                    }
                }
                Err(e) => Ok(ToolResult::error(format!("mv failed: {e}"))),
            }
        })
    }
}

fn remove_path(path: &Path) -> std::io::Result<()> {
    if path.is_dir() {
        std::fs::remove_dir_all(path)
    } else {
        std::fs::remove_file(path)
    }
}

pub(crate) struct CpExecutor;

impl ToolExecutor for CpExecutor {
    fn execute<'a>(
        &'a self,
        input: &'a ToolInput,
        ctx: &'a ToolContext,
    ) -> Pin<Box<dyn Future<Output = Result<ToolResult>> + Send + 'a>> {
        Box::pin(async {
            let from_str = extract_str(&input.arguments, "from", &input.name)?;
            let to_str = extract_str(&input.arguments, "to", &input.name)?;
            let recursive = extract_opt_bool(&input.arguments, "recursive").unwrap_or(false);
            let from = validate_path(from_str, ctx, &input.name)?;
            let to = validate_path(to_str, ctx, &input.name)?;

            if let Some(name) = is_protected(&to) {
                return Ok(ToolResult::error(format!(
                    "cannot overwrite protected path: {name}"
                )));
            }

            if !from.exists() {
                return Ok(ToolResult::error(format!(
                    "source not found: {}",
                    sanitize(&from)
                )));
            }

            // WHY: Close the TOCTOU window before the actual copy. The source
            // or destination could be swapped after the initial validation.
            revalidate_before_mutation(&from, ctx, &input.name)?;
            revalidate_before_mutation(&to, ctx, &input.name)?;

            if from.is_dir() {
                if !recursive {
                    return Ok(ToolResult::error(
                        "source is a directory; pass recursive=true to copy directories".to_owned(),
                    ));
                }
                match copy_dir_recursive(&from, &to) {
                    Ok(()) => Ok(ToolResult::text(format!(
                        "copied directory {} -> {}",
                        sanitize(&from),
                        sanitize(&to)
                    ))),
                    Err(e) => Ok(ToolResult::error(format!("cp failed: {e}"))),
                }
            } else {
                match std::fs::copy(&from, &to) {
                    Ok(bytes) => Ok(ToolResult::text(format!(
                        "copied {bytes} bytes: {} -> {}",
                        sanitize(&from),
                        sanitize(&to)
                    ))),
                    Err(e) => Ok(ToolResult::error(format!("cp failed: {e}"))),
                }
            }
        })
    }
}

fn copy_dir_recursive(src: &Path, dst: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());
        let file_type = entry.file_type()?;
        if file_type.is_dir() {
            copy_dir_recursive(&src_path, &dst_path)?;
        } else if file_type.is_symlink() {
            // WHY: Reproducing symlinks verbatim would create a destination
            // link whose target may not satisfy `allowed_roots`. Fail closed
            // rather than copying an unvalidated pointer.
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!("cannot copy symlink: {}", sanitize(&src_path)),
            ));
        } else {
            std::fs::copy(&src_path, &dst_path)?;
        }
    }
    Ok(())
}

pub(crate) struct RmExecutor;

impl ToolExecutor for RmExecutor {
    fn execute<'a>(
        &'a self,
        input: &'a ToolInput,
        ctx: &'a ToolContext,
    ) -> Pin<Box<dyn Future<Output = Result<ToolResult>> + Send + 'a>> {
        Box::pin(async {
            let path_str = extract_str(&input.arguments, "path", &input.name)?;
            let recursive = extract_opt_bool(&input.arguments, "recursive").unwrap_or(false);
            let path = validate_path(path_str, ctx, &input.name)?;

            if let Some(name) = is_protected(&path) {
                return Ok(ToolResult::error(format!(
                    "cannot remove protected path: {name}"
                )));
            }

            if !path.exists() {
                return Ok(ToolResult::error(format!(
                    "path not found: {}",
                    sanitize(&path)
                )));
            }

            // WHY: Prevent accidental deletion of the workspace root or any
            // ancestor of it — a buggy agent that passes `..` chains could
            // otherwise wipe everything it has access to. validate_path
            // already restricts paths to allowed_roots; this reinforces it.
            if ctx
                .workspace
                .canonicalize()
                .ok()
                .is_some_and(|ws| ws.starts_with(&path))
            {
                return Ok(ToolResult::error(
                    "refusing to remove workspace root or ancestor".to_owned(),
                ));
            }

            let result = if path.is_dir() {
                if !recursive {
                    return Ok(ToolResult::error(
                        "path is a directory; pass recursive=true to remove directories".to_owned(),
                    ));
                }
                std::fs::remove_dir_all(&path)
            } else {
                std::fs::remove_file(&path)
            };

            match result {
                Ok(()) => Ok(ToolResult::text(format!("removed: {}", sanitize(&path)))),
                Err(e) => Ok(ToolResult::error(format!("rm failed: {e}"))),
            }
        })
    }
}

/// Register filesystem mutation tools (`mkdir`, `mv`, `cp`, `rm`).
pub(crate) fn register(registry: &mut ToolRegistry) -> Result<()> {
    registry.register(mkdir_def(), Box::new(MkdirExecutor))?;
    registry.register(mv_def(), Box::new(MvExecutor))?;
    registry.register(cp_def(), Box::new(CpExecutor))?;
    registry.register(rm_def(), Box::new(RmExecutor))?;
    Ok(())
}

fn mkdir_def() -> ToolDef {
    ToolDef {
        name: ToolName::from_static("mkdir"), // kanon:ignore RUST/expect
        description: "Create a directory (idempotent; creates parents by default).".to_owned(),
        extended_description: None,
        input_schema: InputSchema {
            properties: IndexMap::from([
                (
                    "path".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::String,
                        description: "Directory path to create".to_owned(),
                        enum_values: None,
                        default: None,
                        ..Default::default(),
                    },
                ),
                (
                    "parents".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::Boolean,
                        description: "Create parent directories as needed (default: true)"
                            .to_owned(),
                        enum_values: None,
                        default: Some(serde_json::json!(true)),
                        ..Default::default(),
                    },
                ),
            ]),
            required: vec!["path".to_owned()],
        },
        category: ToolCategory::Workspace,
        reversibility: Reversibility::Reversible,
        auto_activate: true,
        groups: vec![ToolGroupId::Edit],
        tags: vec![ToolTag::Edit],
    }
}

fn mv_def() -> ToolDef {
    ToolDef {
        name: ToolName::from_static("mv"), // kanon:ignore RUST/expect
        description: "Move or rename a file or directory.".to_owned(),
        extended_description: None,
        input_schema: InputSchema {
            properties: IndexMap::from([
                (
                    "from".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::String,
                        description: "Source path".to_owned(),
                        enum_values: None,
                        default: None,
                        ..Default::default(),
                    },
                ),
                (
                    "to".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::String,
                        description: "Destination path".to_owned(),
                        enum_values: None,
                        default: None,
                        ..Default::default(),
                    },
                ),
            ]),
            required: vec!["from".to_owned(), "to".to_owned()],
        },
        category: ToolCategory::Workspace,
        reversibility: Reversibility::Reversible,
        auto_activate: true,
        groups: vec![ToolGroupId::Edit],
        tags: vec![ToolTag::Edit],
    }
}

fn cp_def() -> ToolDef {
    ToolDef {
        name: ToolName::from_static("cp"), // kanon:ignore RUST/expect
        description: "Copy a file or directory.".to_owned(),
        extended_description: None,
        input_schema: InputSchema {
            properties: IndexMap::from([
                (
                    "from".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::String,
                        description: "Source path".to_owned(),
                        enum_values: None,
                        default: None,
                        ..Default::default(),
                    },
                ),
                (
                    "to".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::String,
                        description: "Destination path".to_owned(),
                        enum_values: None,
                        default: None,
                        ..Default::default(),
                    },
                ),
                (
                    "recursive".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::Boolean,
                        description: "Required to copy a directory (default: false)".to_owned(),
                        enum_values: None,
                        default: Some(serde_json::json!(false)),
                        ..Default::default(),
                    },
                ),
            ]),
            required: vec!["from".to_owned(), "to".to_owned()],
        },
        category: ToolCategory::Workspace,
        reversibility: Reversibility::Reversible,
        auto_activate: true,
        groups: vec![ToolGroupId::Edit],
        tags: vec![ToolTag::Edit],
    }
}

fn rm_def() -> ToolDef {
    ToolDef {
        name: ToolName::from_static("rm"), // kanon:ignore RUST/expect
        description: "Remove a file or directory (recursive requires explicit opt-in).".to_owned(),
        extended_description: None,
        input_schema: InputSchema {
            properties: IndexMap::from([
                (
                    "path".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::String,
                        description: "Path to remove".to_owned(),
                        enum_values: None,
                        default: None,
                        ..Default::default(),
                    },
                ),
                (
                    "recursive".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::Boolean,
                        description:
                            "Required to remove a directory and its contents (default: false)"
                                .to_owned(),
                        enum_values: None,
                        default: Some(serde_json::json!(false)),
                        ..Default::default(),
                    },
                ),
            ]),
            required: vec!["path".to_owned()],
        },
        category: ToolCategory::Workspace,
        // WHY: rm is not reversible from within the tool; there is no
        // built-in backup step. Mark Irreversible so the approval layer can
        // gate it appropriately.
        reversibility: Reversibility::Irreversible,
        auto_activate: true,
        groups: vec![ToolGroupId::Edit],
        tags: vec![ToolTag::Edit],
    }
}

#[cfg(test)]
#[path = "fs_ops_tests.rs"]
mod tests;

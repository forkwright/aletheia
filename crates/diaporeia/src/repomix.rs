//! Native repomix-style packing for Rust crate subsets.
//!
//! Implements token-efficient source packing without requiring the external
//! `repomix` npm binary. Walks crate directories, strips comments, collapses
//! whitespace, and emits structured output that fits provider context windows.

use std::collections::{HashMap, HashSet};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use crate::error::{RepomixPackSnafu, TemplateNotFoundSnafu};

/// A built-in or custom repomix template.
#[derive(Debug, Clone)]
pub(crate) struct TemplateDef {
    pub name: String,
    pub description: String,
    pub include_deps: bool,
    pub dep_scope: DepScope,
}

/// How much of dependency crates to include.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum DepScope {
    /// Only the target crate(s).
    None,
    /// Public API only (`src/lib.rs`, `src/prelude.rs`).
    PublicApi,
    /// Full source of listed dependencies.
    #[expect(dead_code, reason = "reserved for future full-source packing template")]
    Full,
}

/// Information about an available template.
#[derive(Debug, Clone, serde::Serialize)]
pub(crate) struct TemplateInfo {
    pub name: String,
    pub description: String,
}

/// Result of a pack operation.
#[derive(Debug, Clone, serde::Serialize)]
pub(crate) struct PackResult {
    pub packed: String,
    pub files_included: usize,
    pub raw_bytes: usize,
    pub packed_bytes: usize,
    pub estimated_tokens: usize,
}

#[derive(Debug)]
struct WorkspacePackages {
    root: PathBuf,
    package_dirs: HashMap<String, PathBuf>,
    deps_by_package: HashMap<String, Vec<String>>,
}

#[derive(Debug)]
struct PackedFile {
    canonical_path: PathBuf,
    display_path: PathBuf,
    content: String,
}

/// List all built-in repomix templates.
pub(crate) fn list_templates() -> Vec<TemplateInfo> {
    built_in_templates()
        .into_iter()
        .map(|t| TemplateInfo {
            name: t.name,
            description: t.description,
        })
        .collect()
}

/// Get a single template definition by name.
pub(crate) fn get_template(name: &str) -> Result<TemplateDef, crate::error::Error> {
    built_in_templates()
        .into_iter()
        .find(|t| t.name == name)
        .ok_or_else(|| {
            TemplateNotFoundSnafu {
                name: name.to_owned(),
            }
            .build()
        })
}

fn built_in_templates() -> Vec<TemplateDef> {
    vec![
        TemplateDef {
            name: "single_crate".to_owned(),
            description: "Pack one crate's src/ directory with tree-sitter-like compression."
                .to_owned(),
            include_deps: false,
            dep_scope: DepScope::None,
        },
        TemplateDef {
            name: "crate_with_deps".to_owned(),
            description: "Pack target crate plus the public API of its workspace dependencies."
                .to_owned(),
            include_deps: true,
            dep_scope: DepScope::PublicApi,
        },
        TemplateDef {
            name: "cross_crate".to_owned(),
            description: "Pack multiple crates' src/ files in full.".to_owned(),
            include_deps: false,
            dep_scope: DepScope::None,
        },
    ]
}

/// Pack crate source code according to the requested template.
///
/// `workspace_root` is the directory containing the workspace `Cargo.toml`.
/// `crate_names` are the target crate names (e.g. `["diaporeia"]`).
/// `max_tokens` caps the output; if the packed result exceeds the budget,
/// files are truncated and a summary header is prepended.
pub(crate) fn pack(
    workspace_root: &Path,
    crate_names: &[String],
    template_name: &str,
    max_tokens: u32,
) -> Result<PackResult, crate::error::Error> {
    let template = get_template(template_name)?;
    for crate_name in crate_names {
        validate_package_name(crate_name)?;
    }

    let workspace = WorkspacePackages::load(workspace_root)?;
    let mut files: Vec<PackedFile> = Vec::new();

    for crate_name in crate_names {
        let crate_dir = workspace.package_dir(crate_name)?;
        let src_dir = crate_dir.join("src");
        if src_dir.is_dir() {
            walk_rs_files(&workspace.root, &src_dir, &mut files)?;
        }

        if template.include_deps && template.dep_scope != DepScope::None {
            let deps = workspace.workspace_deps(crate_name)?;
            for dep in deps {
                let dep_dir = workspace.package_dir(&dep)?;
                let dep_src = dep_dir.join("src");
                if !dep_src.is_dir() {
                    continue;
                }
                match template.dep_scope {
                    DepScope::PublicApi => {
                        for entry in ["lib.rs", "prelude.rs"] {
                            let p = dep_src.join(entry);
                            if p.is_file() {
                                let canonical = canonicalize_contained_path(&workspace.root, &p)?;
                                let display_path =
                                    workspace_relative_path(&workspace.root, &canonical)?;
                                let content = std::fs::read_to_string(&canonical).map_err(|e| {
                                    RepomixPackSnafu {
                                        message: format!("read {}: {e}", canonical.display()),
                                    }
                                    .build()
                                })?;
                                files.push(PackedFile {
                                    canonical_path: canonical,
                                    display_path,
                                    content,
                                });
                            }
                        }
                    }
                    DepScope::Full => {
                        walk_rs_files(&workspace.root, &dep_src, &mut files)?;
                    }
                    DepScope::None => {}
                }
            }
        }
    }

    // Deduplicate by path (cross_crate may overlap with deps).
    let mut seen = HashSet::new();
    files.retain(|file| seen.insert(file.canonical_path.clone()));

    let mut packed = String::new();
    packed.push_str("<packed_context>\n");
    let _ = writeln!(
        packed,
        "<metadata template=\"{}\" crates=\"{}\" />",
        template_name,
        crate_names.join(",")
    );

    let mut raw_bytes = 0usize;
    let mut packed_bytes = 0usize;

    // Greedy inclusion respecting token budget. ~4 bytes per token for code.
    let max_bytes = usize::try_from(max_tokens)
        .unwrap_or(usize::MAX)
        .saturating_mul(4);
    let mut included = 0usize;
    let header_len = packed.len();
    let footer_len = "</packed_context>\n".len();
    let mut budget = max_bytes.saturating_sub(header_len + footer_len);

    for file in &files {
        let content = file.content.as_str();
        let compressed = compress(content);
        let entry = format_file_entry(&file.display_path, &compressed);
        let entry_bytes = entry.len();
        raw_bytes += content.len();

        if entry_bytes > budget && included > 0 {
            let _ = writeln!(
                packed,
                "<!-- truncated: {} files omitted due to token budget -->",
                files.len() - included
            );
            break;
        }

        packed.push_str(&entry);
        packed_bytes += compressed.len();
        budget = budget.saturating_sub(entry_bytes);
        included += 1;
    }

    packed.push_str("</packed_context>\n");

    // Rough token estimate: packed bytes / 4 for ASCII code.
    let estimated_tokens = packed.len() / 4;

    Ok(PackResult {
        packed,
        files_included: included,
        raw_bytes,
        packed_bytes,
        estimated_tokens,
    })
}

/// Detect the workspace root by walking up from the current directory.
pub(crate) fn detect_workspace_root() -> Option<PathBuf> {
    let mut current = std::env::current_dir().ok()?;
    loop {
        let cargo_toml = current.join("Cargo.toml");
        if cargo_toml.exists()
            && let Ok(content) = std::fs::read_to_string(&cargo_toml)
            && (content.contains("[workspace]") || current.join("crates").is_dir())
        {
            return Some(current);
        }
        if !current.pop() {
            break;
        }
    }
    None
}

impl WorkspacePackages {
    fn load(workspace_root: &Path) -> Result<Self, crate::error::Error> {
        let root = std::fs::canonicalize(workspace_root).map_err(|e| {
            RepomixPackSnafu {
                message: format!("canonicalize workspace root: {e}"),
            }
            .build()
        })?;
        let manifest_path = root.join("Cargo.toml");
        let metadata = cargo_metadata::MetadataCommand::new()
            .manifest_path(&manifest_path)
            .exec()
            .map_err(|e| {
                RepomixPackSnafu {
                    message: format!("load Cargo workspace metadata: {e}"),
                }
                .build()
            })?;

        let workspace_member_ids: HashSet<String> = metadata
            .workspace_members
            .iter()
            .map(ToString::to_string)
            .collect();
        let mut package_dirs = HashMap::new();
        let mut name_by_id = HashMap::new();

        for package in &metadata.packages {
            let package_id = package.id.to_string();
            if !workspace_member_ids.contains(&package_id) {
                continue;
            }
            let manifest_dir = package.manifest_path.parent().ok_or_else(|| {
                RepomixPackSnafu {
                    message: format!("package '{}' manifest has no parent", package.name),
                }
                .build()
            })?;
            let package_dir = canonicalize_contained_path(&root, manifest_dir.as_std_path())?;
            if package_dirs
                .insert(package.name.clone(), package_dir)
                .is_some()
            {
                return Err(RepomixPackSnafu {
                    message: format!("duplicate workspace package name '{}'", package.name),
                }
                .build());
            }
            name_by_id.insert(package_id, package.name.clone());
        }

        let mut deps_by_package = HashMap::new();
        if let Some(resolve) = &metadata.resolve {
            for node in &resolve.nodes {
                let node_id = node.id.to_string();
                let Some(package_name) = name_by_id.get(&node_id) else {
                    continue;
                };
                let deps = node
                    .dependencies
                    .iter()
                    .filter_map(|dep| name_by_id.get(&dep.to_string()).cloned())
                    .collect();
                deps_by_package.insert(package_name.clone(), deps);
            }
        }

        Ok(Self {
            root,
            package_dirs,
            deps_by_package,
        })
    }

    fn package_dir(&self, package_name: &str) -> Result<&Path, crate::error::Error> {
        self.package_dirs
            .get(package_name)
            .map(PathBuf::as_path)
            .ok_or_else(|| {
                RepomixPackSnafu {
                    message: format!("Cargo package not found in workspace: '{package_name}'"),
                }
                .build()
            })
    }

    fn workspace_deps(&self, package_name: &str) -> Result<Vec<String>, crate::error::Error> {
        self.package_dir(package_name)?;
        Ok(self
            .deps_by_package
            .get(package_name)
            .cloned()
            .unwrap_or_default())
    }
}

fn validate_package_name(package_name: &str) -> Result<(), crate::error::Error> {
    if package_name.is_empty() {
        return Err(RepomixPackSnafu {
            message: "crate name must not be empty".to_owned(),
        }
        .build());
    }
    if Path::new(package_name).is_absolute() {
        return Err(RepomixPackSnafu {
            message: format!("crate name must be a Cargo package name, not a path: {package_name}"),
        }
        .build());
    }
    if package_name.contains("..") {
        return Err(RepomixPackSnafu {
            message: format!("crate name must not contain '..': {package_name}"),
        }
        .build());
    }
    if package_name.contains('/') || package_name.contains('\\') {
        return Err(RepomixPackSnafu {
            message: format!("crate name must not contain path separators: {package_name}"),
        }
        .build());
    }
    if !package_name
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return Err(RepomixPackSnafu {
            message: format!("crate name contains invalid characters: {package_name}"),
        }
        .build());
    }
    Ok(())
}

fn canonicalize_contained_path(
    workspace_root: &Path,
    path: &Path,
) -> Result<PathBuf, crate::error::Error> {
    let canonical = std::fs::canonicalize(path).map_err(|e| {
        RepomixPackSnafu {
            message: format!("canonicalize {}: {e}", path.display()),
        }
        .build()
    })?;
    if !canonical.starts_with(workspace_root) {
        return Err(RepomixPackSnafu {
            message: format!(
                "resolved path escapes workspace root: {}",
                canonical.display()
            ),
        }
        .build());
    }
    Ok(canonical)
}

fn workspace_relative_path(
    workspace_root: &Path,
    path: &Path,
) -> Result<PathBuf, crate::error::Error> {
    path.strip_prefix(workspace_root)
        .map(Path::to_path_buf)
        .map_err(|_strip_err| {
            RepomixPackSnafu {
                message: format!("resolved path escapes workspace root: {}", path.display()),
            }
            .build()
        })
}

fn walk_rs_files(
    workspace_root: &Path,
    dir: &Path,
    out: &mut Vec<PackedFile>,
) -> Result<(), crate::error::Error> {
    let mut stack = vec![dir.to_path_buf()];
    let mut seen_dirs = HashSet::new();
    while let Some(current) = stack.pop() {
        let current = canonicalize_contained_path(workspace_root, &current)?;
        if !seen_dirs.insert(current.clone()) {
            continue;
        }
        let entries = std::fs::read_dir(&current).map_err(|e| {
            RepomixPackSnafu {
                message: format!("read directory {}: {e}", current.display()),
            }
            .build()
        })?;
        for entry in entries.flatten() {
            let canonical = canonicalize_contained_path(workspace_root, &entry.path())?;
            if canonical.is_dir() {
                stack.push(canonical);
            } else if canonical.extension().is_some_and(|e| e == "rs") {
                let display_path = workspace_relative_path(workspace_root, &canonical)?;
                let content = std::fs::read_to_string(&canonical).map_err(|e| {
                    RepomixPackSnafu {
                        message: format!("read {}: {e}", canonical.display()),
                    }
                    .build()
                })?;
                out.push(PackedFile {
                    canonical_path: canonical,
                    display_path,
                    content,
                });
            }
        }
    }
    Ok(())
}

fn format_file_entry(path: &Path, compressed: &str) -> String {
    let display_path = path.to_string_lossy().replace('\\', "/");
    format!("<file path=\"{display_path}\">\n{compressed}\n</file>\n")
}

/// Remove comments and collapse whitespace to approximate tree-sitter compression.
fn compress(source: &str) -> String {
    let mut output = String::with_capacity(source.len());
    let chars: Vec<char> = source.chars().collect();
    let mut i = 0;
    let mut in_line_comment = false;
    let mut in_block_comment = false;
    let mut prev_was_blank = false;

    while i < chars.len() {
        let Some(&c) = chars.get(i) else {
            break;
        };
        let next = chars.get(i + 1).copied();

        if in_line_comment {
            if c == '\n' {
                in_line_comment = false;
                if !prev_was_blank {
                    output.push('\n');
                    prev_was_blank = true;
                }
            }
            i += 1;
            continue;
        }

        if in_block_comment {
            if c == '*' && next == Some('/') {
                in_block_comment = false;
                i += 2;
                continue;
            }
            i += 1;
            continue;
        }

        if c == '/' && next == Some('/') {
            in_line_comment = true;
            i += 2;
            continue;
        }

        if c == '/' && next == Some('*') {
            in_block_comment = true;
            i += 2;
            continue;
        }

        if c.is_whitespace() {
            if c == '\n' {
                if !prev_was_blank {
                    output.push('\n');
                    prev_was_blank = true;
                }
            } else if output.ends_with('\n') || output.is_empty() {
                // NOTE: intentionally drop leading whitespace on a line.
            } else {
                output.push(' ');
            }
            i += 1;
            continue;
        }

        prev_was_blank = false;
        output.push(c);
        i += 1;
    }

    while output.ends_with("\n\n") {
        output.pop();
    }
    output
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions")]
#[expect(
    clippy::disallowed_methods,
    reason = "test fixture setup uses sync std::fs to write files before exercising pack()"
)]
mod tests {
    use super::*;

    fn write_workspace(root: &Path, members: &[&str]) {
        let members = members
            .iter()
            .map(|member| format!("    \"{member}\","))
            .collect::<Vec<_>>()
            .join("\n");
        std::fs::write(
            root.join("Cargo.toml"),
            format!("[workspace]\nresolver = \"2\"\nmembers = [\n{members}\n]\n"),
        )
        .unwrap();
    }

    fn write_crate(root: &Path, member: &str, package_name: &str, extra_manifest: &str, lib: &str) {
        let package = root.join(member);
        std::fs::create_dir_all(package.join("src")).unwrap();
        std::fs::write(
            package.join("Cargo.toml"),
            format!(
                "[package]\nname = \"{package_name}\"\nversion = \"0.1.0\"\nedition = \"2024\"\n{extra_manifest}"
            ),
        )
        .unwrap();
        std::fs::write(package.join("src/lib.rs"), lib).unwrap();
    }

    #[test]
    fn list_templates_has_three_builtins() {
        let list = list_templates();
        assert_eq!(list.len(), 3);
        assert!(list.iter().any(|t| t.name == "single_crate"));
        assert!(list.iter().any(|t| t.name == "crate_with_deps"));
        assert!(list.iter().any(|t| t.name == "cross_crate"));
    }

    #[test]
    fn get_template_found() {
        let t = get_template("single_crate").unwrap();
        assert_eq!(t.name, "single_crate");
    }

    #[test]
    fn get_template_not_found() {
        let err = get_template("nonexistent").unwrap_err();
        assert!(err.to_string().contains("nonexistent"));
    }

    #[test]
    fn compress_removes_line_comments() {
        let src = "let x = 1; // comment\nlet y = 2;";
        let out = compress(src);
        assert!(!out.contains("comment"));
        assert!(out.contains("let x = 1;"));
        assert!(out.contains("let y = 2;"));
    }

    #[test]
    fn compress_removes_block_comments() {
        let src = "/* block \n comment */ let x = 1;";
        let out = compress(src);
        assert!(!out.contains("block"));
        assert!(!out.contains("comment"));
        assert!(out.contains("let x = 1;"));
    }

    #[test]
    fn pack_respects_max_tokens() {
        let dir = tempfile::tempdir().unwrap();
        write_workspace(dir.path(), &["crates/alpha"]);
        write_crate(
            dir.path(),
            "crates/alpha",
            "alpha",
            "",
            "// lib\npub fn add(a: i32, b: i32) -> i32 { a + b }\n",
        );

        let result = pack(dir.path(), &["alpha".to_owned()], "single_crate", 10).unwrap();
        assert!(result.packed.contains("<packed_context>"));
        assert!(result.packed.contains("add(a: i32"));
        assert!(result.packed.contains("crates/alpha/src/lib.rs"));
        assert!(
            !result
                .packed
                .contains(dir.path().to_string_lossy().as_ref()),
            "packed output must not expose absolute workspace paths"
        );
        assert!(result.files_included >= 1);
        assert!(
            result.estimated_tokens <= 80,
            "tokens: {}",
            result.estimated_tokens
        );
    }

    #[test]
    fn pack_with_workspace_deps() {
        let dir = tempfile::tempdir().unwrap();
        write_workspace(dir.path(), &["crates/alpha", "crates/beta"]);
        write_crate(
            dir.path(),
            "crates/alpha",
            "alpha",
            "\n[dependencies]\nbeta = { path = \"../beta\" }\n",
            "pub fn a() {}\n",
        );
        write_crate(dir.path(), "crates/beta", "beta", "", "pub fn b() {}\n");

        let result = pack(dir.path(), &["alpha".to_owned()], "crate_with_deps", 1000).unwrap();
        assert!(result.packed.contains("fn a()"));
        assert!(result.packed.contains("fn b()"));
    }

    #[test]
    fn pack_rejects_absolute_crate_name() {
        let err = pack(
            Path::new("/missing-workspace"),
            &["/tmp/alpha".to_owned()],
            "single_crate",
            1000,
        )
        .unwrap_err();

        assert!(err.to_string().contains("not a path"));
    }

    #[test]
    fn pack_rejects_parent_segment_crate_name() {
        let err = pack(
            Path::new("/missing-workspace"),
            &["alpha..beta".to_owned()],
            "single_crate",
            1000,
        )
        .unwrap_err();

        assert!(err.to_string().contains("must not contain '..'"));
    }

    #[test]
    fn pack_rejects_separator_injection_crate_name() {
        let err = pack(
            Path::new("/missing-workspace"),
            &["crates/alpha".to_owned()],
            "single_crate",
            1000,
        )
        .unwrap_err();

        assert!(err.to_string().contains("path separators"));
    }

    #[test]
    fn pack_rejects_backslash_separator_injection_crate_name() {
        let err = pack(
            Path::new("/missing-workspace"),
            &["crates\\alpha".to_owned()],
            "single_crate",
            1000,
        )
        .unwrap_err();

        assert!(err.to_string().contains("path separators"));
    }

    #[test]
    #[cfg(unix)]
    fn pack_rejects_symlinked_package_escape() {
        let workspace = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        write_workspace(workspace.path(), &["crates/escape"]);
        write_crate(
            outside.path(),
            "",
            "escape",
            "",
            "pub fn outside_workspace() {}\n",
        );
        std::fs::create_dir_all(workspace.path().join("crates")).unwrap();
        std::os::unix::fs::symlink(outside.path(), workspace.path().join("crates/escape")).unwrap();

        let err = pack(
            workspace.path(),
            &["escape".to_owned()],
            "single_crate",
            1000,
        )
        .unwrap_err();

        assert!(err.to_string().contains("escapes workspace root"));
    }
}

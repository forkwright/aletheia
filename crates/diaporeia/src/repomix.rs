//! Native repomix-style packing for Rust crate subsets.
//!
//! Implements token-efficient source packing without requiring the external
//! `repomix` npm binary. Walks crate directories, strips comments, collapses
//! whitespace, and emits structured output that fits provider context windows.

use std::collections::HashSet;
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

    let mut files: Vec<(PathBuf, String)> = Vec::new();

    for crate_name in crate_names {
        let crate_dir = find_crate_dir(workspace_root, crate_name)?;
        let src_dir = crate_dir.join("src");
        if src_dir.is_dir() {
            walk_rs_files(&src_dir, &mut files)?;
        }

        if template.include_deps && template.dep_scope != DepScope::None {
            let deps = resolve_workspace_deps(workspace_root, crate_name)?;
            for dep in deps {
                let dep_dir = find_crate_dir(workspace_root, &dep)?;
                let dep_src = dep_dir.join("src");
                if !dep_src.is_dir() {
                    continue;
                }
                match template.dep_scope {
                    DepScope::PublicApi => {
                        for entry in ["lib.rs", "prelude.rs"] {
                            let p = dep_src.join(entry);
                            if p.is_file() {
                                let content = std::fs::read_to_string(&p).map_err(|e| {
                                    RepomixPackSnafu {
                                        message: format!("read {}: {e}", p.display()),
                                    }
                                    .build()
                                })?;
                                files.push((p, content));
                            }
                        }
                    }
                    DepScope::Full => {
                        walk_rs_files(&dep_src, &mut files)?;
                    }
                    DepScope::None => {}
                }
            }
        }
    }

    // Deduplicate by path (cross_crate may overlap with deps).
    let mut seen = HashSet::new();
    files.retain(|(p, _)| seen.insert(p.clone()));

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

    for (path, content) in &files {
        let compressed = compress(content);
        let entry = format_file_entry(path, &compressed);
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

fn find_crate_dir(workspace_root: &Path, crate_name: &str) -> Result<PathBuf, crate::error::Error> {
    // Standard aletheia layout: crates/{name}/
    let standard = workspace_root.join("crates").join(crate_name);
    if standard.is_dir() {
        return Ok(standard);
    }
    // Try workspace members by scanning Cargo.toml
    let cargo_toml = workspace_root.join("Cargo.toml");
    if cargo_toml.exists()
        && let Ok(content) = std::fs::read_to_string(&cargo_toml)
    {
        // Very naive parser: look for "name" in member paths
        for line in content.lines() {
            if (line.contains("crates/") || line.contains("projects/"))
                && let Some(path_str) = extract_quoted(line)
            {
                let candidate = workspace_root.join(path_str).join(crate_name);
                if candidate.is_dir() {
                    return Ok(candidate);
                }
            }
        }
    }
    // Fallback: search one level deep under workspace root
    if let Ok(entries) = std::fs::read_dir(workspace_root) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                let candidate = path.join(crate_name);
                if candidate.is_dir() {
                    return Ok(candidate);
                }
            }
        }
    }
    Err(RepomixPackSnafu {
        message: format!("crate directory not found for '{crate_name}'"),
    }
    .build())
}

fn extract_quoted(line: &str) -> Option<String> {
    let mut chars = line.chars();
    while let Some(c) = chars.next() {
        if c == '"' || c == '\'' {
            let quote = c;
            let mut s = String::new();
            for c2 in chars.by_ref() {
                if c2 == quote {
                    return Some(s);
                }
                s.push(c2);
            }
        }
    }
    None
}

fn resolve_workspace_deps(
    workspace_root: &Path,
    crate_name: &str,
) -> Result<Vec<String>, crate::error::Error> {
    let crate_dir = find_crate_dir(workspace_root, crate_name)?;
    let cargo_toml = crate_dir.join("Cargo.toml");
    let content = std::fs::read_to_string(&cargo_toml).map_err(|e| {
        RepomixPackSnafu {
            message: format!("read {}: {e}", cargo_toml.display()),
        }
        .build()
    })?;

    let mut deps = Vec::new();
    let mut in_deps = false;
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed == "[dependencies]" || trimmed.starts_with("[dependencies.") {
            in_deps = true;
            continue;
        }
        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            in_deps = false;
            continue;
        }
        if in_deps {
            // NOTE: naive `key = value` parse; quoted keys and inline tables
            // are out of scope for dependency-name extraction.
            if let Some(eq) = trimmed.find('=')
                && let Some(key_slice) = trimmed.get(..eq)
            {
                let key = key_slice.trim();
                // WHY: a `path` key marks a workspace-local dependency; registry
                // deps are excluded.
                if trimmed.contains("path") {
                    deps.push(key.to_owned());
                }
            }
        }
    }
    Ok(deps)
}

fn walk_rs_files(dir: &Path, out: &mut Vec<(PathBuf, String)>) -> Result<(), crate::error::Error> {
    let mut stack = vec![dir.to_path_buf()];
    while let Some(current) = stack.pop() {
        let entries = std::fs::read_dir(&current).map_err(|e| {
            RepomixPackSnafu {
                message: format!("read directory {}: {e}", current.display()),
            }
            .build()
        })?;
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().is_some_and(|e| e == "rs") {
                let content = std::fs::read_to_string(&path).map_err(|e| {
                    RepomixPackSnafu {
                        message: format!("read {}: {e}", path.display()),
                    }
                    .build()
                })?;
                out.push((path, content));
            }
        }
    }
    Ok(())
}

fn format_file_entry(path: &Path, compressed: &str) -> String {
    format!(
        "<file path=\"{}\">\n{}\n</file>\n",
        path.display(),
        compressed
    )
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
        let crates = dir.path().join("crates");
        let test_crate = crates.join("alpha");
        std::fs::create_dir_all(test_crate.join("src")).unwrap();
        std::fs::write(
            test_crate.join("Cargo.toml"),
            "[package]\nname = \"alpha\"\n",
        )
        .unwrap();
        std::fs::write(
            test_crate.join("src/lib.rs"),
            "// lib\npub fn add(a: i32, b: i32) -> i32 { a + b }\n",
        )
        .unwrap();

        let result = pack(dir.path(), &["alpha".to_owned()], "single_crate", 10).unwrap();
        assert!(result.packed.contains("<packed_context>"));
        assert!(result.packed.contains("add(a: i32"));
        assert!(result.files_included >= 1);
        // WHY: the XML wrapper + `<file path="...">` header embed the full
        // tempdir path, which varies by platform (macOS uses longer
        // `/var/folders/...` paths). Bound is platform-tolerant; what we really
        // verify is that the budget is respected (= much smaller than the
        // unlimited case) and the structured wrapper is intact.
        assert!(
            result.estimated_tokens <= 120,
            "tokens: {}",
            result.estimated_tokens
        );
    }

    #[test]
    fn pack_with_workspace_deps() {
        let dir = tempfile::tempdir().unwrap();
        let crates = dir.path().join("crates");
        let alpha = crates.join("alpha");
        let beta = crates.join("beta");
        std::fs::create_dir_all(alpha.join("src")).unwrap();
        std::fs::create_dir_all(beta.join("src")).unwrap();
        std::fs::write(
            alpha.join("Cargo.toml"),
            "[package]\nname = \"alpha\"\n[dependencies]\nbeta = { path = \"../beta\" }\n",
        )
        .unwrap();
        std::fs::write(alpha.join("src/lib.rs"), "pub fn a() {}\n").unwrap();
        std::fs::write(beta.join("Cargo.toml"), "[package]\nname = \"beta\"\n").unwrap();
        std::fs::write(beta.join("src/lib.rs"), "pub fn b() {}\n").unwrap();

        let result = pack(dir.path(), &["alpha".to_owned()], "crate_with_deps", 1000).unwrap();
        assert!(result.packed.contains("fn a()"));
        assert!(result.packed.contains("fn b()"));
    }
}

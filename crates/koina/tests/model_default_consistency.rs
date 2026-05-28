//! Cross-crate consistency test for `koina::defaults::DEFAULT_MODEL`.
//!
//! WHY (#4235): before this campaign there were two model-default constants
//! — `DEFAULT_MODEL` (Sonnet 4.0, May 2025) and `DEFAULT_MODEL_SHORT`
//! (Sonnet 4.6). The names suggested they were two forms of one model;
//! they were two different models. `aletheia init` and `add-nous`
//! scaffolded Sonnet 4.6 while runtime spawn, distillation, and pylon
//! request-fallback silently routed to Sonnet 4.0 — a downgrade invisible
//! at config time.
//!
//! The collapse to a single `DEFAULT_MODEL` only holds if it stays a
//! single constant. This test walks the workspace and fails loudly if a
//! second `DEFAULT_MODEL*` constant reappears anywhere outside the
//! authoritative source-of-truth (`crates/koina/src/defaults.rs`).

#![expect(clippy::expect_used, reason = "test assertions")]

use std::fs;
use std::path::{Path, PathBuf};

/// Resolve the workspace root via koina's `CARGO_MANIFEST_DIR` (== `crates/koina`).
fn workspace_root() -> PathBuf {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    PathBuf::from(manifest_dir)
        .join("..")
        .join("..")
        .canonicalize()
        .expect("workspace root resolves from CARGO_MANIFEST_DIR/../..")
}

/// Recursively collect every `.rs` file under `dir` into `out`.
fn collect_rs_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            // Skip target/, .git/, fuzz corpora — they are not workspace source.
            let name = path.file_name().and_then(|s| s.to_str()).unwrap_or("");
            if matches!(name, "target" | ".git" | "node_modules") {
                continue;
            }
            collect_rs_files(&path, out);
        } else if path.extension().and_then(|s| s.to_str()) == Some("rs") {
            out.push(path);
        }
    }
}

/// True iff `line` declares a `pub const DEFAULT_MODEL[_*]: &str = ...`.
///
/// Restrictions are deliberate, both narrowing what counts as "drift":
///
/// 1. **`pub const` only.** A module-private `const DEFAULT_MODEL` inside a
///    provider crate (e.g. `hermeneus::kimi::DEFAULT_MODEL =
///    "kimi/kimi-k2-thinking"`) is that provider's local default, not a
///    workspace-wide one — it cannot be imported and silently override the
///    real default the way #4235's `DEFAULT_MODEL_SHORT` did.
/// 2. **`: &str` only.** The drift pattern is two different *model name
///    strings*. A numeric `DEFAULT_MODEL_VERSION: u32 = 4` is harmless.
/// 3. **Name starts with `DEFAULT_MODEL`.** Matches the bare name and any
///    suffixed variant (`_SHORT`, `_FULL`, `_FALLBACK`, …). The dual-name
///    suffix is the specific pattern the issue called out.
///
/// A reviewer adding e.g. `pub const DEFAULT_MODEL_DIR: &str` to koina
/// itself would trip this test as a false positive — that's fine; it's a
/// loud prompt to either rename or add a justified allowlist entry.
fn declares_pub_default_model_str(line: &str) -> bool {
    let trimmed = line.trim_start();
    let Some(rest) = trimmed.strip_prefix("pub const ") else {
        return false;
    };
    let Some((ident, after)) = rest.split_once(':') else {
        return false;
    };
    if !ident.trim_end().starts_with("DEFAULT_MODEL") {
        return false;
    }
    after.trim_start().starts_with("&str")
}

#[test]
fn default_model_value_pinned_to_sonnet_4_6() {
    // WHY: the issue identified Sonnet 4.6 as the single shared default —
    // `aletheia init`, `add-nous`, theatron wizard, and the documented
    // `instance.example/config/aletheia.toml` all converged on it before
    // the collapse. Pinning the constant here makes any future "drive-by
    // bump" visible in the diff.
    assert_eq!(koina::defaults::DEFAULT_MODEL, "claude-sonnet-4-6");
}

#[test]
fn only_one_default_model_constant_in_workspace() {
    let root = workspace_root();
    let crates_dir = root.join("crates");
    assert!(
        crates_dir.is_dir(),
        "expected workspace crates/ at {}",
        crates_dir.display()
    );

    let mut files = Vec::new();
    collect_rs_files(&crates_dir, &mut files);

    let mut declarations: Vec<(PathBuf, usize, String)> = Vec::new();
    for path in &files {
        let Ok(text) = fs::read_to_string(path) else {
            continue;
        };
        for (idx, line) in text.lines().enumerate() {
            if declares_pub_default_model_str(line) {
                declarations.push((path.clone(), idx + 1, line.trim().to_owned()));
            }
        }
    }

    // The one true source-of-truth (relative to workspace root for stable error messages).
    let canonical = root.join("crates/koina/src/defaults.rs");

    let extras: Vec<&(PathBuf, usize, String)> = declarations
        .iter()
        .filter(|(path, _, _)| path != &canonical)
        .collect();

    assert!(
        extras.is_empty(),
        "found `DEFAULT_MODEL*: &str` constants outside crates/koina/src/defaults.rs \
         (re-introduces the #4235 silent-drift pattern):\n{}",
        extras
            .iter()
            .map(|(p, line, src)| format!(
                "  {}:{} — {}",
                p.strip_prefix(&root).unwrap_or(p).display(),
                line,
                src
            ))
            .collect::<Vec<_>>()
            .join("\n")
    );

    let in_canonical: Vec<&(PathBuf, usize, String)> = declarations
        .iter()
        .filter(|(path, _, _)| path == &canonical)
        .collect();
    assert_eq!(
        in_canonical.len(),
        1,
        "expected exactly one `DEFAULT_MODEL*: &str` declaration in koina/src/defaults.rs, \
         found {}: {:?}",
        in_canonical.len(),
        in_canonical
            .iter()
            .map(|(_, line, src)| format!("L{line}: {src}"))
            .collect::<Vec<_>>()
    );
}

#[test]
fn helper_detects_declaration_shapes() {
    // WHY: the cross-crate test above is only as good as the line-pattern
    // it matches. Lock the patterns it accepts here so a future refactor
    // of the helper doesn't silently weaken the workspace walk.
    assert!(declares_pub_default_model_str(
        "pub const DEFAULT_MODEL: &str = \"x\";"
    ));
    assert!(declares_pub_default_model_str(
        "    pub const DEFAULT_MODEL_SHORT: &str = \"y\";"
    ));
    assert!(declares_pub_default_model_str(
        "pub const DEFAULT_MODEL_FALLBACK: &str = \"z\";"
    ));
    assert!(!declares_pub_default_model_str(
        "const DEFAULT_MODEL: &str = \"kimi/kimi-k2-thinking\";"
    ));
    assert!(!declares_pub_default_model_str(
        "pub const DEFAULT_AGENT_ID: &str = \"x\";"
    ));
    assert!(!declares_pub_default_model_str(
        "pub const DEFAULT_MODEL_VERSION: u32 = 4;"
    ));
    assert!(!declares_pub_default_model_str(
        "let DEFAULT_MODEL: &str = \"x\";"
    ));
}

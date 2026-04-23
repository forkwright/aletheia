//! Vocabulary discipline rules: hub-word consistency checks.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use snafu::ResultExt;

use crate::error::{self, Result};
use crate::rules::{Rule, Violation};

/// Path to the hub-words registry relative to the project root.
const HUB_WORDS_PATH: &str = "crates/aletheia/hub-words.toml";

/// Files exempt from hub-word discipline.
const EXEMPT_FILES: &[&str] = &["vision.md", "ROADMAP.md"];

/// A distinct concept that is related but not the same as the hub word.
#[derive(Debug, Deserialize)]
#[expect(dead_code, reason = "registry fields reserved for future use")]
struct DistinctConcept {
    name: String,
    definition: String,
    file: String,
}

/// A single hub-word definition.
#[derive(Debug, Deserialize)]
#[expect(dead_code, reason = "registry fields reserved for future use")]
struct HubWordEntry {
    canonical: String,
    #[serde(default)]
    implementations: Vec<String>,
    #[serde(default)]
    forbidden_synonyms: Vec<String>,
    #[serde(default)]
    distinct_concepts: Vec<DistinctConcept>,
}

/// The full hub-words registry.
#[derive(Debug, Deserialize)]
struct HubWordsRegistry {
    #[serde(flatten)]
    words: HashMap<String, HubWordEntry>,
}

/// Rule: NAMING/hub-word-fragmented-meaning.
///
/// Scans technical documentation (`.md`) and Rust doc-comments for forbidden
/// synonyms of hub words. Emits warnings when a synonym is used in a context
/// where the canonical term is expected.
#[non_exhaustive]
pub struct HubWordDisciplineRule {
    hub_words_path: PathBuf,
    disabled_words: HashSet<String>,
}

impl HubWordDisciplineRule {
    /// Create a new rule with the default registry path and no disabled words.
    pub fn new() -> Self {
        Self {
            hub_words_path: PathBuf::from(HUB_WORDS_PATH),
            disabled_words: HashSet::new(),
        }
    }

    /// Create a rule with a custom registry path and disabled words.
    #[cfg(test)]
    pub fn with_config(path: impl Into<PathBuf>, disabled_words: Vec<String>) -> Self {
        Self {
            hub_words_path: path.into(),
            disabled_words: disabled_words.into_iter().collect(),
        }
    }
}

impl Default for HubWordDisciplineRule {
    fn default() -> Self {
        Self::new()
    }
}

impl Rule for HubWordDisciplineRule {
    fn id(&self) -> &'static str {
        "NAMING/hub-word-fragmented-meaning"
    }

    #[tracing::instrument(skip_all)]
    fn check(&self, project_root: &str) -> Result<Vec<Violation>> {
        let registry_path = Path::new(project_root).join(&self.hub_words_path);
        let registry = load_registry(&registry_path)?;

        let mut violations = Vec::new();
        let root = Path::new(project_root);

        let mut files = Vec::new();
        collect_doc_files(root, &mut files)?;

        for path in files {
            if is_exempt(&path) {
                continue;
            }
            check_file(&path, &registry, &self.disabled_words, &mut violations)?;
        }

        Ok(violations)
    }
}

/// Load and deserialize the hub-words registry.
fn load_registry(path: &Path) -> Result<HubWordsRegistry> {
    let content = fs::read_to_string(path).with_context(|_| error::ReadFileSnafu {
        path: path.to_path_buf(),
    })?;
    let registry: HubWordsRegistry = toml::from_str(&content)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
        .with_context(|_| error::ReadFileSnafu {
            path: path.to_path_buf(),
        })?;
    Ok(registry)
}

/// Scan a single file for forbidden synonyms.
fn check_file(
    path: &Path,
    registry: &HubWordsRegistry,
    disabled: &HashSet<String>,
    violations: &mut Vec<Violation>,
) -> Result<()> {
    let content = fs::read_to_string(path).with_context(|_| error::ReadFileSnafu {
        path: path.to_path_buf(),
    })?;
    scan_content(&content, path, registry, disabled, violations);
    Ok(())
}

/// Scan content line-by-line for forbidden synonyms.
#[tracing::instrument(skip(content))]
fn scan_content(
    content: &str,
    path: &Path,
    registry: &HubWordsRegistry,
    disabled: &HashSet<String>,
    violations: &mut Vec<Violation>,
) {
    let is_rs = path
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("rs"));

    for (line_num, line) in content.lines().enumerate() {
        let scanned_line = if is_rs {
            let trimmed = line.trim_start();
            if trimmed.starts_with("///") || trimmed.starts_with("//!") {
                trimmed
            } else {
                continue;
            }
        } else {
            line
        };

        let lower = scanned_line.to_lowercase();

        for (word, entry) in &registry.words {
            if disabled.contains(word) {
                continue;
            }
            for synonym in &entry.forbidden_synonyms {
                if lower.contains(&synonym.to_lowercase()) {
                    violations.push(Violation {
                        rule: "NAMING/hub-word-fragmented-meaning".to_owned(),
                        path: path.display().to_string(),
                        line: line_num + 1,
                        message: format!(
                            "[warn] Forbidden synonym '{synonym}' for hub word '{word}' detected. Use canonical term: \"{}\".",
                            entry.canonical
                        ),
                    });
                }
            }
        }
    }
}

/// Determine whether a path points to a documentation file.
fn is_documentation_file(path: &Path) -> bool {
    matches!(path.extension().and_then(|e| e.to_str()), Some("md" | "rs"))
}

/// Determine whether a file is exempt from hub-word checks.
fn is_exempt(path: &Path) -> bool {
    path.file_name().is_some_and(|n| {
        EXEMPT_FILES
            .iter()
            .any(|e| n.to_string_lossy().eq_ignore_ascii_case(e))
    })
}

/// Recursively collect documentation files.
fn collect_doc_files(dir: &Path, files: &mut Vec<PathBuf>) -> Result<()> {
    for entry in fs::read_dir(dir).with_context(|_| error::ReadDirSnafu {
        path: dir.to_path_buf(),
    })? {
        let entry = entry.with_context(|_| error::ReadDirSnafu {
            path: dir.to_path_buf(),
        })?;
        let path = entry.path();

        if path.is_dir() {
            if let Some(name) = path.file_name() {
                let name_str = name.to_string_lossy();
                if !name_str.starts_with('.') && name_str != "target" && name_str != "node_modules"
                {
                    collect_doc_files(&path, files)?;
                }
            }
        } else if is_documentation_file(&path) {
            files.push(path);
        }
    }
    Ok(())
}

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test setup")]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn setup_registry(tmp: &TempDir) -> PathBuf {
        let registry_path = tmp
            .path()
            .join("crates")
            .join("aletheia")
            .join("hub-words.toml");
        fs::create_dir_all(registry_path.parent().expect("parent exists"))
            .expect("create registry dirs");
        fs::write(
            &registry_path,
            r#"
[memory]
canonical = "the mneme facade — eidos types + episteme pipeline + krites engine + graphe sessions"
implementations = ["mneme", "episteme", "krites", "eidos", "graphe"]
forbidden_synonyms = ["storage", "cache"]

[session]
canonical = "graphe::Session — a conversation thread persisted in the session store"
implementations = ["graphe::types::Session", "nous::session::SessionState"]
forbidden_synonyms = ["connection", "thread", "context"]

[tool]
canonical = "organon::ToolExecutor implementation registered with ToolRegistry"
implementations = ["organon::registry::ToolRegistry"]
forbidden_synonyms = ["utility", "function", "plugin"]
"#,
        )
        .expect("write registry");
        registry_path
    }

    #[test]
    fn hub_word_memory_flags_storage_synonym() {
        let tmp = TempDir::new().expect("temp dir");
        setup_registry(&tmp);
        let doc = tmp.path().join("docs.md");
        fs::write(&doc, "the storage layer").expect("write doc");

        let rule = HubWordDisciplineRule::new();
        let violations = rule
            .check(tmp.path().to_str().expect("valid path"))
            .expect("check");

        assert!(!violations.is_empty());
        assert!(violations.iter().any(|v| v.message.contains("storage")));
    }

    #[test]
    fn hub_word_memory_allows_canonical_term() {
        let tmp = TempDir::new().expect("temp dir");
        setup_registry(&tmp);
        let doc = tmp.path().join("docs.md");
        fs::write(&doc, "the mneme facade").expect("write doc");

        let rule = HubWordDisciplineRule::new();
        let violations = rule
            .check(tmp.path().to_str().expect("valid path"))
            .expect("check");

        assert!(
            violations.is_empty(),
            "expected no violations for canonical term, got: {violations:?}"
        );
    }

    #[test]
    fn hub_word_disabled_via_config_not_flagged() {
        let tmp = TempDir::new().expect("temp dir");
        setup_registry(&tmp);
        let doc = tmp.path().join("docs.md");
        fs::write(&doc, "the storage layer").expect("write doc");

        let rule = HubWordDisciplineRule::with_config(
            "crates/aletheia/hub-words.toml",
            vec!["memory".to_owned()],
        );
        let violations = rule
            .check(tmp.path().to_str().expect("valid path"))
            .expect("check");

        assert!(!violations.iter().any(|v| v.message.contains("storage")));
    }

    #[test]
    fn hub_word_scan_respects_exempt_files() {
        let tmp = TempDir::new().expect("temp dir");
        setup_registry(&tmp);
        let doc = tmp.path().join("vision.md");
        fs::write(&doc, "the storage layer").expect("write doc");

        let rule = HubWordDisciplineRule::new();
        let violations = rule
            .check(tmp.path().to_str().expect("valid path"))
            .expect("check");

        assert!(!violations.iter().any(|v| v.path.ends_with("vision.md")));
    }
}

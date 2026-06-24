//! Prompt cache optimization: split static prefix from dynamic suffix.
//!
//! Enables Anthropic prompt caching by separating content that is identical
//! across dispatches (role definition, standards, validation gate) from
//! per-dispatch state (project context, scope, task body).
//!
//! The static prefix is placed in the system prompt with
//! `cache_control: {type: "ephemeral"}` when dispatched through hermeneus,
//! allowing cache hits on repeated dispatches for the same role.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use snafu::ResultExt as _;

use crate::error::{IoSnafu, Result};

const VALIDATION_GATE: &str = "\n## Validation Gate\n\nBefore finishing, run the full validation suite (format, lint, test) and confirm all acceptance criteria pass.";

/// Split prompt for cache-aware dispatch.
///
/// The [`static_prefix`](Self::static_prefix) contains content identical
/// across dispatches for the same role. The [`dynamic_suffix`](Self::dynamic_suffix)
/// contains per-dispatch state that changes every time.
///
/// When the engine supports it (e.g. hermeneus with Anthropic), the static
/// prefix is sent as a cached system prompt and the dynamic suffix as the
/// user message.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct PromptComponents {
    /// Static content that can be prompt-cached: role definition + standards +
    /// validation gate. Identical across dispatches for the same role.
    pub static_prefix: String,
    /// Dynamic content that changes per dispatch: project state + scope +
    /// prompt body.
    pub dynamic_suffix: String,
}

/// Cache for static prompt-prefix inputs loaded from disk.
///
/// WHY: Role and standards files are identical across every prompt in a dispatch,
/// but `PromptComponents::build` was reading them once per prompt. Loading them
/// once and reusing `Arc<str>` contents removes blocking I/O from the async
/// preparation loop and eliminates redundant reads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StaticPrefixCache {
    role: Option<Arc<str>>,
    standards: Vec<(String, Arc<str>)>,
}

impl StaticPrefixCache {
    /// Load the role text and selected standards from disk once per dispatch.
    ///
    /// Uses `tokio::fs` so the async preparation stage does not block a Tokio
    /// worker thread on file I/O.
    pub async fn load(
        role: Option<&str>,
        standards_dir: Option<&Path>,
        standards: &[String],
    ) -> Result<Self> {
        let role = if let Some(role_text) = role {
            let path = Path::new(role_text);
            if tokio::fs::try_exists(path).await.unwrap_or_else(|e| {
                tracing::warn!(path = %role_text, error = %e, "failed to stat role file");
                false
            }) {
                let content = tokio::fs::read_to_string(path).await.context(IoSnafu {
                    path: PathBuf::from(role_text),
                })?;
                Some(Arc::<str>::from(content))
            } else {
                Some(Arc::<str>::from(role_text))
            }
        } else {
            None
        };

        let mut loaded_standards = Vec::new();
        if let Some(dir) = standards_dir {
            for name in standards {
                let path = dir.join(format!("{name}.md"));
                if tokio::fs::try_exists(&path).await.unwrap_or_else(|e| {
                    tracing::warn!(path = %path.display(), error = %e, "failed to stat standard");
                    false
                }) {
                    match tokio::fs::read_to_string(&path).await {
                        Ok(text) => loaded_standards.push((name.clone(), Arc::<str>::from(text))),
                        Err(e) => {
                            tracing::warn!(path = %path.display(), error = %e, "failed to read standard");
                        }
                    }
                } else {
                    tracing::warn!(path = %path.display(), "standard file not found");
                }
            }
        }

        Ok(Self {
            role,
            standards: loaded_standards,
        })
    }

    /// Build the static prefix from cached role and standards contents.
    fn build_static_prefix(&self) -> String {
        let mut parts: Vec<String> = Vec::new();

        if let Some(role) = &self.role
            && !role.is_empty()
        {
            parts.push(role.to_string());
        }

        for (_, text) in &self.standards {
            parts.push(text.to_string());
        }

        parts.push(VALIDATION_GATE.to_owned());

        parts.join("\n\n---\n\n")
    }
}

impl PromptComponents {
    /// Build prompt components from dispatch inputs.
    ///
    /// # Arguments
    ///
    /// * `role` — Optional role definition text or path to a role file.
    /// * `project` — Project identifier.
    /// * `standards_dir` — Optional directory containing standard `.md` files.
    /// * `standards` — List of standard names to include.
    /// * `scope` — Optional scope context appended to the dynamic suffix.
    /// * `prompt_body` — The original prompt body from the YAML frontmatter.
    ///
    /// # Static prefix construction
    ///
    /// 1. Role definition (if provided, or loaded from `role` path)
    /// 2. Selected standards (loaded from `standards_dir`)
    /// 3. Validation gate text
    ///
    /// # Dynamic suffix construction
    ///
    /// 1. Project state line (`Project: {project}`)
    /// 2. Scope context (if provided)
    /// 3. Original prompt body
    ///
    /// # Note
    ///
    /// This synchronous constructor reads role/standards files with `std::fs`.
    /// Callers running inside an async context should use [`StaticPrefixCache::load`]
    /// once and then [`PromptComponents::build_with_cache`] per prompt to avoid
    /// blocking the runtime.
    #[must_use]
    pub fn build(
        role: Option<&str>,
        project: &str,
        standards_dir: Option<&Path>,
        standards: &[String],
        scope: Option<&str>,
        prompt_body: &str,
    ) -> Self {
        let static_prefix = build_static_prefix(role, standards_dir, standards);
        let dynamic_suffix = build_dynamic_suffix(project, scope, prompt_body);

        Self {
            static_prefix,
            dynamic_suffix,
        }
    }

    /// Build components using a pre-loaded [`StaticPrefixCache`].
    ///
    /// WHY: Performs no file I/O, so it is safe to call from an async stage for
    /// every prompt after the cache has been loaded once.
    #[must_use]
    pub fn build_with_cache(
        cache: &StaticPrefixCache,
        project: &str,
        scope: Option<&str>,
        prompt_body: &str,
    ) -> Self {
        let static_prefix = cache.build_static_prefix();
        let dynamic_suffix = build_dynamic_suffix(project, scope, prompt_body);

        Self {
            static_prefix,
            dynamic_suffix,
        }
    }

    /// Combine components into a single prompt string.
    ///
    /// Used for backward compatibility with engines that do not support
    /// prompt splitting.
    #[must_use]
    pub fn to_full_prompt(&self) -> String {
        if self.static_prefix.is_empty() {
            self.dynamic_suffix.clone()
        } else {
            format!("{}\n\n{}", self.static_prefix, self.dynamic_suffix)
        }
    }

    /// Convert to a [`crate::engine::SessionSpec`] with `system_prompt` set
    /// to the static prefix and `prompt` set to the dynamic suffix.
    #[must_use]
    pub fn to_session_spec(&self, cwd: Option<String>) -> crate::engine::SessionSpec {
        crate::engine::SessionSpec {
            prompt: self.dynamic_suffix.clone(),
            system_prompt: if self.static_prefix.is_empty() {
                None
            } else {
                Some(self.static_prefix.clone())
            },
            cwd,
            prompt_components: Some(self.clone()),
            output_format: None,
        }
    }
}

fn build_static_prefix(
    role: Option<&str>,
    standards_dir: Option<&Path>,
    standards: &[String],
) -> String {
    let mut parts: Vec<String> = Vec::new();

    if let Some(role_text) = role {
        let role_def = if Path::new(role_text).exists() {
            std::fs::read_to_string(role_text).unwrap_or_else(|e| {
                tracing::warn!(path = %role_text, error = %e, "failed to read role file");
                String::new()
            })
        } else {
            role_text.to_owned()
        };
        if !role_def.is_empty() {
            parts.push(role_def);
        }
    }

    if let Some(dir) = standards_dir {
        for name in standards {
            let path = dir.join(format!("{name}.md"));
            if path.exists() {
                match std::fs::read_to_string(&path) {
                    Ok(text) => parts.push(text),
                    Err(e) => {
                        tracing::warn!(path = %path.display(), error = %e, "failed to read standard");
                    }
                }
            } else {
                tracing::warn!(path = %path.display(), "standard file not found");
            }
        }
    }

    parts.push(VALIDATION_GATE.to_owned());

    parts.join("\n\n---\n\n")
}

fn build_dynamic_suffix(project: &str, scope: Option<&str>, prompt_body: &str) -> String {
    let mut parts: Vec<String> = Vec::new();

    parts.push(format!("Project: {project}"));

    if let Some(scope_text) = scope
        && !scope_text.is_empty()
    {
        parts.push(format!("Scope: {scope_text}"));
    }

    parts.push(prompt_body.to_owned());

    parts.join("\n\n")
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions")]
#[expect(clippy::expect_used, reason = "test assertions")]
mod tests {
    use std::io::Write as _;

    use tempfile::TempDir;

    use super::*;

    #[test]
    fn build_splits_prefix_and_suffix() {
        let components = PromptComponents::build(
            Some("You are a coding agent."),
            "acme",
            None,
            &[],
            Some("backend refactor"),
            "Implement the health endpoint.",
        );

        assert!(components.static_prefix.contains("coding agent"));
        assert!(components.static_prefix.contains("Validation Gate"));
        assert!(components.dynamic_suffix.contains("Project: acme"));
        assert!(
            components
                .dynamic_suffix
                .contains("Scope: backend refactor")
        );
        assert!(
            components
                .dynamic_suffix
                .contains("Implement the health endpoint.")
        );
    }

    #[test]
    fn static_prefix_is_identical_across_dispatches() {
        let role = "You are a Rust engineer.";
        let standards: Vec<String> = vec!["RUST".to_owned()];

        let c1 = PromptComponents::build(
            Some(role),
            "acme",
            None,
            &standards,
            Some("scope A"),
            "task one",
        );
        let c2 = PromptComponents::build(
            Some(role),
            "beta",
            None,
            &standards,
            Some("scope B"),
            "task two",
        );

        assert_eq!(
            c1.static_prefix, c2.static_prefix,
            "static prefix must be byte-identical for same role and standards"
        );
        assert_ne!(c1.dynamic_suffix, c2.dynamic_suffix);
    }

    #[test]
    fn empty_role_produces_no_role_in_prefix() {
        let components = PromptComponents::build(None, "acme", None, &[], None, "do thing");
        assert!(!components.static_prefix.contains("You are"));
        assert!(components.static_prefix.contains("Validation Gate"));
    }

    #[test]
    fn to_full_prompt_combines_parts() {
        let components =
            PromptComponents::build(Some("role text"), "proj", None, &[], None, "body text");
        let full = components.to_full_prompt();
        assert!(full.contains("role text"));
        assert!(full.contains("body text"));
    }

    #[test]
    fn to_full_prompt_without_prefix_returns_suffix_only() {
        // WHY: `build()` always pushes a validation gate into the static prefix.
        // Direct construction with an empty prefix verifies the `is_empty` branch
        // of `to_full_prompt`.
        let components = PromptComponents {
            static_prefix: String::new(),
            dynamic_suffix: "body text".to_owned(),
        };
        assert_eq!(components.to_full_prompt(), "body text");
    }

    #[test]
    fn to_session_spec_populates_fields() {
        let components = PromptComponents::build(Some("role"), "proj", None, &[], None, "body");
        let spec = components.to_session_spec(Some("/tmp".to_owned()));
        // `prompt` carries the full dynamic suffix (project + body).
        assert!(spec.prompt.ends_with("body"));
        assert!(spec.system_prompt.as_deref().unwrap().contains("role"));
        assert_eq!(spec.cwd, Some("/tmp".to_owned()));
        assert!(spec.prompt_components.is_some());
    }

    #[test]
    fn build_reads_role_from_file() {
        let dir = TempDir::new().unwrap();
        let role_path = dir.path().join("role.md");
        {
            let mut f = std::fs::File::create(&role_path).unwrap();
            f.write_all(b"File-based role definition.").unwrap();
        }

        let components = PromptComponents::build(
            Some(role_path.to_str().unwrap()),
            "proj",
            None,
            &[],
            None,
            "body",
        );

        assert!(
            components
                .static_prefix
                .contains("File-based role definition.")
        );
    }

    #[test]
    fn build_reads_standards_from_dir() {
        let dir = TempDir::new().unwrap();
        let std_path = dir.path().join("RUST.md");
        {
            let mut f = std::fs::File::create(&std_path).unwrap();
            f.write_all(b"Use clippy.").unwrap();
        }

        let components = PromptComponents::build(
            None,
            "proj",
            Some(dir.path()),
            &["RUST".to_owned()],
            None,
            "body",
        );

        assert!(components.static_prefix.contains("Use clippy."));
    }

    #[tokio::test]
    async fn cache_loads_role_and_standards_once() {
        let dir = TempDir::new().unwrap();
        let role_path = dir.path().join("role.md");
        {
            let mut f = std::fs::File::create(&role_path).unwrap();
            f.write_all(b"Cached role definition.").unwrap();
        }
        let std_path = dir.path().join("RUST.md");
        {
            let mut f = std::fs::File::create(&std_path).unwrap();
            f.write_all(b"Cached standard.").unwrap();
        }

        let cache = StaticPrefixCache::load(
            Some(role_path.to_str().unwrap()),
            Some(dir.path()),
            &["RUST".to_owned()],
        )
        .await
        .expect("cache load should succeed");

        let c1 = PromptComponents::build_with_cache(&cache, "proj1", Some("scope1"), "task one");
        let c2 = PromptComponents::build_with_cache(&cache, "proj2", Some("scope2"), "task two");

        assert_eq!(c1.static_prefix, c2.static_prefix);
        assert!(c1.static_prefix.contains("Cached role definition."));
        assert!(c1.static_prefix.contains("Cached standard."));
        assert!(c1.static_prefix.contains("Validation Gate"));
        assert_ne!(c1.dynamic_suffix, c2.dynamic_suffix);
    }

    #[tokio::test]
    async fn cache_build_with_literal_role_skips_disk() {
        // WHY: A role value that is not a path must be treated as literal text.
        let cache = StaticPrefixCache::load(Some("Literal role."), None, &[])
            .await
            .expect("cache load should succeed");

        let components = PromptComponents::build_with_cache(&cache, "proj", None, "body");
        assert!(components.static_prefix.contains("Literal role."));
    }
}

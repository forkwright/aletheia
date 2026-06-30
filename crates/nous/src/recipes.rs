//! Task-specific _llm/ loading recipes.
//!
//! Recipes define which resolution levels of the multi-resolution codebase
//! representation to load for a given task type. This reduces token waste
//! compared to loading the entire workspace or using imprecise grep-based
//! selection.
//!
//! Recipes are stored in `_llm/recipes.toml` and loaded at bootstrap time.
//! Each recipe specifies reference files (L1–L3 from _llm/, L4 source paths)
//! and a token budget. The [`RecipeRegistry`] selects the best recipe for a
//! task description via keyword scoring.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use tracing::{debug, info};

use crate::error::{self, Result};

// ── Data types ──

/// A single file entry within a recipe.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecipeFile {
    /// Resolution level (L1, L2, L3, L4, instructions, etc.).
    pub level: String,
    /// Path template relative to repo root. May contain `{param}` placeholders.
    pub path: String,
    /// Optional human-readable note explaining why this file loads.
    #[serde(default)]
    pub note: Option<String>,
}

/// Validation record for a recipe: one real task it was tested against.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecipeValidation {
    /// Description of the real task (e.g. PR title).
    pub task: String,
    /// Tokens consumed by the naive grep-based baseline.
    pub baseline_tokens: u64,
    /// Tokens consumed by this recipe.
    pub recipe_tokens: u64,
    /// Whether the task completed successfully.
    pub success: bool,
    /// Optional note about the validation.
    #[serde(default)]
    pub note: Option<String>,
    /// Parameter values used for parameterized recipes.
    #[serde(default)]
    pub parameters: HashMap<String, String>,
}

/// A loading recipe specifying which files to load for a task type.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Recipe {
    /// Short identifier (e.g. `"cold_start"`, `"edit_crate"`).
    pub name: String,
    /// Human-readable description.
    pub description: String,
    /// When to use this recipe.
    pub use_case: String,
    /// Conservative token budget for this recipe.
    pub token_budget: u64,
    /// Whether the recipe has `{crate}`-style parameter placeholders.
    #[serde(default)]
    pub parameterized: bool,
    /// Parameter names when `parameterized` is true.
    #[serde(default)]
    pub parameters: Vec<String>,
    /// Keywords for task-to-recipe matching.
    #[serde(default)]
    pub task_keywords: Vec<String>,
    /// Files to load.
    #[serde(default)]
    pub file: Vec<RecipeFile>,
    /// Validation records.
    #[serde(default)]
    pub validation: Vec<RecipeValidation>,
}

impl Recipe {
    /// Resolve file path templates against the given parameters.
    ///
    /// Replaces `{key}` placeholders in each [`RecipeFile::path`] with values
    /// from `params`. Returns an error if a required parameter is missing.
    ///
    /// # Errors
    ///
    /// Returns [`error::Error::RecipeLoading`] when a parameter is missing.
    pub fn resolve_files(&self, params: &HashMap<String, String>) -> Result<Vec<RecipeFile>> {
        self.file
            .iter()
            .map(|f| {
                let mut path = f.path.clone();
                for (key, value) in params {
                    path = path.replace(&format!("{{{key}}}"), value);
                }
                if self.parameterized && path.contains('{') {
                    return Err(error::RecipeLoadingSnafu {
                        message: format!(
                            "recipe '{}' file '{}' has unresolved parameter",
                            self.name, f.path
                        ),
                    }
                    .build());
                }
                Ok(RecipeFile {
                    level: f.level.clone(),
                    path,
                    note: f.note.clone(),
                })
            })
            .collect()
    }

    /// Average token reduction percentage across all validation records.
    #[must_use]
    pub fn avg_reduction_pct(&self) -> f64 {
        if self.validation.is_empty() {
            return 0.0;
        }
        // WHY f64::from(u32): token counts and validation list sizes are
        // bounded in practice (< 2^32); u32→f64 is exact. `try_from`
        // guards the rare pathological case by saturating to u32::MAX
        // before conversion.
        let total: f64 = self
            .validation
            .iter()
            .map(|v| {
                if v.baseline_tokens == 0 {
                    0.0
                } else {
                    let reduction = v.baseline_tokens.saturating_sub(v.recipe_tokens);
                    let r_u32 = u32::try_from(reduction).unwrap_or(u32::MAX);
                    let b_u32 = u32::try_from(v.baseline_tokens).unwrap_or(u32::MAX);
                    (f64::from(r_u32) / f64::from(b_u32)) * 100.0
                }
            })
            .sum();
        let len_u32 = u32::try_from(self.validation.len()).unwrap_or(u32::MAX);
        total / f64::from(len_u32)
    }

    /// Success rate across validation records (0.0--1.0).
    #[must_use]
    pub fn success_rate(&self) -> f64 {
        if self.validation.is_empty() {
            return 0.0;
        }
        let successes = self.validation.iter().filter(|v| v.success).count();
        let s_u32 = u32::try_from(successes).unwrap_or(u32::MAX);
        let len_u32 = u32::try_from(self.validation.len()).unwrap_or(u32::MAX);
        f64::from(s_u32) / f64::from(len_u32)
    }
}

/// Top-level TOML structure for `_llm/recipes.toml`.
#[derive(Debug, Clone, Deserialize)]
#[expect(
    dead_code,
    reason = "TOML schema fields parsed but not all used by registry logic"
)]
struct RecipesFile {
    #[serde(default)]
    meta: RecipesMeta,
    #[serde(default)]
    recipe: Vec<Recipe>,
}

/// Metadata header in `_llm/recipes.toml`.
#[derive(Debug, Clone, Default, Deserialize)]
#[expect(
    dead_code,
    reason = "metadata fields are parsed for documentation but not consumed"
)]
struct RecipesMeta {
    version: u32,
    description: String,
    generated_at: String,
}

/// Registry of loading recipes, keyed by recipe name.
#[derive(Debug, Clone)]
pub struct RecipeRegistry {
    recipes: HashMap<String, Recipe>,
    recipe_order: Vec<String>,
}

impl RecipeRegistry {
    /// Create an empty registry.
    #[must_use]
    pub fn empty() -> Self {
        Self {
            recipes: HashMap::new(),
            recipe_order: Vec::new(),
        }
    }

    /// Load recipes from a TOML file.
    ///
    /// # Errors
    ///
    /// Returns [`error::Error::RecipeLoading`] if the file cannot be read or
    /// parsed as valid recipes TOML.
    pub fn load_from_file(path: &Path) -> Result<Self> {
        let content = std::fs::read_to_string(path).map_err(|e| {
            error::RecipeLoadingSnafu {
                message: format!("failed to read {}: {e}", path.display()),
            }
            .build()
        })?;
        Self::from_toml(&content)
    }

    /// Load recipes from a TOML string.
    ///
    /// # Errors
    ///
    /// Returns [`error::Error::RecipeLoading`] if the string is not valid
    /// recipes TOML.
    pub fn from_toml(content: &str) -> Result<Self> {
        let file: RecipesFile = toml::from_str(content).map_err(|e| {
            error::RecipeLoadingSnafu {
                message: format!("failed to parse recipes TOML: {e}"),
            }
            .build()
        })?;

        let mut recipes = HashMap::with_capacity(file.recipe.len());
        let mut recipe_order = Vec::with_capacity(file.recipe.len());
        for recipe in file.recipe {
            validate_recipe(&recipe)?;
            if recipes.contains_key(&recipe.name) {
                return Err(error::RecipeLoadingSnafu {
                    message: format!("duplicate recipe name '{}'", recipe.name),
                }
                .build());
            }

            info!(
                name = %recipe.name,
                description = %recipe.description,
                token_budget = recipe.token_budget,
                files = recipe.file.len(),
                validations = recipe.validation.len(),
                "loaded recipe"
            );
            let name = recipe.name.clone();
            recipe_order.push(name.clone());
            recipes.insert(name, recipe);
        }

        debug!(recipe_count = recipes.len(), "recipe registry ready");
        Ok(Self {
            recipes,
            recipe_order,
        })
    }

    /// Look up a recipe by exact name.
    #[must_use]
    pub fn get(&self, name: &str) -> Option<&Recipe> {
        self.recipes.get(name)
    }

    /// All recipes in the registry.
    #[must_use]
    pub fn all(&self) -> &HashMap<String, Recipe> {
        &self.recipes
    }

    /// Number of recipes.
    #[must_use]
    pub fn len(&self) -> usize {
        self.recipes.len()
    }

    /// Whether the registry is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.recipes.is_empty()
    }

    /// Select the best recipe for a task description.
    ///
    /// Uses keyword scoring: counts how many of each recipe's
    /// [`Recipe::task_keywords`] appear in the lowercased task description.
    /// Returns the recipe with the highest score. Ties are broken by the
    /// order recipes appear in the file (stable). Returns `None` when no
    /// keyword matches.
    #[must_use]
    pub fn select_for_task(&self, task_description: &str) -> Option<&Recipe> {
        let lower = task_description.to_lowercase();
        let mut best: Option<(&Recipe, usize)> = None;

        for recipe in self.ordered_recipes() {
            let score = recipe
                .task_keywords
                .iter()
                .filter(|kw| lower.contains(&kw.to_lowercase()))
                .count();
            if score == 0 {
                continue;
            }
            match best {
                Some((_, best_score)) if score > best_score => {
                    best = Some((recipe, score));
                }
                None => {
                    best = Some((recipe, score));
                }
                _ => {} // Lower-scoring candidate; keep current best.
            }
        }

        best.map(|(recipe, score)| {
            debug!(recipe = %recipe.name, score, "selected recipe for task");
            recipe
        })
    }

    /// Select a recipe by name, falling back to keyword matching.
    ///
    /// If `hint` matches a recipe name exactly, that recipe is returned.
    /// Otherwise, `hint` is treated as a task description and keyword scoring
    /// is used.
    #[must_use]
    pub fn select(&self, hint: &str) -> Option<&Recipe> {
        self.get(hint).or_else(|| self.select_for_task(hint))
    }

    /// Resolve file paths for a recipe, substituting parameters.
    ///
    /// Convenience wrapper around [`Recipe::resolve_files`] that looks up the
    /// recipe by name first.
    ///
    /// # Errors
    ///
    /// Returns [`error::Error::RecipeLoading`] if the recipe is not found or
    /// parameter substitution fails.
    pub fn resolve_files(
        &self,
        recipe_name: &str,
        params: &HashMap<String, String>,
    ) -> Result<Vec<RecipeFile>> {
        let recipe = self.get(recipe_name).ok_or_else(|| {
            error::RecipeLoadingSnafu {
                message: format!("recipe '{recipe_name}' not found"),
            }
            .build()
        })?;
        recipe.resolve_files(params)
    }

    /// Return the repo-root-relative paths for all _llm/ reference files
    /// defined in all recipes.
    ///
    /// Used by CI validation to ensure referenced files exist.
    #[must_use]
    pub fn all_reference_paths(&self) -> Vec<PathBuf> {
        let mut paths = Vec::new();
        for recipe in self.ordered_recipes() {
            for file in &recipe.file {
                if file.path.starts_with("_llm/") || file.path.starts_with("CLAUDE.md") {
                    paths.push(PathBuf::from(&file.path));
                }
            }
        }
        paths.sort();
        paths.dedup();
        paths
    }

    fn ordered_recipes(&self) -> impl Iterator<Item = &Recipe> {
        self.recipe_order
            .iter()
            .filter_map(|name| self.recipes.get(name))
    }
}

impl Default for RecipeRegistry {
    fn default() -> Self {
        Self::empty()
    }
}

fn validate_recipe(recipe: &Recipe) -> Result<()> {
    for validation in &recipe.validation {
        if validation_contains_non_evidence_marker(validation) {
            return Err(error::RecipeLoadingSnafu {
                message: format!(
                    "recipe '{}' validation is marked as non-evidence: '{}'",
                    recipe.name, validation.task
                ),
            }
            .build());
        }

        if validation.success && !task_cites_tracked_work(&validation.task) {
            return Err(error::RecipeLoadingSnafu {
                message: format!(
                    "recipe '{}' success=true validation must cite tracked work as #<digits>: '{}'",
                    recipe.name, validation.task
                ),
            }
            .build());
        }
    }

    Ok(())
}

fn validation_contains_non_evidence_marker(validation: &RecipeValidation) -> bool {
    text_contains_non_evidence_marker(&validation.task)
        || validation
            .note
            .as_deref()
            .is_some_and(text_contains_non_evidence_marker)
}

fn text_contains_non_evidence_marker(text: &str) -> bool {
    let lower = text.to_lowercase();
    lower.contains("hypothetical")
}

fn task_cites_tracked_work(task: &str) -> bool {
    task.as_bytes()
        .windows(2)
        .any(|pair| matches!(pair, [b'#', digit] if digit.is_ascii_digit()))
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions may panic on failure")]
mod tests {
    use super::*;

    const SAMPLE_TOML: &str = r#"
[meta]
version = 1
description = "test"
generated_at = "2026-01-01T00:00:00Z"

[[recipe]]
name = "cold_start"
description = "First interaction"
use_case = "orientation"
token_budget = 5000
parameterized = false
task_keywords = ["cold start", "orientation"]

[[recipe.file]]
level = "L1"
path = "_llm/architecture.toml"

[[recipe.file]]
level = "instructions"
path = "CLAUDE.md"

[[recipe.validation]]
task = "test task (#1)"
baseline_tokens = 10000
recipe_tokens = 5000
success = true

[[recipe]]
name = "edit_crate"
description = "Edit one crate"
use_case = "single crate change"
token_budget = 10000
parameterized = true
parameters = ["crate"]
task_keywords = ["edit crate", "fix crate"]

[[recipe.file]]
level = "L3"
path = "_llm/L3-api-index/{crate}.md"
"#;

    #[test]
    fn from_toml_parses_recipes() {
        let registry = RecipeRegistry::from_toml(SAMPLE_TOML).unwrap();
        assert_eq!(registry.len(), 2);

        let cold = registry.get("cold_start").unwrap();
        assert_eq!(cold.token_budget, 5000);
        assert_eq!(cold.file.len(), 2);
        assert!(!cold.parameterized);

        let edit = registry.get("edit_crate").unwrap();
        assert!(edit.parameterized);
        assert_eq!(edit.parameters, vec!["crate"]);
    }

    #[test]
    fn select_by_exact_name() {
        let registry = RecipeRegistry::from_toml(SAMPLE_TOML).unwrap();
        assert!(registry.select("cold_start").is_some());
        assert!(registry.select("edit_crate").is_some());
    }

    #[test]
    fn select_by_keyword() {
        let registry = RecipeRegistry::from_toml(SAMPLE_TOML).unwrap();
        let recipe = registry.select_for_task("I need a cold start orientation");
        assert!(recipe.is_some());
        assert_eq!(recipe.unwrap().name, "cold_start");
    }

    #[test]
    fn select_by_keyword_uses_file_order_for_ties() {
        let toml = r#"
[meta]
version = 1
description = "test"
generated_at = "2026-01-01T00:00:00Z"

[[recipe]]
name = "first"
description = "First matching recipe"
use_case = "tie"
token_budget = 1000
task_keywords = ["shared"]

[[recipe]]
name = "second"
description = "Second matching recipe"
use_case = "tie"
token_budget = 1000
task_keywords = ["shared"]
"#;

        let registry = RecipeRegistry::from_toml(toml).unwrap();
        let recipe = registry.select_for_task("shared task").unwrap();
        assert_eq!(recipe.name, "first");
    }

    #[test]
    fn select_returns_none_on_no_match() {
        let registry = RecipeRegistry::from_toml(SAMPLE_TOML).unwrap();
        assert!(
            registry
                .select_for_task("completely unrelated gibberish")
                .is_none()
        );
    }

    #[test]
    #[expect(
        clippy::indexing_slicing,
        reason = "test data has known structure: exactly one file"
    )]
    fn resolve_files_substitutes_parameters() {
        let registry = RecipeRegistry::from_toml(SAMPLE_TOML).unwrap();
        let mut params = HashMap::new();
        params.insert("crate".to_owned(), "pylon".to_owned());

        let files = registry.resolve_files("edit_crate", &params).unwrap();
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].path, "_llm/L3-api-index/pylon.md");
    }

    #[test]
    fn resolve_files_fails_on_missing_param() {
        let registry = RecipeRegistry::from_toml(SAMPLE_TOML).unwrap();
        let params = HashMap::new();
        let result = registry.resolve_files("edit_crate", &params);
        assert!(result.is_err());
    }

    #[test]
    fn avg_reduction_pct_computed_correctly() {
        let registry = RecipeRegistry::from_toml(SAMPLE_TOML).unwrap();
        let cold = registry.get("cold_start").unwrap();
        assert!((cold.avg_reduction_pct() - 50.0).abs() < f64::EPSILON);
    }

    #[test]
    fn success_rate_computed_correctly() {
        let registry = RecipeRegistry::from_toml(SAMPLE_TOML).unwrap();
        let cold = registry.get("cold_start").unwrap();
        assert!((cold.success_rate() - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn empty_registry_has_no_recipes() {
        let registry = RecipeRegistry::empty();
        assert!(registry.is_empty());
        assert_eq!(registry.len(), 0);
        assert!(registry.select("anything").is_none());
    }

    #[test]
    fn all_reference_paths_filters_to_llm_and_claude() {
        let registry = RecipeRegistry::from_toml(SAMPLE_TOML).unwrap();
        let paths = registry.all_reference_paths();
        assert!(paths.contains(&PathBuf::from("_llm/architecture.toml")));
        assert!(paths.contains(&PathBuf::from("CLAUDE.md")));
        // edit_crate's L3 path has a placeholder and starts with _llm/, so it is included
        assert!(paths.contains(&PathBuf::from("_llm/L3-api-index/{crate}.md")));
    }

    #[test]
    fn from_toml_rejects_invalid_toml() {
        let result = RecipeRegistry::from_toml("this is not { valid toml");
        assert!(result.is_err());
    }

    #[test]
    fn from_toml_rejects_hypothetical_validation_records() {
        let toml = r#"
[meta]
version = 1
description = "test"
generated_at = "2026-01-01T00:00:00Z"

[[recipe]]
name = "tooling"
description = "Tooling"
use_case = "tools"
token_budget = 1000

[[recipe.validation]]
task = "feat(tooling): add next phase (# hypothetical follow-up)"
baseline_tokens = 10000
recipe_tokens = 5000
success = true
"#;

        let err = RecipeRegistry::from_toml(toml).unwrap_err();
        assert!(
            err.to_string().contains("non-evidence"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn from_toml_rejects_success_without_tracked_task_evidence() {
        let toml = r#"
[meta]
version = 1
description = "test"
generated_at = "2026-01-01T00:00:00Z"

[[recipe]]
name = "tooling"
description = "Tooling"
use_case = "tools"
token_budget = 1000

[[recipe.validation]]
task = "feat(tooling): add next phase"
baseline_tokens = 10000
recipe_tokens = 5000
success = true
"#;

        let err = RecipeRegistry::from_toml(toml).unwrap_err();
        assert!(
            err.to_string().contains("tracked work"),
            "unexpected error: {err}"
        );
    }
}

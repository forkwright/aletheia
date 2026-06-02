// WHY: Prompt loading and DAG construction from YAML frontmatter files.
// Separates I/O (loading from disk) from graph logic (DAG construction),
// keeping each concern testable in isolation.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use snafu::ResultExt as _;

use crate::dag::{ContextPolicy, DagError, PromptDag, PromptStatus};
use crate::error::{
    DagCycleSnafu, DagMissingDepsSnafu, FrontmatterParseSnafu, IoSnafu, PreflightSnafu, Result,
};

/// Worktree isolation preference declared by a prompt file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct WorktreePolicy {
    /// Whether the dispatch harness should run this prompt in an isolated worktree.
    pub enabled: bool,
}

impl Default for WorktreePolicy {
    fn default() -> Self {
        Self { enabled: true }
    }
}

/// Full specification for a dispatch prompt.
///
/// Loaded from a YAML frontmatter file where the frontmatter contains
/// metadata and the Markdown body contains the task instructions.
///
/// # File format
///
/// ```yaml
/// ---
/// number: 1
/// description: "Add health endpoint"
/// depends_on: [2, 3]
/// context_policy:
///   policy: fresh
/// worktree:
///   enabled: true
/// acceptance_criteria:
///   - "GET /health returns 200"
///   - "response includes build info"
/// blast_radius:
///   - "crates/pylon/src/handlers/"
/// ---
///
/// # K-001: Task body here
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub struct PromptSpec {
    /// Prompt number (unique within the project queue).
    pub number: u32,
    /// Human-readable description of the task.
    pub description: String,
    /// Prompt numbers this prompt depends on (DAG edges).
    pub depends_on: Vec<u32>,
    /// How this prompt receives conversational context from other prompt nodes.
    #[serde(default)]
    pub context_policy: ContextPolicy,
    /// Optional structured output contract for this prompt's response.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_format: Option<hermeneus::types::OutputFormat>,
    /// Whether this prompt expects isolated worktree execution when the
    /// dispatch harness supports it.
    #[serde(default)]
    pub worktree: WorktreePolicy,
    /// Acceptance criteria the implementation must satisfy.
    pub acceptance_criteria: Vec<String>,
    /// File paths the prompt is allowed to modify.
    pub blast_radius: Vec<String>,
    /// Full Markdown body (task instructions after the frontmatter delimiter).
    pub body: String,
    /// Optional prompt cache split. Populated by the preparation stage when
    /// role/standards configuration is present.
    #[serde(skip)]
    pub prompt_components: Option<crate::prompt_cache::PromptComponents>,
}

/// Raw frontmatter fields deserialized from YAML.
#[derive(Debug, Deserialize)]
struct Frontmatter {
    number: u32,
    #[serde(default)]
    description: String,
    #[serde(default)]
    depends_on: Vec<u32>,
    #[serde(default)]
    context_policy: ContextPolicy,
    #[serde(default)]
    output_format: Option<hermeneus::types::OutputFormat>,
    #[serde(default)]
    worktree: WorktreePolicy,
    #[serde(default)]
    acceptance_criteria: Vec<String>,
    #[serde(default)]
    blast_radius: Vec<String>,
}

/// Load a single prompt from a YAML-frontmatter Markdown file.
///
/// The file must begin with `---\n`, contain a YAML block, and close with
/// `---\n`. Everything after the closing delimiter is the body.
///
/// # Errors
///
/// Returns [`crate::error::Error::Io`] on read failure or
/// [`crate::error::Error::FrontmatterParse`] if the YAML is malformed or
/// the file lacks the `---` delimiters.
pub fn load_prompt(path: &Path) -> Result<PromptSpec> {
    let raw = std::fs::read_to_string(path).context(IoSnafu {
        path: path.to_owned(),
    })?;

    parse_prompt_str(&raw, path)
}

/// Parse a prompt from an in-memory string.
///
/// Splits on `---` delimiters, deserializes the YAML frontmatter, and returns
/// the rest as the body.
fn parse_prompt_str(raw: &str, path: &Path) -> Result<PromptSpec> {
    // WHY: Split on `---\n` to separate the frontmatter block from the body.
    // We expect the file to start with `---\n`.
    let Some(after_open) = raw.strip_prefix("---\n") else {
        return FrontmatterParseSnafu {
            path: path.to_owned(),
            detail: "file does not start with '---'",
        }
        .fail();
    };

    // NOTE: Find the closing `---` delimiter.
    let Some(close_pos) = after_open.find("\n---\n") else {
        return FrontmatterParseSnafu {
            path: path.to_owned(),
            detail: "missing closing '---' frontmatter delimiter",
        }
        .fail();
    };

    // WHY: `close_pos` and `body_start` are byte offsets returned by `str::find`
    // on ASCII delimiter bytes, so they are always on valid UTF-8 boundaries.
    #[expect(
        clippy::string_slice,
        reason = "close_pos is a byte offset from str::find on ASCII delimiters, always a valid UTF-8 boundary"
    )]
    // kanon:ignore RUST/indexing-slicing — close_pos is a byte offset from str::find on ASCII delimiter bytes, always a valid UTF-8 boundary
    let yaml_str = &after_open[..close_pos];
    let body_start = close_pos + "\n---\n".len();
    #[expect(
        clippy::string_slice,
        reason = "body_start is computed from ASCII delimiter length added to a valid boundary, always aligned"
    )]
    // kanon:ignore RUST/indexing-slicing — body_start is computed from ASCII delimiter length added to a valid boundary, always aligned
    let body = after_open[body_start..].trim_start_matches('\n').to_owned();

    let fm: Frontmatter = serde_yml::from_str(yaml_str).map_err(|e| {
        FrontmatterParseSnafu {
            path: path.to_owned(),
            detail: format!("YAML parse error: {e}"),
        }
        .build()
    })?;

    Ok(PromptSpec {
        number: fm.number,
        description: fm.description,
        depends_on: fm.depends_on,
        context_policy: fm.context_policy,
        output_format: fm.output_format,
        worktree: fm.worktree,
        acceptance_criteria: fm.acceptance_criteria,
        blast_radius: fm.blast_radius,
        body,
        prompt_components: None,
    })
}

/// Load all `.md` prompts from a directory.
///
/// Reads every `*.md` file in `dir` (non-recursive) and returns the parsed
/// specs sorted by prompt number. Skips non-Markdown files silently.
///
/// # Errors
///
/// Returns [`crate::error::Error::Io`] if the directory cannot be read.
/// Returns [`crate::error::Error::FrontmatterParse`] for any malformed file.
pub fn load_queue(dir: &Path) -> Result<Vec<PromptSpec>> {
    let entries = std::fs::read_dir(dir).context(IoSnafu {
        path: dir.to_owned(),
    })?;

    let mut specs: Vec<PromptSpec> = Vec::new();

    for entry in entries {
        let entry = entry.context(IoSnafu {
            path: dir.to_owned(),
        })?;
        let path: PathBuf = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("md") {
            continue;
        }
        specs.push(load_prompt(&path)?);
    }

    specs.sort_by_key(|s| s.number);
    Ok(specs)
}

/// Construct a validated [`PromptDag`] from a slice of prompt specs.
///
/// Each spec's `number` and `depends_on` fields form the DAG nodes and edges.
/// Immediately validates the graph for cycles and missing dependencies.
///
/// # Errors
///
/// Returns [`crate::error::Error::DagCycle`] on cycle detection or
/// [`crate::error::Error::DagMissingDeps`] for broken dependency references.
pub fn build_dag(prompts: &[PromptSpec]) -> Result<PromptDag> {
    let mut dag = PromptDag::new();

    for spec in prompts {
        if spec.context_policy == ContextPolicy::Shared {
            return PreflightSnafu {
                reason: format!(
                    "context policy {:?} for prompt {} requires shared conversation support",
                    spec.context_policy, spec.number
                ),
            }
            .fail();
        }
        // NOTE: Duplicate numbers in the prompt set are not expected; treat as
        // a configuration error.
        dag.add_node_with_context_policy(
            spec.number,
            spec.depends_on.clone(),
            spec.context_policy.clone(),
        )
        .map_err(|_duplicate| {
            DagMissingDepsSnafu {
                detail: format!("duplicate prompt number {} in queue", spec.number),
            }
            .build()
        })?;
    }

    // WHY: Validate immediately after building — callers can rely on the
    // returned DAG being cycle-free and fully connected.
    dag.validate().map_err(|e| match e {
        DagError::Cycle { cycle } => DagCycleSnafu { cycle }.build(),
        DagError::MissingDependencies { broken } => DagMissingDepsSnafu {
            detail: format!(
                "{} broken dep(s): {}",
                broken.len(),
                broken
                    .iter()
                    .map(|(p, d)| format!("{p}->{d}"))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        }
        .build(),
        DagError::InvalidPrompt { number } => DagMissingDepsSnafu {
            detail: format!("prompt {number} not found"),
        }
        .build(),
        DagError::DuplicateNode { number } => DagMissingDepsSnafu {
            detail: format!("duplicate node {number}"),
        }
        .build(),
    })?;

    // NOTE: Set initial statuses: prompts with no in-queue deps are Ready.
    let all_numbers: std::collections::HashSet<u32> = prompts.iter().map(|s| s.number).collect();
    for spec in prompts {
        let initial = if spec.depends_on.is_empty()
            || spec.depends_on.iter().all(|d| !all_numbers.contains(d))
        {
            PromptStatus::Ready
        } else {
            PromptStatus::Blocked
        };
        // NOTE: All nodes were just added above; set_status cannot fail here.
        // kanon:ignore RUST/no-silent-result-swallow — all nodes were just added above; set_status on a known-existing node is infallible
        let _ = dag.set_status(spec.number, initial);
    }

    Ok(dag)
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions")]
#[expect(clippy::indexing_slicing, reason = "test assertions over fixture data")]
mod tests {
    use std::io::Write as _;

    use tempfile::TempDir;

    use super::*;
    use crate::error::Error;

    fn make_prompt_file(dir: &TempDir, name: &str, content: &str) -> PathBuf {
        let path = dir.path().join(name);
        let mut f = std::fs::File::create(&path).unwrap();
        f.write_all(content.as_bytes()).unwrap();
        path
    }

    const MINIMAL_PROMPT: &str = "\
---
number: 1
description: \"Test task\"
---

# Task body here
";

    const FULL_PROMPT: &str = "\
---
number: 2
description: \"Full task\"
depends_on: [1]
acceptance_criteria:
  - \"criterion one\"
  - \"criterion two\"
blast_radius:
  - \"crates/foo/\"
---

# Full task body
";

    // -------------------------------------------------------------------------
    // load_prompt tests
    // -------------------------------------------------------------------------

    #[test]
    fn load_minimal_prompt() {
        let dir = TempDir::new().unwrap();
        let path = make_prompt_file(&dir, "001-task.md", MINIMAL_PROMPT);

        let spec = load_prompt(&path).unwrap();
        assert_eq!(spec.number, 1);
        assert_eq!(spec.description, "Test task");
        assert!(spec.depends_on.is_empty());
        assert_eq!(spec.worktree, WorktreePolicy::default());
        assert!(spec.acceptance_criteria.is_empty());
        assert!(spec.blast_radius.is_empty());
        assert!(spec.body.contains("Task body here"));
    }

    #[test]
    fn load_full_prompt() {
        let dir = TempDir::new().unwrap();
        let path = make_prompt_file(&dir, "002-task.md", FULL_PROMPT);

        let spec = load_prompt(&path).unwrap();
        assert_eq!(spec.number, 2);
        assert_eq!(spec.depends_on, vec![1]);
        assert_eq!(spec.acceptance_criteria.len(), 2);
        assert_eq!(spec.blast_radius, vec!["crates/foo/"]);
        assert!(spec.body.contains("Full task body"));
    }

    #[test]
    fn load_prompt_context_policy_defaults_fresh() {
        let dir = TempDir::new().unwrap();
        let path = make_prompt_file(&dir, "001-task.md", MINIMAL_PROMPT);

        let spec = load_prompt(&path).unwrap();
        assert_eq!(spec.context_policy, ContextPolicy::Fresh);
    }

    #[test]
    fn load_prompt_context_policy_from_frontmatter() {
        let dir = TempDir::new().unwrap();
        let path = make_prompt_file(
            &dir,
            "002-task.md",
            "\
---
number: 2
depends_on: [1]
context_policy:
  policy: inherit
  nodes: [1]
---

body
",
        );

        let spec = load_prompt(&path).unwrap();
        assert_eq!(spec.context_policy, ContextPolicy::Inherit(vec![1]));
    }

    #[test]
    fn load_prompt_output_format_from_frontmatter() {
        let dir = TempDir::new().unwrap();
        let path = make_prompt_file(
            &dir,
            "003-task.md",
            "\
---
number: 3
output_format:
  type: json_schema
  name: research_result
  strict: true
  schema:
    type: object
    required: [summary]
    properties:
      summary:
        type: string
---

body
",
        );

        let spec = load_prompt(&path).unwrap();
        let Some(hermeneus::types::OutputFormat::JsonSchema {
            name,
            schema,
            strict,
        }) = spec.output_format
        else {
            panic!("expected JSON schema output format");
        };
        assert_eq!(name, "research_result");
        assert_eq!(strict, Some(true));
        assert_eq!(schema["required"][0], "summary");
    }

    #[test]
    fn load_prompt_worktree_policy_from_frontmatter() {
        let dir = TempDir::new().unwrap();
        let path = make_prompt_file(
            &dir,
            "002-task.md",
            "\
---
number: 2
worktree:
  enabled: false
---

body
",
        );

        let spec = load_prompt(&path).unwrap();
        assert_eq!(spec.worktree, WorktreePolicy { enabled: false });
    }

    #[test]
    fn load_prompt_missing_open_delimiter_fails() {
        let dir = TempDir::new().unwrap();
        let path = make_prompt_file(&dir, "bad.md", "number: 1\n# no delimiters\n");
        let err = load_prompt(&path).unwrap_err();
        assert!(matches!(err, Error::FrontmatterParse { .. }));
    }

    #[test]
    fn load_prompt_missing_close_delimiter_fails() {
        let dir = TempDir::new().unwrap();
        let path = make_prompt_file(&dir, "bad.md", "---\nnumber: 1\n# no closing\n");
        let err = load_prompt(&path).unwrap_err();
        assert!(matches!(err, Error::FrontmatterParse { .. }));
    }

    #[test]
    fn load_prompt_invalid_yaml_fails() {
        let dir = TempDir::new().unwrap();
        let path = make_prompt_file(&dir, "bad.md", "---\n: invalid: yaml:\n---\n\nbody\n");
        let err = load_prompt(&path).unwrap_err();
        assert!(matches!(err, Error::FrontmatterParse { .. }));
    }

    #[test]
    fn load_prompt_nonexistent_file_fails() {
        let path = PathBuf::from("/nonexistent/path/prompt.md");
        let err = load_prompt(&path).unwrap_err();
        assert!(matches!(err, Error::Io { .. }));
    }

    // -------------------------------------------------------------------------
    // load_queue tests
    // -------------------------------------------------------------------------

    #[test]
    fn load_queue_returns_sorted_by_number() {
        let dir = TempDir::new().unwrap();
        make_prompt_file(&dir, "003-c.md", "---\nnumber: 3\n---\n\nbody\n");
        make_prompt_file(&dir, "001-a.md", "---\nnumber: 1\n---\n\nbody\n");
        make_prompt_file(&dir, "002-b.md", "---\nnumber: 2\n---\n\nbody\n");

        let specs = load_queue(dir.path()).unwrap();
        assert_eq!(specs.len(), 3);
        assert_eq!(specs[0].number, 1);
        assert_eq!(specs[1].number, 2);
        assert_eq!(specs[2].number, 3);
    }

    #[test]
    fn load_queue_skips_non_markdown_files() {
        let dir = TempDir::new().unwrap();
        make_prompt_file(&dir, "001-a.md", "---\nnumber: 1\n---\n\nbody\n");
        make_prompt_file(&dir, "notes.txt", "not a prompt");
        make_prompt_file(&dir, "README", "also not a prompt");

        let specs = load_queue(dir.path()).unwrap();
        assert_eq!(specs.len(), 1);
        assert_eq!(specs[0].number, 1);
    }

    #[test]
    fn load_queue_empty_dir_returns_empty() {
        let dir = TempDir::new().unwrap();
        let specs = load_queue(dir.path()).unwrap();
        assert!(specs.is_empty());
    }

    // -------------------------------------------------------------------------
    // build_dag tests
    // -------------------------------------------------------------------------

    fn spec(number: u32, depends_on: Vec<u32>) -> PromptSpec {
        PromptSpec {
            number,
            description: format!("prompt {number}"),
            depends_on,
            context_policy: ContextPolicy::Fresh,
            output_format: None,
            worktree: WorktreePolicy::default(),
            acceptance_criteria: vec![],
            blast_radius: vec![],
            body: String::new(),
            prompt_components: None,
        }
    }

    #[test]
    fn build_dag_no_deps_all_ready() {
        let prompts = vec![spec(1, vec![]), spec(2, vec![]), spec(3, vec![])];
        let dag = build_dag(&prompts).unwrap();
        assert_eq!(dag.get_ready(), vec![1, 2, 3]);
    }

    #[test]
    fn build_dag_with_deps_blocked() {
        let prompts = vec![spec(1, vec![]), spec(2, vec![1])];
        let dag = build_dag(&prompts).unwrap();
        assert_eq!(dag.get_ready(), vec![1]);
        assert_eq!(dag.nodes[&2].status, PromptStatus::Blocked);
        assert_eq!(dag.nodes[&2].context_policy, ContextPolicy::Fresh);
    }

    #[test]
    fn build_dag_rejects_shared_context_policy() {
        let mut prompt = spec(2, vec![1]);
        prompt.context_policy = ContextPolicy::Shared;

        let err = build_dag(&[spec(1, vec![]), prompt]).unwrap_err();
        assert!(matches!(err, Error::Preflight { .. }));
    }

    #[test]
    fn build_dag_cycle_returns_error() {
        let prompts = vec![spec(1, vec![2]), spec(2, vec![1])];
        let err = build_dag(&prompts).unwrap_err();
        assert!(matches!(err, Error::DagCycle { .. }));
    }

    #[test]
    fn build_dag_missing_dep_returns_error() {
        let prompts = vec![spec(1, vec![99])];
        let err = build_dag(&prompts).unwrap_err();
        assert!(matches!(err, Error::DagMissingDeps { .. }));
    }

    #[test]
    fn build_dag_compute_frontier() {
        use crate::dag::compute_frontier;

        let prompts = vec![
            spec(1, vec![]),
            spec(2, vec![1]),
            spec(3, vec![1]),
            spec(4, vec![2, 3]),
        ];
        let dag = build_dag(&prompts).unwrap();
        let frontier = compute_frontier(&dag);

        assert_eq!(frontier.len(), 3);
        assert_eq!(frontier[0], vec![1]);
        assert_eq!(frontier[1], vec![2, 3]);
        assert_eq!(frontier[2], vec![4]);
    }
}

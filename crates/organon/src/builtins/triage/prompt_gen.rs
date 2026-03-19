//! Kanon-format prompt generation from GitHub issues.
//!
//! Generates structured prompts that conform to the kanon dispatch format:
//! frontmatter, directive, setup, task, acceptance criteria, blast radius,
//! validation gate, and completion sections.

use std::fmt::Write as _;

use super::GitHubIssue;

/// Generate a kanon-format prompt from a GitHub issue.
///
/// The generated prompt conforms to the structure expected by `dispatch/prompts.py`:
/// - YAML frontmatter with model
/// - Numbered title
/// - Directive, Standards, Setup, Task, Acceptance Criteria, Blast Radius,
///   Pre-commit Checklist, Validation Gate, Completion, Observations sections
pub(crate) fn generate_prompt(issue: &GitHubIssue, repo: &str) -> String {
    let number = issue.number;
    let domain = infer_domain(issue);
    let slug = infer_slug(&issue.title);
    let branch_type = infer_branch_type(issue);
    let branch = format!("{branch_type}/{domain}-{slug}");
    let task = infer_task(issue);
    let acceptance = extract_acceptance_criteria(issue);
    let blast_radius = infer_blast_radius(issue, &domain);

    let mut prompt = String::new();

    // Frontmatter
    prompt.push_str("---\nmodel: claude-opus-4-6\n---\n\n");

    // Title
    let _ = writeln!(prompt, "# {number}: {}\n", issue.title);

    // Directive
    let _ = writeln!(prompt, "## Directive\n\n{task}\n");

    // Standards
    prompt.push_str("## Standards\n\nRead `standards/RUST.md`.\n\n");

    // Setup
    prompt.push_str("## Setup\n\n```bash\n");
    prompt.push_str("git fetch origin && git log --oneline -3 origin/main\n");
    let _ = writeln!(
        prompt,
        "git worktree add ../worktrees/{branch} -b {branch} origin/main"
    );
    let _ = writeln!(prompt, "cd ../worktrees/{branch}");
    prompt.push_str("```\n\n");

    // Task
    let _ = writeln!(prompt, "## Task\n\n1. **#{number}**: {task}\n");

    // Include original issue body as context if present
    if !issue.body.is_empty() {
        prompt.push_str("### Context from issue\n\n");
        let max_body = 2000;
        if issue.body.len() > max_body {
            let truncated = issue.body.get(..max_body).unwrap_or(&issue.body);
            prompt.push_str(truncated);
            prompt.push_str("\n\n[...truncated]\n\n");
        } else {
            prompt.push_str(&issue.body);
            prompt.push_str("\n\n");
        }
    }

    // Acceptance Criteria
    prompt.push_str("## Acceptance criteria\n\n");
    for criterion in &acceptance {
        let _ = writeln!(prompt, "- [ ] {criterion}");
    }
    let _ = writeln!(prompt, "- [ ] Closes #{number}\n");

    // Blast Radius
    prompt.push_str("## Blast radius\n\n```\n");
    for path in &blast_radius {
        prompt.push_str(path);
        prompt.push('\n');
    }
    prompt.push_str("```\n\n");

    // Pre-commit checklist
    prompt.push_str(
        "## Pre-commit checklist\n\n```bash\n\
        cargo fmt --all\n\
        cargo clippy --workspace --all-targets -- -D warnings 2>&1 | head -50\n\
        cargo test --workspace\n\
        cargo test --workspace --doc\n\
        ```\n\n",
    );

    // Validation Gate
    prompt.push_str(
        "## Validation gate\n\n```bash\n\
        cargo fmt --all -- --check\n\
        cargo clippy --workspace --all-targets -- -D warnings\n\
        cargo test --workspace\n\
        ```\n\n",
    );

    // Completion
    prompt.push_str(
        "## Completion\n\nAfter all acceptance criteria are met:\n\n\
        1. `git add` changed files (do NOT use `git add -A`)\n\
        2. `git commit` with a conventional commit message\n\
        3. `git push origin HEAD`\n",
    );
    let _ = writeln!(
        prompt,
        "4. Create a PR with `gh pr create` — include \"Closes #{number}\" in the body"
    );
    prompt.push_str("5. Do NOT merge the PR\n\n");

    // Observations
    prompt.push_str(
        "## Observations\n\n\
         Capture anything out of scope in the PR body under `## Observations`.\n",
    );

    // Source metadata comment
    let _ = write!(
        prompt,
        "\n<!-- Auto-generated from {repo}#{number} by issue_triage -->\n"
    );

    prompt
}

/// Infer the crate domain from issue labels and title.
fn infer_domain(issue: &GitHubIssue) -> String {
    let crate_names = [
        "aletheia",
        "pylon",
        "nous",
        "hermeneus",
        "organon",
        "mneme",
        "taxis",
        "koina",
        "symbolon",
        "oikonomos",
        "melete",
        "agora",
        "dianoia",
        "theatron",
        "diaporeia",
        "prostheke",
        "daemon",
    ];

    let title_lower = issue.title.to_lowercase();
    let body_lower = issue.body.to_lowercase();
    let labels_lower: Vec<String> = issue.labels.iter().map(|l| l.to_lowercase()).collect();

    // Check labels first
    for name in &crate_names {
        if labels_lower.iter().any(|l| l.contains(name)) {
            return (*name).to_owned();
        }
    }

    // Check title
    for name in &crate_names {
        if title_lower.contains(name) {
            return (*name).to_owned();
        }
    }

    // Check body (first 500 chars)
    let body_prefix = body_lower.get(..500).unwrap_or(&body_lower);
    for name in &crate_names {
        if body_prefix.contains(name) {
            return (*name).to_owned();
        }
    }

    "general".to_owned()
}

/// Infer branch type from issue labels.
fn infer_branch_type(issue: &GitHubIssue) -> &'static str {
    let labels_lower: Vec<String> = issue.labels.iter().map(|l| l.to_lowercase()).collect();

    if labels_lower
        .iter()
        .any(|l| l.contains("bug") || l.contains("fix"))
    {
        "fix"
    } else if labels_lower.iter().any(|l| l.contains("refactor")) {
        "refactor"
    } else if labels_lower
        .iter()
        .any(|l| l.contains("docs") || l.contains("documentation"))
    {
        "docs"
    } else if labels_lower.iter().any(|l| l.contains("test")) {
        "test"
    } else if labels_lower
        .iter()
        .any(|l| l.contains("perf") || l.contains("performance"))
    {
        "perf"
    } else if labels_lower
        .iter()
        .any(|l| l.contains("chore") || l.contains("maintenance"))
    {
        "chore"
    } else {
        "feat"
    }
}

/// Create a URL-safe slug from a title (max 30 chars).
fn infer_slug(title: &str) -> String {
    let slug: String = title
        .to_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();

    let mut result = String::new();
    let mut last_was_hyphen = false;
    for c in slug.chars() {
        if c == '-' {
            if !last_was_hyphen && !result.is_empty() {
                result.push('-');
                last_was_hyphen = true;
            }
        } else {
            result.push(c);
            last_was_hyphen = false;
        }
    }

    let trimmed = result.trim_end_matches('-');
    let max_len = 30;
    if trimmed.len() > max_len {
        trimmed
            .get(..max_len)
            .unwrap_or(trimmed)
            .trim_end_matches('-')
            .to_owned()
    } else {
        trimmed.to_owned()
    }
}

/// Convert issue title to an imperative task description.
fn infer_task(issue: &GitHubIssue) -> String {
    let title = issue.title.trim();

    // If already starts with an imperative verb, use as-is
    let imperative_prefixes = [
        "add ",
        "fix ",
        "implement ",
        "refactor ",
        "remove ",
        "update ",
        "create ",
        "delete ",
        "improve ",
        "migrate ",
        "replace ",
        "support ",
        "enable ",
        "disable ",
        "move ",
        "rename ",
        "extract ",
        "split ",
        "merge ",
        "clean ",
        "optimize ",
        "reduce ",
        "increase ",
    ];

    let title_lower = title.to_lowercase();
    for prefix in &imperative_prefixes {
        if title_lower.starts_with(prefix) {
            return title.to_owned();
        }
    }

    // Infer verb from labels
    let labels_lower: Vec<String> = issue.labels.iter().map(|l| l.to_lowercase()).collect();

    if labels_lower.iter().any(|l| l.contains("bug")) {
        format!("Fix {}", lowercase_first(title))
    } else if labels_lower
        .iter()
        .any(|l| l.contains("enhancement") || l.contains("feature"))
    {
        format!("Implement {}", lowercase_first(title))
    } else if labels_lower.iter().any(|l| l.contains("refactor")) {
        format!("Refactor {}", lowercase_first(title))
    } else if labels_lower.iter().any(|l| l.contains("docs")) {
        format!("Document {}", lowercase_first(title))
    } else if labels_lower.iter().any(|l| l.contains("test")) {
        format!("Add tests for {}", lowercase_first(title))
    } else {
        format!("Implement {}", lowercase_first(title))
    }
}

/// Lowercase the first character of a string.
fn lowercase_first(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) => {
            let mut result = c.to_lowercase().to_string();
            result.push_str(chars.as_str());
            result
        }
        None => String::new(),
    }
}

/// Extract acceptance criteria from issue body.
///
/// Looks for checkbox items or modal patterns (`should`/`must`/`needs to`).
fn extract_acceptance_criteria(issue: &GitHubIssue) -> Vec<String> {
    let mut criteria = Vec::new();

    // Extract checkbox items from body
    for line in issue.body.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed
            .strip_prefix("- [ ] ")
            .or_else(|| trimmed.strip_prefix("- [x] "))
            .or_else(|| trimmed.strip_prefix("* [ ] "))
            .or_else(|| trimmed.strip_prefix("* [x] "))
            && !rest.is_empty()
        {
            criteria.push(rest.to_owned());
        }
    }

    // If no checkboxes, look for modal patterns
    if criteria.is_empty() {
        for line in issue.body.lines() {
            let trimmed = line.trim();
            if (trimmed.contains(" should ")
                || trimmed.contains(" must ")
                || trimmed.contains(" needs to "))
                && trimmed.len() > 10
                && trimmed.len() < 200
            {
                criteria.push(trimmed.to_owned());
            }
        }
    }

    // Fallback: generate generic criteria from title
    if criteria.is_empty() {
        criteria.push(format!("Implementation addresses #{}", issue.number));
        criteria.push("Unit tests cover new functionality".to_owned());
        criteria.push("No regressions in existing tests".to_owned());
    }

    criteria
}

/// Infer blast radius from issue body (backtick paths) or domain.
fn infer_blast_radius(issue: &GitHubIssue, domain: &str) -> Vec<String> {
    let mut paths = Vec::new();

    // Extract backtick-quoted paths from body
    let mut in_backtick = false;
    let mut current = String::new();
    for c in issue.body.chars() {
        if c == '`' {
            if in_backtick {
                let trimmed = current.trim().to_owned();
                if looks_like_path(&trimmed) {
                    paths.push(trimmed);
                }
                current.clear();
            }
            in_backtick = !in_backtick;
        } else if in_backtick {
            current.push(c);
        }
    }

    // Fallback to domain-based path
    if paths.is_empty() {
        if domain == "general" {
            paths.push("crates/".to_owned());
        } else {
            paths.push(format!("crates/{domain}/src/"));
        }
    }

    paths
}

/// Heuristic: does this string look like a file path?
fn looks_like_path(s: &str) -> bool {
    (s.contains('/') || s.contains('.'))
        && !s.contains(' ')
        && s.len() > 3
        && s.len() < 200
        && !s.starts_with("http")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_issue(title: &str, body: &str, labels: &[&str]) -> GitHubIssue {
        GitHubIssue {
            number: 42,
            title: title.to_owned(),
            body: body.to_owned(),
            labels: labels.iter().map(|s| (*s).to_owned()).collect(),
            milestone: None,
            author: "alice".to_owned(),
            created_at: "2026-03-01T00:00:00Z".to_owned(),
            priority_label: None,
        }
    }

    #[test]
    fn generated_prompt_has_required_sections() {
        let issue = make_issue(
            "Add query cache to mneme",
            "We need a query cache.\n- [ ] Cache hits under 1ms\n- [ ] TTL support",
            &["enhancement"],
        );
        let prompt = generate_prompt(&issue, "forkwright/aletheia");

        assert!(prompt.contains("## Directive"), "missing Directive section");
        assert!(prompt.contains("## Standards"), "missing Standards section");
        assert!(prompt.contains("## Setup"), "missing Setup section");
        assert!(prompt.contains("## Task"), "missing Task section");
        assert!(
            prompt.contains("## Acceptance criteria"),
            "missing Acceptance criteria section"
        );
        assert!(
            prompt.contains("## Blast radius"),
            "missing Blast radius section"
        );
        assert!(
            prompt.contains("## Validation gate"),
            "missing Validation gate section"
        );
        assert!(
            prompt.contains("## Completion"),
            "missing Completion section"
        );
        assert!(
            prompt.contains("## Observations"),
            "missing Observations section"
        );
    }

    #[test]
    fn generated_prompt_has_frontmatter() {
        let issue = make_issue("Test issue", "", &[]);
        let prompt = generate_prompt(&issue, "forkwright/aletheia");
        assert!(prompt.starts_with("---\n"), "must start with frontmatter");
        assert!(
            prompt.contains("model: claude-opus-4-6"),
            "must specify model"
        );
    }

    #[test]
    fn generated_prompt_references_issue_number() {
        let issue = make_issue("Fix bug", "", &["bug"]);
        let prompt = generate_prompt(&issue, "forkwright/aletheia");
        assert!(
            prompt.contains("Closes #42"),
            "must reference issue for closure"
        );
        assert!(prompt.contains("#42"), "must reference issue number");
    }

    #[test]
    fn domain_inferred_from_labels() {
        let issue = make_issue("Fix thing", "", &["pylon"]);
        assert_eq!(infer_domain(&issue), "pylon");
    }

    #[test]
    fn domain_inferred_from_title() {
        let issue = make_issue("Fix mneme query cache", "", &[]);
        assert_eq!(infer_domain(&issue), "mneme");
    }

    #[test]
    fn domain_fallback_to_general() {
        let issue = make_issue("Vague issue", "", &[]);
        assert_eq!(infer_domain(&issue), "general");
    }

    #[test]
    fn branch_type_from_bug_label() {
        let issue = make_issue("Crash", "", &["bug"]);
        assert_eq!(infer_branch_type(&issue), "fix");
    }

    #[test]
    fn branch_type_default_to_feat() {
        let issue = make_issue("New thing", "", &[]);
        assert_eq!(infer_branch_type(&issue), "feat");
    }

    #[test]
    fn task_adds_verb_prefix() {
        let issue = make_issue("Memory leak in cache", "", &["bug"]);
        let task = infer_task(&issue);
        assert!(
            task.starts_with("Fix "),
            "bug should get Fix prefix: {task}"
        );
    }

    #[test]
    fn task_preserves_imperative() {
        let issue = make_issue("Add query cache", "", &[]);
        let task = infer_task(&issue);
        assert_eq!(task, "Add query cache");
    }

    #[test]
    fn acceptance_criteria_from_checkboxes() {
        let issue = make_issue(
            "Task",
            "Requirements:\n- [ ] First thing\n- [ ] Second thing\n- [x] Done thing",
            &[],
        );
        let criteria = extract_acceptance_criteria(&issue);
        assert_eq!(criteria.len(), 3, "should extract all checkboxes");
        assert_eq!(criteria.first(), Some(&"First thing".to_owned()));
    }

    #[test]
    fn acceptance_criteria_fallback() {
        let issue = make_issue("Simple task", "No checkboxes here.", &[]);
        let criteria = extract_acceptance_criteria(&issue);
        assert!(!criteria.is_empty(), "should generate fallback criteria");
        assert!(
            criteria.iter().any(|c| c.contains("#42")),
            "fallback should reference issue"
        );
    }

    #[test]
    fn blast_radius_from_backtick_paths() {
        let issue = make_issue(
            "Fix",
            "The bug is in `crates/pylon/src/handlers/session.rs`",
            &[],
        );
        let paths = infer_blast_radius(&issue, "general");
        assert!(
            paths.iter().any(|p| p.contains("crates/pylon")),
            "should extract path from backticks: {paths:?}"
        );
    }

    #[test]
    fn blast_radius_fallback_to_domain() {
        let issue = make_issue("Fix thing", "No paths mentioned", &[]);
        let paths = infer_blast_radius(&issue, "pylon");
        assert_eq!(paths, vec!["crates/pylon/src/"]);
    }

    #[test]
    fn slug_generation() {
        assert_eq!(infer_slug("Add Query Cache"), "add-query-cache");
        assert_eq!(infer_slug("Fix: memory leak!"), "fix-memory-leak");
    }
}

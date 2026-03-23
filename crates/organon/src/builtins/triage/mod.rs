//! Agent self-prompted issue triage tools.
//!
//! Enables agents to scan open GitHub issues, evaluate relevance, generate
//! structured prompts conforming to the kanon format, and stage them for
//! human approval before dispatch.
//!
//! Tools:
//! - `issue_scan`: Fetch and filter open GitHub issues
//! - `issue_triage`: Score issues, generate prompts, write to staging
//! - `issue_approve`: Move staged prompts from staging to queue (human gate)
mod prompt_gen;
mod scoring;

use std::fmt::Write as _;
use std::future::Future;
use std::pin::Pin;

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use tracing::instrument;

use aletheia_koina::id::ToolName;

use crate::error::Result;
use crate::registry::{ToolExecutor, ToolRegistry};
use crate::types::{
    InputSchema, PropertyDef, PropertyType, Reversibility, ToolCategory, ToolContext, ToolDef,
    ToolInput, ToolResult,
};

use prompt_gen::generate_prompt;
use scoring::{compute_priority_score, score_relevance};

use super::workspace::{extract_opt_u64, extract_str};

/// A parsed GitHub issue with extracted metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct GitHubIssue {
    /// Issue number.
    pub(crate) number: u64,
    /// Issue title.
    pub(crate) title: String,
    /// Issue body (markdown).
    pub(crate) body: String,
    /// Labels attached to the issue.
    pub(crate) labels: Vec<String>,
    /// Milestone name, if any.
    pub(crate) milestone: Option<String>,
    /// Issue author login.
    pub(crate) author: String,
    /// ISO-8601 creation timestamp.
    pub(crate) created_at: String,
    /// GitHub-assigned priority label value (e.g. `priority/high`).
    pub(crate) priority_label: Option<String>,
}

/// Relevance assessment for a single issue.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct RelevanceResult {
    /// The assessed issue.
    pub(crate) issue: GitHubIssue,
    /// Relevance score between 0.0 and 1.0.
    pub(crate) relevance: f64,
    /// Human-readable rationale for the score.
    pub(crate) rationale: String,
}

/// A staged prompt awaiting human approval.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct StagedPrompt {
    /// Prompt filename (without path).
    pub(crate) filename: String,
    /// The issue this prompt was generated from.
    pub(crate) issue_number: u64,
    /// Combined priority score (relevance x priority x impact).
    pub(crate) priority_score: f64,
    /// Full path to the staged file.
    pub(crate) staged_path: String,
}

fn require_services(
    ctx: &ToolContext,
) -> std::result::Result<&crate::types::ToolServices, ToolResult> {
    ctx.services
        .as_deref()
        .ok_or_else(|| ToolResult::error("tool services not configured"))
}

/// Extract optional string field from JSON arguments.
fn extract_opt_str<'a>(args: &'a serde_json::Value, field: &str) -> Option<&'a str> {
    args.get(field).and_then(serde_json::Value::as_str)
}

// ---------------------------------------------------------------------------
// `issue_scan` tool
// ---------------------------------------------------------------------------

struct IssueScanExecutor;

impl ToolExecutor for IssueScanExecutor {
    fn execute<'a>(
        &'a self,
        input: &'a ToolInput,
        ctx: &'a ToolContext,
    ) -> Pin<Box<dyn Future<Output = Result<ToolResult>> + Send + 'a>> {
        Box::pin(async {
            let services = match require_services(ctx) {
                Ok(s) => s,
                Err(r) => return Ok(r),
            };

            let repo = extract_str(&input.arguments, "repo", &input.name)?;
            let label_filter = extract_opt_str(&input.arguments, "label");
            let milestone_filter = extract_opt_str(&input.arguments, "milestone");
            let limit = extract_opt_u64(&input.arguments, "limit").unwrap_or(30);

            let issues = match fetch_issues(
                &services.http_client,
                repo,
                label_filter,
                milestone_filter,
                limit,
            )
            .await
            {
                Ok(issues) => issues,
                Err(msg) => return Ok(ToolResult::error(msg)),
            };

            let summary = format_issue_summary(&issues);
            Ok(ToolResult::text(summary))
        })
    }
}

// ---------------------------------------------------------------------------
// `issue_triage` tool
// ---------------------------------------------------------------------------

struct IssueTriageExecutor;

impl ToolExecutor for IssueTriageExecutor {
    fn execute<'a>(
        &'a self,
        input: &'a ToolInput,
        ctx: &'a ToolContext,
    ) -> Pin<Box<dyn Future<Output = Result<ToolResult>> + Send + 'a>> {
        Box::pin(async {
            let services = match require_services(ctx) {
                Ok(s) => s,
                Err(r) => return Ok(r),
            };

            let repo = extract_str(&input.arguments, "repo", &input.name)?;
            let staging_dir = extract_str(&input.arguments, "staging_dir", &input.name)?;
            let label_filter = extract_opt_str(&input.arguments, "label");
            let milestone_filter = extract_opt_str(&input.arguments, "milestone");
            let threshold = extract_opt_f64(&input.arguments, "threshold").unwrap_or(0.3);
            let context_keywords =
                extract_opt_str(&input.arguments, "context_keywords").unwrap_or("");
            let limit = extract_opt_u64(&input.arguments, "limit").unwrap_or(30);

            // 1. Fetch issues
            let issues = match fetch_issues(
                &services.http_client,
                repo,
                label_filter,
                milestone_filter,
                limit,
            )
            .await
            {
                Ok(issues) => issues,
                Err(msg) => return Ok(ToolResult::error(msg)),
            };

            if issues.is_empty() {
                return Ok(ToolResult::text("No open issues found matching filters."));
            }

            // 2. Score relevance and filter by threshold
            let keywords: Vec<&str> = context_keywords
                .split(',')
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .collect();

            let mut scored: Vec<RelevanceResult> = issues
                .into_iter()
                .map(|issue| {
                    let (relevance, rationale) = score_relevance(&issue, &keywords);
                    RelevanceResult {
                        issue,
                        relevance,
                        rationale,
                    }
                })
                .filter(|r| r.relevance >= threshold)
                .collect();

            if scored.is_empty() {
                return Ok(ToolResult::text(format!(
                    "No issues met the relevance threshold ({threshold:.1})."
                )));
            }

            // 3. Compute priority scores and sort descending
            scored.sort_by(|a, b| {
                let pa = compute_priority_score(a);
                let pb = compute_priority_score(b);
                pb.partial_cmp(&pa).unwrap_or(std::cmp::Ordering::Equal)
            });

            // 4. Generate prompts and write to staging
            let staging = std::path::Path::new(staging_dir);
            if let Err(e) = tokio::fs::create_dir_all(staging).await {
                return Ok(ToolResult::error(format!(
                    "failed to create staging directory: {e}"
                )));
            }

            let mut staged: Vec<StagedPrompt> = Vec::new();
            for result in &scored {
                let prompt_content = generate_prompt(&result.issue, repo);
                let filename = format!(
                    "{}-{}.md",
                    result.issue.number,
                    slugify(&result.issue.title),
                );
                let path = staging.join(&filename);

                if let Err(e) = tokio::fs::write(&path, &prompt_content).await {
                    tracing::warn!(
                        issue = result.issue.number,
                        error = %e,
                        "failed to write staged prompt"
                    );
                    continue;
                }

                let priority = compute_priority_score(result);
                tracing::info!(
                    issue = result.issue.number,
                    relevance = result.relevance,
                    priority_score = priority,
                    filename = %filename,
                    rationale = %result.rationale,
                    "staged prompt generated"
                );

                staged.push(StagedPrompt {
                    filename: filename.clone(),
                    issue_number: result.issue.number,
                    priority_score: priority,
                    staged_path: path.display().to_string(),
                });
            }

            let summary = format_triage_summary(&staged, &scored);
            Ok(ToolResult::text(summary))
        })
    }
}

// ---------------------------------------------------------------------------
// `issue_approve` tool
// ---------------------------------------------------------------------------

struct IssueApproveExecutor;

impl ToolExecutor for IssueApproveExecutor {
    fn execute<'a>(
        &'a self,
        input: &'a ToolInput,
        ctx: &'a ToolContext,
    ) -> Pin<Box<dyn Future<Output = Result<ToolResult>> + Send + 'a>> {
        Box::pin(async {
            let staging_dir = extract_str(&input.arguments, "staging_dir", &input.name)?;
            let queue_dir = extract_str(&input.arguments, "queue_dir", &input.name)?;
            let prompt_id = extract_str(&input.arguments, "prompt_id", &input.name)?;

            let staging = std::path::Path::new(staging_dir);
            let queue = std::path::Path::new(queue_dir);

            // Find the prompt file in staging
            let source = match find_staged_prompt(staging, prompt_id).await {
                Ok(p) => p,
                Err(msg) => return Ok(ToolResult::error(msg)),
            };

            // Ensure queue directory exists
            if let Err(e) = tokio::fs::create_dir_all(queue).await {
                return Ok(ToolResult::error(format!(
                    "failed to create queue directory: {e}"
                )));
            }

            let filename = source.file_name().map_or_else(
                || "unknown.md".to_owned(),
                |n| n.to_string_lossy().into_owned(),
            );
            let dest = queue.join(&filename);

            // Move file from staging to queue
            if let Err(e) = tokio::fs::rename(&source, &dest).await {
                // Fallback: copy + remove (cross-device moves)
                if let Err(copy_err) = tokio::fs::copy(&source, &dest).await {
                    return Ok(ToolResult::error(format!(
                        "failed to move prompt to queue: rename={e}, copy={copy_err}"
                    )));
                }
                if let Err(rm_err) = tokio::fs::remove_file(&source).await {
                    tracing::warn!(
                        path = %source.display(),
                        error = %rm_err,
                        "failed to remove staged file after copy"
                    );
                }
            }

            let approver = ctx.nous_id.as_str();
            let timestamp = jiff::Zoned::now()
                .strftime("%Y-%m-%dT%H:%M:%S%:z")
                .to_string();

            tracing::info!(
                prompt_id = prompt_id,
                approver = approver,
                timestamp = %timestamp,
                source = %source.display(),
                destination = %dest.display(),
                "prompt approved and moved to queue"
            );

            Ok(ToolResult::text(format!(
                "Approved: {filename}\n\
                 Moved: staging -> queue\n\
                 Approver: {approver}\n\
                 Timestamp: {timestamp}\n\
                 Path: {}",
                dest.display()
            )))
        })
    }
}

// ---------------------------------------------------------------------------
// Helper functions
// ---------------------------------------------------------------------------

/// Fetch open issues from GitHub API.
///
/// # Errors
///
/// Returns an error string if the HTTP request or JSON parsing fails.
#[instrument(skip(client), fields(repo = %repo))]
async fn fetch_issues(
    client: &reqwest::Client,
    repo: &str,
    label: Option<&str>,
    milestone: Option<&str>,
    limit: u64,
) -> std::result::Result<Vec<GitHubIssue>, String> {
    let mut url = format!(
        "https://api.github.com/repos/{repo}/issues?state=open&per_page={limit}&sort=updated&direction=desc"
    );
    if let Some(l) = label {
        let _ = write!(url, "&labels={l}");
    }
    if let Some(m) = milestone {
        let _ = write!(url, "&milestone={m}");
    }

    let response = client
        .get(&url)
        .header("Accept", "application/vnd.github.v3+json")
        .header(
            "User-Agent",
            concat!(
                "aletheia/",
                env!("CARGO_PKG_VERSION"),
                " (github.com/forkwright/aletheia)"
            ),
        )
        .send()
        .await
        .map_err(|e| format!("GitHub API request failed: {e}"))?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(format!("GitHub API error ({status}): {body}"));
    }

    let items: Vec<serde_json::Value> = response
        .json()
        .await
        .map_err(|e| format!("failed to parse GitHub response: {e}"))?;

    let mut issues = Vec::new();
    for item in items {
        // Skip pull requests (GitHub issues API includes PRs)
        if item.get("pull_request").is_some() {
            continue;
        }

        let number = item
            .get("number")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(0);
        let title = item
            .get("title")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("")
            .to_owned();
        let body = item
            .get("body")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("")
            .to_owned();
        let author = item
            .get("user")
            .and_then(|u| u.get("login"))
            .and_then(serde_json::Value::as_str)
            .unwrap_or("")
            .to_owned();
        let created_at = item
            .get("created_at")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("")
            .to_owned();
        let milestone = item
            .get("milestone")
            .and_then(|m| m.get("title"))
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned);

        let labels: Vec<String> = item
            .get("labels")
            .and_then(serde_json::Value::as_array)
            .map(|arr| {
                arr.iter()
                    .filter_map(|l| l.get("name").and_then(serde_json::Value::as_str))
                    .map(str::to_owned)
                    .collect()
            })
            .unwrap_or_default();

        let priority_label = labels
            .iter()
            .find(|l| l.starts_with("priority/") || l.starts_with('P'))
            .cloned();

        issues.push(GitHubIssue {
            number,
            title,
            body,
            labels,
            milestone,
            author,
            created_at,
            priority_label,
        });
    }

    Ok(issues)
}

/// Find a staged prompt by ID (issue number prefix or filename).
async fn find_staged_prompt(
    staging: &std::path::Path,
    prompt_id: &str,
) -> std::result::Result<std::path::PathBuf, String> {
    let mut entries = tokio::fs::read_dir(staging)
        .await
        .map_err(|e| format!("failed to read staging directory: {e}"))?;

    let mut candidates = Vec::new();
    while let Some(entry) = entries
        .next_entry()
        .await
        .map_err(|e| format!("failed to read staging entry: {e}"))?
    {
        let name = entry.file_name().to_string_lossy().into_owned();
        // Match by exact filename or by issue number prefix
        let dash_prefix = format!("{prompt_id}-");
        let dot_prefix = format!("{prompt_id}.");
        if name == prompt_id || name.starts_with(&dash_prefix) || name.starts_with(&dot_prefix) {
            candidates.push(entry.path());
        }
    }

    match candidates.len() {
        0 => Err(format!("no staged prompt found matching '{prompt_id}'")),
        // SAFETY: len() == 1 guarantees next() returns Some
        1 => Ok(candidates.into_iter().next().unwrap_or_default()),
        n => Err(format!(
            "ambiguous prompt_id '{prompt_id}': matched {n} files"
        )),
    }
}

/// Convert a title to a URL-safe slug.
fn slugify(title: &str) -> String {
    let slug: String = title
        .to_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    // Collapse multiple hyphens and trim
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
    // Trim trailing hyphen and limit length
    let trimmed = result.trim_end_matches('-');
    let max_len = 50;
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

/// Extract optional `f64` field from JSON arguments.
fn extract_opt_f64(args: &serde_json::Value, field: &str) -> Option<f64> {
    args.get(field).and_then(serde_json::Value::as_f64)
}

/// Format a human-readable summary of fetched issues.
fn format_issue_summary(issues: &[GitHubIssue]) -> String {
    use std::fmt::Write as _;

    if issues.is_empty() {
        return "No open issues found matching filters.".to_owned();
    }

    let mut out = format!("Found {} open issues:\n\n", issues.len());
    for issue in issues {
        let _ = writeln!(out, "#{} — {}", issue.number, issue.title);
        if !issue.labels.is_empty() {
            let _ = writeln!(out, "  Labels: {}", issue.labels.join(", "));
        }
        if let Some(ref ms) = issue.milestone {
            let _ = writeln!(out, "  Milestone: {ms}");
        }
        if let Some(ref p) = issue.priority_label {
            let _ = writeln!(out, "  Priority: {p}");
        }
        let _ = writeln!(out, "  Created: {}", issue.created_at);
        out.push('\n');
    }
    out
}

/// Format a summary of the triage operation.
fn format_triage_summary(staged: &[StagedPrompt], scored: &[RelevanceResult]) -> String {
    use std::fmt::Write as _;

    let mut out = format!(
        "Triage complete: {}/{} issues passed threshold.\n\
         Staged {} prompts.\n\n",
        scored.len(),
        scored.len(),
        staged.len(),
    );

    for sp in staged {
        let _ = write!(
            out,
            "  #{} — {} (priority: {:.2})\n    -> {}\n",
            sp.issue_number, sp.filename, sp.priority_score, sp.staged_path,
        );
    }

    if staged.is_empty() {
        out.push_str("  (no prompts staged — check write permissions)\n");
    }

    out.push_str(
        "\nUse issue_approve to move staged prompts to the dispatch queue.\n\
         Prompts are in staging only — not yet dispatchable.",
    );

    out
}

// ---------------------------------------------------------------------------
// Tool definitions
// ---------------------------------------------------------------------------

fn issue_scan_def() -> ToolDef {
    ToolDef {
        name: ToolName::from_static("issue_scan"), // kanon:ignore RUST/expect
        description: "Fetch open GitHub issues filtered by labels or milestone. Returns issue metadata for triage.".to_owned(),
        extended_description: Some(
            "Fetches open issues from a GitHub repository via the REST API. \
             Filters by label and milestone. Returns title, body, labels, priority, \
             and creation date for each issue. Use before `issue_triage` to preview \
             available work.".to_owned()
        ),
        input_schema: InputSchema {
            properties: IndexMap::from([
                ("repo".to_owned(), PropertyDef {
                    property_type: PropertyType::String,
                    description: "GitHub repository in owner/repo format (e.g. `forkwright/aletheia`)".to_owned(),
                    enum_values: None,
                    default: None,
                }),
                ("label".to_owned(), PropertyDef {
                    property_type: PropertyType::String,
                    description: "Filter by label name (optional)".to_owned(),
                    enum_values: None,
                    default: None,
                }),
                ("milestone".to_owned(), PropertyDef {
                    property_type: PropertyType::String,
                    description: "Filter by milestone name (optional)".to_owned(),
                    enum_values: None,
                    default: None,
                }),
                ("limit".to_owned(), PropertyDef {
                    property_type: PropertyType::Integer,
                    description: "Maximum number of issues to fetch (default: 30)".to_owned(),
                    enum_values: None,
                    default: Some(serde_json::json!(30)),
                }),
            ]),
            required: vec!["repo".to_owned()],
        },
        category: ToolCategory::Planning,
        reversibility: Reversibility::FullyReversible,
        auto_activate: false,
    }
}

fn issue_triage_def() -> ToolDef {
    ToolDef {
        name: ToolName::from_static("issue_triage"), // kanon:ignore RUST/expect
        description: "Score GitHub issues for relevance, generate kanon-format prompts, and stage them for human approval.".to_owned(),
        extended_description: Some(
            "Fetches open issues, scores each for relevance (0.0-1.0) against provided \
             context keywords and capability set, generates structured prompts in kanon \
             format, ranks by priority, and writes to a staging directory. Prompts are NOT \
             injected into the dispatch queue — use `issue_approve` for that.".to_owned()
        ),
        input_schema: InputSchema {
            properties: IndexMap::from([
                ("repo".to_owned(), PropertyDef {
                    property_type: PropertyType::String,
                    description: "GitHub repository in owner/repo format (e.g. `forkwright/aletheia`)".to_owned(),
                    enum_values: None,
                    default: None,
                }),
                ("staging_dir".to_owned(), PropertyDef {
                    property_type: PropertyType::String,
                    description: "Path to the staging directory for generated prompts".to_owned(),
                    enum_values: None,
                    default: None,
                }),
                ("label".to_owned(), PropertyDef {
                    property_type: PropertyType::String,
                    description: "Filter by label name (optional)".to_owned(),
                    enum_values: None,
                    default: None,
                }),
                ("milestone".to_owned(), PropertyDef {
                    property_type: PropertyType::String,
                    description: "Filter by milestone name (optional)".to_owned(),
                    enum_values: None,
                    default: None,
                }),
                ("threshold".to_owned(), PropertyDef {
                    property_type: PropertyType::Number,
                    description: "Minimum relevance score (0.0-1.0) to generate a prompt (default: 0.3)".to_owned(),
                    enum_values: None,
                    default: Some(serde_json::json!(0.3)),
                }),
                ("context_keywords".to_owned(), PropertyDef {
                    property_type: PropertyType::String,
                    description: "Comma-separated keywords describing agent's current context and capabilities".to_owned(),
                    enum_values: None,
                    default: None,
                }),
                ("limit".to_owned(), PropertyDef {
                    property_type: PropertyType::Integer,
                    description: "Maximum number of issues to fetch (default: 30)".to_owned(),
                    enum_values: None,
                    default: Some(serde_json::json!(30)),
                }),
            ]),
            required: vec!["repo".to_owned(), "staging_dir".to_owned()],
        },
        category: ToolCategory::Planning,
        reversibility: Reversibility::PartiallyReversible,
        auto_activate: false,
    }
}

fn issue_approve_def() -> ToolDef {
    ToolDef {
        name: ToolName::from_static("issue_approve"), // kanon:ignore RUST/expect
        description:
            "Move a staged prompt from staging to the dispatch queue. Human approval gate."
                .to_owned(),
        extended_description: Some(
            "Moves a previously staged prompt from the staging directory to the dispatch \
             queue. This is the human approval gate — prompts only become dispatchable \
             after explicit approval. Logs the approver identity and timestamp."
                .to_owned(),
        ),
        input_schema: InputSchema {
            properties: IndexMap::from([
                (
                    "staging_dir".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::String,
                        description: "Path to the staging directory containing generated prompts"
                            .to_owned(),
                        enum_values: None,
                        default: None,
                    },
                ),
                (
                    "queue_dir".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::String,
                        description: "Path to the dispatch queue directory".to_owned(),
                        enum_values: None,
                        default: None,
                    },
                ),
                (
                    "prompt_id".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::String,
                        description: "Issue number or filename of the staged prompt to approve"
                            .to_owned(),
                        enum_values: None,
                        default: None,
                    },
                ),
            ]),
            required: vec![
                "staging_dir".to_owned(),
                "queue_dir".to_owned(),
                "prompt_id".to_owned(),
            ],
        },
        category: ToolCategory::Planning,
        reversibility: Reversibility::PartiallyReversible,
        auto_activate: false,
    }
}

/// Register triage tools into the registry.
///
/// # Errors
///
/// Returns an error if any tool name collides with an already-registered tool.
pub(crate) fn register(registry: &mut ToolRegistry) -> Result<()> {
    registry.register(issue_scan_def(), Box::new(IssueScanExecutor))?;
    registry.register(issue_triage_def(), Box::new(IssueTriageExecutor))?;
    registry.register(issue_approve_def(), Box::new(IssueApproveExecutor))?;
    Ok(())
}

#[cfg(test)]
#[path = "triage_tests.rs"]
mod tests;

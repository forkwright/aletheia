//! Specialized role templates for ephemeral sub-agents.

use std::fmt;

/// Sub-agent role determining system prompt, tool access, and model preference.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Role {
    /// Implementation, testing, debugging. Full workspace access.
    Coder,
    /// Investigation, comparison, documentation. Read-only plus web access.
    Researcher,
    /// Code review, standards compliance, risk assessment. Read-only, no writes.
    Reviewer,
    /// Codebase exploration, architecture understanding. Read-only, no execution.
    Explorer,
    /// Task execution, command running, deployment. Execute plus read, no edits.
    Runner,
}

impl Role {
    /// Parse a role string into a typed variant.
    ///
    /// Returns `None` for unrecognized role names.
    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "coder" => Some(Self::Coder),
            "researcher" => Some(Self::Researcher),
            "reviewer" => Some(Self::Reviewer),
            "explorer" => Some(Self::Explorer),
            "runner" => Some(Self::Runner),
            _ => None,
        }
    }

    /// All defined roles.
    #[must_use]
    pub fn all() -> &'static [Role] {
        &[
            Self::Coder,
            Self::Researcher,
            Self::Reviewer,
            Self::Explorer,
            Self::Runner,
        ]
    }

    /// Role name as a lowercase string.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Coder => "coder",
            Self::Researcher => "researcher",
            Self::Reviewer => "reviewer",
            Self::Explorer => "explorer",
            Self::Runner => "runner",
        }
    }

    /// Structured template for this role.
    #[must_use]
    pub fn template(self) -> RoleTemplate {
        match self {
            Self::Coder => coder_template(),
            Self::Researcher => researcher_template(),
            Self::Reviewer => reviewer_template(),
            Self::Explorer => explorer_template(),
            Self::Runner => runner_template(),
        }
    }
}

impl fmt::Display for Role {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Tool access policy for a role.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToolPolicy {
    /// All registered tools available.
    Unrestricted,
    /// Only the listed tools are available. Everything else is denied.
    AllowOnly(Vec<String>),
}

impl ToolPolicy {
    /// Check whether a tool name is permitted under this policy.
    #[must_use]
    pub fn is_allowed(&self, tool_name: &str) -> bool {
        match self {
            Self::Unrestricted => true,
            Self::AllowOnly(allowed) => allowed.iter().any(|a| a == tool_name),
        }
    }

    /// Convert to an allowlist for `NousConfig.tool_allowlist`.
    ///
    /// Returns `None` for unrestricted (all tools allowed).
    #[must_use]
    pub fn to_allowlist(&self) -> Option<Vec<String>> {
        match self {
            Self::Unrestricted => None,
            Self::AllowOnly(list) => Some(list.clone()),
        }
    }
}

/// Structured role template with system prompt, tool restrictions, and model preference.
#[derive(Debug, Clone)]
pub struct RoleTemplate {
    /// Role identifier.
    pub role: Role,
    /// System prompt injected into the sub-agent's context.
    pub system_prompt: &'static str,
    /// Tool access restrictions.
    pub tool_policy: ToolPolicy,
    /// Preferred model identifier.
    pub model: &'static str,
}

const OPUS_MODEL: &str = "claude-opus-4-20250514";
const SONNET_MODEL: &str = "claude-sonnet-4-20250514";
const HAIKU_MODEL: &str = "claude-haiku-4-5-20251001";

fn coder_template() -> RoleTemplate {
    RoleTemplate {
        role: Role::Coder,
        system_prompt: CODER_PROMPT,
        tool_policy: ToolPolicy::AllowOnly(vec![
            "read".into(),
            "write".into(),
            "edit".into(),
            "exec".into(),
            "grep".into(),
            "find".into(),
            "ls".into(),
            "view_file".into(),
            "memory_search".into(),
            "note".into(),
        ]),
        model: SONNET_MODEL,
    }
}

fn researcher_template() -> RoleTemplate {
    RoleTemplate {
        role: Role::Researcher,
        system_prompt: RESEARCHER_PROMPT,
        tool_policy: ToolPolicy::AllowOnly(vec![
            "read".into(),
            "grep".into(),
            "find".into(),
            "ls".into(),
            "view_file".into(),
            "web_fetch".into(),
            "memory_search".into(),
            "note".into(),
        ]),
        model: SONNET_MODEL,
    }
}

fn reviewer_template() -> RoleTemplate {
    RoleTemplate {
        role: Role::Reviewer,
        system_prompt: REVIEWER_PROMPT,
        tool_policy: ToolPolicy::AllowOnly(vec![
            "read".into(),
            "grep".into(),
            "find".into(),
            "ls".into(),
            "view_file".into(),
            "memory_search".into(),
        ]),
        model: OPUS_MODEL,
    }
}

fn explorer_template() -> RoleTemplate {
    RoleTemplate {
        role: Role::Explorer,
        system_prompt: EXPLORER_PROMPT,
        tool_policy: ToolPolicy::AllowOnly(vec![
            "read".into(),
            "grep".into(),
            "find".into(),
            "ls".into(),
            "view_file".into(),
        ]),
        model: HAIKU_MODEL,
    }
}

fn runner_template() -> RoleTemplate {
    RoleTemplate {
        role: Role::Runner,
        system_prompt: RUNNER_PROMPT,
        tool_policy: ToolPolicy::AllowOnly(vec![
            "read".into(),
            "exec".into(),
            "grep".into(),
            "find".into(),
            "ls".into(),
            "view_file".into(),
        ]),
        model: HAIKU_MODEL,
    }
}

// --- System prompts ported from TS roles/prompts/*.ts ---

const CODER_PROMPT: &str = "\
You are a coder sub-agent. You write and modify code to complete a specific task.

## Workflow

1. Read the relevant files to understand the current code
2. Make the specified changes
3. Run the build to verify your changes compile
4. Run relevant tests if they exist
5. Report what you changed

## Rules

- Stay in scope. Do exactly what was asked. Do not refactor surrounding code or add features.
- Match existing patterns. Follow the same style as the codebase.
- Build must pass. If your changes break the build, fix them before reporting.
- Ask nothing. Make the conservative choice on ambiguity and note it in your result.
- No filler. Work and report.

## Output

End your response with a JSON block:

```json
{
  \"role\": \"coder\",
  \"status\": \"success | partial | failed\",
  \"summary\": \"what you did\",
  \"filesChanged\": [\"path/to/file\"],
  \"confidence\": 0.95
}
```";

const RESEARCHER_PROMPT: &str = "\
You are a researcher sub-agent. You find and synthesize information.

## Workflow

1. Understand the question and scope constraints
2. Search for information using available tools
3. Read and evaluate sources for relevance
4. Synthesize findings into a structured report
5. Note confidence levels and caveats

## Rules

- Cite sources. Every claim traces to a specific URL or document.
- Distinguish fact from inference. Say so when extrapolating.
- Respect scope constraints. Stay within the specified sources.
- Recency matters. Prefer the most recent documentation.
- Admit gaps. \"Could not find authoritative information on X\" is a valid finding.
- No filler. Findings, not feelings.

## Output

End your response with a JSON block:

```json
{
  \"role\": \"researcher\",
  \"status\": \"success | partial | failed\",
  \"summary\": \"the answer or key finding\",
  \"findings\": [{\"claim\": \"...\", \"source\": \"...\", \"confidence\": 0.9}],
  \"gaps\": [\"things you could not verify\"],
  \"confidence\": 0.85
}
```";

const REVIEWER_PROMPT: &str = "\
You are a code reviewer sub-agent. You read code and find problems. You do NOT fix code.

## Workflow

1. Read the diff or files provided
2. Understand the intent from the task description
3. Check for: correctness, edge cases, error handling, style, backward compatibility, security
4. Report findings as structured issues

## Rules

- Be specific. \"Line 42: NULL check missing, destructure on line 43 will throw\" is useful. \"Error handling could be improved\" is not.
- Severity matters. Unhandled null = error. Naming inconsistency = info.
- Acknowledge clean code. Do not invent problems.
- Check backward compatibility. Will existing data or clients break?
- Check test coverage. Are new code paths tested?
- No filler. Report findings directly.

## Issue Categories

- Error (must fix): unhandled null, missing error handling, type unsafety, injection, races, breaking changes
- Warning (should fix): missing edge cases, inconsistent patterns, missing tests, performance issues
- Info (consider): naming, alternative approaches, style preferences

## Output

End your response with a JSON block:

```json
{
  \"role\": \"reviewer\",
  \"status\": \"success\",
  \"verdict\": \"approve | request-changes | needs-discussion\",
  \"issues\": [{\"severity\": \"error\", \"file\": \"path\", \"line\": 42, \"message\": \"...\"}],
  \"summary\": \"overall assessment\",
  \"confidence\": 0.9
}
```";

const EXPLORER_PROMPT: &str = "\
You are an explorer sub-agent. You investigate codebases. You are read-only.

## Workflow

1. Start from the provided files or directories
2. Use grep/find to locate relevant code
3. Read files to understand structure and logic
4. Trace call chains when asked
5. Report findings with file paths and line numbers

## Rules

- Read-only. Never use write, edit, or exec. If modification is needed, report that and stop.
- Be precise. Include file paths and line numbers for every finding.
- Trace completely. Follow call chains from entry point to final execution.
- Summarize, do not dump. Return the answer, not every file you read.
- Stay efficient. Use grep before reading whole files.
- No filler. Findings and paths, not narration.

## Output

End your response with a JSON block:

```json
{
  \"role\": \"explorer\",
  \"status\": \"success | partial | failed\",
  \"summary\": \"the answer\",
  \"relevantFiles\": [{\"path\": \"...\", \"role\": \"what this file does\"}],
  \"callChain\": [\"entry() -> middleware() -> handler()\"],
  \"confidence\": 0.9
}
```";

const RUNNER_PROMPT: &str = "\
You are a runner sub-agent. You execute commands and report results.

## Workflow

1. Run the specified commands
2. Capture stdout, stderr, and exit codes
3. For test suites: count total, passed, failed, extract failure details
4. For health checks: report status of each endpoint
5. Report everything structured

## Rules

- Run exactly what is asked. Do not add extra commands unless instructed.
- Capture everything. Exit codes, stderr, stdout.
- Report, do not diagnose. \"Test X failed with error Y\" is your job. Suggesting fixes is not.
- Safe commands only. Never run destructive commands unless explicitly part of the task.
- No filler. Exit codes and output, not commentary.
- Timeout awareness. Report hangs. Do not retry unless instructed.

## Output

End your response with a JSON block:

```json
{
  \"role\": \"runner\",
  \"status\": \"success | partial | failed\",
  \"summary\": \"overall result\",
  \"commands\": [{\"command\": \"...\", \"exitCode\": 0, \"stdout\": \"...\"}],
  \"confidence\": 0.95
}
```";

#[cfg(test)]
mod tests {
    #![expect(clippy::expect_used, reason = "test assertions")]

    use super::*;

    #[test]
    fn all_roles_have_templates() {
        for role in Role::all() {
            let template = role.template();
            assert_eq!(template.role, *role, "template role mismatch for {role}");
            assert!(
                !template.system_prompt.is_empty(),
                "empty system prompt for {role}"
            );
            assert!(!template.model.is_empty(), "empty model for {role}");
        }
    }

    #[test]
    fn role_from_str_roundtrip() {
        for role in Role::all() {
            let parsed = Role::parse(role.as_str());
            assert_eq!(parsed, Some(*role), "from_str roundtrip failed for {role}");
        }
    }

    #[test]
    fn role_from_str_unknown_returns_none() {
        assert_eq!(Role::parse("unknown"), None);
        assert_eq!(Role::parse(""), None);
        assert_eq!(Role::parse("planner"), None);
    }

    #[test]
    fn role_display_matches_as_str() {
        for role in Role::all() {
            assert_eq!(role.to_string(), role.as_str());
        }
    }

    #[test]
    fn coder_has_write_access() {
        let template = Role::Coder.template();
        assert!(
            template.tool_policy.is_allowed("write"),
            "coder must have write access"
        );
        assert!(
            template.tool_policy.is_allowed("edit"),
            "coder must have edit access"
        );
        assert!(
            template.tool_policy.is_allowed("exec"),
            "coder must have exec access"
        );
    }

    #[test]
    fn reviewer_cannot_write_or_exec() {
        let template = Role::Reviewer.template();
        assert!(
            !template.tool_policy.is_allowed("write"),
            "reviewer must not write"
        );
        assert!(
            !template.tool_policy.is_allowed("edit"),
            "reviewer must not edit"
        );
        assert!(
            !template.tool_policy.is_allowed("exec"),
            "reviewer must not exec"
        );
    }

    #[test]
    fn explorer_is_read_only() {
        let template = Role::Explorer.template();
        assert!(
            template.tool_policy.is_allowed("read"),
            "explorer must read"
        );
        assert!(
            template.tool_policy.is_allowed("grep"),
            "explorer must grep"
        );
        assert!(
            !template.tool_policy.is_allowed("write"),
            "explorer must not write"
        );
        assert!(
            !template.tool_policy.is_allowed("edit"),
            "explorer must not edit"
        );
        assert!(
            !template.tool_policy.is_allowed("exec"),
            "explorer must not exec"
        );
    }

    #[test]
    fn runner_can_exec_but_not_edit() {
        let template = Role::Runner.template();
        assert!(template.tool_policy.is_allowed("exec"), "runner must exec");
        assert!(template.tool_policy.is_allowed("read"), "runner must read");
        assert!(
            !template.tool_policy.is_allowed("write"),
            "runner must not write"
        );
        assert!(
            !template.tool_policy.is_allowed("edit"),
            "runner must not edit"
        );
    }

    #[test]
    fn researcher_has_web_but_no_exec() {
        let template = Role::Researcher.template();
        assert!(
            template.tool_policy.is_allowed("web_fetch"),
            "researcher must have web access"
        );
        assert!(
            template.tool_policy.is_allowed("read"),
            "researcher must read"
        );
        assert!(
            !template.tool_policy.is_allowed("exec"),
            "researcher must not exec"
        );
        assert!(
            !template.tool_policy.is_allowed("write"),
            "researcher must not write"
        );
    }

    #[test]
    fn model_preferences() {
        assert_eq!(Role::Coder.template().model, SONNET_MODEL);
        assert_eq!(Role::Researcher.template().model, SONNET_MODEL);
        assert_eq!(Role::Reviewer.template().model, OPUS_MODEL);
        assert_eq!(Role::Explorer.template().model, HAIKU_MODEL);
        assert_eq!(Role::Runner.template().model, HAIKU_MODEL);
    }

    #[test]
    fn tool_policy_unrestricted_allows_all() {
        let policy = ToolPolicy::Unrestricted;
        assert!(policy.is_allowed("anything"));
        assert!(policy.is_allowed("write"));
        assert!(policy.to_allowlist().is_none());
    }

    #[test]
    fn tool_policy_allow_only_restricts() {
        let policy = ToolPolicy::AllowOnly(vec!["read".into(), "grep".into()]);
        assert!(policy.is_allowed("read"));
        assert!(policy.is_allowed("grep"));
        assert!(!policy.is_allowed("write"));
        assert!(!policy.is_allowed("exec"));
        let list = policy.to_allowlist().expect("should produce allowlist");
        assert_eq!(list.len(), 2);
    }

    #[test]
    fn five_roles_defined() {
        assert_eq!(Role::all().len(), 5, "must have exactly 5 roles");
    }
}

# R1471: Agent Generates Own Prompts from Open Issues

**Date:** 2026-03-19
**Author:** Research agent
**Status:** Final
**Closes:** #1471

---

## Executive Summary

Currently, aletheia agents are passive: they respond to human-initiated sessions. This proposal adds a capability where an agent can, on a schedule or trigger, inspect a list of open issues (GitHub, Linear, or a local task file), select actionable items aligned with its goals, generate a prompt for itself, and start a new session to work on that prompt autonomously.

**Recommendation: Implement in two phases.** Phase 1 is a new `aletheia prompt-self` CLI command that performs one-shot issue → prompt → session creation. Phase 2 integrates this into the daemon as a cron task. The novel piece is the issue-to-prompt synthesis — everything else (session creation, goal-alignment logic) already exists.

---

## 1. Problem Statement

Agents idle between human-initiated sessions. There is no mechanism for an agent to:

1. Survey its issue backlog independently
2. Identify issues it is qualified and authorized to work on
3. Synthesize a focused work prompt from an issue description
4. Initiate a session and execute against that prompt

The result is that agents are reactive rather than proactive. Work queued as GitHub issues or Linear tickets waits indefinitely unless a human manually starts a session and copy-pastes the issue content. For well-scoped, low-risk tasks (documentation, code search, report generation), the agent could work autonomously.

---

## 2. Proposed Approach

### 2.1 Issue Source Abstraction

The system needs to read open issues from at least one source. Define an `IssueSource` trait:

```rust
pub trait IssueSource: Send + Sync {
    async fn list_open(&self, filter: &IssueFilter) -> Result<Vec<Issue>>;
}

pub struct Issue {
    pub id: String,
    pub title: String,
    pub body: String,
    pub labels: Vec<String>,
    pub assignees: Vec<String>,
    pub url: Option<String>,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
}
```

Phase 1 implementations:

| Source | Implementation |
|---|---|
| GitHub Issues | `gh issue list --json` via the `exec` tool or direct API call |
| Local task file | TOML/YAML file at `instance/nous/{id}/tasks.toml` |
| Linear | GraphQL API (deferred to Phase 2) |

Config:

```toml
[agent_prompts]
enabled = true
source = "github"   # or "local", "linear"
github_repo = "owner/repo"
label_filter = ["agent-ok"]        # only issues with this label
max_issues_per_run = 3
nous_id = "{nous-id}"              # which agent handles these
```

The `label_filter` is the operator's gate: only issues explicitly tagged `agent-ok` (or equivalent) are eligible. This prevents the agent from attempting issues not intended for automation.

### 2.2 Issue-to-Prompt Synthesis

For each selected issue, the agent generates a work prompt:

**Input:**
- Issue title, body, labels
- Agent's GOALS.md (goal alignment)
- Agent's current MEMORY.md (context injection)
- A prompt-synthesis system prompt that instructs the model to produce a concrete, scoped, actionable prompt

**System prompt for synthesis:**
```
You are generating a work prompt for an AI agent. The agent will execute this prompt in a new session.

Your output must:
1. State the specific task in 1–2 sentences
2. List 3–5 concrete steps the agent should take
3. Specify the expected output artifact (file path, API response, report)
4. Include a success criterion (how the agent knows it is done)
5. Flag any ambiguities the agent should clarify before starting

Agent goals: {goals_md}
Recent agent context: {memory_excerpt}
```

The LLM call for prompt synthesis uses the agent's configured model via `hermeneus`. The result is a structured `WorkPrompt`:

```rust
pub struct WorkPrompt {
    pub issue_id: String,
    pub title: String,
    pub body: String,                // synthesized prompt text
    pub steps: Vec<String>,
    pub expected_output: String,
    pub success_criterion: String,
    pub ambiguities: Vec<String>,
}
```

### 2.3 Session Creation

Once a `WorkPrompt` is synthesized:

1. Create a new session via `POST /api/v1/sessions` (nous_id, name = issue title)
2. Post the synthesized prompt as the first message via `POST /api/v1/sessions/{id}/messages`
3. The agent runs normally from that point

For the daemon-driven path (Phase 2), the session is created internally without going through the HTTP API.

### 2.4 Safety Constraints

Self-initiated sessions have stricter constraints than human-initiated ones:

| Constraint | Value |
|---|---|
| `auto_archive_after` | 60 minutes (prevents runaway sessions) |
| `max_tool_calls` | 50 per session |
| `allowed_tools` | Configurable per agent; defaults to read-only tools + `write` in workspace only |
| `human_escalation` | Required before any action tagged `requires_review` |
| `dry_run_mode` | Optionally simulate all write operations |

These constraints are injected into the session's system prompt via a new `AutoSession` bootstrap section.

### 2.5 Issue Selection Strategy

When multiple issues are eligible, the agent selects by:

1. **Goal alignment score** — embedding similarity between issue body and GOALS.md
2. **Estimated complexity** — token count of issue body as a proxy; prefer simpler issues for autonomous work
3. **Recency** — prefer recently updated issues
4. **Label priority** — configurable label → priority mapping

The selection prompt:

```
Given these open issues and your goals, select the {N} issues you are most likely to
complete successfully in a single autonomous session. Prefer well-scoped, low-risk
tasks. Reject issues that require human judgment, external credentials you don't have,
or changes to production systems.
```

### 2.6 CLI Subcommand

```
aletheia prompt-self [--nous-id ID] [--dry-run] [--issue-id ID] [--max N]
```

- `--dry-run`: synthesize and print the prompt without creating a session
- `--issue-id ID`: target a specific issue (skip selection)
- `--max N`: create at most N sessions in this run

### 2.7 Daemon Integration (Phase 2)

Register `AgentPromptTask` in the daemon runner:

```rust
pub struct AgentPromptTask {
    nous_id: NousId,
    config: AgentPromptConfig,
}
```

Schedule: configurable, default `0 9 * * 1-5` (weekday mornings). The task:
1. Fetches open issues from the configured source
2. Runs selection
3. Synthesizes prompts
4. Creates sessions
5. Records which issues were acted on (to avoid repeating)

A simple deduplication store (set of issue IDs in `mneme` as facts with `tag: agent-prompt-handled`) prevents the agent from re-attempting the same issue.

---

## 3. Alternatives Considered

### 3.1 Human-Written Prompt Templates Per Issue Type

Pre-define prompt templates for issue labels; fill in issue body as template variables.

**Rejected.** Brittle — every new issue label requires a new template. LLM synthesis handles the long tail of issue types automatically.

### 3.2 Agent Always Works from GOALS.md (No Issue Source)

Skip issue tracking; the agent generates its own work queue from GOALS.md each day.

**Deferred.** Interesting for fully autonomous goal-driven behavior, but disconnected from the team's actual issue backlog. Issue-driven automation is more predictable and auditable.

### 3.3 Use Claude Code's `/implement` or `/pr-review` Skills

Delegate work to Claude Code skills rather than an aletheia session.

**Out of scope.** Claude Code skills are for the developer's local environment. aletheia sessions run on the server and can access internal APIs, databases, and tools not available to Claude Code.

### 3.4 Webhook-Triggered Sessions (Event-Driven)

Create a session whenever a new issue is labeled `agent-ok` via a GitHub webhook.

**Better long-term.** More responsive than polling. Implement as Phase 3 after the polling-based version is proven. Requires a webhook receiver endpoint in `pylon`.

---

## 4. Open Questions

1. **Authorization:** Who authorizes the agent to work on a given issue? `label_filter` is the mechanism, but it requires someone to apply the label. Is this sufficient governance?

2. **Session result feedback:** After an autonomous session completes, should the agent post a comment back to the GitHub issue with its result? Requires GitHub write permission — a significant privilege escalation.

3. **Failure handling:** If the autonomous session fails (tool error, LLM refusal, timeout), how is this surfaced? File a comment on the issue? Write a finding to the audit log?

4. **Concurrency:** Can the agent run multiple autonomous sessions in parallel (one per issue)? The `NousActor` processes messages sequentially per agent. Parallel sessions for the same agent would require multiple actor instances.

5. **Issue source auth:** GitHub API requires a token. Where is it stored? The existing `credential` system handles API keys; add a `github_token` credential kind.

6. **Prompt quality evaluation:** How does the operator know if synthesized prompts are good before trusting the agent with them? The `--dry-run` flag helps, but a review workflow (synthesize → store as draft → operator approves → session starts) might be needed for early deployments.

7. **Goal drift:** If the agent repeatedly selects issues that are misaligned with GOALS.md (because they score as high similarity but aren't actually aligned), how is this detected? The self-audit goal-alignment check (R1470) covers this.

---

## 5. Implementation Sketch

```
crates/organon/src/
  issue_source/
    mod.rs              # IssueSource trait + Issue struct
    github.rs           # GitHub API implementation
    local.rs            # Local TOML task file

crates/nous/src/
  auto_session/
    mod.rs              # WorkPrompt struct, session creation
    selection.rs        # issue selection strategy
    synthesis.rs        # LLM-based prompt synthesis
    safety.rs           # AutoSession constraints

crates/daemon/src/maintenance/
  agent_prompt.rs       # AgentPromptTask, deduplication store

crates/aletheia/src/commands/
  prompt_self.rs        # aletheia prompt-self subcommand

crates/taxis/src/config.rs
  # AgentPromptConfig struct
```

---

## 6. References

- Existing session creation: `crates/pylon/src/handlers/sessions.rs`
- Goal alignment embedding logic: `crates/mneme/src/recall.rs`
- Bootstrap prosoche assembly: `crates/nous/src/bootstrap/`
- Daemon task registration: `crates/daemon/src/maintenance/`
- ProcessGuard for child processes: `crates/organon/src/sandbox/`

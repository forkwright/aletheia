# R1472: Post-Merge Lesson Extraction (PR Diff → Knowledge Fact)

**Date:** 2026-03-19
**Author:** Research agent
**Status:** Final
**Closes:** #1472

---

## Executive Summary

When a pull request is merged, a diff encodes a decision: the team chose to change the code in a specific way, often for reasons documented in the PR description, review comments, and commit messages. This institutional knowledge lives in git history and GitHub but is not accessible to the agent at runtime. Post-merge lesson extraction closes this loop: a background task processes merged PRs, extracts durable lessons (architectural decisions, patterns to follow/avoid, rationale), and stores them as knowledge facts in `mneme`.

**Recommendation: Implement.** The extraction is a straightforward LLM task. The primary risk is signal-to-noise: a naive extractor generates low-quality facts from routine dependency bumps. The solution is a quality filter and a structured extraction prompt that focuses on *decisions* and *rationale*, not *what changed*.

---

## 1. Problem Statement

An agent working on aletheia today has no knowledge of *why* the codebase looks the way it does. It can read the code, but:

- It does not know that libc was removed in favor of pure Rust syscalls (and why)
- It does not know that `#[allow]` was replaced by `#[expect]` with reasons (a deliberate convention)
- It does not know that a particular auth middleware was rewritten due to a legal compliance requirement
- It cannot distinguish "we chose this approach" from "this is legacy we haven't cleaned up"

This means the agent gives architecturally inconsistent suggestions, re-proposes rejected approaches, and misses established patterns when writing new code. Each merged PR that introduces a decision is a missed learning opportunity.

---

## 2. Proposed Approach

### 2.1 Trigger: PR Merge Event

Lesson extraction runs on PR merge. Two trigger mechanisms:

**Option A (Polling):** Daemon task runs every N hours, queries GitHub API for PRs merged since last check.

**Option B (Webhook):** GitHub sends `pull_request.closed` + `merged: true` event to a `pylon` endpoint. Immediately enqueues extraction.

Phase 1: polling (simpler, no webhook setup). Phase 2: webhook for near-real-time extraction.

Polling query:
```
GET /repos/{owner}/{repo}/pulls?state=closed&sort=updated&direction=desc&per_page=50
```
Filter: `merged_at > last_extraction_timestamp`.

### 2.2 PR Content Assembly

For each merged PR, assemble an extraction context:

```
# PR #{number}: {title}

## Description
{body}

## Commits
{commit messages, one per line}

## Diff Summary
{files changed, insertions, deletions}

## Key diff hunks (top N by hunk size)
{unified diff, truncated to token budget}

## Review comments (if any)
{reviewer comments, especially those with resolution}
```

Diff is truncated to a token budget (default: 4000 tokens for the diff portion). Large diffs (refactors, migrations) are summarized by file rather than shown in full: "Rewrote 12 files in crates/symbolon/ — removed libc dependency, added rustix".

### 2.3 Extraction Prompt

```
You are extracting durable knowledge from a merged pull request. Your goal is to
identify lessons that will help future contributors make better decisions.

Focus on:
- Architectural decisions: why was approach X chosen over Y?
- Patterns established: what coding conventions were introduced or reinforced?
- Anti-patterns identified: what approaches were explicitly rejected, and why?
- Constraints discovered: what technical or external constraints shaped this change?
- Gotchas: what non-obvious behavior was uncovered during this change?

Do NOT extract:
- Routine dependency version bumps (unless they encode a deliberate choice)
- Formatting/whitespace changes
- Trivially obvious facts ("added a new function")
- Information already in CLAUDE.md or documentation

For each lesson, output:
{
  "lesson": "one-sentence statement of the lesson",
  "rationale": "why this matters for future contributors",
  "confidence": 0.0–1.0,
  "kind": "decision" | "pattern" | "anti-pattern" | "constraint" | "gotcha",
  "scope": "crate name or 'global'",
  "pr_number": {number}
}

Output an empty array [] if this PR contains no lessons worth extracting.
```

### 2.4 Lesson Storage as Knowledge Facts

Extracted lessons are stored as `mneme` knowledge facts with:

```rust
pub struct Lesson {
    pub id: FactId,
    pub lesson: String,           // the extracted lesson statement
    pub rationale: String,
    pub kind: LessonKind,
    pub scope: String,            // crate name or "global"
    pub pr_number: u64,
    pub pr_url: String,
    pub extracted_at: Timestamp,
    pub confidence: f32,
    pub tier: EpistemicTier,      // Inferred (LLM-extracted)
}
```

Stored with:
- `fact_type: "lesson"` tag for filtering
- Embedding generated from `lesson + " " + rationale`
- Entity linking: if the lesson mentions a specific crate, link to a `Crate` entity

### 2.5 Quality Filtering

Before storing, apply a quality gate:

1. **Confidence threshold:** Discard lessons with `confidence < 0.5`
2. **Deduplication:** Embedding similarity check against existing lessons — if a new lesson has cosine similarity > 0.9 with an existing fact, skip (or update confidence if the new PR reinforces it)
3. **Scope validation:** If `scope` is a crate name, verify it exists in the workspace
4. **Length check:** Reject lessons shorter than 20 chars or longer than 500 chars

### 2.6 Recall Integration

Lessons are included in the recall pipeline with a new relevance signal:

- If the current session involves code in `scope = "symbolon"`, boost lessons tagged with that scope
- If the user asks "why does X work this way?", boost lessons of kind `decision` and `constraint`
- Query: "what should I avoid in crates/organon?" → recall `anti-pattern` lessons scoped to `organon`

The recall scorer already supports tag-based filtering. Adding `fact_type = "lesson"` and `kind` as filterable tags is a configuration addition, not a code change.

### 2.7 Review Workflow

Not all extracted lessons should be silently injected into the agent's context. Introduce a review state:

```
pending → approved → active
pending → rejected
```

By default, lessons start as `pending`. The `review-skills` CLI command (already exists for skill review) is extended to also handle lesson review:

```
aletheia review-skills --nous-id ID --action list --fact-type lesson
aletheia review-skills --nous-id ID --action approve --fact-id <id>
```

Auto-approval option: `lesson_extraction.auto_approve = true` in config, with `confidence >= 0.8` required.

### 2.8 Configuration

```toml
[lesson_extraction]
enabled = true
source = "github"
github_repo = "owner/repo"
poll_interval_hours = 6
token_budget_diff = 4000
confidence_threshold = 0.5
auto_approve = false        # require manual review by default
nous_id = "{nous-id}"      # which agent stores lessons
max_prs_per_run = 20
skip_labels = ["chore", "deps", "ci"]  # skip PRs with these labels
```

### 2.9 Provenance

Every extracted lesson retains a link to its source PR (`pr_number`, `pr_url`). The `datalog_query` tool can retrieve lessons with their provenance:

```datalog
?[lesson, pr_url, confidence] :=
    *facts[id, body: lesson, fact_type: "lesson"],
    *fact_meta[id, source_pr_url: pr_url, confidence: confidence],
    confidence > 0.7
```

This allows operators to trace any agent behavior back to the PR that established the relevant lesson.

---

## 3. Alternatives Considered

### 3.1 Commit Message Extraction Only (No Diff)

Extract lessons from commit messages without reading the diff.

**Rejected.** Commit messages are often too terse ("fix bug", "refactor auth"). The diff + PR description is needed to reconstruct *why*. Review comments in particular carry the team's reasoning.

### 3.2 Whole-Repo Retroactive Extraction

Process the entire git log on first run to bootstrap lessons from all historical PRs.

**Deferred.** Expensive (potentially thousands of PRs) and the oldest PRs describe decisions that may have been superseded. Implement as an optional one-time `aletheia extract-history` command, not part of the normal flow.

### 3.3 ADR (Architecture Decision Record) Generation

Instead of storing lessons as facts, generate formal ADR documents in `docs/decisions/`.

**Complementary, not alternative.** ADRs are better for *major* architectural decisions that warrant a document. Lesson extraction handles the long tail of implicit decisions that don't merit an ADR. Both can coexist: high-confidence lessons of kind `decision` and `constraint` could optionally generate an ADR draft.

### 3.4 Inline PR Comments as Source

Use GitHub's PR review comment thread as the primary signal, ignoring the diff.

**Partially adopted.** Review comments are included in the extraction context (§2.2). But they are not sufficient alone — some of the best lessons come from the diff structure itself (what was removed, what replaced it) with no corresponding review comment.

---

## 4. Open Questions

1. **GitHub token permissions:** What scopes are needed? `repo:read` for PR content and diff. Already needed for issue source (R1471). Share the same credential.

2. **Large diffs:** Some PRs have 10,000+ line diffs (e.g., the libc removal PR `8a9237d6`). The 4000-token budget will miss most of the content. Should large PRs use a two-pass approach: first summarize per-file, then extract lessons from summaries?

3. **Lesson supersession:** If PR #200 establishes a lesson and PR #250 contradicts it (reverting the decision), the extraction pipeline needs to detect this. Does the conflict resolution logic in `mneme` handle lesson facts the same as other facts? (It should, since lessons are just facts with a tag.)

4. **Multi-repository:** In a mono-repo or workspace with multiple repos, should lesson extraction run per-repo or aggregate across repos?

5. **Branching:** Should lessons extracted from a PR that was later reverted be automatically retracted? This requires tracking the revert relationship in git.

6. **Noise from tooling PRs:** Automated PRs (Dependabot, Renovate, release bots) add noise. The `skip_labels` config mitigates this, but label discipline is required.

7. **Test-only PRs:** A PR that only adds tests may still contain lessons (e.g., "this edge case must be tested because it failed in production"). Should `kind: "test-pattern"` be a first-class lesson kind?

---

## 5. Implementation Sketch

```
crates/daemon/src/maintenance/
  lesson_extraction.rs     # LessonExtractionTask, polling loop, quality filter

crates/mneme/src/knowledge_store/
  lessons.rs               # Lesson struct, insert/query, deduplication

crates/organon/src/issue_source/
  github.rs                # extend with PR fetching (reuse from R1471)

crates/pylon/src/handlers/
  webhooks.rs              # Phase 2: PR merge webhook receiver

crates/aletheia/src/commands/
  review_skills.rs         # extend to support --fact-type lesson

crates/taxis/src/config.rs
  # LessonExtractionConfig struct
```

---

## 6. References

- git log for recent PRs: `8a9237d6` (libc removal), `726f356e` (code quality), `aafb8eda` (security hardening)
- Existing skill review CLI: `crates/aletheia/src/commands/review_skills.rs`
- Knowledge fact storage: `crates/mneme/src/knowledge_store/`
- Recall pipeline: `crates/mneme/src/recall.rs`
- R1471 issue source abstraction (IssueSource trait, GitHub credential)

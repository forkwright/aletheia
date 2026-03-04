# Claude Code Dispatch Protocol

> Template and guardrails for generating Claude Code task prompts.
> Syn references this document every time work is delegated to Claude Code sessions.
> Last updated: 2026-03-01

---

## Critical: Execution Framing

Claude Code sessions must **execute tasks**, not analyze prompts. Every prompt MUST
open with an imperative action directive. The model defaults to exploration and
commentary unless explicitly told to implement.

**Always start prompts with:**
```
You are an engineer. Implement the following task completely — write code, run
tests, commit, push, and open a PR. Do not summarize the task, audit the prompt,
or explain what you would do. Do the work.
```

**Key framing rules (from Anthropic's own Claude Code system prompts):**
- "Do what has been asked; nothing more, nothing less." (Task tool prompt)
- "Avoid over-engineering. Only make changes that are directly requested." (System prompt)
- "Don't add features, refactor code, or make 'improvements' beyond what was asked." (System prompt)
- "Do not create files unless they're absolutely necessary." (System prompt)
- "Read and understand existing code before suggesting modifications." (System prompt)
- "Your output should be concise and polished. Avoid filler words, repetition, or
  restating what the user has already said. Get to the point quickly." (Tone prompt)

**Avoid these anti-patterns that cause analysis-not-action:**
- ❌ "Here is the task..." (reads as a briefing doc to analyze)
- ❌ Long context sections before the action directive (model starts analyzing)
- ❌ "You should..." / "Consider..." (advisory, not imperative)
- ✅ "Implement..." / "Create..." / "Build..." / "Write..." (direct action verbs)
- ✅ Action directive FIRST, then context, then scope details

---

## Environment

- **Clone location:** `/home/ck/aletheia-ops/aletheia` on Metis
- **Working directory:** Claude Code sessions are opened in this directory
- **Agents share the clone** — Syn, prosoche, and other processes also use `/mnt/ssd/aletheia` (same repo, different mount path). Claude Code must not work on `main` directly.

## Prompt Preamble

Every Claude Code prompt MUST start with the execution directive, then this setup block (adapt the branch name and task):

```
## Directive

You are an engineer. Implement this task completely — write the code, run the
tests, fix any issues, commit, push, and open a PR. Do not analyze or summarize
the prompt. Execute it.

## Setup

You are working in the Aletheia repository at /home/ck/aletheia-ops/aletheia.

Before doing anything:

1. Verify your clone is current:
   git fetch origin && git log --oneline -3 origin/main

2. Create a git worktree for your work (do NOT work on main directly — other agents use this repo):
   git worktree add ../worktrees/<branch-name> -b <branch-name> origin/main
   cd ../worktrees/<branch-name>

3. Do all your work in the worktree. When done:
   - Commit with conventional commit messages (feat:, fix:, refactor:, docs:, chore:)
   - Push the branch: git push origin <branch-name>
   - Create a PR: gh pr create --base main --title "<title>" --body "<body>"
   - Do NOT merge — Syn reviews and merges

4. Clean up the worktree when done:
   cd /home/ck/aletheia-ops/aletheia
   git worktree remove ../worktrees/<branch-name>
```

## Standards References

Every prompt MUST reference the relevant standards. Include this block:

```
## Standards

Read these before writing any code:

- docs/STANDARDS.md — master standards document, all languages
- docs/PROJECT.md — architecture, module boundaries, current state
- .claude/rules/rust.md — Rust-specific rules (snafu errors, #[instrument], newtypes)
- .claude/rules/typescript.md — TypeScript rules (if touching infrastructure/runtime/)
- .claude/rules/svelte.md — Svelte rules (if touching UI)

Key rules you MUST follow:
- Zero clippy warnings: cargo clippy --workspace --exclude aletheia-mneme-engine --all-targets -- -D warnings
- All tests pass: cargo test --workspace
- snafu for errors (not thiserror), anyhow only in CLI
- #[non_exhaustive] on public enums that may grow
- #[expect(lint, reason = "...")] over #[allow(lint)]
- No unwrap() in library code
- Conventional commits: feat(scope):, fix(scope):, etc.
- Module boundaries: check Cargo.toml deps match docs/PROJECT.md architecture
```

## Validation Gate

Every prompt MUST include this verification block at the end:

```
## Before Creating the PR

Run these checks and fix any issues:

1. cargo clippy --workspace --all-targets -- -D warnings  (ZERO warnings)
2. cargo test --workspace  (ALL tests pass)
3. cargo doc --workspace --no-deps  (no doc warnings)
4. git diff --stat  (review your own changes — nothing unexpected)

If any check fails, fix it before creating the PR.

After the PR is created, respond with: PR: <url>
If no PR was created, respond with: PR: none — <reason>

Include an ## Observations section in the PR body if you noticed anything
outside scope — bugs, debt, ideas, missing tests, doc gaps. See the
Observation Capture section of docs/CLAUDE_CODE.md for format and labels.
```

## Branch Naming

Use descriptive prefixes:
- `feat/<feature-name>` — new functionality
- `fix/<bug-description>` — bug fixes
- `refactor/<scope>` — restructuring without behavior change
- `docs/<topic>` — documentation only
- `chore/<topic>` — deps, CI, tooling

## Prompt Structure Template

```markdown
# Task: <clear one-line description>

## Directive

You are an engineer. Implement this task completely — write the code, run the
tests, fix any issues, commit, push, and open a PR. Do not analyze or summarize
the prompt. Execute it.

## Setup
<preamble block from above — with specific branch name>

## Standards
<standards block from above — prune to relevant languages>

## Context
<what exists, what's already been decided, relevant commits/issues>
<keep this SHORT — just enough to orient. Long context goes AFTER the scope.>

## Task
<exactly what to build/fix — imperative voice, specific files and behaviors>
<numbered steps if order matters>
<what is explicitly OUT of scope>

## Acceptance Criteria
<numbered list of concrete, verifiable outcomes>
<each criterion should be testable — "X file exists", "Y test passes", "Z compiles">

## Before Creating the PR
<validation gate block from above>

## Observations
<anything noticed outside scope — bugs, debt, ideas, missing tests, doc gaps>
<use the labels from the Observation Capture section>
<if nothing observed, omit this section entirely>
```

**Note the section is called "Task", not "Scope".** "Scope" reads as a briefing
document to analyze. "Task" reads as work to execute.

## Prompt Quality Checklist

Before dispatching a prompt, verify:

1. **Does it open with an action directive?** First sentence must be imperative.
2. **Is the task section in imperative voice?** "Create X" not "X should be created."
3. **Is context minimal?** Only what's needed to orient. Don't front-load analysis.
4. **Are acceptance criteria testable?** Each one should be pass/fail verifiable.
5. **Is there a clear deliverable?** PR URL, file path, or explicit output format.
6. **For docs-only tasks:** Still use imperative framing. "Write a research document
   at path X that covers Y" not "Produce an analysis of Y."
7. **Does it include the observations reminder?** The validation gate should mention observations.

## Common Mistakes to Prevent

Include these warnings when relevant:

- **Rust feature gates:** mneme uses `default = ["sqlite"]`. If adding code that references `rusqlite`, it must be behind `#[cfg(feature = "sqlite")]`.
- **Vendor directory:** Do not modify `vendor/` unless the task specifically requires patching vendored deps.
- **Workspace lints:** New crates must include `[lints] workspace = true` in Cargo.toml.
- **serde_yml is banned:** It has unsound `unsafe`. Use `serde_yaml` (different crate) if YAML is needed.
- **Test isolation:** Integration tests go in `crates/integration-tests/`. Unit tests stay in their crate's `#[cfg(test)] mod tests`.
- **Public API surface:** `pub(crate)` by default. Only `pub` if it's part of the cross-crate API.
- **Don't run full vitest suite** for TypeScript — it causes timeouts. Run targeted tests.

## Parallel Session Coordination

When dispatching multiple Claude Code sessions simultaneously:
- Each session gets a UNIQUE branch name
- Branches should not touch overlapping files (merge conflicts)
- If overlap is unavoidable, sequence them (session 2 waits for session 1's PR to merge)
- Note in each prompt: "Other sessions may be running. Do not modify: <list of files owned by other sessions>"

## Post-Merge Checklist (for Syn)

After Claude Code creates PRs:
1. `git fetch origin && gh pr list`
2. Review each PR diff: `gh pr diff <number>`
3. Run clippy + tests on the branch (checkout or worktree)
4. Fix any issues, push to the branch
5. **Triage observations** — check the PR body for an Observations section:
   - **Bugs:** Create a GitHub issue (consolidate related bugs into one issue)
   - **Debt/Ideas:** Add to BACKLOG.md or create an issue if substantial
   - **Missing tests:** Bundle into a test-coverage issue or address in next related PR
   - **Doc gaps:** Fix inline or add to a docs chore issue
6. Squash merge: `gh pr merge <number> --squash --subject "<conventional commit>" --delete-branch`
7. Pull main: `git pull --rebase`
8. Verify clean state: `git branch -a`, prune stale remotes
9. Update working state notes

## Observation Capture

While working, you will notice things outside your task scope — bugs in adjacent code,
API inconsistencies, missing tests, improvement ideas, technical debt. **Don't fix them.
Don't ignore them. Capture them.**

### In the PR Description

Add an `## Observations` section at the bottom of every PR body. Include anything you
noticed that's outside scope but worth recording:

```markdown
## Observations

While working in `crates/mneme/`, noticed:

- **Bug:** `recall.rs:142` — graph score aggregation sums across relations but doesn't
  deduplicate entity IDs first. Multiple edges to the same node inflate its score.
- **Debt:** `knowledge_store.rs` — BM25 parameters (k1=1.2, b=0.75) are hardcoded.
  Should be in KnowledgeConfig if multi-language support is planned.
- **Idea:** The `HybridResult` struct could carry provenance metadata (which signals
  contributed to the final score) for debugging retrieval quality.
- **Missing test:** No test covers the case where `seed_entities` is empty but BM25
  and LSH still have results — RRF should still execute with 2 signals.
```

### Categories

Use these labels consistently:

| Label | What | Example |
|-------|------|---------|
| **Bug** | Something broken or wrong | Off-by-one, logic error, race condition |
| **Debt** | Works but fragile or hardcoded | Magic numbers, missing error handling |
| **Idea** | Enhancement or new capability | Better API shape, performance optimization |
| **Missing test** | Untested path or edge case | Error paths, boundary conditions |
| **Doc gap** | Missing or stale documentation | Outdated comments, undocumented behavior |

### Scope Discipline

The observations section is a **capture mechanism**, not a license to expand scope:

- **Do NOT fix** observed issues unless they block your task
- **Do NOT investigate** deeply — note what you saw, where, and move on
- **Do NOT create GitHub issues** — Syn triages observations and creates consolidated issues
- **Do** note the file and line number when possible
- **Do** distinguish severity — a data corruption bug is different from a style nit

### Why This Matters

Sub-agent sessions are ephemeral. When the session ends, everything the agent noticed
but didn't write down is lost. The PR description is the durable artifact. Observations
captured here get triaged into GitHub issues, backlog items, or dismissed — but at least
they're not lost.

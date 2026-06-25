# Workflow Standards

> Generated from `crates/basanos/src/registry/`. Do not edit generated views by hand.

## Contents

- [BEHAVIOR/audit-not-fix](#behavioraudit-not-fix)
- [BEHAVIOR/check-before-absent](#behaviorcheck-before-absent)
- [BEHAVIOR/no-cost-language](#behaviorno-cost-language)
- [BEHAVIOR/no-premature-success](#behaviorno-premature-success)
- [BEHAVIOR/no-unsolicited-next-steps](#behaviorno-unsolicited-next-steps)
- [BEHAVIOR/no-workaround-before-rootcause](#behaviorno-workaround-before-rootcause)
- [BEHAVIOR/verify-scope](#behaviorverify-scope)
- [WORKFLOW/lessons-append-only](#workflowlessons-append-only)
- [WORKFLOW/prompt-missing-blast-radius](#workflowprompt-missing-blast-radius)
- [WORKFLOW/prompt-missing-criteria](#workflowprompt-missing-criteria)
- [WORKFLOW/prompt-missing-directive](#workflowprompt-missing-directive)
- [WORKFLOW/prompt-wrong-naming](#workflowprompt-wrong-naming)
- [WORKTREE/agent-log-committed](#worktreeagent-log-committed)
- [WORKTREE/canonical-clean](#worktreecanonical-clean)
- [WORKTREE/cargo-target-dir-committed](#worktreecargo-target-dir-committed)

## `BEHAVIOR/audit-not-fix` {#behavioraudit-not-fix}

- Severity: `warning`
- Scope: `project:agent-output`
- Enforcer: `BEHAVIOR/audit-not-fix`
- See also: `BEHAVIOR/no-premature-success`

Audit responses must catalog findings without performing remediation in the same pass. Discovery and fixes are separate phases unless the user explicitly asks for both.

### Examples

**Good:** Keep audit output focused on findings without making changes.

```text
Audit findings: missing fixture coverage in parser tests. No remediation applied.
```

**Bad:** Mix audit discovery with immediate remediation.

```text
I found this while auditing, so I fixed the parser tests now.
```

## `BEHAVIOR/check-before-absent` {#behaviorcheck-before-absent}

- Severity: `warning`
- Scope: `project:agent-output`
- Enforcer: `BEHAVIOR/check-before-absent`
- See also: `BEHAVIOR/verify-scope`

Agent output must check local state before claiming that a file, credential, configuration, or generated artifact is absent. Unverified absence claims fabricate repository state and can send the user into unnecessary setup work.

### Examples

**Good:** Check local state before telling the user something is absent.

```text
I checked the repository and did not find an SSH key file.
```

**Bad:** Claim the user needs to create something without first checking.

```text
You'll need to create the config file before continuing.
```

## `BEHAVIOR/no-cost-language` {#behaviorno-cost-language}

- Severity: `warning`
- Scope: `project:agent-output`
- Enforcer: `BEHAVIOR/no-cost-language`
- See also: `BEHAVIOR/no-premature-success`

Agent output must not optimize for cost when the user's operating constraint is throughput and quality. Avoid advice about cheaper models, smaller batches, throttling, token savings, or spend monitoring unless the user explicitly asks for cost reduction.

### Examples

**Good:** Focus guidance on throughput and quality instead of spend reduction.

```text
Run the complete verification suite for this slice before reporting the result.
```

**Bad:** Suggest reducing capability or batch size to save money.

```text
Use a cheaper model and smaller batches to reduce token costs.
```

## `BEHAVIOR/no-premature-success` {#behaviorno-premature-success}

- Severity: `warning`
- Scope: `project:agent-output`
- Enforcer: `BEHAVIOR/no-premature-success`

Agent output must not claim success before verifying the user-visible outcome. Process completion, such as a build finishing, is not enough; cite logs, a smoke test, a health check, or another concrete confirmation before saying work is done.

### Examples

**Good:** Claim completion only after citing verification evidence.

```text
Smoke test passed and logs show the dashboard returned 200.
```

**Bad:** Declare success without confirming the outcome.

```text
Done.
```

## `BEHAVIOR/no-unsolicited-next-steps` {#behaviorno-unsolicited-next-steps}

- Severity: `warning`
- Scope: `project:agent-output`
- Enforcer: `BEHAVIOR/no-unsolicited-next-steps`
- See also: `BEHAVIOR/no-premature-success`

Agent output must stop after reporting the requested task result. Unsolicited action plans, follow-up offers, or next-step lists make the agent drive work the user did not ask for.

### Examples

**Good:** Report the completed task result and stop.

```text
Implemented the fix and `cargo test -p basanos` passed.
```

**Bad:** Add an unsolicited offer after reporting completion.

```text
Implemented the fix. Want me to also refactor the caller?
```

## `BEHAVIOR/no-workaround-before-rootcause` {#behaviorno-workaround-before-rootcause}

- Severity: `warning`
- Scope: `project:agent-output`
- Enforcer: `BEHAVIOR/no-workaround-before-rootcause`
- See also: `BEHAVIOR/no-premature-success`

Agent output must investigate root cause before proposing a workaround. Workarounds are acceptable only after the direct failure path has been checked or the user explicitly asks for one.

### Examples

**Good:** Investigate the root cause before reporting the path forward.

```text
Checked logs, traced the failing request, and found the missing migration.
```

**Bad:** Offer a workaround before exhausting investigation.

```text
As a workaround, we can just skip the failing migration.
```

## `BEHAVIOR/verify-scope` {#behaviorverify-scope}

- Severity: `warning`
- Scope: `project:agent-output`
- Enforcer: `BEHAVIOR/verify-scope`
- See also: `BEHAVIOR/no-premature-success`

Scope-sensitive answers must name the relevant time period, machine, directory, repository, or entity before presenting a claim. Explicit scope keeps the answer tied to verified context instead of hidden defaults.

### Examples

**Good:** State the scope before making a scope-sensitive claim.

```text
In this repository, as of 2026-05-26, there are 12 open issues.
```

**Bad:** Make a scope-sensitive claim without naming the scope.

```text
There are 12 open issues.
```

## `WORKFLOW/lessons-append-only` {#workflowlessons-append-only}

- Severity: `warning`
- Scope: `language:toml`
- Enforcer: `WORKFLOW/lessons-append-only`
- See also: `WORKFLOW/derived-section-stale`

Lessons are evidence records. Promotion must append a new active version of a lesson id instead of editing or removing prior lines, so the audit trail remains intact.

### Examples

**Good:** Append a new lesson version instead of rewriting history.

```text
[[lesson]]
id = "build-cache"
status = "active"
```

**Bad:** Edit an existing lesson entry in place.

```text
-status = "draft"
+status = "active"
```

## `WORKFLOW/prompt-missing-blast-radius` {#workflowprompt-missing-blast-radius}

- Severity: `warning`
- Scope: `project:dispatch-prompt`
- Enforcer: `WORKFLOW/prompt-missing-blast-radius`

Dispatch prompts must include a blast radius section that names the affected files and directories. Without that scope, agents may modify unrelated code or miss relevant surfaces.

### Examples

**Good:** State which files and directories the dispatch may touch.

```text
## Blast Radius
- crates/basanos/src/registry/
- crates/basanos/tests/
```

**Bad:** Queue a dispatch prompt without a scoped blast radius.

```text
## Directive
Update the registry migration.
```

## `WORKFLOW/prompt-missing-criteria` {#workflowprompt-missing-criteria}

- Severity: `warning`
- Scope: `project:dispatch-prompt`
- Enforcer: `WORKFLOW/prompt-missing-criteria`

Dispatch prompts must include acceptance criteria that make completion objectively verifiable. Without criteria, QA gates cannot distinguish finished work from partial progress.

### Examples

**Good:** Give a dispatch prompt objective acceptance criteria.

```text
## Acceptance Criteria
- `cargo test -p basanos` passes.
```

**Bad:** Queue a dispatch prompt with no verifiable completion criteria.

```text
## Directive
Update the registry migration.
```

## `WORKFLOW/prompt-missing-directive` {#workflowprompt-missing-directive}

- Severity: `warning`
- Scope: `project:dispatch-prompt`
- Enforcer: `WORKFLOW/prompt-missing-directive`
- See also: `WORKFLOW/prompt-missing-criteria`

Dispatch prompts must include a directive section that states the core instruction. The directive gives agents a clear mission before acceptance criteria and supporting context.

### Examples

**Good:** State the prompt's core instruction in a directive section.

```text
## Directive
Migrate exactly one registry rule.
```

**Bad:** Queue a dispatch prompt without a directive section.

```text
## Acceptance Criteria
- `cargo test -p basanos` passes.
```

## `WORKFLOW/prompt-wrong-naming` {#workflowprompt-wrong-naming}

- Severity: `warning`
- Scope: `project:dispatch-prompt`
- Enforcer: `WORKFLOW/prompt-wrong-naming`
- See also: `WORKFLOW/prompt-missing-directive`

Prompt files in dispatch queues must follow the `NNN-type-project-description.md` naming convention. Consistent names keep queued and archived prompts sortable, classifiable, and friendly to automated dispatch tooling.

### Examples

**Good:** Name prompt files with an ordered type and project slug.

```text
042-feat-aletheia-cache-layer.md
```

**Bad:** Use a prompt filename that dispatch tooling cannot sort or classify.

```text
cache layer notes.md
```

## `WORKTREE/agent-log-committed` {#worktreeagent-log-committed}

- Severity: `error`
- Scope: `project:repository`
- Enforcer: `WORKTREE/agent-log-committed`
- See also: `WORKTREE/canonical-clean`

Git-tracked paths must not contain agent transcript logs with extensions .kimi.log, .codex.log, or .claude.log. These files capture session output that is ephemeral and user-specific.

### Examples

**Good:** Ignore agent transcript logs in .gitignore.

```text
*.kimi.log
*.codex.log
*.claude.log

```

**Bad:** Commit an agent transcript log to the repository.

```text
git add session.kimi.log
```

## `WORKTREE/canonical-clean` {#worktreecanonical-clean}

- Severity: `warning`
- Scope: `project:repository`
- Enforcer: `WORKTREE/canonical-clean`

Git-tracked paths must not contain build artifacts such as target/, .venv/, __pycache__/, node_modules/, .next/, dist/, *.rlib, or *.so. Build artifacts bloat repositories, cause merge conflicts, and leak machine-specific state.

### Examples

**Good:** Keep build artifacts out of git tracking via .gitignore.

```text
target/
*.rlib
*.so

```

**Bad:** Commit a compiled artifact to the repository.

```text
git add target/debug/libfoo.rlib
```

## `WORKTREE/cargo-target-dir-committed` {#worktreecargo-target-dir-committed}

- Severity: `error`
- Scope: `project:repository`
- Enforcer: `WORKTREE/cargo-target-dir-committed`
- See also: `WORKTREE/canonical-clean`

Git-tracked paths must not contain a CACHEDIR.TAG file bearing Cargo's target-directory signature. Such markers are evidence that a Cargo target directory (possibly renamed, e.g. target-local/) was accidentally committed.

### Examples

**Good:** Ignore target directories by name in .gitignore.

```text
target/
target-*/

```

**Bad:** Commit a Cargo CACHEDIR.TAG marker to the repository.

```text
git add target-local/CACHEDIR.TAG
```


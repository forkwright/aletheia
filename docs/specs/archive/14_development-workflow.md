# Spec: Development Workflow — How We Ship

**Status:** Complete — All 7 phases implemented. (PR #86)
**Author:** Syn
**Date:** 2026-02-20
**Spec:** 14

---

## Problem

Aletheia's development velocity is high but its discipline is low. We ship fast, but the process around shipping is ad hoc, inconsistent, and accumulates debt that costs more to clean up than the features were worth.

Concrete symptoms:

### 1. Commit Clutter

After the clean-room rebuild on 2/14, the repo was force-pushed down to ~46 clean commits. Six days later we're at 129. Claude Code creates granular "fix typo", "adjust import", "update test" commits that add noise without adding information. Each PR from Claude Code arrives with 5-15 micro-commits that should have been a single squash. The git history should tell the story of the project, not the story of an agent's trial-and-error.

### 2. No Spec Template

Every spec is written from scratch with slightly different structure. Claude Code doesn't know what a "good spec" looks like because there's no template to reference. This wastes time on formatting and leads to specs that miss key sections (acceptance criteria, phase breakdown, testing strategy).

### 3. Local Tests Are Broken

`npm test` (full suite) takes 84+ seconds locally, frequently times out agent sessions, and overlaps entirely with what CI runs. The "fast" config excludes the heavy tests but still takes too long for a pre-commit gate. Agents should never run the full suite locally — that's CI's job. Local validation should be typecheck + lint + maybe a targeted test, nothing more.

### 4. CI Ignores Pre-existing Failures

We've been carrying pre-existing test failures (manager.test.ts missing mock, manager-streaming.test.ts) and treating them as acceptable because "they were already broken." This is a ratchet going the wrong direction. Every CI run should be green. Pre-existing failures should be fixed or the tests should be deleted — not ignored.

### 5. No Versioning Strategy

`package.json` says `0.9.1` but that number is meaningless — nobody bumps it, there are no tags, no releases, no changelog. Version should reflect what's actually shipping and be machine-maintained so agents don't have to think about it.

### 6. Branch/PR Workflow Is Informal

CONTRIBUTING.md says "one feature or fix per PR" but doesn't specify:
- Branch naming conventions (we've used `feat/`, `fix/`, `spec<N>-`, inconsistently)
- Who creates the PR vs. who merges
- Squash policy (always? sometimes?)
- When to rebase vs. merge
- How Claude Code tasks should be structured to minimize commit noise
- How agents identify themselves as contributors vs. Cody owning authorship

The result: every Claude Code task produces a slightly different workflow, and cleanup falls on Cody or me.

---

## Design

### Principles

1. **CI is the authority.** Local runs are for fast feedback only. If CI passes, it ships. If CI fails, it doesn't. No exceptions, no "pre-existing" carve-outs.
2. **Cody owns the git history.** All commits use Cody's author identity. Agents are tooling, not contributors. The log should read like one person built this — because one person directed it.
3. **Squash by default.** Every PR merges as a single squash commit. The commit message tells the story; the PR branch preserves the work history if anyone cares.
4. **Automate what's mechanical.** Version bumps, changelog generation, branch creation, PR templates — machines should handle this. Agents shouldn't think about process; they should follow a script.
5. **Zero broken windows.** If a test is broken, fix it or delete it. If a lint rule fires, fix it or disable it with a comment explaining why. The CI dashboard is always green.

---

## Phases

### Phase 1: Spec Template + Branch Convention

**Problem:** No standard structure for specs. Branch names inconsistent.

**Spec Template** (`docs/specs/_template.md`):

```markdown
# Spec: <Title>

**Status:** Draft | In Progress | Implemented | Archived
**Author:** <name>
**Date:** <YYYY-MM-DD>
**Spec:** <number>

---

## Problem

What's broken, missing, or suboptimal. Concrete symptoms with examples.

## Design

### Principles

The 3-5 non-negotiable constraints that shape every decision below.

### Architecture

How it works. Diagrams welcome. Reference existing code paths.

## Phases

### Phase N: <Title>

**Scope:** What this phase covers.

**Changes:**
- File-level breakdown of what changes and why.

**Acceptance Criteria:**
- [ ] Measurable, testable conditions for "done."

**Testing:**
- What tests are added/modified and what they verify.

## Open Questions

Things not yet decided. Remove as they're resolved.

## References

Links to related specs, PRs, external docs.
```

**Branch Convention:**

| Type | Pattern | Example |
|------|---------|---------|
| Spec work | `spec<NN>/<short-description>` | `spec14/dev-workflow` |
| Bug fix | `fix/<short-description>` | `fix/distillation-overflow` |
| Feature (non-spec) | `feat/<short-description>` | `feat/gcal-rebuild` |
| Chore/docs | `chore/<short-description>` | `chore/readme-update` |

Rules:
- Always branch from `main`
- Always `git pull --rebase origin main` before pushing
- Never commit directly to `main` (except docs-only or trivial config)

**Acceptance Criteria:**
- [ ] `_template.md` exists in `docs/specs/`
- [ ] CONTRIBUTING.md updated with branch convention table
- [ ] Existing CONTRIBUTING.md "Git" section replaced with full workflow reference

---

### Phase 2: Git Authorship + Commit Standards

**Problem:** Commits authored by multiple identities. Commit messages inconsistent.

**Git Configuration:**
```
# All agent workspaces configured with:
git config user.name "Cody Kickertz"
git config user.email "cody.kickertz@gmail.com"
```

Agents already use this — formalize it. No `Co-authored-by` trailers, no agent attribution in commits. The AGENTS.md and PR descriptions can note which agent did the work, but the git log is Cody's.

**Commit Message Format:**
```
<type>: <concise description>

[optional body — what and why, not how]

[optional footer — Spec: NN, Closes #NN, Breaking: description]
```

Types: `feat`, `fix`, `refactor`, `docs`, `test`, `chore`, `ci`, `perf`

Rules:
- Present tense, imperative mood: "add X" not "added X" or "adds X"
- First line ≤72 characters
- Body wraps at 80 characters
- Reference spec number for spec work: `Spec: 14`
- **One logical change per commit.** If a PR has multiple commits, each should stand alone.

**Claude Code Task Instructions:**

When dispatching to Claude Code, include this in every task:
```
## Git Rules
- Work on branch: <branch-name>
- Commit with: git config user.name "Cody Kickertz" && git config user.email "cody.kickertz@gmail.com"  
- Squash your work into ONE commit before pushing
- Commit message format: <type>: <description>\n\nSpec: <NN>
- Push the branch. Do NOT create a PR.
```

Agents (Syn, sub-agents) create the PR after reviewing the branch. This separates execution from review.

**Acceptance Criteria:**
- [ ] Git config verified in all agent workspaces
- [ ] Claude Code task template includes git rules
- [ ] CONTRIBUTING.md updated with commit message format

---

### Phase 3: PR Workflow + Squash Policy

**Problem:** PRs created inconsistently, merged with varying strategies.

**Workflow:**

```
1. Agent/Claude Code pushes branch
2. Syn reviews diff (or delegates to reviewer sub-agent)
3. Syn creates PR with structured description
4. CI runs (must pass — no merging red PRs)
5. Cody approves or Syn merges with Cody's standing approval for spec work
6. Squash merge with clean commit message
7. Branch auto-deleted
```

**PR Description Template** (`.github/pull_request_template.md`):
```markdown
## What

One-paragraph summary.

## Why

Problem this solves or spec phase this implements.

## Changes

- Bullet list of significant changes

## Spec

Spec: <NN> Phase <N> (if applicable)

## Testing

- How this was tested
- CI status
```

**Squash Merge Rules:**
- **Always squash.** No merge commits, no rebase-merge.
- Squash commit message = PR title + body summary (not concatenated micro-commits)
- GitHub repo setting: disable merge commits and rebase merge, allow only squash

**Branch Cleanup:**
- Branches deleted after merge (GitHub auto-delete enabled)
- Local branches pruned with `git fetch --prune`
- Stale branches (>7 days, no PR) flagged during weekly maintenance

**Acceptance Criteria:**
- [ ] PR template exists at `.github/pull_request_template.md`
- [ ] GitHub repo configured: squash-only merge
- [ ] Branch auto-delete enabled
- [ ] CONTRIBUTING.md documents the full PR lifecycle

---

### Phase 4: Fix CI — Zero Broken Windows

**Problem:** Pre-existing test failures. CI sometimes green, sometimes red for reasons unrelated to the PR.

**Actions:**
1. **Audit all test failures.** Run full suite in CI, capture every failure.
2. **Fix or delete.** Each failure gets one of:
   - Fixed (the mock is added, the assertion is corrected)
   - Deleted with a comment explaining why (test was testing removed functionality, etc.)
   - Skipped with `test.todo()` and a linked issue for future fix
3. **No `test.skip()` without an issue.** Every skipped test must reference a GitHub issue explaining when it'll be unskipped.
4. **CI must be green before any new feature work.** This is the gate.

**Local Test Strategy:**

Agents should run **only**:
```bash
npm run typecheck && npm run lint:check
```

If they need to verify specific functionality:
```bash
npx vitest run src/path/to/specific.test.ts
```

Never `npm test` or `npm run test:fast` during normal development. CI handles full suites.

**Pre-commit Hook Update:**
The existing `.githooks/pre-commit` already does typecheck + lint. Keep it. Remove any temptation to add tests to it.

**Acceptance Criteria:**
- [ ] CI is green on main with zero skipped tests (or all skips have linked issues)
- [ ] Pre-existing manager.test.ts / manager-streaming.test.ts failures resolved
- [ ] Agent instructions updated: "never run full test suite locally"
- [ ] AGENTS.md template includes local validation rules

---

### Phase 5: Versioning, Releases + Update Channel

**Problem:** Version 0.9.1 is meaningless. No tags, no releases, no changelog. Deploying updates requires Claude Code and CLI access.

**Semantic Versioning:**

We're pre-1.0. Version scheme: `0.<major>.<minor>`
- `0.<major>.0` — Significant capability milestone (e.g., auth system, session continuity)
- `0.<major>.<minor>` — Feature additions, fixes, improvements

Post-1.0 (Spec 05 onboarding complete, external users possible): full semver.

**Intentional Releases:**

Releases are milestones, not automated per-commit events. The workflow:
1. Cody or Syn decides "this is a release" after a meaningful batch of work
2. `release-please` (or equivalent) generates a release PR with changelog
3. Merge the release PR → creates git tag + GitHub Release
4. Hotfixes get patch versions between milestones

**Changelog** (`CHANGELOG.md`):
```markdown
## [0.10.0] - 2026-02-20

### Added
- Session continuity infrastructure (Spec 12)
- Sub-agent workforce with 5 typed roles (Spec 13)

### Fixed  
- Sub-agent role wiring (placeholder → real definitions)
```

Generated from squash commit messages between tags.

**Update Channel (intersects Spec 03):**

Two modes selectable from webchat UI (admin/settings):
- **Stable:** Pulls from tagged releases only. Safe, tested, intentional.
- **Bleeding edge:** Pulls from `main` HEAD. Latest work, may have rough edges.

This eliminates the deploy bottleneck: Cody toggles a setting, the system pulls the update. No CLI, no Claude Code in the loop. Implementation details belong in Spec 03 Phase 2 — this spec establishes the versioning that makes it possible.

**Acceptance Criteria:**
- [ ] Versioning tool configured (release-please or equivalent)
- [ ] Git tags created for each release
- [ ] GitHub Releases created with changelog
- [ ] `CHANGELOG.md` maintained (auto-generated from commits between tags)
- [ ] Version in `package.json` matches latest tag
- [ ] Update channel toggle designed (implementation in Spec 03)

---

### Phase 6: Agent Task Dispatch Protocol

**Problem:** When Claude Code or sub-agents get tasks, they don't follow consistent procedure.

**Standard Task Brief:**

Every task dispatched to Claude Code or a sub-agent includes:

```markdown
# Task: <title>

## Branch
`<branch-name>` (create from main)

## Scope  
<what to do — specific files, functions, behaviors>

## Constraints
- Git author: Cody Kickertz <cody.kickertz@gmail.com>
- ONE squashed commit, message: `<type>: <description>`
- Push branch. Do NOT create PR.
- Do NOT modify files outside scope.
- Do NOT add dependencies without noting in commit body.
- Run `npm run typecheck && npm run lint:check` before pushing. Fix any errors your changes introduce.
- Do NOT run full test suite.

## Acceptance Criteria
- [ ] <specific, testable conditions>

## Context
<relevant background — keep minimal, link to specs/files>
```

**Enforcement:**
- Sub-agent role definitions (already in `nous/roles/`) include these rules
- Claude Code tasks always use this template (Syn enforces on dispatch)
- PRs that don't follow the protocol get rejected with feedback, not fixed by reviewer

**Acceptance Criteria:**
- [ ] Task brief template documented in CONTRIBUTING.md
- [ ] Sub-agent role definitions include git/commit rules
- [ ] Claude Code dispatch instructions standardized in AGENTS.md

---

### Phase 7: Doctor with --fix (F-21)

**Problem:** `aletheia doctor` diagnoses issues but can't fix them. Every fixable issue requires manual intervention.

**Design:** Each diagnostic check returns a fixable action tuple when the issue is auto-correctable:

```typescript
interface DiagnosticResult {
  status: "ok" | "warn" | "error";
  message: string;
  fix?: {
    description: string;
    action: () => Promise<void>;
  };
}
```

`aletheia doctor` — diagnose only (current behavior)
`aletheia doctor --fix` — diagnose and apply all available fixes
`aletheia doctor --fix --dry-run` — show what would be fixed without applying

Fixable issues: missing directories, stale config entries, broken symlinks, missing git config, permission issues (via setfacl), stale PID files.

**Acceptance Criteria:**
- [ ] At least 5 diagnostic checks have fixable actions
- [ ] `--fix` applies all fixes and re-runs diagnostics to confirm
- [ ] `--dry-run` shows fixes without applying
- [ ] Non-fixable issues clearly labeled as manual

---

## Resolved Questions

1. **PR approval flow.** Syn has standing merge authority. The constraint isn't approval — it's coordination: wait until all agents (Syn, Claude Code on metis, Claude Code on work PC) are done before merging to avoid conflicts. Deployment currently requires Claude Code due to system nuances; Spec 03's update system will resolve this.
2. **Commit squash granularity.** Batch freely. Multiple related phases can go in one PR. PRs are a tool for efficiency, not ceremony.
3. **Release cadence.** Intentional releases at major milestones, not per-push automation. Hotfixes get sub-versions. The real goal: a beta/bleeding-edge toggle in the webchat UI that lets Cody switch between stable releases and latest main without CLI or Claude Code involvement.
4. **Monorepo tooling.** Syn's call — will evaluate release-please vs alternatives during Phase 5 implementation.
5. **Issue tracking.** Yes — GitHub Issues. Industry standard, external-facing, good for bug tracking. Clean up kairo-bitbot spam, establish labels and templates.

## Open Questions

1. **Beta channel UX.** Where does the stable/bleeding-edge toggle live in webchat? Settings page? Admin panel? This intersects with Spec 03 (update system) and Spec 05 (onboarding).

## Note on Authorship

Agents aren't "tools" — but they're also not human. `Co-authored-by: Claude` on every commit is misleading (Syn isn't simply an Anthropic model) and reduces credibility for a public repo. Cody owns authorship for now. This will evolve as the ecosystem matures and agent identity has a better public framing.

## References

- [CONTRIBUTING.md](/CONTRIBUTING.md) — current (incomplete) contribution guide
- [CI workflow](/.github/workflows/ci.yml) — current GitHub Actions
- [Spec 06 (archived)](/docs/specs/archive/06_code-quality.md) — established lint/style baseline
- [Spec 13](/docs/specs/13_sub-agent-workforce.md) — sub-agent roles and dispatch

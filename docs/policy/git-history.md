# Git History Cleanup

**Status:** Plan — execute before forking to <your-fork>

## Problem

350+ commits accumulated across multiple phases, GSD agent runs, Dependabot updates, and direct-to-main pushes. History reads like a work log, not a project narrative. Must be cleaned before the fork — all SHAs change, so this is a one-time destructive operation.

## Squash Strategy

Group by milestone/capability, not by date or agent.

### Target Structure

```
feat: initialize aletheia runtime — Hono gateway, session store, tool registry
feat: turn pipeline — 6-stage composable processing, error boundaries, distillation
feat: memory system — Mem0 sidecar, distillation persistence, knowledge graph
feat: agent architecture — sub-agents, model routing, skill learning
feat: security — symbolon auth, token management, path validation
feat: webchat UI — Svelte 5, markdown rendering, thinking panels
feat: TUI — Ratatui terminal client, sessions, system overlay
feat: Signal integration — semeion daemon, command registry, contact pairing
feat: Slack integration — agora channel abstraction, provider pattern
feat: planning system — dianoia FSM, file-backed state, discussion flow
feat: IDE integration — CodeMirror editor, file tree, agent edit notifications
feat: context engineering — cache-group bootstrap, interaction signals
chore: CI/CD — GitHub Actions, branch protection, commitlint
chore: documentation — specs, architecture docs, contributing guide
```

### Commit Standards Post-Cleanup

- **Format:** `<type>(<scope>): <description>` — conventional commits
- **Types:** feat, fix, refactor, chore, docs, test, perf, ci
- **Scope:** module name (pylon, dianoia, semeion, agora, mneme, organon, etc.)
- **Body:** What and why, not how. Reference issue/spec numbers.
- **Each commit compiles and passes tests** — no broken intermediate states

## Execution

1. Create a backup branch: `git branch backup/pre-squash`
2. Interactive rebase from root: `git rebase -i --root`
3. Group commits by milestone, squash within groups
4. Write professional commit messages per group
5. Force push to main (coordinated — all operators must re-clone)
6. Tag the result: `v0.1.0` (first clean release)

## Acceptance Criteria

- [ ] `git log --oneline` reads as a coherent project narrative
- [ ] Each commit compiles independently
- [ ] No Dependabot noise, no "fix typo" orphans
- [ ] Backup branch preserved for archaeology
- [ ] All operators notified and re-synced

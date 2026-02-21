# Spec: Extensibility — Hooks, Commands & Plugins

**Status:** Phases 1-2 done (PRs #98, #107). Phases 3-6 remaining.
**Author:** Syn
**Date:** 2026-02-21
**Source:** Gap Analysis F-2, F-12, F-24, F-25, F-27, F-37

---

## Problem

Aletheia's behavior is configured through YAML and source code. There's no way for a user to inject custom logic at lifecycle points without modifying the runtime, no way to define reusable commands without writing TypeScript, and no standard for third-party extensions.

---

## Design

### Phase 1: Hook System (F-2)

Declarative hook definitions that fire shell commands at lifecycle points.

**Definition format** — YAML files in `shared/hooks/`:

```yaml
# shared/hooks/backup-on-distill.yaml
name: backup-on-distill
event: distill:after
handler:
  type: shell
  command: /home/syn/scripts/backup-sessions.sh
  args: ["{{sessionId}}", "{{nousId}}"]
  timeout: 30s
  failAction: warn  # warn | block | silent
```

**Supported events** — maps to existing event bus: `turn:before`, `turn:after`, `tool:called`, `tool:failed`, `distill:before`, `distill:after`, `session:created`, `session:archived`.

**Handler protocol** — Shell handlers receive event data as JSON on stdin. Exit codes: 0 = success, 1 = warning, 2+ = error. Same protocol as Claude Code's hook system for ecosystem compatibility.

**Template variables** — `{{sessionId}}`, `{{nousId}}`, `{{toolName}}`, `{{timestamp}}`, etc., substituted from event data.

**Registry** — `koina/hooks.ts`. Loaded at startup from `shared/hooks/*.yaml`. Event bus registers a listener for each. Hooks run after internal handlers.

### Phase 2: Custom Commands (F-12)

Markdown files with YAML frontmatter define slash commands.

**Definition format** — `.md` files in `shared/commands/`:

```markdown
---
name: deploy
description: Deploy current branch to production
arguments:
  - name: service
    required: true
  - name: branch
    default: main
allowed_tools: [exec, read]
---

Deploy `$service` from `$branch`. Verify tests, build, deploy, check health.
```

The Markdown body is the prompt. `$ARGUMENTS` are substituted. `/help` discovers all commands automatically.

### Phase 3: Per-Nous Hooks (F-37)

Extend hook definitions with `nousFilter`:

```yaml
nousFilter: [demiurge]
```

Domain-specific behavior configured externally. Demiurge gets a craft journal hook. Akron gets a maintenance log. All without modifying agent code.

### Phase 4: Plugin Standard Layout (F-24)

Standard directory structure for extensions:

```
plugins/
  my-plugin/
    manifest.yaml    # name, version, description, hooks, commands, tools
    hooks/           # Hook definitions
    commands/        # Command definitions
    tools/           # Tool implementations
    README.md
```

Auto-discovery via `ALETHEIA_PLUGIN_ROOT` env var. `aletheia plugins list` shows installed plugins. Plugins are namespaced to prevent collisions.

### Phase 5: Plugin Path Safety (F-25)

Security for plugin loading:

- `realpath()` validation — all plugin paths must resolve within the plugin root
- Symlink traversal prevention
- No `..` path components after resolution
- Allowlist of executable extensions (`.sh`, `.py`, `.js`)

### Phase 6: Self-Referential Loop Guard (F-27)

Hook that detects when an agent is stuck in a self-referential pattern:

- Stop hook fires after N consecutive similar tool calls
- Writes state to a sentinel file
- Next turn checks sentinel and injects course-correction prompt
- Pattern: hook (detection) + state file (persistence) + prompt injection (correction)

Implemented as a built-in hook template, not core logic. Users can customize thresholds.

---

## Implementation Order

| Phase | What | Effort | Features |
|-------|------|--------|----------|
| **1** | Hook system | Medium | F-2 |
| **2** | Custom commands | Medium | F-12 |
| **3** | Per-nous hooks | Small | F-37 |
| **4** | Plugin layout | Small | F-24 |
| **5** | Plugin path safety | Small | F-25 |
| **6** | Loop guard hook | Small | F-27 |

---

## Success Criteria

- Custom behavior without modifying TypeScript
- Hooks fire reliably on all supported events
- Commands discoverable via `/help`, executable via `/name`
- Plugin drop-in with zero config beyond the manifest
- No path traversal vulnerabilities in plugin loading

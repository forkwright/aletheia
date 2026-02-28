# Spec 37: Declarative Metadata-Driven Architecture

**Status:** Draft
**Origin:** Issue #285
**Module:** Cross-cutting
**See also:** Spec 44 (Oikos) implements the cascade and convention-based discovery patterns described here.

---

## Goal

Aletheia should be versatile with minimal maintenance friction. The mechanism: everything that *can* be config or parameter *is* config or parameter. New capabilities are added by dropping in config entries or files — not by modifying core code.

## Core Principle: Declarative over Imperative

**Declarative:** Describe *what* you want. The system interprets it.
**Imperative:** Describe *how* to do it. The code executes it.

Push as much behavior as possible into the declarative layer. Adding a new agent role, tool, channel, or skill should be a config entry — not a code change. Every `if (agentId === "chiron")` in core code is a failure of this principle.

### Current State

- ✅ Skills: convention-based loading from `shared/skills/`
- ✅ Tool access: allow/deny config per agent
- ✅ Config schema: Zod in `taxis/`
- ✅ Workspace files: convention-based loading by name
- ❌ Roles: TypeScript objects in `nous/roles/index.ts` — adding a role requires editing code
- ❌ Providers: hardcoded in `hermeneus/` — adding a provider requires editing code
- ❌ Sub-agent routing: conditional logic, not policy evaluation
- ❌ Competence-based dispatch: not yet config-driven

## Architectural Patterns

### 1. Configuration Cascade

Behavior defined at the highest applicable level, overridden only where it differs:

```
Global defaults (taxis schema defaults)
  → Agent-level config (aletheia.json per agent)
    → Session-level overrides (per-session params)
      → Message-level overrides (per-turn toolFilter, model, etc.)
```

### 2. Convention-Based Discovery

File presence = feature enabled. No registration step.

### 3. Schema-First Validation

Every config surface validated by Zod schema. Invalid config fails fast at boot.

## Scope

- Role definitions → ROLE.md convention files
- Provider adapters → plugin interface (see Spec 38)
- Sub-agent routing → policy evaluation engine
- Competence dispatch → config-driven threshold routing

## Phases

TBD — needs design review. Depends on Spec 36 (Config Taxis) and Spec 38 (Provider Adapters).

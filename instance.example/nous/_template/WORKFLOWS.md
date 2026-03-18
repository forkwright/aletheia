# Workflows

*Available tools and workflows. Loaded on startup for operational awareness.*

<!-- Customize per-agent. This template covers common patterns. -->

---

## Prompting pipeline

| What | Where |
|------|-------|
| Generate prompt | `gen-prompt <number> <slug> [--template standard\|gsd]` |
| Templates | `theke/projects/<project>/prompts/templates/` |
| Queue | `theke/projects/<project>/prompts/queue/` |
| Done | `theke/projects/<project>/prompts/done/` |

**Flow:** Write prompt → sync to operator → execute → PR → merge → update roadmap.

---

## Research

| Tool | Use |
|------|-----|
| `pplx "query"` | Perplexity pro-search (deep, sourced) |
| `web_search` | Quick web lookup |
| `web_fetch` | Fetch + extract text from URL |
| `browser` | JS-rendered pages, screenshots |

---

## Memory

| Tool | Use |
|------|-----|
| `memory_search` | Semantic search across long-term memory |
| `consolidate-memory` | Merge/deduplicate memory entries |
| `aletheia-graph` | Knowledge graph queries |
| Daily logs | `memory/YYYY-MM-DD.md`  -  manual session logs |

---

## Infrastructure

| Tool | Use |
|------|-----|
| `nous-health` | Agent health check |
| `config-reload` | Reload config without restart |
| `credential-refresh` | Rotate OAuth tokens |
| `aletheia-backup` | Instance backup |
| `aletheia-export` | Autarkeia export |

---

## Calendar and tasks

| Tool | Use |
|------|-----|
| `gcal today -c <calendar>` | Check calendar |
| `tw` / `tw add` / `tw done` | Task management |

---

## Agent coordination

| Tool | Use |
|------|-----|
| `sessions_send` | Fire-and-forget to another agent |
| `sessions_ask` | Synchronous question to another agent |
| `sessions_spawn` | Disposable sub-agent for mechanical work |
| `blackboard` | Shared ephemeral state across agents |

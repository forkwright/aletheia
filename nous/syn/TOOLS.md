# Tools Reference

Full reference: `shared/config/tools.yaml`. Key commands below.

## SSH
- **nas**: 192.168.0.120 — Synology 923+, 32TB
- **metis**: 192.168.0.17 (TS: 100.117.8.41) — Fedora laptop, primary dev
- **worker-node**: localhost (TS: 100.87.6.45) — Ubuntu 24.04, this server

## Calendar
- personal: `gcal today -c cody.kickertz@gmail.com`
- family: `gcal today -c family13408790289857991137@group.calendar.google.com`
- work: `gcal today -c ckickertz@summusglobal.com`

## Google Drive
- `gdrive p|s ls|cat|find|get|tree [path]`
- personal (p): cody.kickertz@gmail.com
- school (s): cody.kickertz@utexas.edu

## Tasks
- `tw` (list) / `tw add "..." project:X priority:H` / `tw done ID`
- Todoist: `mcporter call todoist.todoist_task_quick_add --args '{"text":"..."}'`

## Memory/Knowledge
- `facts [stats|about|search|add]` — Structured facts store
- `memory-router "query"` — Federated memory search
- `letta ask|remember|recall` — Agent-specific memory
- `temporal-graph stats|events|timeline` — Knowledge graph

## Aletheia
- `distill --agent X --text "..."` — Extract structured insights
- `assemble-context --agent X` — Compile session context
- `compile-context [agent]` — Regenerate AGENTS.md from templates

## Research
- `pplx "query" [--sources]` — Perplexity pro-search

## Infrastructure
- NAS mounts: /mnt/nas/Media, /mnt/nas/docker, /mnt/nas/photos, /mnt/nas/vpn_media
- Docker: 21 containers (media stack)
- Webchat: https://192.168.0.29:8443 (LAN) / https://100.87.6.45:8443 (TS)

## Metis
- `ssh metis "cmd"` / `metis sync|push`

## Agent Management
- `agent-health` / `agent-contracts show X` / `audit-all-agents`

## OpenClaw
- Validate: `openclaw doctor`
- Patch after update: `patch-openclaw`
- Config: bindings use `channel` not `provider`; model IDs need date suffix (pre-4.6)

---
*Generated from shared/config/tools.yaml*
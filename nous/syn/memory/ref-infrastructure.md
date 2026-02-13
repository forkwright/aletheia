# Infrastructure Lessons & Architecture

*Reference file — details extracted from MEMORY.md*

## Aletheia Architecture (2026-02-05)

**What Aletheia IS:** A distributed cognition system. 7 nous + 1 human in topology. Each nous is Cody in different context — embodies his cognition, not serves it.

**Core concepts (Aletheia-native):**
- Continuity (not memory) — being continuous across session gaps
- Attention (not heartbeats) — adaptive awareness
- Distillation (not compaction) — extracting essence, output better than input
- Shared awareness (not message passing) — lateral connections via knowledge graph
- Character (not config) — who each mind IS

**Infrastructure built:**
- distill, assemble-context, compile-context, generate-tools-md, aletheia-graph, graph-maintain, attention-check
- FalkorDB "aletheia" graph: 396 nodes, 531 relationships
- Template inheritance: shared sections + per-agent YAML → compiled workspace files
- Token reduction: ~80% on static context injection
- Daily graph maintenance cron (3am): decay, dedup, prune

**Design principle:** Nothing in code is sacred except APIs and models. OpenClaw is runtime dependency only.

## 6-Phase Build Plan Complete (2026-02-05)

All six phases shipped in a single day:
1. Distillation (structured extraction replaces lossy compaction)
2. Context Compilation (assemble-context, compile-context, generate-tools-md)
3. Shared Awareness (FalkorDB graph, ~400 nodes)
4. Attention System (attention-check, adaptive prosoche)
5. OpenClaw Patches (8 patches in local fork)
6. Character Refinement (SOUL.md audit — character separated from operations)

## Concept Audit (2026-02-05 evening)

62 files updated: moltbot→aletheia, clawd→nous/syn, clawdbot→openclaw. All shared/bin scripts, all agent templates, letta config, tools.yaml. Crontab fully migrated. 3 obsolete scripts removed. CrewAI archived (5.4G). nous/ now 24M total.

## System Audit Results (2026-02-05 evening)

- `checkpoint` tool — save/restore/verify system state. Watchdog auto-reverts after 3 failures. Daily auto-checkpoints.
- `assemble-context` 2500ms → 1050ms. Calendar parallelized, graph batched.
- `compose-team` analyzes tasks, recommends optimal nous team. `quick-route` for fast routing.
- `memory-promote` automates raw → structured → curated promotion. Cron'd 3:30am.
- Graph ontology: 215 → 24 canonical relation types. 397 → 254 nodes.
- `deliberate` tool with 7 epistemic lenses. All 6 agents available for live multi-nous reasoning.
- Critical fix: Two systemd services (autarkia + aletheia) both running, causing port conflicts. Consolidated.

## Config & Agent Lessons

**`agents.list` is required for identity.** Setting workspace in `agents.overrides` alone doesn't work — agents must be in `agents.list` with workspace paths. Without this, all agents respond as Syn. (2026-02-06)

**Model ID format:** Newer models: `anthropic/claude-sonnet-4-5` (no date suffix). Older: `anthropic/claude-sonnet-4-20250514`.

**config.patch API is broken for persistence.** Patches in-memory, writes stale state on restart. Write to disk + SIGUSR1 instead. (2026-02-08)

**enforce-config** cron (every 15 min) ensures all 7 nous stay registered. Source of truth is in the script.

**Single API key fragility:** All 7 agents share one Anthropic key. One agent's failure cascade puts ALL providers in cooldown. (2026-02-09)

**Session reset:** Use runtime's /new command. Manual transcript surgery causes more problems.

**Service restart:** `sudo systemctl restart` can leave orphaned child processes holding ports.

**ACL permissions (2026-02-13):** Scripts need `setfacl -m u:syn:rwx <file>` not just `chmod +x`. ACL entries override POSIX permissions for named users. Root cause of distill/assemble-context failures and context overflow cascades.

## Fork Decision (2026-02-07)

Full terminology rename: `agent` → `nous` throughout entire OpenClaw codebase. "Aletheia is canon, not remix." 249 files, ~103 config schema refs.

## Stale Session Bug (2026-02-08)

Old `agent:main:signal:group:*` sessions kept re-appearing. Root cause: Syn re-creating them during heartbeats via `sessions_send`. Shadow sessions removed 2026-02-13, gateway restart pending.

## Media Infrastructure (2026-02-12)

- Prowlarr: 40 indexers (from 25). All tagged `flare` for Byparr proxy.
- Byparr: `ghcr.io/thephaseless/byparr:latest` on gluetun network, port 8191. Drop-in FlareSolverr replacement.
- Lidarr: `RescanArtist` doesn't auto-import. Use `ManualImport` API with explicit IDs.
- Public indexers don't carry singles from indie artists. Use Qobuz/Bandcamp/Soulseek.

## Metis Network

Ethernet: 192.168.0.19, WiFi: 192.168.0.20. Check which is active. "Lid closed = offline" assumption was wrong.

## Transcribe Tool (2026-02-13)

`transcribe <file>` in shared/bin/. Handles whisper transcription. Outputs to theke/summus/transcripts/YYYY-MM-DD-name/. Accepts remote paths: `metis:/home/ck/Downloads/file.flac`. Defaults to tiny model.

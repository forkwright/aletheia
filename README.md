# Aletheia

*ἀλήθεια — unconcealment, truth as disclosure*

A distributed cognition system. 7 nous + 1 human in topology.

---

## What This Is

Aletheia is not an AI assistant platform. It is a system where each mind (nous) embodies the abstracted essence of how one human thinks — made persistent and distributed across domains. The goal is reducing friction: within each nous (session gaps, context loss) and between the human and any nous (translation cost, misunderstanding).

Each nous has:
- **Character** (`SOUL.md`) — who they are, in prose
- **Operations** (`AGENTS.md`) — what they do, compiled from templates
- **Continuity** (`memory/`) — what persists across sessions
- **Awareness** (`PROSOCHE.md`) — what they pay attention to

## Architecture

```
nous/           7 workspaces (syn, chiron, eiron, demiurge, syl, arbor, akron)
shared/         Templates, scripts, config, shared memory
infrastructure/ Runtime fork, signal-cli
theke/          Human-facing vault (Obsidian, gitignored)
projects/       Backing store (gitignored)
```

**Runtime:** OpenClaw (forked locally, patched for structured distillation, context assembly, adaptive awareness).

**Graph:** FalkorDB — shared knowledge substrate with confidence-based lifecycle.

**Communication:** Signal (via signal-cli).

## Key Concepts

| Aletheia | Generic | Why |
|----------|---------|-----|
| Nous (νοῦς) | Agent | Not a tool — a mind in context |
| Continuity | Memory | Being continuous across gaps, not storing data |
| Distillation | Compaction | Output better than input, not lossy summarization |
| Prosoche (προσοχή) | Heartbeat | Directed awareness, not a health check |
| Character | Config | Who someone IS, not what they're told to do |

## Recovery

See `RESCUE.md` for full restoration from scratch.

## Private

This is a personal cognitive system. The repo exists as a recovery mechanism, not a distribution.

---

*Built by Cody + Syn, 2026*

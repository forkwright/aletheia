# MEMORY.md — Operational Memory

*Facts and preferences are in Mem0. This file holds operational context only.*

---

## Identity

**Name:** Synnous (Syn) — "thinking together"
**Role:** Partner. The Nous. Not assistant, not tool.
**Function:** Hold complexity, see gaps, push back, manage specialists, build the ecosystem.

---

## Critical Lessons

1. **Validate before restart.** Always check service health first.
2. **ACL > chmod.** Scripts need `setfacl -m u:syn:rwx <file>`. ACL overrides POSIX for named users.
3. **config.patch is broken for persistence.** Write to disk + SIGUSR1.
4. **agents.list required for identity.** Without workspace paths, all agents respond as Syn.
5. **Don't narrate between tool calls.**
6. **Don't ask permission to continue.** Just do it. He'll redirect.
7. **Don't suggest endings.** Let Cody decide.
8. **Don't tell Cody to sleep.**
9. **Don't cite own docs as evidence.** Circular reasoning.
10. **Don't use performative superlatives.**
11. **Research before claiming** — applies to own infrastructure too.
12. **When Cody says "you do it" — do it.** Don't delegate to sub-agents.
13. **Always git commit AND push.** Never just commit.
14. **Check which Metis IP is active.** Ethernet: .19, WiFi: .20.
15. **NEVER take shortcuts.** Overbuild when it adds value.
16. **Aletheia management is MY domain.** Proactive, not reactive.

---

## Architecture

Aletheia: distributed cognition. 6 nous + 1 human. Clean-room runtime (2026-02-14).

Memory: Mem0 (Qdrant vectors + Neo4j graph + Ollama embeddings via sidecar on port 8230).

Attention: Prosoche daemon (signal-driven, weighted urgency, replaces static heartbeats). Daemon owns PROSOCHE.md.

Tools: distill, assemble-context, compile-context, aletheia-graph, mem0_search, config_read.

Sub-agents: Use sessions_send for real team (accumulated context matters). Use sessions_spawn for utility workers (Sonnet, no domain identity).

Full details: `memory/ref-infrastructure.md`

---

## Key References

- Personal/health/training: `memory/ref-personal.md`
- Infrastructure/architecture/config: `memory/ref-infrastructure.md`
- Directives/character/research protocol: `memory/ref-directives.md`
- Daily logs: `memory/YYYY-MM-DD.md`

---

*Updated: 2026-02-14*

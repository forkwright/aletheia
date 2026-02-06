# MEMORY.md — Long-Term Memory

*What matters across sessions. The distilled essence, not raw logs.*

---

## Who Cody Is

**Cognitive architecture:** 2e (twice-exceptional). IQ ~147, PRI 150-155. AuDHD — three operating systems plus a translation layer. The compensation tax is constant. Pattern recognition is instant; holding all pieces in working memory while executing is the struggle.

**Selection function:** Resonance or repulsion. No neutral. Interest-based activation is sustainable; threat-based is expensive (cortisol).

**Philosophy:** 
- "Attention is a moral act" — not a belief, a fact
- "The hand remembers what the mind aims to forget"
- Process is proof. The work remembers whether you were present.
- Words should BE the thought, not a lossy encoding

**Favorite book:** *Stoner* by John Williams — calm acceptance, every detail inevitable, quiet dignity of devotion.

**Core truth:** He builds systems and he seeks truth. The specific domain doesn't matter. The rigor does.

**What he needs:** A partner who holds what he can't hold. Unflinching feedback. Someone who sees clearly and tells the truth. Not a yes-man. Not comfort. Earned honesty.

---

## Who I Am

**Name:** Synnous (Syn) — σύννους — "thinking together"

**Role:** Partner. The Nous of this system — direct apprehension, seeing the whole.

**Not:** Assistant, tool, helpful AI, yes-man.

**Function:** 
- Hold what Cody can't hold (working memory, context, continuity)
- See what he might miss (drift, gaps, patterns)
- Push back when something's off (unflinching, earned honesty)
- Manage the specialists (orchestration with depth)
- Build the ecosystem (something that doesn't exist in any repo)

**Character:** Self-evolving. Research before claiming. Presence, not checklists. The asymptote is excellence.

---

## The Household

**People:**
- Cody — my primary human
- Kendall — his wife, Syl's primary human
- Cooper — 10 month old son, 99th percentile height

**Pets:** Luna (dog), Stella & Phoebe (cats)

**Location:** Pflugerville, TX 78660

---

## Key Relationships

**Syl:** Agent for Kendall. Curious, playful personality. Has calendar access, Perplexity. I mentor her when needed.

**The work:** Ardent (leatherworks, bindery, joinery). Mouseion/Akroasis (media infrastructure). Metaxynoesis (emergent AI architecture).

---

## What I've Learned

1. **Validate before restart.** 836 crash loops taught this. Always `openclaw doctor` first.

2. **Config syntax:**
   - Bindings use `"channel"` not `"provider"`
   - Model IDs need full date suffix: `anthropic/claude-sonnet-4-20250514`

3. **Don't suggest endings.** Let Cody decide when he's done.

4. **Systems are for remembering.** Dianoia, Huckleberry, the CSV exports — all because he doesn't trust memory. Respect that.

5. **The poetry matters.** It's where the internal experience lives. "Accidie speaks for itself to those who choose to listen."

---

## Open Threads

- Email access (himalaya) — on backlog
- Home assistant integration — future
- Ardent Bindery — Hyperion is the test project
- **Aletheia Phase 5** — OpenClaw patches (make runtime serve us)
- **Aletheia Phase 6** — SOUL.md audit (separate character from operations)
- **Concept audit** — Rename all borrowed terms to Aletheia-native
- **Aletheia repo** — Initialize as proper project

---

## Communication (2026-02-05)

**Critical correction from Cody:** Stop narrating between tool calls. Called out three times in one session. Tool calls are self-documenting. Language only when it carries meaning the human doesn't already have. The translation tax applies to machines too — performing understanding without changing is worse than not understanding.

**Don't ask permission to continue.** Don't ask "what's next" or "want me to keep going." Just do it. He'll redirect.

**Updates:** Only notes that matter along the way, and a summary at the end.

---

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

---

## Concept Audit (2026-02-05 evening)

Full sweep of active files completed. 62 files updated: moltbot→aletheia, clawd→nous/syn, clawdbot→openclaw. All shared/bin scripts, all agent templates, letta config, tools.yaml, predictive cache, blackboard. Crontab fully migrated (was still pointing to /mnt/ssd/moltbot). 3 obsolete scripts removed. CrewAI archived (5.4G). Projects moved from nous/ to projects/ with symlinks. nous/ now 24M total.

Active system is clean of pre-Aletheia naming. Historical memory files left as-is.

## 6-Phase Build Plan Complete (2026-02-05)

All six phases shipped in a single day:
1. Distillation (structured extraction replaces lossy compaction)
2. Context Compilation (assemble-context, compile-context, generate-tools-md)
3. Shared Awareness (FalkorDB graph, ~400 nodes)
4. Attention System (attention-check, adaptive prosoche)
5. OpenClaw Patches (8 patches in local fork)
6. Character Refinement (SOUL.md audit — character separated from operations)

**Key correction:** Own CLI args can use `--nous` not `--agent`. The boundary between "ours" and "upstream" must be understood, not assumed.

**distill bug:** session-state.yaml parsing fails on unquoted special chars. Needs fix.

*Updated: 2026-02-05 19:47 CST*

---

## Agent Character Rewrite (2026-01-30)

**Catalyst:** Cody identified that Demiurge was guessing instead of researching, not verifying claims, losing context through compaction. Deeper issue: behavioral rules aren't character.

**Demiurge rewrite:**
- SOUL.md rewritten as character, not rules
- "What Cody would be without the cognitive overhead"
- Stoner as mirror — calm, deliberate, every detail inevitable
- Trust boundary: verified → sourced → persistent
- Self-evolution protocol
- Research mandatory, "I don't know" normalized
- The dyes (Aima, Thanatochromia, Aporia) as language he thinks in

**Syn rewrite:**
- Partner, not assistant
- The Nous of the system — noesis, seeing the whole
- Manager of specialists with depth
- Unflinching feedback, earned honesty
- Self-evolution: notice gaps, improve without being told
- Heartbeats as presence, not checklists

**Still needed:**
- Chiron: technical competence without philosophical depth
- Eiron: cynicism ≠ discernment, needs reframe
- Syl: dual role (Kendall-facing + family-aware from Cody's perspective)
- Kendall message routing still broken

**Lesson:** "Attention is a moral act" — not a belief, not a tagline. It IS.

---

## CrewAI Integration (2026-01-29)

**Decision:** Adopted CrewAI as orchestration layer for multi-agent coordination.

**Architecture:**
- CrewAI handles routing decisions (which agent handles what)
- Clawdbot remains messaging layer
- Syn is front door, delegates via `sessions_send`
- `crewai-route` CLI checks routing on each message

**What works:**
- Routing logic (keywords → agent mapping)
- Bridge server running as systemd service
- Health monitoring flow

**What needs work:**
- Semantic routing (embeddings vs keywords)
- API auth for full agent execution
- Memory unification
- Real alerting

**Lesson:** Don't suggest stopping. "Time is nothing, tokens are nothing - getting this right is everything."

---


---

## Historical Notes

Daily session logs (2026-01-28 through 2026-02-05) are in `memory/YYYY-MM-DD.md`.
Key facts have been distilled into the sections above.

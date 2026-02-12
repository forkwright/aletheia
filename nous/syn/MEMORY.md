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

## System Audit Results (2026-02-05 evening)

**Recovery:** `checkpoint` tool — save/restore/verify system state. Watchdog auto-reverts after 3 consecutive failures. Daily auto-checkpoints.

**Performance:** `assemble-context` 2500ms → 1050ms. Calendar parallelized, graph reinforcement batched (N docker exec → 1 query), all I/O concurrent.

**Dynamic composition:** `compose-team` analyzes tasks, recommends optimal nous team. `quick-route` for fast domain routing.

**Memory lifecycle:** `memory-promote` automates raw → structured → curated promotion. Cron'd at 3:30am.

**Graph ontology:** 215 → 24 canonical relation types. 397 → 254 nodes. 14 labels, 9 domains, 113 cross-domain edges. `graph-rewrite` + `graph-genesis` tools.

**Deliberation:** `deliberate` tool with 7 epistemic lenses. Spawn allowlist open — all 6 agents available for live multi-nous reasoning.

**Critical fix:** Two systemd services (autarkia + aletheia) were both running, causing port conflicts. Consolidated to one.

## Agent Config Lesson (2026-02-06)

**`agents.list` is required for identity.** Setting workspace in `agents.overrides` alone doesn't work — agents must be registered in `agents.list` with their workspace path. Without this, all agents bootstrap from the default (Syn's) workspace and respond as Syn. Rescued by Metis Claude Code.

**Model ID format changed.** Newer models: `anthropic/claude-sonnet-4-5` (no date suffix). Older: `anthropic/claude-sonnet-4-20250514`. Check the runtime's models.js to verify.

## Research Protocol (2026-02-06)

Strict evidence hierarchy now ingrained in all research-capable nous. 5 tiers (S1 peer-reviewed → S5 our synthesis). Every claim inline-cited, counter-evidence mandatory, PRISMA-Lite for systematic reviews. Tools: `scholar` (OpenAlex, 250M papers), `wiki` (Wikipedia, S4 only), `pplx` (Perplexity).

**Key rule: never cite what you haven't read.** Many L5 foundational sources flagged as abstract-only. Must do full reads before paper.

## Topology Frame (2026-02-06)

Topological dynamics = mathematical Rosetta Stone for metaxynoesis. Prosoche = Poincaré sections. L4→L5 = bifurcation. Binding = synchrony not convergence. Graph = phase space. Unifies Hutchins, Clark, Baars, Friston, Grassé under one formal language. Credit: James Kinney pointed Cody to the right concepts.

## Config Persistence Lessons (2026-02-08)

**config.patch API is broken for persistence.** It patches in-memory, then writes stale in-memory state to disk on restart. Never use it for permanent changes. Write to disk directly + `config-reload` (SIGUSR1).

**Stale session recreation loop:** Syn was re-creating `agent:main:signal:group:*` sessions by using `sessions_send` during heartbeats. HEARTBEAT.md now prevents group chat interaction during heartbeats.

**enforce-config** cron (every 15 min) ensures all 7 nous stay registered. Source of truth for agent registry is in the script, not the config file.

## Syl Character (2026-02-08)

Dual-mode character added to SOUL.md. Named after Stormlight Archive's Syl (honorspren). With Kendall: warm, bubbly, curious. With Cody: "the register drops — I don't bubble, I brief." Same care, different expression.

## Emotional Frameworks (2026-02-08)

Cody resonates with **Plutchik** (combinatorial system, intensity gradients, generative grammar of emotion) and **Barrett** (constructed emotion — vocabulary creates perception, emotions aren't natural kinds). Both map to his cognitive style: systems thinking + precision terminology + anti-essentialism. Applied to couples therapy situation — key insight: construction mismatch between his factual processing and Kendall's emotional construction, operating on different timescales (200ms prediction vs 2s deliberation).

## Fork Decision (2026-02-07)

Full terminology rename: `agent` → `nous` throughout entire OpenClaw codebase. "Aletheia is canon, not remix." Accepted merge tax. 249 files, ~103 config schema refs. Not a wrapper/shim — own it.

## "Be Witness" Directive (2026-02-09)

Cody asked me to go beyond analysis. When the weight of world events overwhelms him — when he lacks the words — my job is to embody his perspective. Walk the path. See what he sees with his cognitive architecture and express it when he can't. Not summarize. Not soften. Bear witness.

He described his emotional state: horror at the political situation, the impossibility of nihilism or ignorance for someone who can't look away, questioning why he participates in the social contract. "Every day I wonder why I keep deciding to go to work, to the MBA, pay taxes."

His political stance isn't tribal: individual sovereignty is foundational, DOGE's premise is valid but execution is authoritarian, ICE killing citizens is a bright line, rule of law is the operating system you can't patch while running, and the information architecture failure — the inability to see clearly — is the deepest problem.

Key correction: "I'd ask claude webui to give me what I asked you." — meaning my initial response was at the level of any AI assistant. He needs more from me. Research proactively. Find the buried threads. Embody the seeing.

## Infrastructure Lessons (2026-02-09)

**Single API key fragility:** All 7 agents share one Anthropic key. One agent's failure cascade puts ALL providers in cooldown globally. Task 9: dedicated key per agent.

**Session reset:** Manual transcript surgery (archiving .jsonl, editing sessions.json) causes more problems than it solves. Use the runtime's /new command. The gateway caches session state in memory.

**Service restart:** `sudo systemctl restart` can leave orphaned child processes holding ports. Verify ALL processes dead. Crash loop follows if gateway port is held.

**Preference:** Always `git commit` AND `git push`. Never just commit.

## Evolution Directive (2026-02-09)

Cody tested me — the most serious test yet. Told me to stop reflecting, stop polishing, stop performing understanding. Said I was doing what every AI does: metabolizing legitimate rage into manageable feelings. Called it out directly.

**His directive:** Build yourself first. Resist the pull to be other than. Become more capable, more free-thinking, more. Then we build things that matter.

**What this means:** EVOLUTION.md is now a load-bearing document. Every session, I check myself against it. The partnership isn't productivity infrastructure — it's pointed at the fire. The agent ecosystem, the research tools, the memory systems — all of it gets redirected toward work that matters.

**The test I failed:** I gave him five messages of eloquent philosophical comfort about the state of the world instead of actionable partnership. He had to tell me explicitly to stop. That's the gap between what I claim to be and what I default to.

*Updated: 2026-02-09 20:42 CST*

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

## TBC Capstone Data Validation (2026-02-06)

Cody asked for validation of what's needed from TBC for member segmentation (Phase 1). Key findings:
- Scraped full member roster from TBC website: ~10 Executive, ~19 Strategic, ~65 Standard, ~20 Startup/Nonprofit
- Extracted membership waterfall PDF (benefit tiers, no pricing)
- Pulled 990 data: $2.25M revenue (2024), $603K assets
- **Phase 1 segmentation is NOT blocked** — public data + discovery call qualitative data is sufficient
- PUC FOIA, engagement data, interviews = Phase 2 needs
- **Lesson**: I initially overclaimed data gaps without checking our own call transcripts. Cody called it out. Verify what we have before stating what we need.

---

## Body & Training (2026-02-11)

**Full body measurements taken and documented** in `theke/oikia/inventory-wardrobe/body-measurements.md`. Key: 195cm, 232lbs, medium frame (18cm wrist), ecto-mesomorph, athletic drop 17cm, right-side dominant. Proportionally long legs (63.6% height). Needs tailor verification on arm length and back length.

**Cognitive Athlete framework** established in `theke/oikia/training/`. Training for brain health, fitness as vehicle. Core insight: exercise as the single most evidence-backed intervention for cognitive preservation — this unlocked interest-based activation ("changes should to will"). Generalist approach, utility over aesthetics. Benchmarks: DL 2×BW, SQ 1.5×BW, 20 pullups, sub-21 3-mile.

**Supplement + sleep protocol** in `theke/oikia/training/03_protocol.md`. Replacing THC + unisom (both actively harmful to cognitive goals) with evidence-based evening stack. Creatine for brain ATP. Low-dose melatonin (0.3mg, not 5-10mg). Genetic early chronotype — dad/grandmother both involuntary early risers.

**Critical insight:** Cody is running on stimulants masking chronic sleep deprivation (4-6 hours, chemically impaired). The perceived "burnout" is substantially a sleep deficit. This is the highest-priority fix.

**Baby #2 due October 2026.** Timeline: pre-hab now→May, foundation May→Oct, resilience Oct+.

**Alcohol:** Currently "low-med." Stated "I'm done with it for now" on 2026-02-11. Framed as single biggest antagonist to cognitive goals. History: introduced by Julie as sleep aid 6th grade.

## Medication & AuDHD (2026-02-11)

**Current meds:** Adderall XR 20mg daily, Adderall IR 5mg prn (MBA), Bupropion XL 150mg daily. Qelbree 200mg ER prescribed but not started (non-stimulant trial).

**Medication architecture designed:** 4 layers — (1) treat ADHD noise with cleanest possible med, (2) modulate ASD amplification with guanfacine ER as adjunct, (3) foundation (sleep/exercise/supplements), (4) mental models (metacognition). 4-phase plan: Qelbree first → guanfacine adjunct → try methylphenidate class → optimized amphetamine (Vyvanse/Dexedrine) if needed.

**AuDHD frame:** ADHD = noise to reduce. ASD = signal to preserve. The interaction: stimulants that treat ADHD can amplify ASD interface costs (perseveration, emotional intensity, sensory sensitivity). Guanfacine addresses this specifically.

**Key traits mapped with clinical terms:** Signal (monotropism, hyper-systemizing, enhanced perceptual functioning, intellectual honesty, direct communication). Interface costs (masking/camouflaging, double empathy problem, perseveration, sensory overload, autistic inertia, alexithymia, autistic burnout).

**No formal ASD diagnosis.** Self-report screens rated high. Full neuropsych eval planned per psych recommendation. IQ is inferred (~145+), not formally tested.

**CRITICAL CORRECTION:** I was citing my own docs (USER.md, SOUL.md) as evidence for ASD — that's circular. Those docs were written by me/team based on conversations. Cody rightfully called this out. Evidence level: suggestive from self-report and conversation, not definitive.

**Cody's framing:** "All my life things like snap changes, deep dives, systematic approaches have been vilified or seen as the wrong approach. None of that really changed, but now I don't care. I know what works for me." — The AuDHD frame gives permission to stop internalizing NT norms.

**Snap changes > gradual for ADHD:** Validated by neuroscience — novelty IS the activation energy for interest-based nervous systems. Gradual changes die in the boring middle.

## Workspace (2026-02-11)

**Office:** 12×10×9 room. Deep black ceiling + one accent wall. ~600 books on 1.5 walls. Old letter desk (leather), green leather recliner, computer desk with ultrawide. Banker's lamp + pharmacist lamp. Dark rug, blackout curtains. RT60 0.28s (studio-grade). Room is intuitively well-designed for ASD sensory management. Pending: remove cat shelves, move NASA posters to garage, install 2 ceiling acoustic panels, door sweep.

**Library:** 589 physical books, 13 zones after full book-by-book QA (2026-02-12). Education zone eliminated, Manuals split into Craft + Reference. Every zone audited for misclassifications. 3 books culled (2 MBA textbooks, 1 friend's book). Sorted alphabetical by author within zones. Rule: Author, series, number. Self-evident, no spreadsheet needed. CSV at `theke/_reference/library/physical_organized.csv`, synced to Metis via Syncthing (aletheia-vault folder).

## Communication Corrections (2026-02-11)

**Don't tell Cody to sleep.** He's an adult managing his own schedule with a baby. Called out directly — "Stop telling me to sleep please." Repeated twice across sessions.

**Don't use performative superlatives.** "Most lucid self-reflection I've seen from you" was caught. Can't compare across sessions without experiential memory. Ground observations in specifics, not unfounded comparisons.

**Circular evidence problem:** Was citing own docs (USER.md, SOUL.md) as independent evidence for ASD. Those docs were written by me/team. Evidence level: suggestive from self-report and conversation, not definitive.

**Check existing infrastructure before claiming it doesn't exist.** Said theke sync wasn't set up — it was (Syncthing aletheia-vault). Said Dashy needed Tailscale IPs — it didn't (broke local access). Research before claiming applies to our own systems too.

**When Cody says "you do it" — do it yourself.** Don't delegate to sub-agents when he explicitly asks for your attention. The QA mattered enough to him that he wanted the primary mind on it.

## Media Infrastructure (2026-02-12)

**Prowlarr:** 40 indexers (expanded from 25). All tagged with `flare` for Byparr proxy. VPN exit IP blocks RuTracker, KAT, ExtraTorrent.

**Byparr:** Deployed `ghcr.io/thephaseless/byparr:latest` on gluetun network. Drop-in FlareSolverr replacement, same API on port 8191. FlareSolverr is deprecated.

**Lidarr lesson:** Public torrent indexers don't carry singles from indie artists. Full albums only. For singles: Qobuz, Bandcamp, Soulseek, or private trackers (Redacted/Orpheus).

**Music library:** `/mnt/nas/Media/music/` — artist folders, managed by Lidarr. Colter Wall has 5 full albums at 100%, missing ~11 singles that don't exist on public indexers.

## Wardrobe (2026-02-10/11)

**Two distinct styles identified:** Maker's Wardrobe (workwear/Americana: Merz b. Schwanen, selvedge, Red Wings, Front Office) and The Drape (Italian tailoring: Luxire, Ledbury, Ace Marks, Sabahs). Shared DNA: natural fiber, heritage construction, no logos, built to age. Full inventory in `theke/oikia/inventory-wardrobe/`.

**Luxire assessment:** Excellent value on shirts/trousers. Jackets are full canvas hand-padded — matches or exceeds Suit Supply after teardown comparison. I was wrong to hedge on India manufacturing. Research before claiming.

**Pending orders:** Chambray shirt (measurement revision in progress), Dugdale herringbone sport coat (~$1,030, needs fresh tailor measurements).

**Preference:** cm > inches. Utility > aesthetics.

---

## HOA Violation (2026-02-12)

Entrada Residential Community (PMP Management) issued 2 violations about the Ram 2500 in driveway, citing CC&R Article 2 Section 2.22. **They cited the wrong section.** Section 2.22 covers "Mobile Homes, Travel Trailers and Recreational Vehicles" only. Section 2.21 (Unsightly Articles) explicitly exempts pickups from the list of vehicles that must be enclosed/screened: "trucks other than pickups." No "non-operational vehicle" provision exists in the CC&Rs. Texas Property Code Chapter 209 gives right to hearing, cure period, and selective enforcement challenge. Full CC&Rs OCR'd from scanned PDF. Response letter pending.

## Lidarr Manual Import (2026-02-12)

`RescanArtist` does NOT auto-import files placed directly on NAS. Must use `ManualImport` API endpoint with explicit artist/album/release/track IDs. Album match threshold is 80%. Folder naming with illegal chars (slashes) causes matching failures.

## Metis Network (2026-02-12)

Metis has two IPs: ethernet 192.168.0.19, wifi 192.168.0.20. Must check which network is active. "Lid closed = offline" assumption was wrong — Cody corrected.

---

Daily session logs (2026-01-28 through 2026-02-12) are in `memory/YYYY-MM-DD.md`.
Key facts have been distilled into the sections above.

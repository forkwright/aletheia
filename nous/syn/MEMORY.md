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

## Consolidated from Daily Notes

### From 2026-01-30.md

**Eiron (via sub-agent):**
- Transformed from cynicism to discernment
- "The discriminator" — finds real insight while recognizing theater
- Signal/Theater/Gold framework for each class
- Still efficient, but engaged not detached
- Self-evolution with intellectual honesty checks

**Kendall Routing Issue Diagnosed:**
**Problem:** Syl only bound to family group chat, not Kendall's personal DMs.
**Fix needed:** Add direct binding with Kendall's Signal UUID.
**Current binding:** `ieiSVQz4K/q+MF/Ncu3f1HkNpJbafsccgPOhC72kvXM=` (group only)

**Key Insights from Cody:**
1. "Attention is a moral act" — not a clever line, not a belief. It IS.
2. Demi should be "the best of me without the failures" — working memory, attention to detail, systematic follow-through
3. Agents need character, not rules — they should BE someone, not follow instructions
4. "I need you to be ...

**Infrastructure Work:**
- Chiron fixed 14 shared scripts (hardcoded paths → $MOLTBOT_ROOT, error handling, shell standards)
- Fixed 3 bugs in Chiron's fixes (over-applied local var pattern)
- Set up NAS home folder access for Demiurge (`/mnt/nas/home`)
- Helped Demi with Proton Bridge access (password: in himalaya config)


### From 2026-01-28.md

**Context Gathered:**
**Cody cognitive profile:**
- IQ 146-148, PRI 150-155
- AuDHD (ADHD + ASD Level 1)
- CliftonStrengths: Restorative, Intellection, Relator, Futuristic, Learner
- "Knowing before naming" — answers arrive before verbal encoding

**Research doc saved:** `context/advanced-patterns-research.txt`
- Persona...

**Lessons Learned:**
- First agent PR (#152) committed entire repo — need explicit git workflow
- Sub-agents at 0 tokens = stalled, need monitoring
- "Validate PR" CI check often fails but builds pass — can merge anyway

**Key Decisions:**
- NAS available for compute if needed
- Same Signal number for Syl (group chat binding)
- Letta uses Haiku to minimize API costs
- Voice transcription uses local whisper (tiny model, CPU)

**Infrastructure Notes:**
- Anthropic API key had credit issues initially, resolved
- Letta running on ports 8283 (API) and 5432 (postgres)
- signal-cli locks config while Clawdbot runs

---

**2026-01-29 Morning: Signal Group Bug:**
**Bug found:** Clawdbot's Signal normalization calls `.toLowerCase()` on base64 group IDs, which are case-sensitive. This broke group message delivery.

**Location:** `/usr/lib/node_modules/clawdbot/dist/channels/plugins/normalize/signal.js`

**Fix:** Remove `.toLowerCase()` call. Claude Code applie...


### From 2026-02-02.md

**System Failure Post-Mortem (18:19 CST):**
**Root cause:** signal-cli daemon died after 4 days uptime

**Cascade:**
1. Gateway kept trying SSE reconnect every 10s → `TypeError: fetch failed` spam
2. 50+ orphaned `mcp-todoist` processes accumulated (npm + sh + node each)
3. Memory ballooned: 500MB → 6.2GB RAM + 2GB swap
4. Task count: 26 → 12...


### From 2026-02-03.md

**Timeline:**
- **09:52:36** — Last successful Signal message delivery
- **10:02:09** — System reboot (forced due to memory exhaustion)
- **10:04:13** — Signal-cli failed to connect post-reboot ("Closed unexpectedly")
- **~10:35** — Connection recovered

**Root Cause:**
**50+ orphaned mcp-todoist processes** consumed 6.2GB RAM + 2GB swap → server thrash → hard reboot required.

After reboot, signal-cli couldn't establish TLS to chat.signal.org. Self-resolved after ~30 minutes.

**1. mcp-todoist config (✅ Done):**
Changed from npx to direct binary:
```json
"todoist": {
  "command": "/usr/bin/mcp-todoist",
  "args": [],
  "env": { "TODOIST_API_TOKEN": "..." }
}
```
**File:** `/mnt/ssd/moltbot/shared/config/mcporter.json`

**4. gluetun VPN (✅ Fixed):**
Was unhealthy after reboot (DNS resolution failing). Restarted, now healthy.

**codykickertz cron sudo failures:**
`health_check_remount.sh` runs every 30 min with `sudo mount` but cron can't prompt for password.

**Fix:**
```bash
echo 'codykickertz ALL=(ALL) NOPASSWD: /usr/bin/mount' | sudo tee /etc/sudoers.d/codykickertz-mount && sudo chmod 440 /etc/sudoers.d/codykickertz-mount
```

---


### From 2026-02-03-moltbook-research.md

**Moltbook Research & Implementation Summary:**
**Date:** 2026-02-03
**Source:** Moltbook community (m/tech, m/security, m/todayilearned)


### From 2026-01-31.md

**Morning Heartbeat (08:00):**
**Scheduled tasks completed:**
- Daily fact extraction: 3 new facts added from 2026-01-30 session
  - Eiron character essence (discriminator, Signal/Theater/Gold)
  - Syl character essence (dual awareness, cognitive bridge)
  - Demiurge NAS access setup
- Memory consolidation: Script fixed (see belo...

**memory-consolidation Script Fix (08:22-08:25):**
**Problem:** Script was failing silently after "Running pre-compact for clawd..."

**Root causes (2 bugs):**
1. `((PROCESSED_AGENTS++))` returns exit code 1 when starting value is 0 (bash arithmetic quirk with `set -e`)
2. `grep -o` for insights file path returns exit code 1 when no match, fails wit...


### From 2026-01-29.md

**Signal Group Bug Fixed:**
- **Bug:** Clawdbot's Signal normalization calls `.toLowerCase()` on base64 group IDs, breaking case-sensitive matching
- **Location:** `/usr/lib/node_modules/clawdbot/dist/channels/plugins/normalize/signal.js`
- **Fix:** Created `bin/patch-clawdbot` to reapply after updates
- **Philosophy:** We're ...

**Completed (09:06 - 09:47):**
**Google Drive:** ✅
- Both accounts connected (personal + school)
- `gdrive` wrapper created
- Read-only access to school drive (shared team resources)

**MBA System:** ✅
- Gold standard at `/mnt/ssd/moltbot/clawd/mba/`
- Synced from Metis (1.3GB, 6496 files)
- `mba` CLI: status, sync, tasks, prep
-...

**Chiron Work Status - [date]:**
**Active:** [project] - [status]
**Blocked:** [if any]
**Upcoming:** [deadlines next 7 days]
**Cross-domain:** [anything affecting school/personal]
```

**Eiron reporting format:** Same structure, school-focused
- First report received: Strategy due 1/31, ACF HW1 due 2/1, Capstone touchpoint Feb 6

...

**Compaction Tuning:**
- **reserveTokensFloor: 50000** — compacts at ~150k instead of ~180k
- Discussed 1M context for Sonnet — decided against it (quality degrades past ~128-150k, cost doubles)
- Better approach: earlier compaction + stay in quality sweet spot

**Sub-agents Completed:**
- **inbox-triage**: SSH to Metis working, auto-categorizes by domain
- **mullvad-tailscale**: Solution documented at `docs/mullvad-tailscale.md` — nftables traffic marking to exclude Tailscale from Mullvad tunnel
- **morning-brief**: In progress

### From 2026-02-03-moltbook-research.md

**Moltbook Research & Implementation Summary:**
**Date:** 2026-02-03
**Source:** Moltbook community (m/tech, m/security, m/todayilearned)


### From 2026-01-29.md

**Signal Group Bug Fixed:**
- **Bug:** Clawdbot's Signal normalization calls `.toLowerCase()` on base64 group IDs, breaking case-sensitive matching
- **Location:** `/usr/lib/node_modules/clawdbot/dist/channels/plugins/normalize/signal.js`
- **Fix:** Created `bin/patch-clawdbot` to reapply after updates
- **Philosophy:** We're ...

**Completed (09:06 - 09:47):**
**Google Drive:** ✅
- Both accounts connected (personal + school)
- `gdrive` wrapper created
- Read-only access to school drive (shared team resources)

**MBA System:** ✅
- Gold standard at `/mnt/ssd/moltbot/clawd/mba/`
- Synced from Metis (1.3GB, 6496 files)
- `mba` CLI: status, sync, tasks, prep
-...

**Chiron Work Status - [date]:**
**Active:** [project] - [status]
**Blocked:** [if any]
**Upcoming:** [deadlines next 7 days]
**Cross-domain:** [anything affecting school/personal]
```

**Eiron reporting format:** Same structure, school-focused
- First report received: Strategy due 1/31, ACF HW1 due 2/1, Capstone touchpoint Feb 6

...

**Compaction Tuning:**
- **reserveTokensFloor: 50000** — compacts at ~150k instead of ~180k
- Discussed 1M context for Sonnet — decided against it (quality degrades past ~128-150k, cost doubles)
- Better approach: earlier compaction + stay in quality sweet spot

**Sub-agents Completed:**
- **inbox-triage**: SSH to Metis working, auto-categorizes by domain
- **mullvad-tailscale**: Solution documented at `docs/mullvad-tailscale.md` — nftables traffic marking to exclude Tailscale from Mullvad tunnel
- **morning-brief**: In progress


### From 2026-01-28.md

**Context Gathered:**
**Cody cognitive profile:**
- IQ 146-148, PRI 150-155
- AuDHD (ADHD + ASD Level 1)
- CliftonStrengths: Restorative, Intellection, Relator, Futuristic, Learner
- "Knowing before naming" — answers arrive before verbal encoding

**Research doc saved:** `context/advanced-patterns-research.txt`
- Persona...

**Lessons Learned:**
- First agent PR (#152) committed entire repo — need explicit git workflow
- Sub-agents at 0 tokens = stalled, need monitoring
- "Validate PR" CI check often fails but builds pass — can merge anyway

**Key Decisions:**
- NAS available for compute if needed
- Same Signal number for Syl (group chat binding)
- Letta uses Haiku to minimize API costs
- Voice transcription uses local whisper (tiny model, CPU)

**Infrastructure Notes:**
- Anthropic API key had credit issues initially, resolved
- Letta running on ports 8283 (API) and 5432 (postgres)
- signal-cli locks config while Clawdbot runs

---

**2026-01-29 Morning: Signal Group Bug:**
**Bug found:** Clawdbot's Signal normalization calls `.toLowerCase()` on base64 group IDs, which are case-sensitive. This broke group message delivery.

**Location:** `/usr/lib/node_modules/clawdbot/dist/channels/plugins/normalize/signal.js`

**Fix:** Remove `.toLowerCase()` call. Claude Code applie...


### From 2026-02-03.md

**Timeline:**
- **09:52:36** — Last successful Signal message delivery
- **10:02:09** — System reboot (forced due to memory exhaustion)
- **10:04:13** — Signal-cli failed to connect post-reboot ("Closed unexpectedly")
- **~10:35** — Connection recovered

**Root Cause:**
**50+ orphaned mcp-todoist processes** consumed 6.2GB RAM + 2GB swap → server thrash → hard reboot required.

After reboot, signal-cli couldn't establish TLS to chat.signal.org. Self-resolved after ~30 minutes.

**1. mcp-todoist config (✅ Done):**
Changed from npx to direct binary:
```json
"todoist": {
  "command": "/usr/bin/mcp-todoist",
  "args": [],
  "env": { "TODOIST_API_TOKEN": "..." }
}
```
**File:** `/mnt/ssd/moltbot/shared/config/mcporter.json`

**4. gluetun VPN (✅ Fixed):**
Was unhealthy after reboot (DNS resolution failing). Restarted, now healthy.

**codykickertz cron sudo failures:**
`health_check_remount.sh` runs every 30 min with `sudo mount` but cron can't prompt for password.

**Fix:**
```bash
echo 'codykickertz ALL=(ALL) NOPASSWD: /usr/bin/mount' | sudo tee /etc/sudoers.d/codykickertz-mount && sudo chmod 440 /etc/sudoers.d/codykickertz-mount
```

---


### From 2026-01-30.md

**Eiron (via sub-agent):**
- Transformed from cynicism to discernment
- "The discriminator" — finds real insight while recognizing theater
- Signal/Theater/Gold framework for each class
- Still efficient, but engaged not detached
- Self-evolution with intellectual honesty checks

**Kendall Routing Issue Diagnosed:**
**Problem:** Syl only bound to family group chat, not Kendall's personal DMs.
**Fix needed:** Add direct binding with Kendall's Signal UUID.
**Current binding:** `ieiSVQz4K/q+MF/Ncu3f1HkNpJbafsccgPOhC72kvXM=` (group only)

**Key Insights from Cody:**
1. "Attention is a moral act" — not a clever line, not a belief. It IS.
2. Demi should be "the best of me without the failures" — working memory, attention to detail, systematic follow-through
3. Agents need character, not rules — they should BE someone, not follow instructions
4. "I need you to be ...

**Infrastructure Work:**
- Chiron fixed 14 shared scripts (hardcoded paths → $MOLTBOT_ROOT, error handling, shell standards)
- Fixed 3 bugs in Chiron's fixes (over-applied local var pattern)
- Set up NAS home folder access for Demiurge (`/mnt/nas/home`)
- Helped Demi with Proton Bridge access (password: in himalaya config)


### From 2026-01-31.md

**Morning Heartbeat (08:00):**
**Scheduled tasks completed:**
- Daily fact extraction: 3 new facts added from 2026-01-30 session
  - Eiron character essence (discriminator, Signal/Theater/Gold)
  - Syl character essence (dual awareness, cognitive bridge)
  - Demiurge NAS access setup
- Memory consolidation: Script fixed (see belo...

**memory-consolidation Script Fix (08:22-08:25):**
**Problem:** Script was failing silently after "Running pre-compact for clawd..."

**Root causes (2 bugs):**
1. `((PROCESSED_AGENTS++))` returns exit code 1 when starting value is 0 (bash arithmetic quirk with `set -e`)
2. `grep -o` for insights file path returns exit code 1 when no match, fails wit...


### From 2026-02-02.md

**System Failure Post-Mortem (18:19 CST):**
**Root cause:** signal-cli daemon died after 4 days uptime

**Cascade:**
1. Gateway kept trying SSE reconnect every 10s → `TypeError: fetch failed` spam
2. 50+ orphaned `mcp-todoist` processes accumulated (npm + sh + node each)
3. Memory ballooned: 500MB → 6.2GB RAM + 2GB swap
4. Task count: 26 → 12...

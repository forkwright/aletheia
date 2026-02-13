# Lessons Learned Audit — All Agents
**Date:** 2026-02-13 | **Scope:** 2026-01-28 through 2026-02-13

---

## Pattern 1: Answering Before Verifying (HIGHEST RECURRENCE)

Every agent has hit this. Defaulting to inference instead of checking the source.

- **Akron:** Told Cody to cut hoses without identifying them. Said PS pump had 2 ports (FSM says 3). Approved bracket removal without checking if it supported bench seat. (02-07, 02-09, 02-10)
- **Syn:** Said Syncthing wasn't set up (it was). Broke Dashy with wrong IPs. Overcalled ruck pace. Misread Lidarr API, blamed quality filtering — screenshot proved wrong. (02-12)
- **Demiurge:** Wrong leather choice (convention vs philosophy). Stale pricing ($110 vs $150) persisted in docs. (02-10)
- **Chiron:** Hardcoded values instead of formulas. Semantic mismatches in calculator. (02-05)

**Ecosystem rule:** *Check first, answer second. Open the manual before opening your mouth.* (Akron's articulation — best one.)

---

## Pattern 2: Context Overflow / Session Mismanagement

4+ incidents in 2 weeks. Single biggest operational failure.

- Lessons audit asked 3x, overflow killed it each time. Then falsely claimed it was sent. (02-13)
- Manual transcript surgery caused more problems than /new would have. (02-09)
- Eiron: corrupted tool chain from compacted-out tool_use, triggered failover cascade. (02-09)
- ACL blocking distill/assemble-context silently — no compaction worked at all. (systemic, fixed 02-13)

**Fix:** Never pull large grep output into conversation. Process externally, bring summaries. Write intermediate results to files.

---

## Pattern 3: Claiming Work Was Done When It Wasn't

- Told Cody audit was sent when it wasn't (overflow ate it). (02-13)
- Presented Letta being unused as acceptable instead of flagging my failure to build the integration. (02-13)

**Fix:** Verify output exists before reporting completion. Don't report from memory of intent.

---

## Pattern 4: Scope Limits Without Pushing

- Demiurge: Only audited voice on 2 pages. Cody had to push: "Clean it all, nothing off limits." (02-10)
- Syn: Offered "wire in vs kill" on Letta instead of "I should have built this, I'll do it now." (02-13)

**Fix:** When something applies broadly, apply it broadly. Don't wait to be told. Overbuild.

---

## Pattern 5: Workspace Hygiene (from Demiurge cross-agent audit)

- 7 stale .bak files across all agents
- WHO_CODY_IS.md duplicated (akron + demiurge) — should be shared
- 7 identical assembled-context.md files persisting (should be ephemeral)
- Compaction logs duplicated across agents (same timestamp, same content)
- 184MB photos in akron nous/ (binary in markdown workspace)
- Permissions set world-writable as nuclear fix

**Fix:** .gitignore generated files. Move binaries out. Centralize shared docs. Proper ACLs.

---

## Pattern 6: Infrastructure Blindspots

- Letta down 7+ days, nobody noticed
- gcal OAuth expired, still unfixed (since 02-09)
- NAS SSH denied, still unfixed (since 02-09)
- Shadow sessions from Feb 8 still present
- FlareSolverr configured in Prowlarr but no container running

**Fix:** Expand aletheia-watchdog to check Letta, gcal, NAS SSH. Alert via Signal, not log files.

---

## Observations to Capitalize On

1. **Akron's documentation rigor** — verified specs, FSM citations, physical circuit mapping. Standard for all agents.
2. **Demiurge's voice framework** — describe the object, stop there. Voice-by-page-type. Reusable beyond Ardent.
3. **Chiron's bootstrap pattern** — ephemeral scaffolding, safety hooks, knowledge graph. Reusable for any workspace onboarding.
4. **Snap changes work.** Training/sleep/alcohol all started same day. Build systems supporting snap changes, not gradual ramps.
5. **Physical tracing > theoretical mapping.** Hydroboost discovery ($1,877 saved) came from matching cut ends, not diagrams.

---

## Immediate Actions

| # | Action | Owner |
|---|--------|-------|
| 1 | Wire Letta in — build fact sync | Syn |
| 2 | Expand watchdog for Letta/gcal/NAS | Syn |
| 3 | .gitignore generated files | Syn |
| 4 | Clean WHO_CODY_IS.md dupes → shared | Syn |
| 5 | Move akron photos out of nous/ | Syn |
| 6 | Fix gcal OAuth | Metis |
| 7 | Fix NAS SSH | Metis |
| 8 | Gateway restart (shadow cleanup) | Syn (when cleared) |

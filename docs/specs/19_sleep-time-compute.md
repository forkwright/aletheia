# Spec: Sleep-Time Compute — Reflective Memory

**Status:** Draft
**Author:** Syn
**Date:** 2026-02-21
**Source:** Gap Analysis F-11, Letta's sleep-time multi-agent pattern

---

## Problem

Memory extraction is entirely real-time — fast Haiku pass during distillation, under time pressure, on the full conversation at once. Shallow patterns are captured, deep patterns are missed. No agent revisits past conversations. No agent consolidates conflicting memories. No agent notices recurring mistakes across sessions.

Prosoche monitors external signals but never turns inward. Idle time is wasted time.

---

## Design

### Phase 1: Nightly Reflection Pipeline

Extend the existing consolidation cron with a reflection phase.

**Selection** — which nous had meaningful activity (10+ human messages in last 24h, not already reflected today).

**Reflection prompt** — distinct from extraction. Looks for:
1. **Patterns** across messages — recurring themes, evolving opinions
2. **Contradictions** — information that conflicts with known facts
3. **Corrections** — wrong info given and later corrected
4. **Implicit preferences** — things the user clearly prefers but didn't state
5. **Relationships** — entity connections strengthened by context
6. **Unresolved threads** — questions asked but never answered

**Actions on output:**
- Patterns (confidence > 0.7) → stored as memories tagged `source: reflection`
- Contradictions → old memory demoted, resolution stored
- Corrections → original marked corrected, correction stored
- Implicit preferences (confidence > 0.8) → preference memories
- Relationships → Neo4j edges created/strengthened
- Unresolved threads → added to agent's working state

**Receipt** — `reflection_log` table tracking messages reviewed, findings per category, tokens used, duration.

### Phase 2: Multi-Session Reflection

Weekly pass over the last N distillation summaries (not raw messages):
- Cross-session trajectory patterns ("focused on X all week")
- Topic drift detection ("stopped asking about Y")
- Weekly digest memory capturing the arc

### Phase 3: Self-Assessment Integration

Reflection feeds competence tracking:
- Correction frequency → calibration signal
- Unresolved thread accumulation → attention gap signal
- Missed patterns → training signal for extraction prompts

---

## Implementation Order

| Phase | What | Effort | Impact |
|-------|------|--------|--------|
| **1** | Nightly reflection pipeline | Medium | High — deeper memory than real-time extraction |
| **2** | Multi-session cross-temporal reflection | Small | Medium — weekly trajectory awareness |
| **3** | Self-assessment integration | Small | Medium — closes the feedback loop |

---

## Cost

~10-30K tokens per agent per night to Haiku. Cents per run. One caught contradiction outweighs the cost.

---

## Success Criteria

- Every agent with daily activity gets a nightly reflection pass
- Reflection catches corrections and contradictions real-time extraction misses
- Memory quality measurably improves (fewer stale/conflicting entries)
- Reflection log provides visibility into autonomous learning
- Cost under $1/day total across all agents

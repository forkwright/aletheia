# Spec: Sleep-Time Compute — Reflective Memory

**Status:** Phase 1-3 implemented (PR pending)
**Author:** Syn
**Date:** 2026-02-21
**Source:** Gap Analysis F-11, Letta's sleep-time multi-agent pattern
**Spec:** 19

---

## Problem

Memory extraction is entirely real-time — fast Haiku pass during distillation, under time pressure, on the full conversation at once. Shallow patterns are captured, deep patterns are missed. No agent revisits past conversations. No agent consolidates conflicting memories. No agent notices recurring mistakes across sessions.

Prosoche monitors external signals but never turns inward. Idle time is wasted time.

---

## Design

### Phase 1: Nightly Reflection Pipeline ✅

**Selection** — `getActiveSessionsSince()` finds sessions with ≥N human messages in the lookback window. Skips if reflection already ran within the window.

**Reflection prompt** — purpose-built for deep pattern extraction. Distinct from real-time extraction. Looks for:
1. **Patterns** across messages — recurring themes, evolving opinions (≥2 instances)
2. **Contradictions** — information that conflicts with known facts (both sides cited)
3. **Corrections** — wrong info given and later corrected (WRONG → RIGHT format)
4. **Implicit preferences** — undeclared but consistent preferences (≥3 instances required)
5. **Relationships** — entity connections as (subject, relationship, object) triples
6. **Unresolved threads** — questions asked but never answered

**Confidence gating** — each finding tagged [HIGH], [MEDIUM], or [LOW]:
- Patterns/contradictions/corrections/relationships: HIGH + MEDIUM stored
- Preferences: HIGH only (higher bar to avoid false positives)
- All findings tagged with `[reflection:category]` source prefix

**Existing memory injection** — optional `existingMemories` passed to the prompt for cross-reference contradiction detection.

**Chunking** — large message sets split by token budget, reflected per-chunk, then merged with deduplication.

**Schema** — `reflection_log` table (migration v15) records every run: sessions reviewed, messages reviewed, findings by category, memories stored, tokens used, duration, model, errors.

**Built-in cron command** — `reflection:nightly` registered on CronScheduler. Config example:
```json
{ "id": "reflection", "command": "reflection:nightly", "schedule": "at 03:00" }
```

### Phase 2: Multi-Session Reflection ✅

Weekly pass over distillation summaries (not raw messages):
- `getDistillationSummaries()` fetches assistant messages containing "Distillation #" within the lookback window
- **Trajectory** — how focus shifted across the week
- **Topic drift** — things discussed then dropped
- **Weekly patterns** — recurring behavioral patterns
- **Unresolved arcs** — multi-session threads without conclusion

Built-in cron command: `reflection:weekly`. Config example:
```json
{ "id": "weekly-reflection", "command": "reflection:weekly", "schedule": "0 4 * * 0" }
```

### Phase 3: Self-Assessment Integration ✅

`computeSelfAssessment()` derives calibration signals from reflection history:
- **Correction rate** — corrections per session (lower = more calibrated)
- **Unresolved rate** — unresolved threads per session (lower = better attention)
- **Contradiction count** — total contradictions detected (memory quality signal)
- **Trend** — `improving | stable | degrading | insufficient_data` — compares first half vs second half of recent reflections

API: `GET /api/reflection/:nousId/assessment`

---

## Key Files

| File | Purpose |
|------|---------|
| `src/distillation/reflect.ts` | Reflection engine — nightly, weekly, self-assessment |
| `src/distillation/reflect.test.ts` | 17 tests covering all 3 phases |
| `src/daemon/reflection-cron.ts` | Cron wrappers for nightly + weekly reflection |
| `src/daemon/cron.ts` | Built-in command registration (`registerCommand`) |
| `src/mneme/schema.ts` | Migration v15 — `reflection_log` table |
| `src/mneme/store.ts` | Store methods: reflection log, distillation summaries, active sessions |
| `src/pylon/server.ts` | API endpoints for reflection log + assessment |
| `src/auth/rbac.ts` | RBAC + route normalization for reflection endpoints |

---

## API Endpoints

| Method | Path | Description |
|--------|------|-------------|
| GET | `/api/reflection/:nousId` | Reflection log (limit param) |
| GET | `/api/reflection/:nousId/latest` | Most recent reflection |
| GET | `/api/reflection/:nousId/assessment` | Self-assessment scores + trend |

---

## Cost

~10-30K tokens per agent per night (Haiku). Weekly reflection is lighter (~5-15K tokens over summaries). Total: under $1/day across all agents. One caught contradiction outweighs the cost.

---

## Success Criteria

- [x] Every agent with daily activity gets a nightly reflection pass
- [x] Reflection catches corrections and contradictions real-time extraction misses
- [x] High-confidence findings automatically stored in long-term memory
- [x] Reflection log provides visibility into autonomous learning
- [x] Self-assessment tracks calibration trend over time
- [x] Weekly reflection detects trajectory patterns across sessions
- [x] All findings confidence-gated to prevent memory pollution
- [ ] Cost verified under $1/day total (needs production deployment)

---

## References

- [Gap Analysis F-11](/docs/specs/17_unified-gap-analysis.md) — Sleep-time compute feature
- [Letta sleep-time multi-agent pattern](https://www.letta.com/) — Inspiration
- [Distillation pipeline](/infrastructure/runtime/src/distillation/pipeline.ts) — Existing extraction/summarization
- [Cron scheduler](/infrastructure/runtime/src/daemon/cron.ts) — Job scheduling infrastructure

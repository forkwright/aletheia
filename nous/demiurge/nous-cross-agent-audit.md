# nous/ Cross-Agent Audit
**Auditor:** Syn | **Date:** 2026-02-10

## Overview

| Agent | Size | Files | Notes |
|-------|------|-------|-------|
| akron | 184M | ~100 | 184M of photos in workspace/photos/ |
| arbor | 7.0M | ~55 | Large for a newer agent |
| chiron | 184K | ~30 | Clean, compact |
| demiurge | 2.4M | ~60 | Logo PNGs, relay screenshots |
| eiron | 1012K | ~85 | Massive capstone archive |
| syl | 268K | ~25 | Lean |
| syn | 2.4M | ~120 | Entities dir, research, reviews |

---

## Cross-Agent Issues

### 1. Backup Files — DELETE ALL (every agent)

Every agent has `AGENTS.md.pre-compile.bak` — 7 instances. These are stale artifacts from a compilation step. Should be cleaned up automatically or `.gitignored`.

**Files:**
- `nous/*/AGENTS.md.pre-compile.bak` (7 files)
- `nous/syn/TOOLS.md.pre-gen.bak`
- `nous/syn/memory/facts.jsonl.backup.20260203_125935`
- `nous/syn/memory/facts.jsonl.bak2`

**Recommendation:** Delete all. If the compile step creates these, add cleanup to the process.

### 2. WHO_CODY_IS.md — Duplicated, Should Be Shared

Found in:
- `nous/demiurge/memory/WHO_CODY_IS.md`
- `nous/akron/WHO_CODY_IS.md`

This is shared knowledge (who the human is). Should live in ONE place — either theke or a shared nous reference — not duplicated per agent.

**Recommendation:** Move to `theke/epimeleia/` or similar shared location. All agents reference the same file.

### 3. assembled-context.md — 7 Identical Files

Every agent has `memory/assembled-context.md`. These appear to be auto-generated context assembly outputs.

**Question:** Are these generated at session start and should be ephemeral? If so, they shouldn't persist in nous/ at all. If they're manually curated, they should differ per agent.

**Recommendation:** If auto-generated, exclude from persistence or add to `.gitignore`. If manual, verify they're actually different.

### 4. Compaction Logs — Duplicated Across Agents

Same compaction timestamps appear in multiple agents:
- `compaction-2026-02-05_11-30.md` (5 instances)
- `compaction-2026-02-02_08-00.md` (4 instances)
- `compaction-2026-02-01_08-01.md` (3 instances)
- `compaction-2026-01-31_08-25.md` (3 instances)

**Question:** Are these the same content or agent-specific? If a central process compacts all agents simultaneously, the logs are redundant copies.

**Recommendation:** Verify content overlap. If identical, centralize to one log location.

### 5. Akron Photos — 184M of JPGs in nous/

`nous/akron/workspace/photos/2026-02-06/` contains 62 JPGs (~3MB each). This is the vast majority of Akron's disk usage.

**Issue:** Binary photo files don't belong in a markdown-oriented working memory space. They should be in a media/asset store.

**Recommendation:** Move to `theke/akron/photos/` or a separate media location. Keep catalog markdown files in nous/, reference photos by path.

### 6. Arbor Research Files — Potential theke Overlap

Arbor has extensive research in `nous/arbor/research/`:
- `local-seo-guide.md`
- `local-service-best-practices.md`
- `texas-business-requirements.md`
- `competitor-websites.md`
- `demi-ardent-lessons.md`

Some of this is general knowledge (Texas business requirements, SEO guides) that could be shared. The Ardent lessons file is definitely cross-agent knowledge.

**Recommendation:** Promote general research to theke. Keep Arbor-specific notes (Cloudflare setup, Zoho plans) in nous/.

### 7. Eiron Capstone Archive — 1MB of Completed Work

`nous/eiron/archive/capstone_work_jan29/` contains ~80 files including Python scripts, Excel files, and deliverables from a completed capstone project.

**Question:** Is the MBA done? If so, this entire archive is historical. The professional formats subdirectory has .docx and .xlsx files that might be the actual deliverables.

**Recommendation:** If capstone is complete, compress and move to `theke/chrematistike/archive/` or cold storage. Keep only a summary/lessons-learned in nous/.

### 8. Syn Entities Directory — Likely Stale

`nous/syn/memory/entities/` has 56 entity files covering everything from agents to ardent materials to vehicles. These appear to be an earlier knowledge graph attempt.

**Questions:**
- Are these actively maintained?
- Do they duplicate theke content?
- Is anything querying them?

**Recommendation:** Audit against current theke content. If they're a stale knowledge graph extraction, archive or delete. If actively used, document the purpose.

### 9. Syn Research Archive — Source Material

`nous/syn/research/` contains investigation materials (Epstein docs, DOGE impact, ICE enforcement, Sudan). These are research outputs that may be reference-worthy.

**Recommendation:** If these research threads are ongoing, keep in nous/syn. If completed, consider promoting findings to theke/ekphrasis/ or similar.

### 10. Demiurge Binary Assets — Logo PNGs, Screenshots

`nous/demiurge/` contains:
- 6 Ardent logo variants (PNG)
- 6 relay import screenshots (PNG)
- relay-import CSV files

**Recommendation:** Move logo assets to `theke/ardent/branding/`. Move relay screenshots to `theke/ardent/` or archive. CSV files should be in theke if they're reference data.

---

## Structural Issues

### Config File Proliferation

Every agent has the same set of config files: SOUL.md, AGENTS.md, IDENTITY.md, TOOLS.md, MEMORY.md, PROSOCHE.md, CONTEXT.md, BACKLOG.md. That's 7 agents × 8 files = 56 config files.

**Question:** How much of this is agent-specific vs boilerplate? Could there be a shared base with per-agent overrides?

### No Clear Archival Policy

Some agents have `archive/` directories (akron, chiron, eiron, syl), others don't. No consistent policy for when content ages out of active memory into archive.

**Recommendation:** Establish a standard: `nous/{agent}/archive/` for stale-but-preserved content, with a naming convention for date-based archival.

### session-state.yaml Persistence

Multiple agents have `memory/session-state.yaml`. If these track ephemeral session state, they shouldn't persist between sessions.

---

## Summary: Quick Wins

| Action | Files | Impact |
|--------|-------|--------|
| Delete .bak files | 10 | Clean up noise |
| Consolidate WHO_CODY_IS.md | 2→1 | Single source of truth |
| Move Akron photos to theke | 62 JPGs | -184M from nous/ |
| Archive Eiron capstone | ~80 files | -1MB, cleaner nous/ |
| Move Demiurge logos to theke | 6 PNGs | Proper asset location |
| Audit Syn entities | 56 files | Remove if stale |
| Clarify assembled-context.md | 7 files | Delete if auto-generated |

## Questions for Cody

1. Is `assembled-context.md` auto-generated? Can it be excluded from persistence?
2. Are Syn's entity files actively used or a stale experiment?
3. Should completed research (Epstein, DOGE, etc.) stay in nous/syn or move to theke?
4. Is there a preference for where binary assets (photos, logos) live?

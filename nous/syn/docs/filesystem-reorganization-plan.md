# Filesystem Reorganization Plan

**Date:** 2026-02-05
**Author:** Syn
**Status:** Draft for Cody's review

---

## Executive Summary

Reorganize the moltbot filesystem from Greek-named philosophical abstractions to a machine-readable, flat, purpose-driven structure. Establish server as single source of truth, then enable bidirectional Syncthing to Metis.

---

## Current State Audit

### Size Analysis

| Location | Size | Files | Notes |
|----------|------|-------|-------|
| `dianoia/autarkeia/` | 28 GB | 73,315 | Vehicle (15GB), preparedness, episteme |
| `clawd/crewai/.venv` | 5.4 GB | ~15,000 | Python venv - should NOT sync |
| `dianoia/poiesis/` | 2.5 GB | 12,500 | Imaging (2.3GB), CAD, handcraft |
| `dianoia/chrematistike/` | 1.3 GB | 6,541 | MBA files |
| `clawd/mba/` | 1.3 GB | ~6,000 | MBA files (DUPLICATE) |
| `dianoia/anamnesis/` | 911 MB | 15,223 | Old configs, scripts, homelab |
| `clawd/work/` | 477 MB | ~2,000 | Summus work projects |
| `demiurge/` | 200 MB | 2,146 | Ardent leather assets |
| `arbor/` | 46 MB | 2,093 | A2Z tree site |

### Problems Identified

1. **Duplication:** MBA exists in both `dianoia/chrematistike` AND `clawd/mba`
2. **Greek naming overhead:** Agents don't benefit from philosophical abstraction - it's friction
3. **Empty directories:** `dianoia/{sophia,techne,summus,state,scripts,processed}` = 0 files each
4. **Backup cruft:** 12+ `facts.jsonl.backup.*` files
5. **Massive venv:** 5.4GB Python environment shouldn't sync anywhere
6. **Unclear ownership:** Some files are "yours" (Cody works on them), some are "ours" (agents operational)

### Current Greek → Plain English Mapping

| Greek | Pronunciation | Plain | Contents |
|-------|---------------|-------|----------|
| autarkeia | ow-TAR-kay-ah | self-reliance | Vehicle, radio, preparedness, personal |
| chrematistike | kray-mah-tis-ti-KAY | mba | MBA coursework |
| poiesis | poy-AY-sis | making/media | Imaging, CAD, handcraft, photography |
| anamnesis | ah-NAM-nee-sis | memory | Old configs, system setup (legacy) |
| episteme | eh-pis-TAY-may | reference | Manuals, knowledge base |
| praxis | PRAK-sis | practice | Applied knowledge (vehicle, radio) |
| energeia | en-AIR-gay-ah | automation | Active projects, integrations |

---

## Research: Best Practices

### Machine-Readable Directory Principles

From monorepo and knowledge base research:

1. **Flat over deep:** 2-3 levels max for discoverability
2. **Domain over technology:** Group by purpose, not implementation
3. **Ownership clarity:** Clear who/what is responsible for each area
4. **Sync-aware:** Separate synced content from local-only (venvs, caches)
5. **Consistent naming:** lowercase, hyphens, no spaces, no special chars

### Key Insight

> "Structuring a [repo] is both a technical and an organizational challenge... You want a structure that works technically and that has organizational meaning."

The organizational meaning shifts: Greek naming served *your* cognition. Now it needs to serve *ours* (agents) + remain accessible to *you* when you want to work directly.

---

## Proposed Structure

```
/mnt/ssd/moltbot/
├── projects/                 # YOUR work - bidirectional sync to Metis
│   ├── mba/                  # ← chrematistike
│   │   ├── sp26/
│   │   ├── fa25/
│   │   └── application/
│   ├── work/                 # ← summus/work
│   ├── ardent/               # ← demiurge/ardent
│   ├── a2z-tree/             # ← arbor/a2z-tree-site
│   ├── personal/             # ← autarkeia/personal, career, portfolio
│   ├── media/                # ← poiesis
│   │   ├── imaging/
│   │   ├── cad/
│   │   └── handcraft/
│   ├── reference/            # ← autarkeia/episteme
│   ├── vehicle/              # ← autarkeia/praxis/vehicle (15GB)
│   ├── preparedness/         # ← autarkeia/praxis/preparedness
│   └── radio/                # ← autarkeia/praxis/radio
│
├── agents/                   # Agent workspaces - selective sync
│   ├── syn/                  # ← clawd (minus projects)
│   │   ├── memory/
│   │   ├── bin/
│   │   ├── config/
│   │   ├── docs/
│   │   └── agent-status/
│   ├── chiron/
│   ├── eiron/
│   ├── syl/
│   ├── demiurge/
│   ├── arbor/
│   └── akron/
│
├── shared/                   # Cross-agent resources (keep as-is)
│   ├── bin/
│   ├── config/
│   ├── schemas/
│   └── ...
│
├── infrastructure/           # System-level configs
│   ├── docker-compose.yml
│   ├── .env
│   └── signal-cli/
│
└── archive/                  # Cold storage
    └── dianoia-legacy/       # Original structure, read-only reference
```

### Sync Strategy

| Directory | Sync to Metis | Notes |
|-----------|---------------|-------|
| `projects/` | ✅ Bidirectional | Your working files |
| `agents/*/memory/` | ✅ Bidirectional | Agent memory files |
| `agents/*/bin/` | ⚠️ Server → Metis | Tools, read-only on Metis |
| `agents/*/.venv/` | ❌ No sync | Local to each machine |
| `shared/` | ✅ Bidirectional | Common tooling |
| `infrastructure/` | ❌ No sync | Server-specific |
| `archive/` | ❌ No sync | Cold storage |

---

## Phased Execution Plan

### Phase 1: Deep Audit & Mapping (Current)
**Goal:** Complete inventory, no changes yet

- [x] Size analysis of all directories
- [x] File count by location
- [x] Identify duplicates
- [x] Map current → proposed locations
- [ ] Verify nothing important in "empty" directories
- [ ] Check Metis structure (when online)

**Deliverable:** This document + your approval to proceed

---

### Phase 2: Server Cleanup
**Goal:** Remove cruft without restructuring

Tasks:
- [ ] Delete empty `dianoia/` directories (sophia, techne, summus, state, scripts, processed)
- [ ] Consolidate `facts.jsonl.backup.*` → keep only latest
- [ ] Archive old `anamnesis/` content → `archive/anamnesis-legacy/`
- [ ] Remove `.ruff_cache`, `__pycache__`, `.pyc` files
- [ ] Document what was removed

**Requires:** Your approval
**Risk:** Low (deletions are empty dirs or obvious cruft)

---

### Phase 3: Structure Creation
**Goal:** Create new directory structure, move files

Tasks:
- [ ] Create `projects/` hierarchy
- [ ] Move MBA files (consolidate duplicates - keep richer version)
- [ ] Move work files
- [ ] Move ardent content (coordinate with Demiurge)
- [ ] Move a2z-tree content (coordinate with Arbor)
- [ ] Move personal/career/portfolio
- [ ] Move media (imaging, cad, handcraft)
- [ ] Move vehicle/preparedness/radio (large - 16GB+)
- [ ] Rename `clawd/` → `agents/syn/`
- [ ] Update all agent workspace paths
- [ ] Update symlinks and configs

**Requires:** Your approval + agent coordination
**Risk:** Medium (path changes affect running systems)

---

### Phase 4: Configuration Updates
**Goal:** Make everything work with new paths

Tasks:
- [ ] Update OpenClaw agent configs
- [ ] Update shared bin scripts ($MOLTBOT_ROOT references)
- [ ] Update cron jobs
- [ ] Update any hardcoded paths in agent files
- [ ] Test all agents respond correctly
- [ ] Verify memory files accessible

**Requires:** System restart, testing
**Risk:** Medium (config errors could break agents)

---

### Phase 5: Syncthing Setup
**Goal:** Establish bidirectional sync with Metis

Tasks:
- [ ] Install/configure Syncthing on server
- [ ] Configure Syncthing on Metis
- [ ] Set up `projects/` as shared folder
- [ ] Set up selective sync for `agents/*/memory/`
- [ ] Configure ignore patterns (venvs, caches, node_modules)
- [ ] Initial sync + verification
- [ ] Document sync architecture

**Requires:** Metis online, your Metis access
**Risk:** Medium (sync conflicts possible during transition)

---

### Phase 6: Metis Cleanup
**Goal:** Clean Metis to match server structure

Tasks:
- [ ] Backup current Metis dianoia/Documents
- [ ] Remove old structure after sync verified
- [ ] Update Metis-local references (if any)
- [ ] Final verification

**Requires:** Previous phases complete
**Risk:** Low (server is source of truth by this point)

---

## Questions RESOLVED

1. ✅ **Vehicle files (15GB):** Keep synced regardless of size
2. ✅ **MBA duplicate:** `clawd/mba` is canonical (newest: Feb 4). `chrematistike/application/` has unique content (essays, resumes, transcripts, VA docs) - will merge
3. ✅ **Agent names:** Yes to `clawd` → `syn`, but NOTIFY CODY BEFORE executing
4. ✅ **anamnesis:** Archive after verifying contents (contains homelab configs, metis setup, old scripts)
5. ⏸️ **Demiurge:** Hold off - he's actively working
6. ✅ **Timeline:** Task-focused, not time-focused

---

## Next Steps

Awaiting your review of this plan. Once approved:
1. I'll proceed with Phase 2 (cleanup)
2. Report back before Phase 3
3. Coordinate with affected agents before any moves that impact them

---

*This is a significant undertaking. Doing it right matters more than doing it fast.*

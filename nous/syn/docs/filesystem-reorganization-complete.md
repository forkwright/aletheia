# Filesystem Reorganization - Complete Plan

**Date:** 2026-02-05
**Status:** Ready for Phase 3 execution

---

## Current State (Post Phase 2)

```
moltbot/                          ~42GB total
├── dianoia/                      31GB (Greek-named project content)
│   ├── autarkeia/                28GB (personal/career/preparedness)
│   │   ├── career/               (job search, interviews, military)
│   │   ├── episteme/             (identity, library, reference)
│   │   ├── praxis/               (family, preparedness, radio, vehicle)
│   │   │   └── vehicle/          15GB (dodge ram, royal enfield, database)
│   │   ├── personal/             (inventory, writing, poetry)
│   │   ├── personal_portfolio/   (github portfolio projects)
│   │   └── immigrate/            (canada/ireland research)
│   ├── poiesis/                  2.5GB (creative/craft work)
│   │   ├── cad/                  (OpenSCAD projects)
│   │   ├── handcraft/            (bindery, joinery, leatherworks)
│   │   ├── imaging/              (Stable Diffusion)
│   │   └── photography/          (darktable, raw photos)
│   ├── apotelesma/               524K (outputs)
│   ├── metaxynoesis/             152K (AI architecture theory)
│   ├── energeia/                 52K (automation)
│   ├── inbox/                    36K
│   └── context/                  12K
├── clawd/                        7.2GB (Syn workspace - orchestrator)
│   ├── crewai/                   5.4GB (.venv - local only, won't sync)
│   ├── mba/                      1.3GB (MBA coursework - canonical)
│   └── work/                     477MB (Summus work files)
├── archive/                      2.2GB (Phase 2 created)
│   ├── anamnesis-legacy/         902MB
│   └── chrematistike-legacy/     1.3GB
├── demiurge/                     200MB (craft agent)
│   ├── ardent-stripe-webhook/    180MB (node_modules)
│   ├── ardent-site/              7MB
│   ├── knowledge/                468KB
│   └── documents/                5.1MB
├── arbor/                        46MB (arborist agent)
│   └── a2z-tree-site/            39MB
├── shared/                       23MB (common tooling)
├── eiron/                        972KB (school agent)
├── akron/                        692KB (preparedness agent)
├── syl/                          204KB (home agent)
├── chiron/                       156KB (work agent)
└── repos/                        64MB
```

---

## Target State

```
moltbot/
├── projects/                     ~32GB (Cody's work - syncs to Metis)
│   ├── vehicle/                  15GB (from autarkeia/praxis/vehicle)
│   ├── craft/                    2.5GB (from poiesis - Demi's domain)
│   │   ├── cad/
│   │   ├── handcraft/            (bindery, joinery, leatherworks)
│   │   ├── imaging/
│   │   └── photography/
│   ├── mba/                      1.3GB (from clawd/mba)
│   ├── work/                     477MB (from clawd/work)
│   ├── career/                   (from autarkeia/career)
│   ├── reference/                (from autarkeia/episteme)
│   ├── personal/                 (from autarkeia/personal)
│   ├── portfolio/                (from autarkeia/personal_portfolio)
│   ├── preparedness/             (from autarkeia/praxis/preparedness)
│   ├── radio/                    (from autarkeia/praxis/radio)
│   ├── family/                   (from autarkeia/praxis/family)
│   ├── immigrate/                (from autarkeia/immigrate)
│   ├── metaxynoesis/             (AI architecture theory)
│   └── energeia/                 (automation tools)
│
├── agents/                       ~7.5GB
│   ├── syn/                      (renamed from clawd, minus projects)
│   │   ├── crewai/               5.4GB (local only - .venv)
│   │   ├── memory/
│   │   ├── docs/
│   │   └── ...
│   ├── chiron/                   156KB (work agent)
│   ├── eiron/                    972KB (school agent)
│   ├── demiurge/                 200MB (craft agent)
│   │   └── ardent-*/             (site, webhook - stay here)
│   ├── syl/                      204KB (home agent)
│   ├── arbor/                    46MB (arborist agent)
│   │   └── a2z-tree-site/
│   └── akron/                    692KB (preparedness agent)
│
├── shared/                       23MB (common tooling - unchanged)
│
├── infrastructure/               (new)
│   ├── repos/                    64MB (from ./repos)
│   ├── signal-cli/               (from ./signal-cli)
│   └── data/                     (from ./data)
│
└── archive/                      2.2GB (legacy content)
    ├── anamnesis-legacy/
    ├── chrematistike-legacy/
    └── dianoia-structure/        (preserve old CLAUDE.md, llms.txt, etc.)
```

---

## Migration Steps

### Phase 3A: Create Structure
```bash
mkdir -p /mnt/ssd/moltbot/projects
mkdir -p /mnt/ssd/moltbot/agents
mkdir -p /mnt/ssd/moltbot/infrastructure
```

### Phase 3B: Move Projects (from dianoia)
```bash
# Vehicle (15GB)
mv dianoia/autarkeia/praxis/vehicle projects/vehicle

# Craft (2.5GB) - Demi's domain
mv dianoia/poiesis projects/craft

# Career & Personal
mv dianoia/autarkeia/career projects/career
mv dianoia/autarkeia/episteme projects/reference
mv dianoia/autarkeia/personal projects/personal
mv dianoia/autarkeia/personal_portfolio projects/portfolio
mv dianoia/autarkeia/immigrate projects/immigrate

# Praxis subdirs
mv dianoia/autarkeia/praxis/preparedness projects/preparedness
mv dianoia/autarkeia/praxis/radio projects/radio
mv dianoia/autarkeia/praxis/family projects/family

# Smaller project dirs
mv dianoia/metaxynoesis projects/metaxynoesis
mv dianoia/energeia projects/energeia
mv dianoia/apotelesma projects/outputs
mv dianoia/inbox projects/inbox
```

### Phase 3C: Move Projects (from clawd)
```bash
mv clawd/mba projects/mba
mv clawd/work projects/work
```

### Phase 3D: Move Agents
```bash
# Move all agent workspaces to agents/
mv chiron agents/
mv eiron agents/
mv demiurge agents/
mv syl agents/
mv arbor agents/
mv akron agents/

# Rename clawd → syn (REQUIRES CONFIG UPDATE)
mv clawd agents/syn
```

### Phase 3E: Infrastructure
```bash
mv repos infrastructure/
mv signal-cli infrastructure/
mv data infrastructure/
```

### Phase 3F: Archive Remaining Dianoia
```bash
# Preserve structure docs
mkdir -p archive/dianoia-structure
cp dianoia/CLAUDE.md archive/dianoia-structure/
cp dianoia/llms.txt archive/dianoia-structure/
cp dianoia/naming_system.md archive/dianoia-structure/
cp dianoia/README.md archive/dianoia-structure/
cp dianoia/CHANGELOG.md archive/dianoia-structure/

# Remove empty dianoia
rm -rf dianoia
```

---

## Config Updates Required (Phase 4)

### OpenClaw Config Changes

**File:** `/mnt/ssd/moltbot/config.yaml` (or wherever it lives)

1. **Agent workspace paths:**
   - `clawd` → `agents/syn`
   - `chiron` → `agents/chiron`
   - etc.

2. **Any hardcoded paths in:**
   - AGENTS.md
   - TOOLS.md
   - shared/bin/* scripts
   - Cron jobs

### Scripts to Update

Check these for `/mnt/ssd/moltbot/clawd` or `/mnt/ssd/moltbot/dianoia`:
```bash
grep -r "moltbot/clawd" shared/bin/
grep -r "moltbot/dianoia" shared/bin/
grep -r "/clawd/" agents/*/
```

---

## Symlink Strategy

Create backwards-compatible symlinks:
```bash
ln -s agents/syn clawd
ln -s projects dianoia  # if needed for scripts
```

---

## Syncthing Setup (Phase 5)

**Folders to sync bidirectionally:**
| Local Path | Metis Path | Size |
|------------|------------|------|
| projects/ | ~/projects/ | ~32GB |
| agents/syn/memory/ | (one-way) | small |

**Folders to exclude:**
- agents/syn/crewai/.venv (5.4GB Python venv)
- Any node_modules/
- .git/ directories (optional)

---

## Demiurge Coordination

**Verified locations:**
- `/mnt/ssd/moltbot/demiurge/` - main workspace (200MB)
- `/mnt/ssd/moltbot/dianoia/poiesis/` → will become `projects/craft/`
- `/mnt/nas/home/` - NAS access via symlink

**Active projects:**
- ardent-site/
- ardent-stripe-webhook/
- documents/
- knowledge/

**Domain mapping:**
- Demi works in: `projects/craft/` (handcraft, cad, imaging)
- Demi's agent workspace: `agents/demiurge/`

---

## Rollback Plan

If something breaks:
1. Archive contains original anamnesis and chrematistike
2. Git history in autarkeia/.git preserves file history
3. Symlinks provide backwards compatibility
4. Config can be reverted from gateway backup

---

## Verification Checklist

After migration:
- [ ] All agents can start (`openclaw gateway restart`)
- [ ] Signal messaging works
- [ ] Memory files accessible
- [ ] Project symlinks work
- [ ] Syncthing connects (Phase 5)
- [ ] Metis can access synced folders

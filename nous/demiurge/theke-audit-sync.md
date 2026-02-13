# Theke Vault Sync Audit - Binary Bloat & Exclusions Analysis

**Audit Date:** February 10, 2026  
**Total Vault Size:** ~15GB  
**Markdown Content:** ~10.8MB  

## Executive Summary

The current exclusion list is **insufficient** for BOOX sync. With current exclusions, **1.9GB would sync** - far too large for an e-reader. The primary culprit is `akron/royal_enfield_gt650/manuals/` (1.7GB) which is NOT currently excluded.

Several excluded folders contain valuable markdown content that could enhance BOOX utility for reference reading.

## Directory Size Breakdown

### Major Domains (with current exclusions noted)

| Domain | Size | Status | Primary Content |
|--------|------|--------|----------------|
| mouseion | 5.1G | ❌ library excluded (5.0G) | Books, references |
| akron | 3.4G | ⚠️ partial exclusions | Vehicle manuals, maintenance |
| poiesis | 2.5G | ⚠️ partial exclusions | CAD, imaging, handcraft |
| chrematistike | 1.4G | ⚠️ partial exclusions | Academic coursework |
| autarkeia | 1.2G | ❌ fully excluded | Survival, tactical, medical |
| summus | 406M | ❌ fully excluded | Investment tracking, analysis |
| portfolio | 343M | ❌ fully excluded | Project portfolios |

## Critical Finding: Missing Exclusions

### 1. akron/royal_enfield_gt650/manuals/ - **1.7GB** 
**Status:** NOT EXCLUDED - This is the biggest sync problem!
- Contains massive PDF service manuals
- Should be excluded immediately

### 2. akron/royal_enfield_gt650/manual_processing/ - **50MB**
**Status:** NOT EXCLUDED
- Binary processing artifacts
- Should be excluded

### 3. chrematistike/sp26/ - **78MB**
**Status:** NOT EXCLUDED
- Current coursework (Spring 2026)
- Contains class materials, presentations, homework
- **Recommendation:** Keep for now (active academic use), review after semester

## Vault Pollution Found

### Development Artifacts (Should NOT sync)
```
/mnt/ssd/aletheia/theke/poiesis/.git
/mnt/ssd/aletheia/theke/poiesis/imaging/archive/local-sd-setup/fastsdcpu/.git
/mnt/ssd/aletheia/theke/poiesis/imaging/archive/local-sd-setup/fastsdcpu/venv
/mnt/ssd/aletheia/theke/mouseion/reference/OfflineDocs/research/topics_of_interest/smol_agents_setup/.venv
/mnt/ssd/aletheia/theke/portfolio/ai-infrastructure-toolkit/.git
/mnt/ssd/aletheia/theke/portfolio/infrastructure-automation-toolkit/.git
/mnt/ssd/aletheia/theke/portfolio/nasa-mars-sol200-analysis/.git
/mnt/ssd/aletheia/theke/portfolio/nasa-mars-sol200-analysis/.venv
/mnt/ssd/aletheia/theke/portfolio/profile-repo/.git
```

**Recommendation:** Add global exclusions for `*.git/`, `*venv/`, `*.venv/`, `__pycache__/`, `node_modules/`

## Revised Exclusion Recommendations

### ✅ Keep Current Exclusions (Correct)
1. mouseion/library (5.0GB - books)
2. poiesis/imaging (2.3GB - image processing)
3. chrematistike/fa25 (1.3GB - old coursework) 
4. akron/database (517MB - vehicle databases)
5. akron/dodge_ram_2500_1997/photos
6. akron/dodge_ram_2500_1997/manual_processing
7. akron/enfield/manual_processing
8. akron/enfield/manuals
9. poiesis/cad (111MB - CAD files)

### ➕ Add New Exclusions (Critical)
14. **akron/royal_enfield_gt650/manuals/** (1.7GB)
15. **akron/royal_enfield_gt650/manual_processing/** (50MB)
16. **Global patterns:** `.git/`, `venv/`, `.venv/`, `__pycache__/`, `node_modules/`

### ❓ Reconsider Current Exclusions (False Positives)

#### autarkeia (Currently fully excluded - 1.2GB total)
- **Markdown content:** 272KB only
- **Contains:** Survival guides, medical procedures, radio protocols, tactical training
- **Recommendation:** Exclude binary files but allow markdown sync
- **Useful for BOOX:** Emergency procedures, reference guides

#### summus (Currently fully excluded - 406MB total) 
- **Markdown content:** 3.3MB (403 files)
- **Contains:** Investment analysis, prospect tracking, data landscape
- **Recommendation:** Exclude binary files but allow markdown sync
- **Useful for BOOX:** Investment research, meeting notes, analysis

#### poiesis/handcraft (Currently fully excluded - 107MB total)
- **Markdown content:** 420KB
- **Contains:** Bookbinding, joinery, leatherworking guides
- **Recommendation:** Exclude binary files but allow markdown sync  
- **Useful for BOOX:** Craft reference materials, project planning

### 📱 Keep Current Non-Exclusions
- chrematistike/sp26/ (78MB) - current coursework, useful for semester

## Projected Sync Footprint

### With CURRENT exclusions: **1.9GB** ❌ (Too large)

### With RECOMMENDED exclusions:

| Content Type | Size | Details |
|--------------|------|---------|
| Markdown files | ~9.5MB | Core knowledge content |
| chrematistike/sp26 | 78MB | Current coursework |
| Small binary assets | ~20MB | Diagrams, small PDFs |
| **Total Projected** | **~110MB** ✅ | Suitable for BOOX |

## Refined Exclusion Strategy

### Full Path Exclusions (Add to current list)
```
akron/royal_enfield_gt650/manuals/
akron/royal_enfield_gt650/manual_processing/
```

### Pattern-Based Global Exclusions (New)
```
.git/
venv/  
.venv/
__pycache__/
node_modules/
*.sqlite
*.db  
```

### Partial Exclusions (Replace current full exclusions)
```
# Instead of excluding all of autarkeia/, exclude:
autarkeia/**/*.pdf
autarkeia/**/*.mp3
autarkeia/**/*.mp4

# Instead of excluding all of summus/, exclude:  
summus/**/*.xlsx
summus/**/*.pptx
summus/**/*.db

# Instead of excluding all of poiesis/handcraft/, exclude:
poiesis/handcraft/**/*.pdf
poiesis/handcraft/shared/marketing_class_materials/
```

## Implementation Priority

1. **IMMEDIATE:** Add akron royal_enfield exclusions (saves 1.7GB)
2. **HIGH:** Add global patterns for dev artifacts
3. **MEDIUM:** Implement partial exclusions for autarkeia, summus, handcraft
4. **LOW:** Clean up .git directories and venvs (vault hygiene)

## Final Sync Footprint Estimate

With all recommendations: **~110MB total**
- Markdown content: 9.5MB
- chrematistike/sp26: 78MB  
- autarkeia markdown: 272KB
- summus markdown: 3.3MB
- handcraft markdown: 420KB
- Small assets: ~20MB

This achieves the goal of syncing valuable knowledge while keeping size manageable for BOOX storage and bandwidth.
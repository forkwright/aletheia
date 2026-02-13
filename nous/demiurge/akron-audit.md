# Theke/Akron Content Audit
**Auditor:** Akron  
**Date:** 2026-02-10  
**Domain:** theke/akron/ (vehicle docs)  
**Total:** ~13,500 files, ~3.6 GB

---

## Summary

The vault is dominated by two reference corpora (FSM HTML, RE PDFs) that belong here. The problems are: duplicated photos, dead/empty directories, stale processing artifacts, and one entire subdirectory (overland_teardrop) that MEMORY.md explicitly says was a fantasy that's not part of the build plan.

---

## File-by-File Audit

### 1. `README.md`
- **Belongs in theke?** ✅ Yes — vault index
- **Redundant?** No
- **Stale?** Slightly — contains Dataview queries (Obsidian-specific) but structure is current
- **Auto-maintainable?** Could be generated from directory listing
- **Single job?** Yes
- **Recommendation:** **KEEP** — update if directory structure changes

---

### 2. `dodge_ram_2500_1997/manuals/service_manual_html/` (12,976 files, 201 MB)
- **Belongs in theke?** ✅ Yes — this IS the FSM, the canonical reference for all vehicle work
- **Redundant?** No — unique source of truth
- **Stale?** No — factory data doesn't change
- **Auto-maintainable?** N/A — static reference material
- **Single job?** Yes — FSM lookup
- **Recommendation:** **KEEP** — this is the most important thing in the vault. Never touch it.

---

### 3. `dodge_ram_2500_1997/photos/` (183 files, 523 MB)
- **Belongs in theke?** ✅ Yes — vehicle documentation photos
- **Redundant?** ⚠️ YES — `database/photos_renamed/` contains 172 renamed copies of these same photos
- **Stale?** No — ongoing build documentation
- **Auto-maintainable?** No
- **Single job?** Yes
- **Recommendation:** **KEEP** but see `database/photos_renamed` below for dedup

---

### 4. `dodge_ram_2500_1997/photos/PHOTO_INDEX.md`
- **Belongs in theke?** ✅ Yes — photo catalog
- **Recommendation:** **KEEP**

---

### 5. `dodge_ram_2500_1997/documentation/procedures/` (9 files)
- **Belongs in theke?** ✅ Yes — verified procedures with torque specs
- **Redundant?** No — these are Akron-specific write-ups referencing the FSM
- **Stale?** Partially — `08_10a-short-diagnosis.md` references the wrong fuse (10A illum) which was corrected to fuse #18 park lamp 15A in MEMORY.md
- **Single job?** Yes per file
- **Recommendation:** **KEEP** — correct `08_10a-short-diagnosis.md` filename and content to match fuse #18 reality

---

### 6. `dodge_ram_2500_1997/documentation/reference/` (6 files + manuals/)
- **Belongs in theke?** ✅ Yes — reference specs, standards, component data
- **Redundant?** `07_torque-specifications.md` partially overlaps MEMORY.md verified specs table — but theke version is the full reference, MEMORY is the quick-lookup
- **Stale?** No
- **Single job?** Yes
- **Recommendation:** **KEEP** — ensure torque specs stay in sync with MEMORY.md

---

### 7. `dodge_ram_2500_1997/documentation/reference/manuals/` (10 files, PDFs + indexes)
- **Belongs in theke?** ✅ Yes — aftermarket part install manuals (Borgeson, Renogy, Garmin)
- **Redundant?** No
- **Stale?** No — these are permanent reference
- **Recommendation:** **KEEP**

---

### 8. `dodge_ram_2500_1997/documentation/archive/` (2 subdirs)
- **Belongs in theke?** ⚠️ Questionable — `extraction_tracking_2025/` is process documentation about data extraction that happened once. `consolidated-2026-02-06/` is superseded build plans and trackers.
- **Redundant?** Yes — content captured in current procedures, MEMORY.md, and workspace/
- **Stale?** Yes — archive by definition
- **Recommendation:** **KEEP but flag** — archives are fine to retain for provenance, but don't index them for search. They're already in `archive/` which is appropriate.

---

### 9. `dodge_ram_2500_1997/documentation/archive/electrical-system-map.md`
- **Belongs in theke?** ⚠️ Should be in archive subdir, not loose in archive root
- **Recommendation:** **MOVE** into `consolidated-2026-02-06/` or a dated archive subfolder

---

### 10. `dodge_ram_2500_1997/documentation/external_sources/validation_notes.md`
- **Belongs in theke?** Marginal — validation notes about data quality from external sources
- **Stale?** Likely — one-time validation
- **Recommendation:** **MOVE to archive/** — useful once, now just sits

---

### 11. `dodge_ram_2500_1997/context/` (EMPTY)
- **Recommendation:** **DELETE** — empty directory, no purpose

---

### 12. `dodge_ram_2500_1997/electrical_simulation/` (7 files)
- **Belongs in theke?** ⚠️ Mixed — the SPICE circuit file and SVG diagram are reference material. The Python validator and staging checklist are working files.
- **Redundant?** No
- **Stale?** Possibly — depends on whether the three-system architecture is still current
- **Single job?** No — mixes reference diagrams with working scripts
- **Recommendation:** **SPLIT** — move `.cir` and `.svg` to `documentation/reference/`, move scripts to nous/akron/workspace/ or delete if one-time

---

### 13. `dodge_ram_2500_1997/manual_processing/` (22 files, includes LanceDB vector store)
- **Belongs in theke?** ⚠️ The processed outputs (JSON, DB) belong with the manuals. The scripts and test files are tooling.
- **Redundant?** Scripts duplicate `manual_processing_template/`
- **Stale?** Test files (`comprehensive_test.py`, `edge_case_test.py`, `valve_adjustment_search.py`) were one-time validation
- **Recommendation:** **KEEP** database + processed data. **DELETE** test scripts (one-time validation). Scripts already templated in `manual_processing_template/`.

---

### 14. `database/` (199 files, 517 MB)
- **Belongs in theke?** ✅ The `vehicle_management_full.db` (434 KB) belongs — it's the parts/maintenance DB
- **Redundant?** 
  - `vehicle_management.db` (0 bytes) — **DELETE**, empty placeholder
  - `photos_renamed/` (172 files) — **REDUNDANT** with `dodge_ram_2500_1997/photos/`. Same photos with standardized names.
  - Many Python scripts (`add_data.py`, `comprehensive_data_update.py`, etc.) are one-time migration/import scripts
  - `archive/qa_tracking_2026/` — process documentation, stale
- **Stale?** Most scripts were run once and not needed again
- **Recommendation:**
  - **KEEP:** `vehicle_management_full.db`, `vehicle_management_schema_v2.sql`, `DATABASE_QUICK_REFERENCE.md`, `QUERY_COOKBOOK.md`
  - **DELETE:** `vehicle_management.db` (empty), `photos_renamed/` (redundant, saves ~100 MB)
  - **ARCHIVE or DELETE:** All Python migration scripts (14 files), SQL scripts, `reports.py`, `setup_mcp_sqlite.sh` — these were one-time tooling
  - **DELETE:** `archive/` — process notes, no longer needed

---

### 15. `guides/` (2 files)
- `harness-connector-map.md` — ✅ Active reference for wiring work
- `wheels-tires-FINAL.md` — ✅ Active reference, matches MEMORY.md wheel/tire decisions
- **Recommendation:** **KEEP** both

---

### 16. `install-docs/` (EMPTY)
- **Recommendation:** **DELETE** — empty directory. Note: nous/akron/workspace/ also has an empty `install-docs/`. Both should go.

---

### 17. `manual_processing_template/` (12 files, 124 KB)
- **Belongs in theke?** ⚠️ These are generic scripts for processing any vehicle manual. Tooling, not knowledge.
- **Redundant?** Partially duplicated in `dodge_ram_2500_1997/manual_processing/scripts/` and `royal_enfield_gt650/manual_processing/scripts/`
- **Stale?** Only if we never process another manual
- **Single job?** Yes — it's a template
- **Recommendation:** **MOVE to shared tools** (`$ALETHEIA_SHARED/tools/manual-processing/`) or keep here but note it's tooling, not domain knowledge. The per-vehicle copies in processing dirs should be deleted (they were generated from this template).

---

### 18. `overland_teardrop/` (6 files, 76 KB)
- **Belongs in theke?** ❌ **NO** — MEMORY.md explicitly states: "Teardrop trailer was a fantasy — not part of the build plan"
- **Redundant?** N/A — doesn't correspond to any active plan
- **Stale?** Completely — represents abandoned planning
- **Recommendation:** **DELETE** — or move to a personal archive if Cody wants to keep the research. But it should not be in the active vehicle knowledge vault.

---

### 19. `royal_enfield_gt650/manuals/` (4 PDFs, 1.7 GB)
- **Belongs in theke?** ✅ Yes — canonical service manuals for the motorcycle
- **Redundant?** No
- **Stale?** No — permanent reference
- **Recommendation:** **KEEP** — note these are very large (833 MB for one PDF). If storage is a concern, these are the largest files in the vault.

---

### 20. `royal_enfield_gt650/documentation/` (8 files)
- **Belongs in theke?** ✅ Yes — maintenance schedules, procedures, parts inventory
- **Stale?** `6000_mile_maintenance_checklist.md/.pdf` may be stale if that service was completed
- **Redundant?** `Speedometer Issue.md` and `Complete.md` — check if these are session artifacts vs curated docs
- **Recommendation:** **KEEP** — review for staleness after next RE service

---

### 21. `royal_enfield_gt650/manual_processing/` (50 MB)
- Same pattern as Ram manual processing — database outputs + scripts + test infrastructure
- **Recommendation:** **KEEP** database/processed. **DELETE** one-time scripts that duplicate the template.

---

### 22. `royal_enfield_gt650/archive/` (9 files)
- Extracted PDF sections + extraction script — one-time processing artifacts
- **Recommendation:** **DELETE** — the processed data is in `manual_processing/processed/`

---

### 23. `scripts/` (3 files, 20 KB)
- Generic text extraction utilities (PDF, DOCX)
- **Belongs in theke?** ⚠️ Tooling, not knowledge
- **Recommendation:** **MOVE to shared tools** or delete if `manual_processing_template/` covers this

---

## Summary of Recommendations

| Action | Items | Space Saved |
|--------|-------|-------------|
| **DELETE** | `overland_teardrop/`, `database/photos_renamed/`, `database/vehicle_management.db` (empty), empty dirs (`context/`, `install-docs/`), `royal_enfield_gt650/archive/`, one-time scripts | ~200 MB |
| **MOVE to shared tools** | `manual_processing_template/`, `scripts/` | 144 KB (declutter) |
| **MOVE to archive** | `database/` migration scripts, `dodge_ram_2500_1997/documentation/external_sources/` | Declutter |
| **SPLIT** | `electrical_simulation/` — reference vs working files | Clarity |
| **FIX** | `08_10a-short-diagnosis.md` — wrong fuse number in filename and content | Accuracy |
| **KEEP** | FSM HTML, photos (one copy), procedures, reference, guides, databases, RE manuals + docs | Core vault |

## Priority Actions (Quick Wins)

1. **Delete `overland_teardrop/`** — explicitly abandoned, shouldn't be in active vault
2. **Delete `database/photos_renamed/`** — 172 redundant photo copies
3. **Delete empty dirs** — `context/`, `install-docs/` (both locations)
4. **Delete `database/vehicle_management.db`** — 0 bytes, empty placeholder
5. **Fix fuse #18 filename** — `08_10a-short-diagnosis.md` → rename to reflect park lamp fuse #18

---

*Audit complete. Findings ready for Demiurge consolidation.*

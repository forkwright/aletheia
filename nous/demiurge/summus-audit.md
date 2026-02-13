# Theke/Summus Audit — Chiron
**Date:** 2026-02-10
**Auditor:** Chiron (work domain agent)
**Scope:** All files in `theke/summus/`

---

## Summary

**Total:** ~1,800 files, ~569MB
**Verdict:** Good bones, significant bloat. The vault conflates four distinct concerns: (1) shared reference knowledge, (2) active project working state, (3) completed deliverables/archives, and (4) personal career documents. Several large binary blobs inflate storage unnecessarily.

---

## Top-Level Files

| File | Size | Recommendation | Reasoning |
|------|------|----------------|-----------|
| `README.md` | 6K | **KEEP** | Good overview, well-maintained. Contains Obsidian dataview queries (works if Obsidian is used, harmless otherwise). |
| `_REGISTRY.md` | 4K | **KEEP** | Critical cross-project index. Actively useful for any agent working this domain. |
| `CLAUDE.md` | 4K | **KEEP** | AI context file for Work Claude Code sessions on Metis. Serves its purpose. |
| `_CLAUDE_CONFIG.md` | 2K | **MERGE** → into `CLAUDE.md` | Redundant with CLAUDE.md. One config file, not two. |
| `CHANGELOG.md` | 10K | **KEEP** | Useful project history. |
| `standard.md` | 12K | **KEEP** | Organization standards. Actively referenced. |
| `_ONBOARDING.md` | 1K | **KEEP** | Low-cost, high-value for new sessions. |
| `llms.txt` | 1K | **KEEP** | Standard llms.txt for AI discovery. |
| `pyproject.toml` | 0.5K | **KEEP** | Project config. |
| `summus.code-workspace` | — | **KEEP** | VS Code workspace config. |
| `.env.redshift` | — | **⚠️ SECURITY: MOVE** | Contains plaintext Redshift credentials (host, user, password). Should NOT be in theke (shared knowledge). Move to a secrets manager or at minimum `chmod 600` and move to a non-shared location. |
| `.last-sync` | — | **KEEP** | Sync metadata. |
| `tasks.db` | 24K | **MOVE → nous/chiron/** | This is agent working state (task tracking), not shared knowledge. Belongs in nous/. |

---

## Directories

### `data_landscape/` — 382 files, 18MB
**Recommendation: KEEP (theke is correct location)**

This is the crown jewel of shared knowledge: SQL scripts, schema documentation, knowledge base, ERDs, runbooks. Exactly what theke is for — reference knowledge that any agent or human session needs.

**Sub-audit:**
- `data_landscape/knowledge_base/` — **KEEP**. Core business logic, glossary, table docs, query patterns. High value.
- `data_landscape/sql/` — **KEEP**. Canonical SQL source of truth. Well-organized by domain.
- `data_landscape/schema/` — **KEEP**. Schema exports, ERDs, queryable SQLite DB, tools. Active reference.
- `data_landscape/schema/db/summus_schema.db` — **KEEP**. Queryable schema. Useful.
- `data_landscape/runbooks/` — **KEEP**. Operational guides.
- `data_landscape/src/` — **KEEP**. CLI tools for schema work.
- `data_landscape/_archive/` — **REVIEW**. Old docs and legacy SQL scripts. Low value but low cost. Keep for now.
- `data_landscape/sql/dm_tests/` — **KEEP**. Dimensional model test scripts. Active development.
- `data_landscape/sql/_review/` — **REVIEW**. 4 SQL files in review status. Either promote or archive.
- `data_landscape/_context/` — **KEEP**. Claude context for this subdomain.

**One job?** Yes — data warehouse documentation and SQL reference. Clear purpose.

### `reporting/` — 369 files, 29MB
**Recommendation: KEEP (theke is correct location)**

Dashboard projects with SQL, hex configs, documentation, validation data. This is production reference — the kind of thing any future session needs to understand "what dashboards exist and how they work."

**Sub-audit:**
- `reporting/dashboards/*/` — **KEEP**. Each dashboard is well-organized with its own CLAUDE.md, CHANGELOG, README.
- `reporting/dashboards/*/sql/archive/` — **REVIEW**. Old validation queries. Low value but low cost.
- `reporting/dashboards/sms_360/final_review/` — **STALE**. Screenshots and CSVs from a review session (Dec 2025). Archive or delete.
- `reporting/dashboards/rso_biweekly/data/survey_investigation/` — **STALE**. Investigation CSVs and query results from a one-time investigation. Archive.
- `reporting/dashboards/fedex/exports/2026-01-14/` — **STALE**. Point-in-time PDF exports. Archive.
- `reporting/ad-hoc/gi_ai_analysis_mary_m/` — **STALE**. Completed analysis with 15+ status/progress documents. The SQL and output are useful; the 8 iteration documents (READY_FOR_EXECUTION, DELIVERY_READY, PUSHBACK_AND_CLARIFICATIONS, etc.) are stale. Consolidate to one summary + final SQL + output.
- `reporting/ad-hoc/condition_pathway_export/` — **KEEP**. Active reference with scripts and CCSR data.
- `reporting/ad-hoc/member_activity_lookup/` — **KEEP**. Useful reusable queries.
- `reporting/ad-hoc/sms_survey_response_rate/` — **KEEP**. Single SQL file, low cost.

**Redundancy note:** `reporting/dashboards/fedex/sms/` overlaps with `reporting/dashboards/sms_360/`. Both contain SMS-related SQL, docs, and hex configs. The fedex/sms/ appears to be an earlier version that was split into its own dashboard (sms_360). The fedex/sms/ should be archived or reduced to a pointer.

### `gnomon/` — 237 files, 17MB
**Recommendation: KEEP but TRIM**

Medical taxonomy system. This is a legitimate shared reference project.

**Issues:**
- `gnomon/medical_taxonomy/ui/build/` — **DELETE**. 12MB of compiled React build artifacts. Regenerable. Should never be in a knowledge vault.
- `gnomon/medical_taxonomy/ui/dist/` — **DELETE**. 2.4MB of compiled Vite build artifacts. Also regenerable.
- `gnomon/medical_taxonomy/.pytest_cache/` — **DELETE**. Test cache, no value.
- `gnomon/medical_taxonomy/ui/node_modules/` — Check if present (wasn't in listing). If so, delete.
- Everything else (src, scripts, sql, tests, docs, data) — **KEEP**. This is the actual project.

**Savings:** ~14.4MB from build artifact deletion alone.

### `_project_context/gnomon/` — 129 files, 282MB
**Recommendation: COMPRESS or MOVE to cold storage**

This is by far the largest directory. 282MB — nearly half the vault's total size.

**Breakdown:**
- `archive/` — 189MB. Historical gnomon development artifacts. Session docs, legacy scripts, reference materials, a 189MB tar.gz backup.
- `medical-taxonomy_backup_20260112.tar.gz` — Likely the bulk of the 282MB. This is a point-in-time backup.

**Assessment:** This is not shared knowledge. It's historical project context — useful for archaeology, not daily reference. The backup file alone is ~189MB of compressed data sitting in the vault permanently.

**Recommendation:**
1. Move `medical-taxonomy_backup_20260112.tar.gz` to NAS cold storage.
2. Keep `_project_context/gnomon/archive/reference/documentation/` (the original specs/guides have reference value).
3. Archive or delete session docs from Oct/Nov 2025 — they served their purpose.
4. The `.env.example`, `PRODUCTION_LOCATION.md`, `REMAINING_ISSUES.md` — KEEP as lightweight reference.

### `prospect_roi/` — 129 files, 14MB
**Recommendation: KEEP (theke is correct)**

Client ROI analysis system with CLI, templates, client data, scripts. This is institutional knowledge — how Summus does prospect ROI calculations.

**Sub-audit:**
- `prospect_roi/cli/` — **KEEP**. Active tooling.
- `prospect_roi/clients/` — **KEEP**. Client-specific data and analysis. Reference for future prospects.
- `prospect_roi/templates/` — **KEEP**. Reusable templates.
- `prospect_roi/scripts/` — **KEEP**. Processing tools.
- `prospect_roi/_archive/insert_scripts/` — **REVIEW**. 25 legacy insert scripts. Low-value but low-cost.
- `prospect_roi/data/roi_analysis_backup_20260112.db` — **STALE**. Backup alongside active `roi_analysis.db`. Only need one. Delete backup or move to cold storage.
- `prospect_roi/roi_analysis.db` (root) vs `prospect_roi/data/roi_analysis.db` — **REDUNDANCY**. Two copies of the same DB? Investigate and consolidate.
- `prospect_roi/migration_mapping_20260112.csv` — **STALE**. One-time migration artifact.

### `career/` — 83 files, 4.3MB
**Recommendation: MOVE to nous/ or separate theke/career/ domain**

Career documents are personal — resume, job search, military records, consulting notes. This is NOT shared work knowledge. It ended up here because summus was historically "everything work-related" but the theke model distinguishes between domain knowledge (theke) and personal/agent working memory (nous).

**Sub-audit:**
- `career/military/` — **SENSITIVE**. DD-214, fitness reports, appointment letters. These are personal documents with PII. Should NOT be in a shared knowledge vault. Move to encrypted storage or nous/ with restricted access.
- `career/job-search/` — **PERSONAL**. Resume, cover letters, job targets, interview prep. Not domain knowledge.
- `career/consulting/jeisys/` — **PERSONAL**. Consulting engagement notes and transcripts.
- `career/career-audit.md` — **PERSONAL**. Deeply personal career analysis with cognitive profile details.
- `career/Linkedin_Banner.*` — **PERSONAL**. Brand assets.
- `career/202602-Kickertz_Resume.odt` — **PERSONAL**. Active resume.

**Strong recommendation:** Create `theke/career/` as its own domain, or move to `nous/` workspace. This doesn't belong alongside SQL scripts and dashboard docs.

### `portfolio/` — 223 files, 158MB
**Recommendation: MOVE portfolio projects out of theke/summus/**

**Breakdown:**
- `nasa-mars-sol200-analysis/` — 157MB. A portfolio showcase project with raw .img data files (Mars Mastcam images). **This is absurd in a knowledge vault.** The raw data alone is 145MB. Move to a git repo or cold storage. Keep only the README/FINDINGS if needed for reference.
- `ai-infrastructure-toolkit/` — 288K. Portfolio project. Belongs in a git repo, not theke.
- `infrastructure-automation-toolkit/` — 288K. Portfolio project. Same.
- `profile-repo/` — 12K. GitHub profile repo. Belongs in git.
- `PORTFOLIO_STRATEGY.md` — **KEEP** (lightweight strategy doc). Or move with career/.
- `PROJECT_STATUS.md` — **KEEP** (lightweight).

**Portfolio projects are code repos, not knowledge.** They should live in git repos under techne/ or dianoia/techne/, not in the theke vault.

### `_archive/` — 109 files, 27MB
**Recommendation: COMPRESS and consider cold storage**

Historical project files from completed work: FedEx Connect, FedEx SMS, RSO biweekly, sales materials.

**Assessment:** This is exactly what an archive should contain. But 27MB of archived SQL investigation queries and CSV exports has diminishing returns. Consider:
1. Keep the README/summary files from each archive section.
2. Compress the rest into a dated tarball on NAS.
3. Or leave as-is — it's already properly segregated with `_archive/` prefix.

**Sub-audit:**
- `_archive/sales/` — Historical ROI dashboards and fact sheets (2024). **STALE** but harmless.
- `_archive/fedex-connect/` — Completed investigation. SQL and docs from Nov 2025. **STALE**.
- `_archive/fedex-sms/` — Pre-SMS360 work. **STALE**.
- `_archive/rso-biweekly/` — Legacy RSO work. **STALE**.
- `_archive/misc_cleanup/` — n8n vendor docs, Mihika queries, AI PDF. **STALE**.

### `bootstrap/` — 20 files, 128K
**Recommendation: KEEP**

Dev environment setup scripts. Lightweight, useful, well-organized. Correct location in theke (shareable setup knowledge).

### `general_folio/` — 13 files, 88K
**Recommendation: REVIEW for consolidation**

Miscellaneous documents. This is a catch-all that's small but unfocused.

- `20260123_sanders_ai_panel_prep.md` — One-time meeting prep. **STALE**.
- `working_patterns_observations.md` — Work observations. Potentially valuable. **KEEP**.
- `work.md` — Brief work notes. **REVIEW** — merge into MEMORY.md or archive.
- `redshift_data-model_scripts/` — Contains only config metadata files. **DELETE** or merge.
- `observations/` — One file (Slack taxonomy AI analysis). **KEEP** or move to data_landscape/knowledge_base/.

### `meetings/` — 10 files, 196K
**Recommendation: KEEP but PRUNE**

Meeting notes have reference value (decisions made, action items agreed).

- `meetings/notes/analytics_weekly/20260209_analytics_weekly.md` — **KEEP**. Recent.
- `meetings/1-12_priorities/` — January planning context. **STALE** but useful for historical context. Keep for now.
- Older meeting notes — **KEEP**. Low cost, occasional reference value.

### `_outputs/` — 13 files, 116K
**Recommendation: CLEAN**

Deliverables staging area. Should be emptied after delivery.

- `20260126_*` files — MCP configs and settings from Jan 26. **STALE**. Were these delivered? If so, archive.
- `sms_360_documentation/` — SMS tables guide. **STALE** if delivered. Archive.
- `install-codium.sh` — One-time script. **DELETE**.

### `_templates/` — 13 files, 88K
**Recommendation: KEEP**

Project templates and code quality configs. Exactly what belongs in shared knowledge.

### `summus_cli/` — 10 files, 68K
**Recommendation: KEEP**

CLI tool for summus admin. Active, useful, well-structured.

### `chiron-tracking/` — 8 files, 23MB
**Recommendation: MOVE → nous/chiron/**

This is explicitly agent working state — Chiron's Crinetics ROI analysis outputs. The name says it: "chiron-tracking."

- `Crinetics_Prospect_ROI.pptx` — 22.5MB. The bulk of this directory is one PowerPoint file.
- CSV data files, SQL queries, presentation content — all from a specific January 2026 task.

**This is nous/ material**, not theke. It's a completed deliverable from a specific task, tied to a specific agent.

### `inbox/` — 5 files, 32K
**Recommendation: DELETE or KEEP empty**

README says "deprecated." Contains only boilerplate files (_CHANGELOG, _CLAUDE, _llms.txt, QUICK_START, _README). No actual items to triage.

Either delete the directory or keep it as an empty staging area.

### `.vscode/` — 3 files
**Recommendation: KEEP**

Editor configuration. Harmless, useful for Metis sessions.

---

## Cross-Cutting Issues

### 1. ⚠️ SECURITY: Credential Exposure
`.env.redshift` contains plaintext Redshift credentials (host, port, database, user, **password**). This file is in a shared knowledge vault. Even if access-controlled, credentials should never be stored in plaintext in a knowledge base.

**Action:** Remove from theke immediately. Use a secrets manager or environment variable injection.

### 2. Redundancy: SQL in Multiple Locations
The same SQL patterns appear in:
- `data_landscape/sql/` (canonical)
- `reporting/dashboards/*/sql/` (dashboard-specific copies)
- `_archive/*/` (historical versions)

The REGISTRY.md correctly notes data_landscape is canonical, but the copies create confusion. Dashboard SQL directories should contain only dashboard-specific queries, not copies of DDL.

### 3. Binary Bloat: 450MB+ in non-knowledge files
| Source | Size | Type |
|--------|------|------|
| `_project_context/gnomon/` | 282MB | Backup tarball + archives |
| `portfolio/nasa-mars-sol200-analysis/data/` | 145MB | Raw Mars imagery |
| `gnomon/medical_taxonomy/ui/build/` | 12MB | Compiled JS/CSS |
| `gnomon/medical_taxonomy/ui/dist/` | 2.4MB | Compiled JS/CSS |
| `chiron-tracking/Crinetics_Prospect_ROI.pptx` | 22.5MB | Deliverable |

That's ~464MB of non-knowledge content in a 569MB vault. **81% of the vault is not knowledge.**

### 4. theke vs. nous Boundary Violations
Several directories contain agent working state rather than shared knowledge:
- `chiron-tracking/` → agent work product
- `tasks.db` → agent task state
- `career/` → personal documents
- `_outputs/` → transient deliverables (should be emptied after delivery)

### 5. Build Artifacts in Version Control
`gnomon/medical_taxonomy/ui/build/` and `ui/dist/` are compiled artifacts that should be in `.gitignore`, not committed to a knowledge vault.

---

## Recommendations Summary

### Immediate Actions
1. **🔴 SECURITY:** Remove `.env.redshift` from theke. Move credentials to secrets management.
2. **🔴 SECURITY:** Review `career/military/` — DD-214 and personal military documents need restricted access.
3. **DELETE:** `gnomon/medical_taxonomy/ui/build/`, `ui/dist/`, `.pytest_cache/` — 14.4MB of regenerable artifacts.

### Short-Term (This Week)
4. **MOVE** `career/` → `theke/career/` (own domain) or `nous/` (personal workspace).
5. **MOVE** `chiron-tracking/` → `nous/chiron/` (agent working state).
6. **MOVE** `tasks.db` → `nous/chiron/` (agent task state).
7. **MOVE** `portfolio/` projects → git repos under techne/. Keep only strategy docs.
8. **MOVE** `_project_context/gnomon/medical-taxonomy_backup_20260112.tar.gz` → NAS cold storage.
9. **CLEAN** `_outputs/` — archive delivered items, delete one-off scripts.

### Medium-Term (This Month)
10. **CONSOLIDATE** `reporting/dashboards/fedex/sms/` with `reporting/dashboards/sms_360/` — eliminate duplication.
11. **CONSOLIDATE** `reporting/ad-hoc/gi_ai_analysis_mary_m/` — reduce 15+ status docs to one summary.
12. **COMPRESS** `_archive/` → tarball on NAS if space is a concern.
13. **AUDIT** `prospect_roi/roi_analysis.db` vs `prospect_roi/data/roi_analysis.db` — consolidate duplicate DBs.
14. **REVIEW** `data_landscape/sql/_review/` — promote or archive 4 stalled SQL files.

### Expected Impact
- **Storage:** ~464MB reduction (81% of current vault) by moving binaries and archives.
- **Clarity:** Clear separation between knowledge (theke), working state (nous), and code (git repos).
- **Security:** No more plaintext credentials in shared vault. Personal documents properly protected.

---

## What Stays in theke/summus/ (The Core)

After cleanup, theke/summus/ should contain:
- `data_landscape/` — SQL, schemas, knowledge base, runbooks
- `reporting/` — Dashboard docs, SQL, hex configs (trimmed)
- `prospect_roi/` — ROI system, templates, client data
- `gnomon/` — Taxonomy system (without build artifacts)
- `bootstrap/` — Dev environment setup
- `meetings/` — Meeting notes with decisions
- `general_folio/` — Misc observations (trimmed)
- `_templates/` — Project templates
- `summus_cli/` — CLI tool
- Top-level docs (README, REGISTRY, CLAUDE, standard, etc.)

**Estimated clean size:** ~90-100MB — actual knowledge.

---

*Audit by Chiron, 2026-02-10*
*Methodology: File-by-file review against 5 audit questions*

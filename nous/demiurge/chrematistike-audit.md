# Chrematistike (MBA) Domain Audit

**Auditor:** Eiron
**Date:** 2026-02-10
**Path:** `theke/chrematistike/`
**Size:** 1.4GB, ~6,555 files
**Status:** MBA program is Spring 2026 (final semester — SP26 is active, FA25 is completed)

---

## Executive Summary

This domain is 88% dead weight by volume. 753MB of extracted video frames from a completed FA25 branding assignment account for over half the total size. The SP26 coursework (78MB) is the only actively relevant area. Several empty scaffold directories, duplicate file formats, and orphaned working files add clutter.

**The core question — what matters post-graduation?** Very little of this belongs in theke (shared knowledge vault). Most is coursework ephemera: assignments, syllabi, lecture PDFs, team submissions. The few genuinely valuable items are frameworks, lessons learned, and select analyses that transfer beyond the classroom.

---

## Top-Level Files

### README.md (2.8KB)
- **Verdict: MOVE → nous/eiron** (or delete)
- Agent working file. Contains dataview queries (Obsidian-specific, won't render here), sync instructions, CLI commands. This is operational context, not shared knowledge.
- Has stale info: "Last sync: never" but `.last-sync` says Feb 3.

### LEARNING_FRAMEWORK.md (5.4KB)
- **Verdict: KEEP in theke — this is gold**
- Cody's cognitive translation model: vertical thinking (levels 3-5) vs course expectations (levels 1-2). Bidirectional translation protocol. Information delivery protocol.
- Applies beyond MBA — this is a reusable framework for how Cody processes structured learning.
- **Action:** Update "Course-Specific Notes" section (still says "To be populated"). Move from chrematistike root to `theke/chrematistike/` root or better: `theke/sophia/` (this is really about learning/cognition, not MBA-specific).

### LESSONS_LEARNED_GLOBAL.md (3.3KB)
- **Verdict: KEEP in theke**
- Cross-course prevention checklist for numerical/written submissions. Concrete, evidence-based.
- The "mathematically correct ≠ Canvas correct" principle is genuinely useful if Cody ever does more structured coursework or certification.
- Last updated Oct 2025. Still relevant.

### .last-sync
- **Verdict: DELETE** — operational artifact, not knowledge.

---

## Top-Level Directories

### `application/` (2.3MB)
- **Contents:** MBA application essay, resume templates, UH transcript, VA documents (DD-214, JST)
- **Verdict: MOVE → personal documents vault (not theke)**
- These are personal identity documents, not shared knowledge. DD-214 and transcripts are sensitive. Resume templates are stale (2022-2023 vintage).
- **Action:** Move VA docs + transcript to a personal/secure location. Delete resume templates (outdated). Essay is historical artifact — archive or delete.

### `archive/` (empty)
- **Verdict: DELETE** — empty directory, no purpose.

### `shared/` (32KB)
- **Contents:** Three empty subdirectories: `frameworks/`, `references/`, `syllabi/`
- **Verdict: DELETE** — scaffolding that was never populated. The intent was good but the work happened in course-specific dirs instead.

### `summaries/` (empty)
- **Verdict: DELETE** — empty directory.

### `tools/` (empty)
- **Verdict: DELETE** — empty directory. Course-specific tools live in their respective course dirs.

### `marketing_class_materials/` (46MB)
- **Contents:** 9 HBS case PDFs with opaque filenames (8140-PDF-ENG.pdf etc.), 1 Kotler textbook chapter (23MB), 1 .docx
- **Verdict: DELETE or ARCHIVE**
- Orphaned. Not inside any semester directory. No README. Filenames are meaningless HBS catalog numbers. Likely from an earlier semester or pre-program purchase.
- The Kotler chapter is 23MB alone — textbook content that shouldn't be in a knowledge vault.
- **Action:** If any cases are worth keeping, rename them meaningfully first. Otherwise delete.

---

## `fa25/` — Fall 2025 (1.3GB, completed semester)

### Overall Assessment
FA25 is done. Grades are in. 99% of this is reference material that will never be touched again.

### `fa25/strategic_branding/` (962MB) ⚠️ BIGGEST OFFENDER
- **presentation_analysis/frames/** (753MB, ~5,695 JPG files): Extracted video frames from peer presentation analysis. One-time assignment output. These are literally screenshots of student presentations at 1fps.
- **Verdict: DELETE frames entirely.** The reviews (the actual deliverable) are in `reviews/` and total ~100KB. The frames were intermediate processing artifacts.
- **presentation_analysis/reviews/** (~50KB): The actual peer reviews. Keep if they contain transferable analytical frameworks; otherwise archive.
- **class_materials/** (202MB): Raw course materials (PDFs, slides). Instructor IP. Not ours to vault.
- **tools/** (80KB): Python scripts for frame extraction, docx/pptx reading. One-time-use scripts.
- **Verdict: DELETE the entire `presentation_analysis/frames/` tree (753MB). ARCHIVE `class_materials/` if needed for reference. DELETE `tools/` (single-use scripts).**

### `fa25/marketing_analytics/` (241MB)
- **Contents:** 6 modules of class materials, 5 homework sets with drafts/archives, exam prep, final prep with cheat sheets, gap analyses
- Large due to homework Excel files and PDF readings.
- **Verdict: ARCHIVE entire directory.**
- **Exception:** `final_prep/` cheat sheets and `summaries/` may contain distilled knowledge worth extracting. The `tools/mktcalc.py` might be reusable.
- **Action:** Extract any genuinely useful formulas/frameworks into LEARNING_FRAMEWORK.md or a `theke/chrematistike/frameworks/` file, then archive the rest.

### `fa25/managerial_accounting/` (30MB)
- **Contents:** 6 class sessions, final review with cheat sheets, practice exams, quizzes.
- **Verdict: ARCHIVE.** Final review cheat sheets may contain useful reference material.

### `fa25/financial_statement_analysis/` (16MB)
- **Contents:** 5 class sessions including Casper case study, Porter framework application, FSA summaries.
- **Verdict: ARCHIVE.** FSA fundamentals (ratio analysis, cash flow analysis) have lasting value but the course-specific materials don't.

### `fa25/investment_management/` (22MB)
- **Contents:** 6 weeks of materials, course project, quizzes.
- **Verdict: ARCHIVE.** Portfolio theory fundamentals are well-documented elsewhere.

### `fa25/README.md`
- **Verdict: KEEP with fa25/** — good documentation of course structure and key formulas. If fa25 is archived, this goes with it.

**FA25 SUMMARY: Delete 753MB of frames. Archive the remaining ~550MB. Extract any lasting frameworks first.**

---

## `sp26/` — Spring 2026 (78MB, ACTIVE semester)

### `sp26/acf/` — Advanced Corporate Finance
- **Contents:** Syllabus (2 versions), lecture PDFs (10 lectures organized by topic), articles (11 research papers), HW1 materials + submissions, empty `quizzes/`, `summaries/`, `tools/`, `examples/` dirs.
- **Sarah-Michelle Finance HW 1.pdf** in hw1/ — teammate's submission for comparison. Useful during course, stale after.
- **Verdict: KEEP during SP26.** Post-graduation: archive. Lecture PDFs are instructor IP. Cody's homework submissions could be archived for reference.
- **Redundancy:** Two syllabus versions (original + updated). Keep only updated.

### `sp26/strategic_mgmt/` — Strategic Management
- **Contents:** Syllabus, readings (7 PDFs), slides (9 lecture decks), media (5 images from slides), team project agreement, Merck mini case, quiz 1 prep.
- Empty `quizzes/`, `summaries/`, `tools/`, `examples/` dirs.
- **Verdict: KEEP during SP26.** Post-graduation: archive most. Rumelt's "Bad Strategy" concepts and the GRASP/CRISP frameworks may have lasting value — extract to a frameworks file.

### `sp26/macro/` — Managerial Macroeconomics
- **Contents:** Syllabus, 4 lecture pptx, 2 readings, and a rich `summaries/` directory.
- `summaries/` has real gold: `TRANSLATION_FRAMEWORK.md`, `VOICE_GUIDE.md`, `compression_protocols.md`, vertical mappings (GDP, growth/Malthus), quiz prep, lecture notes.
- Empty `homework/`, `quizzes/`, `tools/`, `examples/` dirs.
- **Verdict: KEEP during SP26.** The `summaries/` directory is the best example of what theke should contain — distilled, processed knowledge. Post-graduation: move `TRANSLATION_FRAMEWORK.md`, `VOICE_GUIDE.md`, and `compression_protocols.md` up to the root (they're generalizable cognitive tools, not macro-specific).

### `sp26/capstone/` — TBC Capstone Project (largest SP26 dir)
- **Contents:** Extensive project structure with analysis, deliverables, meetings, research, team docs, tools.
- **Redundancy issues:**
  - `ROADMAP.md` + `ROADMAP.docx` (duplicate formats)
  - `roadmap.docx` + `ROADMAP.docx` (duplicate files, different case)
  - `Proposal Plan B - Evan Wehr LLC.docx` exists in both `Project_Planning/` AND `Project_Planning/Proposals/`
  - `Team Charter.docx` in both `Project_Planning/Team Charter/` AND `team/`
  - `meetings/` directory exists at BOTH `capstone/meetings/` AND `capstone/project/meetings/`
  - `validation_meeting_questions.md` in `deliverables/` AND `meetings/` as .docx
  - `01-20_discovery_call_summary` in both `.md` and `.docx`
  - `2026-01-20_discovery_call.md` at `capstone/meetings/` duplicates content in `project/meetings/`
- **Meta-files:** `CLAUDE.md`, `CHANGELOG.md`, `PROJECT_STATUS.md`, `KICKOFF_MEETING_BRIEF.md`, `COVERAGE_CHECKLIST.md`, `GAP_ANALYSIS.md` — these are agent working memory, not knowledge.
- **Verdict: KEEP during SP26 (active project through Apr 15).** But this needs cleanup NOW:
  1. Consolidate meetings into one location
  2. Pick one format (md preferred) and delete duplicate .docx versions
  3. Move agent working files (CLAUDE.md, CHANGELOG.md etc.) → `nous/eiron/`
  4. Remove duplicate files

**Also:** `nous/eiron/archive/capstone_work_jan29/` has 45 files (~4,700 lines) of earlier capstone work — this is agent working memory correctly placed in nous, not theke. No action needed there.

---

## Cross-Cutting Issues

### 1. Theke vs Nous Confusion
Many files in theke are operational (agent context, sync artifacts, changelogs) rather than knowledge. The rule should be:
- **Theke:** Distilled knowledge that outlasts the course. Frameworks, lessons, processed insights.
- **Nous:** Working files, agent memory, project status, changelogs, sync state.

### 2. Empty Scaffold Directories
At least 15 empty directories across the tree (`archive/`, `shared/frameworks/`, `shared/references/`, `shared/syllabi/`, `summaries/`, `tools/`, plus per-course `quizzes/`, `examples/`, `tools/` dirs). These were aspirational structure that never got populated.
- **Action:** Delete all empty dirs. If they're needed later, create them then.

### 3. Format Duplication
Multiple files exist as both `.md` and `.docx` (especially in capstone). This creates sync ambiguity — which is canonical?
- **Rule:** `.md` is canonical for text content. `.docx` only for files that must be shared with teammates in that format.

### 4. Frame Extraction Artifacts (753MB)
The single largest storage waste. 5,695 JPG files from video frame extraction for a completed assignment. The actual deliverable (peer reviews) is ~50KB.

### 5. Sensitive Documents
`application/va_documents/` contains DD-214 and JST — these are sensitive military/personal documents that should not be in a shared knowledge vault.

---

## Recommendations Summary

| Path | Size | Action | Reason |
|------|------|--------|--------|
| `fa25/strategic_branding/presentation_analysis/frames/` | 753MB | **DELETE** | Intermediate artifacts, assignment complete |
| `marketing_class_materials/` | 46MB | **DELETE** | Orphaned, opaque filenames, textbook content |
| `application/` | 2.3MB | **MOVE** to personal/secure | Sensitive docs, not shared knowledge |
| `shared/`, `summaries/`, `tools/`, `archive/` (root) | ~0 | **DELETE** | Empty scaffolding |
| All empty per-course scaffold dirs | ~0 | **DELETE** | Never populated |
| `fa25/` (after frame deletion) | ~550MB | **ARCHIVE** | Completed semester |
| `LEARNING_FRAMEWORK.md` | 5KB | **KEEP + UPDATE** | Genuine gold — cognitive framework |
| `LESSONS_LEARNED_GLOBAL.md` | 3KB | **KEEP** | Evidence-based error prevention |
| `sp26/macro/summaries/TRANSLATION_FRAMEWORK.md` | - | **PROMOTE** to root | Generalizable beyond macro |
| `sp26/macro/summaries/VOICE_GUIDE.md` | - | **PROMOTE** to root | Generalizable beyond macro |
| `sp26/macro/summaries/compression_protocols.md` | - | **PROMOTE** to root | Generalizable beyond macro |
| `sp26/capstone/` duplicate files | - | **DEDUPLICATE** | 6+ pairs of duplicates |
| `sp26/capstone/` agent working files | - | **MOVE** → nous/eiron | CLAUDE.md, CHANGELOG.md, etc. |
| `README.md` (root) | 3KB | **REWRITE** | Stale, operational, has Obsidian queries |
| `.last-sync` | 32B | **DELETE** | Operational artifact |
| `sp26/` (all, post-graduation) | 78MB | **ARCHIVE** | Active now, dead after Apr 2026 |

---

## Post-Graduation Vision

After the MBA completes (Apr 2026), `theke/chrematistike/` should shrink to:

```
chrematistike/
├── README.md                      # What this was, what survived
├── LEARNING_FRAMEWORK.md          # Cognitive translation model
├── LESSONS_LEARNED_GLOBAL.md      # Error prevention checklist
├── TRANSLATION_FRAMEWORK.md       # Level 1-5 translation protocol
├── VOICE_GUIDE.md                 # Natural output voice patterns
├── compression_protocols.md       # Quick reference for level compression
├── frameworks/                    # Extracted frameworks that actually work
│   ├── capital_structure.md       # M&M, trade-off theory, debt overhang
│   ├── strategic_analysis.md      # GRASP, CRISP, Rumelt's kernel
│   ├── marketing_analytics.md     # TEV, conjoint, elasticity formulas
│   └── macro_models.md           # GDP accounting, growth models
└── archive/                       # Compressed archive of raw coursework
    ├── fa25.tar.gz
    └── sp26.tar.gz
```

**Target size: <1MB active + compressed archives.**

The gold is in the frameworks and cognitive tools. Everything else is coursework that served its purpose.

---

## Immediate Quick Wins (do now)

1. **Delete frames** — saves 753MB instantly
2. **Delete empty dirs** — removes clutter from tree views
3. **Delete `marketing_class_materials/`** — orphaned, meaningless filenames
4. **Move `application/va_documents/`** — sensitive docs out of shared vault
5. **Deduplicate capstone** — pick canonical locations, delete copies

**Estimated savings: ~800MB (57% of total)**

---

*Audit complete. Signal/Theater/Gold applied to our own house.*

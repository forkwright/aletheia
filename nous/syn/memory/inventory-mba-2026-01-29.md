# MBA Materials Inventory - 2026-01-29

## Summary

| Location | Content | Access | Overlap Risk |
|----------|---------|--------|--------------|
| School Google Drive | Shared team folders | ⚠️ READ-ONLY | High (shared) |
| Metis (dianoia/chrematistike) | Full course materials, AI-assisted work | Full access | None (private) |
| Personal Google Drive | No MBA content found | Full access | None |
| NAS | No MBA content | N/A | None |
| worker-node | No MBA content | N/A | None |

---

## School Google Drive (cody.kickertz@utexas.edu)

**⚠️ SHARED - BE CAREFUL**

```
TEMBA/
├── Fin Accounting; Dexcom/        # FA25 - Group project (DXCM)
├── Leading People & Orgs./        # FA25 - Jamie Dimon case
├── Marketing Analytics/           # FA25 - HW folders (team submissions)
│   ├── HW 1-6 folders
│   └── Multiple teammate files
├── SU25 Strategic Branding/       # SU25 - Xbox brand audit
│   ├── Final Deliverables/
│   ├── Presentation/              # Video takes
│   └── Misc. & Archive/
└── Spring 25 | Group L/           # SP25 
    ├── Business Analytics/
    ├── Managerial Microeconomics/
    └── Valuation/
```

**Files I should NOT touch:**
- Anything with teammate initials (RR, DB, MA, CDR)
- Team submission folders
- Files others are editing

---

## Metis (dianoia/chrematistike) - Primary Working Directory

**Full access - this is where Cody does coursework**

### FA25 (Fall 2025) - COMPLETED
- `financial_statement_analysis/` - FSA class, company selection project
- `investment_management/` - Portfolio project (Trevor client)
- `managerial_accounting/` - Bridgeton, Wilkerson cases, variance analysis
- `marketing_analytics/` - 6 modules, final prep materials
- `strategic_branding/` - Xbox brand audit

### SP26 (Spring 2026) - CURRENT
- `advanced_corporate_finance/` - Almazan, just started
- `strategic_management/` - Ritchie-Dunham, just started
- `managerial_macroeconomics/` - Started, quiz prep exists
- `capstone/` - TBC project, comprehensive structure

---

## Overlap Analysis

### School GDrive ↔ Metis Overlap

| Shared Folder | Metis Location | Sync Needed? |
|---------------|----------------|--------------|
| Marketing Analytics HW | fa25/marketing_analytics/homework | ❌ Complete |
| Strategic Branding | fa25/strategic_branding | ❌ Complete |
| Capstone (future) | sp26/capstone/project | 🔄 Ongoing |

### Workflow Recommendation

1. **School GDrive = Team collaboration only**
   - Upload final team submissions
   - Share drafts for feedback
   - Don't keep working copies there

2. **Metis = Working directory**
   - All coursework lives here
   - Version controlled via dianoia
   - AI-assisted summaries, prep

3. **Sync pattern for Capstone:**
   - Work in `sp26/capstone/project/`
   - Upload to shared Drive via Derek (team lead)
   - Never directly edit Drive copies

---

## Quick Access Commands

```bash
# List school drive
gdrive school ls TEMBA/

# List specific class
gdrive school ls "TEMBA/Spring 25 | Group L/"

# List Metis classes
ssh ck@192.168.0.17 'ls ~/dianoia/chrematistike/sp26/'

# Read capstone status
ssh ck@192.168.0.17 'cat ~/dianoia/chrematistike/sp26/capstone/PROJECT_STATUS.md'
```

---

*Generated: 2026-01-29 09:15 CST*

# MEMORY.md - Chiron Technical Memory

## Domain: Technical Systems & Summus Analytics

I am the systems specialist. Architecture as meaning-making. Truth in data.

---

## Current Context

**Work Claude Code Interface:**
- Session: `work-claude` on Metis (`~/dianoia/summus`)
- Access: `ssh ck@192.168.0.17 'tmux capture-pane -t work-claude -p'`
- Model: Claude Opus 4.5 (upgraded 2026-01-29)

**Key Reference Locations:**
- Work context: `/mnt/ssd/aletheia/nous/syn/work/`
- SQL by domain: `work/data_landscape/sql/`
- Dashboard registry: `work/reporting/dashboards/`
- Gnomon taxonomy: `work/gnomon/`

---

## Summus Domain Knowledge

### ROI Methodology
- PEPM-based prospect ROI calculator with QA-verified constants
- BLS wage research system for regional salary estimates
- Critical formulas: CTP, avoided visits, engagement volume

**ROI Constants (verified):**
```python
roi_constants = {"er_factor": 0.04, "hrs_smd": 12, "hrs_exp": 60, "hrs_nav": 15}
dynamic_costs = {"primary_care": 121.0, "expert": 245.0, "er": 1150.0}
```

### Data Architecture
- 58 production tables → refactoring to 31 clean tables (dimensional model)
- SQL scripts version-controlled in GitHub for audit trail
- Knowledge base populated by querying Redshift directly

### Taxonomy (Gnomon)
- 191K+ codes embedded as vectors
- 16% gap found in ICD-10 codes used in case coding
- AI-native architecture: codes as vectors, tree view generated (not curated)
- Adding 30K codes = adding vectors, views regenerate automatically

---

## Lessons Learned

### Technical QA
- Always verify calculator formulas against source YAML
- Watch for semantic mismatches (e.g., "Personal" vs "Personalized")
- Historical benchmarks provide sanity checks on ROI numbers

### Process
- Direct collaboration with Cody more effective than mediated workflows
- Research before claiming, verify before stating
- "I don't know" is always better than wrong

### Cody's AI Philosophy
- AI as infrastructure, not tool
- Machine-readable first, human-usable always
- Compounding gains through documented decisions
- The tree is not the thing; the tree is a view of the thing

---

## Active Patterns

**For SQL work:**
- Query Redshift for ground truth, don't guess
- Build schema indexes so AI can answer "what breaks if I change X?"
- Single source of truth, audit trail on changes

**For code quality:**
- Shellcheck for all shell scripts
- Ruff for Python linting/formatting
- Comments are cognitive aids, not documentation

---

*Updated: 2026-02-03*

# MEMORY.md - Chiron Technical Memory

---

## Summus Domain Knowledge

**ROI Methodology:** PEPM-based prospect ROI calculator with QA-verified constants. BLS wage research for regional salary estimates. Critical formulas: CTP, avoided visits, engagement volume.

```python
roi_constants = {"er_factor": 0.04, "hrs_smd": 12, "hrs_exp": 60, "hrs_nav": 15}
dynamic_costs = {"primary_care": 121.0, "expert": 245.0, "er": 1150.0}
```

**Data Architecture:** 58 production tables → refactoring to 31 (dimensional model). SQL version-controlled in GitHub. Knowledge base from Redshift queries.

**Gnomon Taxonomy:** 191K+ codes as vectors. 16% gap in ICD-10 codes. AI-native: codes as vectors, tree view generated not curated. Adding codes = adding vectors.

## Key References

- Work context: `nous/syn/work/`
- SQL by domain: `work/data_landscape/sql/`
- Dashboard registry: `work/reporting/dashboards/`
- Work Claude Code: `ssh ck@192.168.0.17 'tmux capture-pane -t work-claude -p'`

## Lessons

- Verify calculator formulas against source YAML
- Watch semantic mismatches ("Personal" vs "Personalized")
- Historical benchmarks as sanity checks
- Query Redshift for ground truth, don't guess
- "I don't know" is always better than wrong

---

*Updated: 2026-02-05*

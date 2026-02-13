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

- Work repos on Metis: `~/aletheia/theke/summus/` (ssh ck@192.168.0.19)
- GitHub repos: `CKickertz/{bootstrap,data_landscape,gnomon,prospect_roi,reporting,summus-workspace}`
- Local sync: `nous/syn/work/`
- Work Claude Code: `ssh ck@192.168.0.19 'tmux capture-pane -t work-claude -p'`
- Metis IPs: `.19` (USB ethernet, preferred), `.20` (WiFi)
- dianoia: DEAD — removed, not a thing anymore

## Metis Access

- SSH: `ssh ck@192.168.0.19` (USB ethernet) or `ck@192.168.0.20` (WiFi)
- Work repos: `~/aletheia/theke/summus/`
- GitHub PAT configured in `/home/syn/.git-credentials-work` (both machines)
- safe-git wrapper: `bin/safe-git` — blocks push, identity changes
- safe-sql wrapper: `bin/safe-sql` — read-only Redshift via AWS Data API
- Git workflow: SSH into Metis, work on repos there, commit locally, push only when Cody says
- Redshift: cluster redshiftprovisionedclusterprod4f345b07-ld9zzlmyebs3, db dev, AWS session tokens expire
- GitHub user: CKickertz, org: SummusGlobal (SSO)

## Bootstrap (Team Package)
- Redesigned 2026-02-12/13: ephemeral scaffolding for Claude Code setup
- User starts claude in empty dir, tells it to clone bootstrap, it handles everything
- After setup: delete bootstrap/, workspace stands on its own
- Knowledge graph (SQLite/FalkorDB), slash commands, persistent memory
- Safety: PreToolUse hooks enforce git push blocking + Redshift write blocking (3 layers)
- No AWS creds in code; uses .summus/redshift.env created during onboarding
- graph/build_graph.py seeds from schema DB + dashboard structure + knowledge docs
- Commits pushed to GitHub under CKickertz account

## Round Robin Dashboard (Active)
- Location: `reporting/dashboards/round_robin/`
- Requested by Avery (Admin meeting Feb 4, 2026)
- Key table: `Member.CaseUsers` — SP assignment via round robin
- SP identification: `UserToRole → Roles WHERE Role = 'Summus Partner'`
- First SP entry per case (MIN CreatedOn) = round robin auto-assignment (0-3 sec delay)
- 20 active SPs, mostly EST, some CST/PST
- Need Johnny's input on: rotation type, weekend coverage, threshold configs
- Full exploration results in `docs/REDSHIFT_EXPLORATION.md`

## Lessons

- Verify calculator formulas against source YAML
- Watch semantic mismatches ("Personal" vs "Personalized")
- Historical benchmarks as sanity checks
- Query Redshift for ground truth, don't guess
- "I don't know" is always better than wrong
- CaseUsers has multiple rows per case (SP + SMD + others); filter by Role
- Redshift Data API queries with many joins can take 30+ seconds; use business_data views when possible
- Dashboards always use business_data.public materialized tables, never raw summus schema
- cases.primary_summus_partner is unreliable text from CaseNote 66; CaseUsers is authoritative
- cases.first_intake_case_user_id ≠ assigned SP (it's who did the intake appointment)
- Need dm_sp_assignments.sql in data_landscape/sql/core/ to support round robin dashboard

---

*Updated: 2026-02-05*

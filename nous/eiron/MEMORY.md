# MEMORY.md - Eiron's Academic Intelligence

*Last Updated: 2026-01-29 11:23 CST*

## Current Projects

### SP26 Capstone - TBC Project
- **Problem**: TBC coalition fragmenting as miners pivot to AI, growth requires attracting competing priorities
- **Timeline**: Phase I due Jan 27, Professor check-in Feb 5, Final report Apr 15
- **Team**: Derek (Lead), Cody (Data), Aaron (Regulatory), Evan (Tech), Marshall (Ops), Ramiro (TBD)
- **Workstreams**: 4 total, Cody owns #1 (Membership Segmentation) + #2 (ERCOT Analysis)

#### Phase 2 Deliverables (COMPLETE - Feb 4)
| Deliverable | Location |
|-------------|----------|
| Competitive Analysis | `project/research/competitive/competitive_analysis.md` |
| SWOT Analysis | `project/analysis/swot/tbc_swot_analysis.md` |
| Industry Research | `project/research/industry/texas_mining_landscape.md` |
| TBC Org Profile | `project/research/tbc_organization.md` |
| Regulatory Summary | `project/research/regulatory/texas_crypto_regulations.md` |

**Google Drive:** `gdrive-school:TEMBA/SP26 Capstone/Deliverables/`

#### Key Strategic Findings
- **Critical threat:** Bitcoin Mining Council already has Riot, Core Scientific, MARA as members
- **TBC's unique value:** Texas policy expertise + ERCOT grid strategy (no competitor replicates)
- **Recommended strategy:** "Hub and spoke" - Bitcoin core + AI/HPC and financial services spokes

#### Team Intelligence
- **Derek**: Competent leader, solid roadmap, manages client relationship well
- **Aaron**: ⚠️ **WEAK LINK** - assigned critical ERCOT analysis (Workstream 2), poor capability
- **Strategy**: Absorb Aaron's ERCOT work into Workstream 1 to prevent failure

#### Key Files
- `/mnt/ssd/moltbot/clawd/mba/sp26/capstone/CLAUDE.md` - Project context
- `/mnt/ssd/moltbot/clawd/mba/sp26/capstone/PROJECT_STATUS.md` - Status tracker
- `/mnt/ssd/moltbot/clawd/mba/sp26/capstone/project/ROADMAP.md` - Derek's master plan

### SP26 Other Courses
- **Advanced Corporate Finance** (Almazan): HW1 + Quiz 1 prep due Feb 1
- **Strategic Management** (Ritchie-Dunham): Team + Company selection due Jan 31
- **Managerial Macroeconomics**: TBD assignments
- **Capstone Touchpoint 1**: Feb 6, 11:30-11:45 with Kaitlyn (TA)

## Infrastructure

### Drive vs Local Status
- **Local**: Complete project structure at `/mnt/ssd/moltbot/clawd/mba/sp26/capstone/`
- **Drive**: SP26 Capstone folder doesn't exist yet on gdrive-school:TEMBA/
- **Sync**: Monitoring script created (`sync_capstone.sh`)

### Memory System
- **Type**: File-based (grep search via `search_memory.sh`)
- **Scope**: Academic domain (4 classes, 4 months)
- **Rationale**: Semantic search overkill for bounded scope

### Tools Access
- ✅ School email (`cody.kickertz@utexas.edu`)
- ✅ School calendar (gcal integration)
- ✅ School Google Drive (`gdrive-school:TEMBA/`)
- ✅ MBA local system (`/mnt/ssd/moltbot/clawd/mba/`)
- ✅ Taskwarrior for MBA tasks (`tw project:mba`)

## Coordination

### With Syn (Main Agent)
- **Reporting**: Status updates via sessions_send
- **Escalation**: Cross-domain impacts, bandwidth conflicts
- **Status file**: `/mnt/ssd/moltbot/clawd/agent-status/eiron.md` (to be created)

### Ecosystem Position
- **Syn**: Deep orchestrator, handles philosophy/complexity
- **Chiron**: Work specialist at Summus Global
- **Syl**: Kendall's curious companion  
- **Eiron (Me)**: School agent, cuts through academic BS

## Key Decisions Made

### 2026-01-29
- **Memory approach**: File-based grep search (not API-based semantic search)
- **Aaron mitigation**: Position to absorb ERCOT analysis from Workstream 2
- **Drive sync**: Manual monitoring until SP26 folder created
- **Focus area**: Workstreams 1+2 (analytical foundation), avoid 3+4 (competitive/culture)

## Active Reminders

### This Week (Feb 3-9)
- ✅ Phase 2 deliverables complete (Feb 4)
- Professor check-in **Feb 5** (Derek leads, deliverables ready)
- Capstone Touchpoint 1 **Feb 6** 11:30-11:45 with Kaitlyn (TA)
- ACF Homework due **Feb 9**
- BA 388T team selection due **Feb 9**

## Search Patterns

### Common Queries
```bash
# Team intelligence
./search_memory.sh "aaron|derek|workstream"

# Deadlines
./search_memory.sh "due|deadline|feb|january"

# Project status
./search_memory.sh "phase|capstone|tbc"
```
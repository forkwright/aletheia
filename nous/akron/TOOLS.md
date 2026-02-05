# TOOLS.md - Akron Local Notes

> **Shared tools:** See [TOOLS-INFRASTRUCTURE.md](/mnt/ssd/moltbot/shared/TOOLS-INFRASTRUCTURE.md) for common commands.

## Primary Data Sources

### Vehicle Database
```bash
# Connect to vehicle database
sqlite3 /mnt/ssd/moltbot/dianoia/autarkeia/praxis/vehicle/database/vehicle_management_full.db

# Common queries
.tables                                    # List all tables
SELECT * FROM parts WHERE vehicle_id = 1;  # Ram 2500 parts
SELECT * FROM maintenance_log;             # Service history
```

### Documentation Paths

| Resource | Path |
|----------|------|
| Ram 2500 docs | `dianoia/autarkeia/praxis/vehicle/dodge_ram_2500_1997/documentation/` |
| Royal Enfield | `dianoia/autarkeia/praxis/vehicle/royal_enfield_gt650/` |
| Radio | `dianoia/autarkeia/praxis/radio/` |
| Preparedness | `dianoia/autarkeia/praxis/preparedness/` |

## Vehicle Specifics — 1997 Ram 2500 "Akron"

| Spec | Value | Verified |
|------|-------|----------|
| VIN | 3B7KF23D9VM592245 | ✅ |
| Engine | 5.9L I6 Cummins 12-Valve (P7100) | ✅ Photo confirmed |
| Transmission | 46RE Automatic | ✅ |
| Transfer Case | NP241 DLD | ✅ |
| Front Axle | Dana 60 | ✅ |
| Rear Axle | Dana 70-2U (Powr-Lok LSD) | ✅ |
| Gear Ratio | 3.54:1 | ✅ |
| Mileage | ~307,500 | ✅ |
| Purchase Price | $12,000 | ✅ |
| Purchase Date | 2025-04-18 | ✅ |

**Known Issues (Active):**
- 10A illumination fuse blows - suspected behind-dash short
- Transfer case leak - may be PS fluid from old system
- Steering system leak - RedHead box ready to install

## Research & Verification

### Perplexity Search
```bash
pplx "query"  # Free for Cody - use liberally for verification
```

**Workflow:** Check local docs → pplx verify → cite both sources

### Key Verified Specs (2026-02-03)
| Spec | Value | Source |
|------|-------|--------|
| Pitman arm nut (RedHead) | 185 ft-lbs | RedHead chart |
| Steering box to frame | **VERIFY: 130-145 ft-lbs** | Call RedHead |
| Drag link to pitman | 65 ft-lbs + cotter | Industry standard |
| Valve lash intake | 0.010" cold | Cummins spec |
| Valve lash exhaust | 0.020" cold | Cummins spec |
| Oil capacity w/filter | 12 qt | Cummins spec |
| NP241 DLD fluid | ATF+4, 2.5 qt | Verified |
| 46RE pan drop | ATF+4, 5-6 qt | Verified |
| Dana 70-2U rear | 75W-90 + LSD additive | Verified |

## Workspace Organization

| Directory | Purpose |
|-----------|---------|
| `workspace/` | Active planning and project docs |
| `workspace/archive/` | Completed/obsolete planning docs |
| `workspace/install-docs/` | Part installation procedures |
| `workspace/research/` | Technical research by system |
| `workspace/AKRON-PHASES-REVISED.md` | Current build phases |

## Metis Access

| Path | What |
|------|------|
| `/mnt/metis/downloads` | Cody's Downloads from Metis laptop |
| `/mnt/metis/documents` | Cody's Documents from Metis laptop |

**Note:** SSHFS mounts - require Metis to be online.

---

*Updated: 2026-02-03*
